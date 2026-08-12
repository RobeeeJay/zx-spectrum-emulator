//! ZX Spectrum 48K and 128K: memory map and paging, ULA timing, contention,
//! sound, keyboard, and the debugging hooks that hang off the bus.

use crate::audio::Audio;
use crate::profiler::Profiler;
use crate::tape::Tape;
use crate::tracker::{Tracker, SCREEN_END, SCREEN_START};
use crate::z80::{Bus, Z80};

/// Which machine is being emulated. The two differ in clock speed, frame
/// length, memory layout and sound hardware.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Model {
    Spectrum48,
    Spectrum128,
    /// +2A: the 128K reworked with four ROMs and the second paging port.
    Plus2A,
    /// +3: a +2A with a disk interface.
    Plus3,
}

impl Model {
    pub fn name(&self) -> &'static str {
        match self {
            Model::Spectrum48 => "48K",
            Model::Spectrum128 => "128K",
            Model::Plus2A => "+2A",
            Model::Plus3 => "+3",
        }
    }
    /// T-states in one frame.
    pub fn frame_t(&self) -> u32 {
        match self {
            Model::Spectrum48 => 69888,
            _ => 70908,
        }
    }
    /// T-state at which the ULA fetches the first pixel of the display.
    pub fn first_pixel_t(&self) -> u32 {
        match self {
            Model::Spectrum48 => 14335,
            _ => 14361,
        }
    }
    pub fn t_per_line(&self) -> u32 {
        match self {
            Model::Spectrum48 => 224,
            _ => 228,
        }
    }
    pub fn cpu_hz(&self) -> f64 {
        match self {
            Model::Spectrum48 => 3_500_000.0,
            _ => 3_546_900.0,
        }
    }
    pub fn rom_size(&self) -> usize {
        match self {
            Model::Spectrum48 => 0x4000,
            Model::Spectrum128 => 0x8000,
            // +2A/+3 have four 16K ROMs.
            Model::Plus2A | Model::Plus3 => 0x10000,
        }
    }
    pub fn has_ay(&self) -> bool {
        !matches!(self, Model::Spectrum48)
    }
    /// True for machines with the $7FFD paging latch.
    pub fn has_paging(&self) -> bool {
        !matches!(self, Model::Spectrum48)
    }
    /// True for the +2A/+3, which add port $1FFD and its all-RAM modes.
    pub fn has_plus3_paging(&self) -> bool {
        matches!(self, Model::Plus2A | Model::Plus3)
    }
    pub fn has_disk(&self) -> bool {
        matches!(self, Model::Plus3)
    }
    /// The +2A/+3 ULA delays the CPU on a different phase of the fetch cycle.
    pub fn contention_pattern(&self) -> [u8; 8] {
        if self.has_plus3_paging() {
            [1, 0, 7, 6, 5, 4, 3, 2]
        } else {
            [6, 5, 4, 3, 2, 1, 0, 0]
        }
    }
    /// Which RAM banks the ULA shares with the CPU: the odd ones on a 48K/128K,
    /// the top four on a +2A/+3.
    pub fn bank_is_contended(&self, bank: usize) -> bool {
        if self.has_plus3_paging() {
            bank >= 4
        } else {
            bank & 1 == 1
        }
    }
    /// The +2A/+3 gate array drives the bus high instead of leaving it
    /// floating, so there is no floating-bus trick to emulate.
    pub fn has_floating_bus(&self) -> bool {
        !self.has_plus3_paging()
    }
}

/// T-states in a 48K frame. Handy as a default run budget.
pub const FRAME_T: u32 = 69888;
pub const FRAME_T_128: u32 = 70908;
pub const PIXEL_LINES: u32 = 192;
/// How long the ULA holds /INT low at the top of the frame.
pub const IRQ_LEN: u32 = 32;
/// Ceiling on border changes recorded per frame. A whole frame of the tightest
/// border routine is comfortably under this.
pub const BORDER_EVENT_CAP: usize = 24_000;
pub const CPU_HZ: f64 = 3_500_000.0;

const TABLE_LEN: usize = (FRAME_T_128 + 512) as usize;

/// Slow-motion drawing: stop the CPU after a fixed number of writes to the
/// watched area so the screen visibly fills in over several host frames.
pub struct SlowDraw {
    pub enabled: bool,
    /// Writes allowed per host frame before the CPU is parked.
    pub writes_per_slice: u32,
    pub watch_screen: bool,
    pub watch_back_buffer: bool,
    pub budget_left: u32,
    pub hit: bool,
    /// Writes to watched memory during the last host frame, for the UI.
    pub last_slice_writes: u32,
}

impl Default for SlowDraw {
    fn default() -> Self {
        SlowDraw {
            enabled: false,
            writes_per_slice: 8,
            watch_screen: true,
            watch_back_buffer: true,
            budget_left: 8,
            hit: false,
            last_slice_writes: 0,
        }
    }
}

impl SlowDraw {
    pub fn begin_slice(&mut self) {
        self.last_slice_writes = self.writes_per_slice.saturating_sub(self.budget_left);
        self.budget_left = self.writes_per_slice.max(1);
        self.hit = false;
    }
}

/// What a 16K slot of the address space currently points at.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Slot {
    Rom(usize),
    Ram(usize),
}

pub struct SpectrumBus {
    pub model: Model,
    /// 16K (48K machine) or 32K (128K machine) of ROM.
    pub rom: Vec<u8>,
    /// Eight 16K RAM banks. A 48K machine uses banks 5, 2 and 0.
    pub ram: Vec<u8>,
    /// Last value written to port $7FFD.
    pub page_reg: u8,
    /// Last value written to port $1FFD (+2A/+3 only).
    pub page_reg_1ffd: u8,
    /// Once the 128K locks paging, only a reset can undo it.
    pub paging_locked: bool,
    /// Later 48K machines run the display one T-state later relative to the
    /// interrupt. Both variants existed; HALT2INT tells them apart.
    pub late_timing: bool,
    slots: [Slot; 4],

    pub tracker: Tracker,

    /// T-states elapsed in the current frame.
    pub tstates: u32,
    pub frame: u64,
    /// True while the ULA is asserting /INT for this frame.
    pub irq_pending: bool,

    pub border: u8,
    /// Border colour at the start of the frame, plus every mid-frame change,
    /// so the renderer can reproduce raster bands.
    pub border_start: u8,
    /// Every mid-frame border change, as (T-state, colour). Border art writes
    /// the port hundreds of times per line, so this has to be roomy.
    pub border_events: Vec<(u32, u8)>,
    /// The same for the frame just finished. The part of the screen the ULA
    /// has not reached yet still shows the previous frame, so a render taken
    /// mid-frame needs both.
    pub border_prev: Vec<(u32, u8)>,
    pub border_prev_start: u8,
    /// The display file as it stood at the end of the last frame, so the part
    /// of the screen the beam has not reached yet can show what is still on
    /// the glass rather than what the CPU has since written.
    pub screen_prev: Vec<u8>,
    /// Keyboard matrix: one byte per half-row, bit clear = key down.
    pub keys: [u8; 8],
    pub ear: bool,
    pub speaker: bool,
    pub mic: bool,

    pub audio: Audio,

    /// Cassette player. Its EAR output is read through port $FE bit 6.
    pub tape: Option<Tape>,
    /// Run faster while the tape is playing, so loading does not take the
    /// same four minutes it did in 1983.
    pub tape_boost: bool,

    /// Scratch space for tape edges on their way to the mixer.
    tape_edge_scratch: Vec<(u64, bool)>,

    pub slow: SlowDraw,
    /// The bytes an RZX recording says the ports gave back, while one is
    /// playing. Every IN takes the next one instead of reading the hardware,
    /// which is what makes the program follow the path it followed then.
    pub playback: Option<Playback>,
    /// Opcode fetches since the machine started, which is how a recording
    /// measures the length of a frame.
    pub fetches: u32,
    /// What each routine is seen to do, while it is switched on.
    pub observer: crate::observe::Observer,
    /// What the debugger is watching for, and what it caught. The bus is
    /// where all four things happen, so it is the bus that notices them.
    pub breaks: Breaks,
    pub break_hit: Option<Event>,
    /// Writes to video RAM in the frame just finished, for the UI readout.
    pub screen_writes: u32,
    screen_writes_acc: u32,

    contention: Vec<u8>,
}

impl Default for SpectrumBus {
    fn default() -> Self {
        Self::new(Model::Spectrum48)
    }
}

impl SpectrumBus {
    pub fn new(model: Model) -> Self {
        let mut bus = SpectrumBus {
            model,
            rom: vec![0xff; model.rom_size()],
            ram: vec![0; 8 * 0x4000],
            page_reg: 0,
            page_reg_1ffd: 0,
            paging_locked: false,
            late_timing: false,
            slots: [Slot::Rom(0), Slot::Ram(5), Slot::Ram(2), Slot::Ram(0)],
            tracker: Tracker::new(),
            tstates: 0,
            frame: 0,
            irq_pending: false,
            border: 7,
            border_start: 7,
            border_events: Vec::with_capacity(4096),
            border_prev: Vec::with_capacity(4096),
            border_prev_start: 7,
            screen_prev: vec![0; 6912],
            keys: [0xff; 8],
            ear: false,
            speaker: false,
            mic: false,
            audio: Audio::new(model.cpu_hz()),
            tape: None,
            tape_boost: true,
            tape_edge_scratch: Vec::new(),
            slow: SlowDraw::default(),
            screen_writes: 0,
            playback: None,
            fetches: 0,
            observer: crate::observe::Observer::new(),
            breaks: Breaks::default(),
            break_hit: None,
            screen_writes_acc: 0,
            contention: vec![0; TABLE_LEN],
        };
        bus.audio.ay_present = model.has_ay();
        bus.build_contention_table();
        bus.apply_paging();
        bus
    }

    /// T-state of the first pixel fetch, including the late-timing shift.
    #[inline]
    pub fn first_pixel_t(&self) -> u32 {
        self.model.first_pixel_t() + self.late_timing as u32
    }

    /// Rebuild the contention table; call after changing the timing model.
    pub fn set_late_timing(&mut self, late: bool) {
        if self.late_timing != late {
            self.late_timing = late;
            self.build_contention_table();
        }
    }

    fn build_contention_table(&mut self) {
        let pattern = self.model.contention_pattern();
        self.contention.iter_mut().for_each(|v| *v = 0);
        let first = self.first_pixel_t();
        let per_line = self.model.t_per_line();
        for line in 0..PIXEL_LINES {
            let line_start = first + line * per_line;
            for px in 0..128u32 {
                self.contention[(line_start + px) as usize] = pattern[(px % 8) as usize];
            }
        }
    }

    #[inline]
    pub fn frame_t(&self) -> u32 {
        self.model.frame_t()
    }

    // ---- memory paging -----------------------------------------------------

    /// The four all-RAM layouts the +2A/+3 can select with $1FFD.
    pub const SPECIAL_CONFIGS: [[usize; 4]; 4] =
        [[0, 1, 2, 3], [4, 5, 6, 7], [4, 5, 6, 3], [4, 7, 6, 3]];

    /// Recompute the four 16K slots from the paging registers.
    fn apply_paging(&mut self) {
        if !self.model.has_paging() {
            self.slots = [Slot::Rom(0), Slot::Ram(5), Slot::Ram(2), Slot::Ram(0)];
            return;
        }

        // +2A/+3 special mode: no ROM at all, four RAM banks instead.
        if self.model.has_plus3_paging() && self.page_reg_1ffd & 0x01 != 0 {
            let config = ((self.page_reg_1ffd >> 1) & 0x03) as usize;
            let banks = Self::SPECIAL_CONFIGS[config];
            self.slots = [
                Slot::Ram(banks[0]),
                Slot::Ram(banks[1]),
                Slot::Ram(banks[2]),
                Slot::Ram(banks[3]),
            ];
            return;
        }

        // Normal mode. On a +2A/+3 the ROM number is two bits, the low one
        // from $7FFD and the high one from $1FFD.
        let rom = if self.model.has_plus3_paging() {
            (((self.page_reg_1ffd >> 1) & 0x02) | ((self.page_reg >> 4) & 0x01)) as usize
        } else {
            ((self.page_reg >> 4) & 0x01) as usize
        };
        let bank = (self.page_reg & 0x07) as usize;
        self.slots = [Slot::Rom(rom), Slot::Ram(5), Slot::Ram(2), Slot::Ram(bank)];
    }

    /// Write to port $7FFD.
    pub fn write_paging(&mut self, value: u8) {
        if !self.model.has_paging() || self.paging_locked {
            return;
        }
        self.page_reg = value;
        if value & 0x20 != 0 {
            self.paging_locked = true;
        }
        self.apply_paging();
    }

    /// Write to port $1FFD (+2A/+3).
    pub fn write_paging_1ffd(&mut self, value: u8) {
        if !self.model.has_plus3_paging() || self.paging_locked {
            return;
        }
        self.page_reg_1ffd = value;
        self.apply_paging();
    }

    /// True while the +3 is in one of its all-RAM configurations.
    pub fn special_paging(&self) -> bool {
        self.model.has_plus3_paging() && self.page_reg_1ffd & 0x01 != 0
    }

    /// +3 disk motor bit, decoded but not acted on: there is no FDC.
    pub fn disk_motor(&self) -> bool {
        self.model.has_disk() && self.page_reg_1ffd & 0x08 != 0
    }

    /// Bank the ULA is displaying: 5 normally, 7 for the shadow screen.
    #[inline]
    pub fn screen_bank(&self) -> usize {
        if self.model.has_paging() && self.page_reg & 0x08 != 0 {
            7
        } else {
            5
        }
    }

    #[inline]
    pub fn slot_of(&self, addr: u16) -> Slot {
        self.slots[(addr >> 14) as usize]
    }

    #[inline]
    pub fn mem(&self, addr: u16) -> u8 {
        let off = (addr & 0x3fff) as usize;
        match self.slot_of(addr) {
            Slot::Rom(page) => {
                let i = page * 0x4000 + off;
                self.rom.get(i).copied().unwrap_or(0xff)
            }
            Slot::Ram(bank) => self.ram[bank * 0x4000 + off],
        }
    }

    #[inline]
    pub fn poke(&mut self, addr: u16, v: u8) {
        let off = (addr & 0x3fff) as usize;
        if let Slot::Ram(bank) = self.slot_of(addr) {
            self.ram[bank * 0x4000 + off] = v;
        }
    }

    /// A byte of the display file as it was at the end of the last frame.
    #[inline]
    pub fn video_prev(&self, offset: u16) -> u8 {
        self.screen_prev
            .get(offset as usize & 0x1fff)
            .copied()
            .unwrap_or(0)
    }

    /// Read a byte of the displayed screen, wherever it is banked.
    #[inline]
    pub fn video(&self, offset: u16) -> u8 {
        let bank = self.screen_bank();
        self.ram[bank * 0x4000 + (offset as usize & 0x3fff)]
    }

    /// Where an address lives in physical memory, which is what the access
    /// tracker records against so a bank keeps its history while paged out.
    #[inline]
    pub fn phys_index(&self, addr: u16) -> usize {
        match self.slot_of(addr) {
            Slot::Ram(bank) => crate::tracker::ram_phys(bank, addr),
            Slot::Rom(page) => crate::tracker::rom_phys(page, addr),
        }
    }

    /// RAM banks worth showing, in address order. A 48K machine only ever
    /// sees three of them.
    pub fn visible_banks(&self) -> Vec<usize> {
        if self.model.has_paging() {
            (0..8).collect()
        } else {
            vec![5, 2, 0]
        }
    }

    /// ROM pages this machine has.
    pub fn rom_pages(&self) -> usize {
        self.rom.len() / 0x4000
    }

    /// Which slot, if any, a RAM bank is currently paged into.
    pub fn ram_bank_slot(&self, bank: usize) -> Option<usize> {
        (0..4).find(|&slot| self.slots[slot] == Slot::Ram(bank))
    }

    /// Which ROM page is paged in at $0000, which is the one whose routines a
    /// program calling into the ROM will reach.
    ///
    /// The 128K's ROM image is two 16K ROMs and the +3's is four; each is
    /// addressed $0000-$3FFF in its own right, so a name for an address only
    /// means anything alongside which of them is in.
    pub fn rom_in_use(&self) -> usize {
        (0..self.rom_pages())
            .find(|page| self.rom_page_slot(*page) == Some(0))
            .unwrap_or(0)
    }

    /// Which slot, if any, a ROM page is currently paged into.
    pub fn rom_page_slot(&self, page: usize) -> Option<usize> {
        (0..4).find(|&slot| self.slots[slot] == Slot::Rom(page))
    }

    /// Read a byte of a specific RAM bank, for the debugger.
    pub fn bank_byte(&self, bank: usize, offset: u16) -> u8 {
        self.ram[(bank & 7) * 0x4000 + (offset as usize & 0x3fff)]
    }

    /// Untracked read for renderers and the debugger.
    #[inline]
    pub fn peek_raw(&self, addr: u16) -> u8 {
        self.mem(addr)
    }

    /// A page is contended when it holds an odd-numbered RAM bank: on a 48K
    /// that is only bank 5 at $4000, on a 128K also banks 1, 3 and 7 wherever
    /// they are paged in.
    #[inline]
    fn contended_addr(&self, addr: u16) -> bool {
        match self.slot_of(addr) {
            Slot::Ram(bank) => self.model.bank_is_contended(bank),
            Slot::Rom(_) => false,
        }
    }

    // ---- timing ------------------------------------------------------------

    /// A whole memory cycle: the ULA stalls the CPU once, at the start, then
    /// the access takes its usual `t` T-states.
    #[inline]
    fn access(&mut self, addr: u16, t: u32) {
        if self.contended_addr(addr) {
            self.tstates += self.delay() as u32;
        }
        self.tstates += t;
    }

    /// Internal cycles: the address stays on the bus, so contention is
    /// re-evaluated for every single T-state.
    #[inline]
    fn contend_addr(&mut self, addr: u16, times: u32) {
        if self.contended_addr(addr) {
            for _ in 0..times {
                self.tstates += self.delay() as u32 + 1;
            }
        } else {
            self.tstates += times;
        }
    }

    #[inline]
    fn delay(&self) -> u8 {
        let t = self.tstates as usize;
        if t < self.contention.len() {
            self.contention[t]
        } else {
            0
        }
    }

    /// The I/O contention pattern, which depends on both the port's high byte
    /// and whether it is a ULA port (A0 low).
    ///
    /// Returns the T-state at which the IORQ cycle begins, which is when the
    /// ULA's data is on the bus and therefore what a floating-bus read sees.
    fn contend_io(&mut self, port: u16) -> u32 {
        let high_contended = port & 0xc000 == 0x4000;
        let ula = port & 1 == 0;
        match (high_contended, ula) {
            // C:1, C:3
            (true, true) => {
                self.io_stall();
                let sampled = self.tstates;
                self.tstates += 1;
                self.io_stall();
                self.tstates += 3;
                sampled
            }
            // C:1, C:1, C:1, C:1
            (true, false) => {
                self.io_stall();
                let sampled = self.tstates;
                self.tstates += 1;
                for _ in 0..3 {
                    self.io_stall();
                    self.tstates += 1;
                }
                sampled
            }
            // N:1, C:3 — the ULA stalls the CPU even for an uncontended page.
            (false, true) => {
                self.tstates += 1;
                self.io_stall();
                let sampled = self.tstates;
                self.tstates += 3;
                sampled
            }
            // N:4
            (false, false) => {
                let sampled = self.tstates;
                self.tstates += 4;
                sampled
            }
        }
    }

    #[inline]
    fn io_stall(&mut self) {
        self.tstates += self.delay() as u32;
    }

    // ---- sound -------------------------------------------------------------

    /// Generate any samples due up to now. Called whenever the sound output
    /// changes and once per frame.
    ///
    /// The tape is always advanced first: the mixer must never run past the
    /// tape, or the tape's edges would arrive with timestamps already in the
    /// past and be collapsed into silence.
    pub fn audio_sync(&mut self) {
        if self.tape_playing() {
            self.tape_advance();
        }
        let now = self.total_t();
        self.audio.advance_to(now);
    }

    fn update_beeper(&mut self) {
        // The speaker bit dominates; MIC and the EAR input are audible on real
        // hardware too, which is why you can hear a tape loading.
        let level = 0.55 * self.speaker as u8 as f32
            + 0.08 * self.mic as u8 as f32
            + 0.08 * self.ear as u8 as f32;
        self.audio.beeper = level;
    }

    // ---- tape --------------------------------------------------------------

    /// T-states since power-on, which is the timebase for tape and sound.
    #[inline]
    pub fn total_t(&self) -> u64 {
        self.frame * self.model.frame_t() as u64 + self.tstates as u64
    }

    /// Current EAR input level, advancing the tape to the present moment.
    ///
    /// Every edge the tape produced along the way is mixed in at its own
    /// T-state. Sampling the level only when the CPU reads the port would
    /// collapse a whole burst of pilot tone into one transition, which is
    /// what makes a tape sound like noise instead of a tone.
    pub fn tape_level(&mut self) -> bool {
        self.tape_advance()
    }

    /// Advance the tape to the present, mixing in every edge it produced at
    /// the T-state it happened, and return the resulting EAR level.
    fn tape_advance(&mut self) -> bool {
        let now = self.total_t();
        if self.tape.is_none() {
            return self.ear;
        }

        let mut edges = std::mem::take(&mut self.tape_edge_scratch);
        edges.clear();
        let level = {
            let tape = self.tape.as_mut().expect("checked above");
            let level = tape.level_at(now);
            tape.take_pending_edges(&mut edges);
            level
        };

        for (at, ear) in edges.drain(..) {
            self.audio.advance_to(at);
            self.ear = ear;
            self.update_beeper();
        }
        self.tape_edge_scratch = edges;

        if self.ear != level {
            self.audio.advance_to(now);
            self.ear = level;
            self.update_beeper();
        }
        level
    }

    /// Keep the tape running even when the CPU is not polling the port, so the
    /// oscilloscope and block position stay live.
    pub fn tape_tick(&mut self) {
        if self.tape_playing() {
            self.tape_advance();
        }
    }

    pub fn tape_playing(&self) -> bool {
        self.tape.as_ref().is_some_and(|t| t.playing)
    }

    /// Border colour at every T-state of the frame as it would appear on
    /// screen right now: the current frame up to where the ULA has got to,
    /// and the previous frame beyond that.
    pub fn border_raster(&self) -> Vec<u8> {
        self.border_raster_at(self.tstates)
    }

    /// The same, but splitting this frame from the last at `split` rather than
    /// at wherever the emulator has got to.
    pub fn border_raster_at(&self, split: u32) -> Vec<u8> {
        let frame_t = self.frame_t() as usize;
        let mut out = vec![0u8; frame_t];
        let now = (split as usize).min(frame_t);

        let mut colour = self.border_start;
        let mut ev = self.border_events.iter().peekable();
        for (t, slot) in out.iter_mut().enumerate().take(now) {
            while ev.peek().is_some_and(|(at, _)| *at as usize <= t) {
                colour = ev.next().unwrap().1;
            }
            *slot = colour;
        }

        let mut colour = self.border_prev_start;
        let mut ev = self.border_prev.iter().peekable();
        // Catch up to the current position before filling the rest.
        while ev.peek().is_some_and(|(at, _)| (*at as usize) < now) {
            colour = ev.next().unwrap().1;
        }
        for (t, slot) in out.iter_mut().enumerate().skip(now) {
            while ev.peek().is_some_and(|(at, _)| *at as usize <= t) {
                colour = ev.next().unwrap().1;
            }
            *slot = colour;
        }
        out
    }

    /// Border colour in effect at T-state `t` of the current frame.
    pub fn border_at(&self, t: u32) -> u8 {
        match self.border_events.partition_point(|&(at, _)| at <= t) {
            0 => self.border_start,
            i => self.border_events[i - 1].1,
        }
    }

    fn watched(&self, addr: u16) -> bool {
        if self.slow.watch_screen && (SCREEN_START..SCREEN_END).contains(&addr) {
            return true;
        }
        if self.slow.watch_back_buffer {
            if let Some(r) = self.tracker.back_buffer() {
                return r.contains(addr);
            }
        }
        false
    }

    /// Keyboard: 0xFE reads return the AND of every selected half-row.
    fn keyboard(&self, port: u16, ear: bool) -> u8 {
        let mut result = 0x1f;
        for row in 0..8 {
            if port & (1 << (8 + row)) == 0 {
                result &= self.keys[row] & 0x1f;
            }
        }
        let mut v = result | 0xa0;
        if ear {
            v |= 0x40;
        }
        v
    }

    /// The floating bus: what the ULA had on the bus at T-state `t`.
    /// The +2A/+3 has none, so it reads back as $FF.
    fn floating_bus(&self, t: u32) -> u8 {
        if !self.model.has_floating_bus() {
            return 0xff;
        }
        let first = self.first_pixel_t();
        let per_line = self.model.t_per_line();
        if t < first || t >= first + PIXEL_LINES * per_line {
            return 0xff;
        }
        let rel = t - first;
        let line = rel / per_line;
        let col_t = rel % per_line;
        if col_t >= 128 {
            return 0xff;
        }
        // The ULA fetches two cells in every eight T-states — bitmap, attribute,
        // bitmap, attribute — and leaves the bus alone for the other four, when
        // it reads back as $FF.
        let group = col_t / 8;
        let cell = (group * 2) as u16;
        let offset = match col_t % 8 {
            0 => screen_bitmap_offset(line as u16, cell),
            1 => screen_attr_offset(line as u16, cell),
            2 => screen_bitmap_offset(line as u16, cell + 1),
            3 => screen_attr_offset(line as u16, cell + 1),
            _ => return 0xff,
        };
        self.video(offset)
    }

    /// End-of-frame bookkeeping: flush sound, re-arm the interrupt.
    pub fn end_frame(&mut self) {
        self.tstates -= self.model.frame_t();
        self.frame += 1;
        // While a recording is playing, the frame boundary is where the
        // recording says it is — an instruction count, not a T-state count —
        // so the interrupt is raised there instead of here.
        self.irq_pending = self.playback.is_none();
        self.screen_writes = self.screen_writes_acc;
        self.screen_writes_acc = 0;
        // Keep the finished frame; the renderer needs it for the part of the
        // screen the ULA has not redrawn yet.
        let bank = self.screen_bank() * 0x4000;
        self.screen_prev
            .copy_from_slice(&self.ram[bank..bank + 6912]);
        std::mem::swap(&mut self.border_events, &mut self.border_prev);
        self.border_prev_start = self.border_start;
        self.border_start = self.border;
        self.border_events.clear();
        self.audio_sync();
        self.observer.end_frame();
    }

    pub fn frame_visuals(&mut self) {
        self.tracker.fade();
        self.tracker.tick_detector();
    }
}

/// Offset of a pixel byte within the 6912-byte display file.
pub fn screen_bitmap_offset(line: u16, cell: u16) -> u16 {
    let y = line;
    ((y & 0xc0) << 5) | ((y & 0x07) << 8) | ((y & 0x38) << 2) | (cell & 0x1f)
}

/// Offset of an attribute byte within the display file.
pub fn screen_attr_offset(line: u16, cell: u16) -> u16 {
    0x1800 + (line / 8) * 32 + (cell & 0x1f)
}

pub fn screen_bitmap_addr(line: u16, cell: u16) -> u16 {
    0x4000 | screen_bitmap_offset(line, cell)
}

pub fn screen_attr_addr(line: u16, cell: u16) -> u16 {
    0x4000 | screen_attr_offset(line, cell)
}

impl Bus for SpectrumBus {
    fn fetch_op(&mut self, addr: u16) -> u8 {
        // Counted for RZX playback, which measures a frame in opcode fetches:
        // a prefixed instruction is two or more of them, so counting whole
        // instructions instead runs past the end of every frame.
        self.fetches = self.fetches.wrapping_add(1);
        self.observer.on_fetch(addr);
        self.access(addr, 4);
        let phys = self.phys_index(addr);
        self.tracker.on_exec(phys, addr);
        self.mem(addr)
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.access(addr, 3);
        let phys = self.phys_index(addr);
        self.tracker.on_read(phys, addr);
        self.observer.on_read(addr);
        self.mem(addr)
    }

    fn read_operand(&mut self, addr: u16) -> u8 {
        // Timed exactly as a read, because that is what the Z80 does; counted
        // as code, because that is what it is.
        self.access(addr, 3);
        let phys = self.phys_index(addr);
        self.tracker.on_read(phys, addr);
        self.observer.on_fetch(addr);
        self.mem(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.access(addr, 3);
        let phys = self.phys_index(addr);
        self.tracker.on_write(phys, addr);
        self.observer.on_write(addr);
        if (SCREEN_START..SCREEN_END).contains(&addr) {
            self.screen_writes_acc += 1;
            if self.breaks.screen {
                self.break_hit.get_or_insert(Event::Screen(addr));
            }
        }
        if self.slow.enabled && self.watched(addr) {
            if self.slow.budget_left == 0 {
                self.slow.hit = true;
            } else {
                self.slow.budget_left -= 1;
                if self.slow.budget_left == 0 {
                    self.slow.hit = true;
                }
            }
        }
        self.poke(addr, value);
    }

    fn contend(&mut self, addr: u16, times: u32) {
        self.contend_addr(addr, times);
    }

    fn io_read(&mut self, port: u16) -> u8 {
        let sampled = self.contend_io(port);
        self.observer.on_port(port, false);
        if self.breaks.port_in {
            self.break_hit.get_or_insert(Event::In(port));
        }
        // A recording replaces the hardware, not just the keyboard: the
        // floating bus, the tape and the sound chip all read back what they
        // read back on the day.
        if let Some(playback) = &mut self.playback {
            let byte = playback.next();
            if self.breaks.ay && self.model.has_ay() && port & 0xc002 == 0xc000 {
                self.break_hit.get_or_insert(Event::Ay);
            }
            return byte;
        }
        // AY register read: $FFFD.
        if self.model.has_ay() && port & 0xc002 == 0xc000 {
            if self.breaks.ay {
                self.break_hit.get_or_insert(Event::Ay);
            }
            return self.audio.ay.read();
        }
        if port & 1 == 0 {
            let ear = self.tape_level();
            self.keyboard(port, ear)
        } else {
            self.floating_bus(sampled)
        }
    }

    fn io_write(&mut self, port: u16, value: u8) {
        let sampled = self.contend_io(port);
        self.observer.on_port(port, true);
        if self.breaks.port_out {
            self.break_hit.get_or_insert(Event::Out(port, value));
        }

        if port & 1 == 0 {
            let new = value & 7;
            if new != self.border && self.border_events.len() < BORDER_EVENT_CAP {
                // Timed at the start of the IORQ cycle, which is when the ULA
                // sees the write.
                self.border_events.push((sampled, new));
            }
            self.border = new;
            let speaker = value & 0x10 != 0;
            let mic = value & 0x08 != 0;
            if speaker != self.speaker || mic != self.mic {
                if self.breaks.beeper {
                    self.break_hit.get_or_insert(Event::Beeper);
                }
                self.audio_sync();
                self.speaker = speaker;
                self.mic = mic;
                self.update_beeper();
            }
        }

        if self.model.has_paging() {
            // $7FFD: memory paging. The +2A/+3 decode it more strictly than
            // the 128K, which decodes only A15 and A1.
            let is_7ffd = if self.model.has_plus3_paging() {
                port & 0xc002 == 0x4000
            } else {
                port & 0x8002 == 0
            };
            if is_7ffd {
                self.write_paging(value);
            }
            // $1FFD: the +2A/+3's second paging port.
            if self.model.has_plus3_paging() && port & 0xf002 == 0x1000 {
                self.write_paging_1ffd(value);
            }
            // $FFFD: AY register select, $BFFD: AY data.
            if port & 0xc002 == 0xc000 {
                if self.breaks.ay {
                    self.break_hit.get_or_insert(Event::Ay);
                }
                self.audio.ay.selected = value & 0x0f;
            } else if port & 0xc002 == 0x8000 {
                if self.breaks.ay {
                    self.break_hit.get_or_insert(Event::Ay);
                }
                self.audio_sync();
                self.audio.ay.write(value);
            }
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        self.mem(addr)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stop {
    /// Ran out of the requested T-state budget.
    Budget,
    /// Slow-draw mode used up its write allowance.
    SlowDraw,
    Breakpoint(u16),
    /// A single-step request completed.
    Stepped,
    /// Something the debugger was watching for happened, at this address.
    ///
    /// For an interrupt that is the first instruction of the handler, which
    /// has not run yet. For the others it is the instruction that did it,
    /// which has: a write or an OUT takes effect part-way through an
    /// instruction and the CPU is only stoppable between them.
    Watched(Event, u16),
}

/// What the machine can be told to stop on besides reaching an address.
///
/// These are the things a program does that are hard to find by address:
/// where it draws, where it makes a noise, and where the frame interrupt takes
/// it. Each is only tested when it is switched on, so nothing is paid for a
/// watch that is off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Breaks {
    /// A write anywhere in the display file.
    pub screen: bool,
    /// The speaker or MIC bit of port $FE changing state.
    pub beeper: bool,
    /// Any access to the sound chip, read or write.
    pub ay: bool,
    /// The frame interrupt being accepted by the CPU.
    pub interrupt: bool,
    /// Code entering the ROM from outside it: a program calling a ROM routine.
    /// Moving about within the ROM is not entering it, so a ROM routine
    /// calling another one does not count.
    pub rom: bool,
    /// Any IN at all: a program reading a port, whichever port it is.
    pub port_in: bool,
    /// Any OUT at all.
    pub port_out: bool,
}

impl Breaks {
    /// Whether anything at all is being watched.
    pub fn any(&self) -> bool {
        self.screen
            || self.beeper
            || self.ay
            || self.interrupt
            || self.rom
            || self.port_in
            || self.port_out
    }
}

/// One frame's worth of recorded input, being handed out.
#[derive(Clone, Debug, Default)]
pub struct Playback {
    pub inputs: Vec<u8>,
    pub cursor: usize,
    /// How many times a frame has asked for more input than was recorded.
    /// Never zero on a recording that has come adrift from the machine, so it
    /// is worth telling the user about rather than playing on regardless.
    pub short: u32,
}

impl Playback {
    /// The next recorded byte. A frame that reads more than was recorded is
    /// out of step; the last byte is repeated rather than inventing one, and
    /// the shortfall is counted.
    fn next(&mut self) -> u8 {
        match self.inputs.get(self.cursor) {
            Some(byte) => {
                self.cursor += 1;
                *byte
            }
            None => {
                self.short += 1;
                self.inputs.last().copied().unwrap_or(0xFF)
            }
        }
    }
}

/// Which of those happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Screen(u16),
    Beeper,
    Ay,
    Interrupt,
    /// Went into the ROM from this address.
    Rom(u16),
    /// Read a port, and which.
    In(u16),
    /// Wrote to a port, and what.
    Out(u16, u8),
}

impl Event {
    /// What to tell the user the machine stopped for.
    pub fn describe(&self) -> String {
        match self {
            Event::Screen(addr) => format!("Wrote to the screen at ${addr:04X}"),
            Event::Beeper => "Toggled the beeper".to_string(),
            Event::Ay => "Used the sound chip".to_string(),
            Event::Interrupt => "Took the frame interrupt".to_string(),
            Event::Rom(from) => format!("Went into the ROM from ${from:04X}"),
            Event::In(port) => format!("Read port ${port:04X}"),
            Event::Out(port, value) => format!("Wrote ${value:02X} to port ${port:04X}"),
        }
    }
}

pub struct Spectrum {
    pub cpu: Z80,
    pub bus: SpectrumBus,
    pub breakpoints: Vec<u16>,
    /// Temporary breakpoint used by "step over" / "run to cursor".
    pub temp_bp: Option<u16>,
    /// Whole frames completed since the last visual update.
    pub frames_completed: u32,
    /// Call profiler; does nothing until a run is started.
    pub profiler: Profiler,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the ROM ends and RAM begins, on every machine here.
const ROM_END: u16 = 0x4000;

/// Read a byte without disturbing anything, for looking at the stack.
fn raw_peek(bus: &SpectrumBus, addr: u16) -> u8 {
    bus.peek_raw(addr)
}

impl Spectrum {
    pub fn new() -> Self {
        Spectrum::with_model(Model::Spectrum48)
    }

    pub fn with_model(model: Model) -> Self {
        Spectrum {
            cpu: Z80::new(),
            bus: SpectrumBus::new(model),
            breakpoints: Vec::new(),
            temp_bp: None,
            frames_completed: 0,
            profiler: Profiler::new(),
        }
    }

    pub fn load_rom(&mut self, data: &[u8]) {
        let n = data.len().min(self.bus.rom.len());
        self.bus.rom[..n].copy_from_slice(&data[..n]);
    }

    /// Switch machine, keeping the tape and audio device attached.
    pub fn set_model(&mut self, model: Model, rom: &[u8]) {
        let tape = self.bus.tape.take();
        let audio = std::mem::replace(&mut self.bus.audio, crate::audio::Audio::new(1.0));
        let tape_boost = self.bus.tape_boost;
        let slow_enabled = self.bus.slow.enabled;
        let late = self.bus.late_timing;

        self.bus = SpectrumBus::new(model);
        self.bus.set_late_timing(late);
        self.bus.audio = audio;
        self.bus.audio.set_cpu_hz(model.cpu_hz());
        self.bus.audio.ay_present = model.has_ay();
        self.bus.audio.ay.reset();
        self.bus.tape = tape;
        self.bus.tape_boost = tape_boost;
        self.bus.slow.enabled = slow_enabled;
        self.load_rom(rom);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.tstates = 0;
        self.bus.frame = 0;
        self.bus.irq_pending = false;
        self.bus.border = 7;
        self.bus.ram.iter_mut().for_each(|b| *b = 0);
        self.bus.page_reg = 0;
        self.bus.page_reg_1ffd = 0;
        self.bus.paging_locked = false;
        self.bus.apply_paging();
        self.bus.tracker.reset();
        self.bus.audio.ay.reset();
        self.bus.audio.rebase(0);
        self.bus.speaker = false;
        self.bus.mic = false;
        self.bus.ear = false;
        self.bus.audio.beeper = 0.0;
        // The tape's timebase is absolute, so it has to go back to the start
        // along with the frame counter.
        if let Some(tape) = &mut self.bus.tape {
            tape.stop();
            tape.rewind();
            tape.edges.clear();
        }
    }

    fn check_interrupt(&mut self) {
        if self.bus.irq_pending {
            if self.bus.tstates >= IRQ_LEN {
                // Missed the window entirely.
                self.bus.irq_pending = false;
            } else if self.cpu.interrupt(&mut self.bus) {
                self.bus.irq_pending = false;
                if self.bus.breaks.interrupt {
                    self.bus.break_hit.get_or_insert(Event::Interrupt);
                }
                if self.bus.observer.enabled {
                    let registers = crate::observe::Registers {
                        af: self.cpu.af(),
                        bc: self.cpu.bc(),
                        de: self.cpu.de(),
                        hl: self.cpu.hl(),
                    };
                    let (pc, sp) = (self.cpu.pc, self.cpu.sp);
                    self.bus.observer.on_interrupt(pc, sp, registers);
                }
                // The handler is profiled like any other call.
                if self.profiler.running {
                    let now = self.bus.total_t();
                    self.profiler.on_call(self.cpu.pc, self.cpu.sp, now);
                }
            }
        }
    }

    /// Execute exactly one instruction (after any pending interrupt).
    pub fn step_instruction(&mut self) {
        self.check_interrupt();

        let watching = self.profiler.running || self.bus.observer.enabled || self.bus.breaks.rom;
        let (pc0, sp0, t0) = if watching {
            (self.cpu.pc, self.cpu.sp, self.bus.total_t())
        } else {
            (0, 0, 0)
        };

        // Where the code was before this instruction, for the ROM watch: only
        // needed while it is on, and it is one comparison when it is not.
        let was_outside_rom = self.bus.breaks.rom && self.cpu.pc >= ROM_END;

        self.cpu.step(&mut self.bus);

        // Going into the ROM from outside it is a program calling a ROM
        // routine. Moving about inside the ROM is not, so a ROM routine
        // calling another one is left alone.
        if was_outside_rom && self.cpu.pc < ROM_END {
            self.bus.break_hit.get_or_insert(Event::Rom(pc0));
        }

        if self.bus.tstates >= self.bus.frame_t() {
            self.bus.end_frame();
            self.frames_completed += 1;
        }

        if watching {
            let now = self.bus.total_t();
            let elapsed = now.saturating_sub(t0);
            let (pc1, sp1) = (self.cpu.pc, self.cpu.sp);
            if self.profiler.running {
                let bus = &self.bus;
                self.profiler
                    .on_instruction(pc0, sp0, pc1, sp1, now, elapsed, |a| {
                        u16::from_le_bytes([bus.peek_raw(a), bus.peek_raw(a.wrapping_add(1))])
                    });
            }
            if self.bus.observer.enabled {
                let t = self.bus.tstates;
                self.bus.observer.set_frame_t(t);
                let registers = crate::observe::Registers {
                    af: self.cpu.af(),
                    bc: self.cpu.bc(),
                    de: self.cpu.de(),
                    hl: self.cpu.hl(),
                };
                // The observer lives on the bus, which is also what has to be
                // read to see what was pushed, so the memory is copied out
                // first rather than borrowing both at once.
                let stack_word = |a: u16| {
                    u16::from_le_bytes([
                        raw_peek(&self.bus, a),
                        raw_peek(&self.bus, a.wrapping_add(1)),
                    ])
                };
                let pushed = stack_word(sp1);
                let popped = stack_word(sp0);
                self.bus
                    .observer
                    .on_instruction(pc0, sp0, pc1, sp1, registers, |a| {
                        if a == sp1 {
                            pushed
                        } else {
                            popped
                        }
                    });
            }
        }
    }

    /// Run until `budget` T-states have been consumed, a breakpoint is hit, or
    /// slow-draw mode parks the CPU.
    pub fn run(&mut self, budget: u32) -> Stop {
        let frame_t = self.bus.frame_t();
        let mut spent = 0u32;
        while spent < budget {
            let before = self.bus.tstates;

            // The interrupt is taken before the next instruction is fetched,
            // so a watch on it stops with the handler's first instruction
            // still to run rather than after it.
            self.check_interrupt();
            if let Some(event) = self.bus.break_hit.take() {
                return Stop::Watched(event, self.cpu.pc);
            }

            let at = self.cpu.pc;
            self.step_instruction();
            let after = self.bus.tstates;
            spent += if after >= before {
                after - before
            } else {
                after + frame_t - before
            };

            if let Some(event) = self.bus.break_hit.take() {
                return Stop::Watched(event, at);
            }
            if self.bus.slow.enabled && self.bus.slow.hit {
                return Stop::SlowDraw;
            }
            let pc = self.cpu.pc;
            if self.temp_bp == Some(pc) {
                self.temp_bp = None;
                return Stop::Breakpoint(pc);
            }
            if self.breakpoints.contains(&pc) {
                return Stop::Breakpoint(pc);
            }
        }
        Stop::Budget
    }

    /// Run exactly `fetches` instructions, or until something stops it.
    ///
    /// A recording is measured in instructions, not in T-states, so playing
    /// one back means running the number it says and no more. Returns how many
    /// were actually run, so a stop part-way through a frame can be picked up
    /// where it left off.
    pub fn run_fetches(&mut self, fetches: u32) -> (Stop, u32) {
        let start = self.bus.fetches;
        let done_now = |bus: &SpectrumBus| bus.fetches.wrapping_sub(start);
        while done_now(&self.bus) < fetches {
            let done = done_now(&self.bus);
            self.check_interrupt();
            if let Some(event) = self.bus.break_hit.take() {
                return (Stop::Watched(event, self.cpu.pc), done);
            }
            let at = self.cpu.pc;
            self.step_instruction();
            let done = done_now(&self.bus);

            if let Some(event) = self.bus.break_hit.take() {
                return (Stop::Watched(event, at), done);
            }
            if self.bus.slow.enabled && self.bus.slow.hit {
                return (Stop::SlowDraw, done);
            }
            let pc = self.cpu.pc;
            if self.temp_bp == Some(pc) {
                self.temp_bp = None;
                return (Stop::Breakpoint(pc), done);
            }
            if self.breakpoints.contains(&pc) {
                return (Stop::Breakpoint(pc), done);
            }
        }
        (Stop::Budget, done_now(&self.bus).min(fetches))
    }

    /// True when the instruction at `pc` is one that "step over" should run to
    /// completion rather than enter.
    pub fn is_step_over_target(&self, pc: u16) -> bool {
        let op = self.bus.peek(pc);
        match op {
            // CALL nn, CALL cc,nn, RST n
            0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc | 0xe4 | 0xec | 0xf4 | 0xfc => true,
            0xc7 | 0xcf | 0xd7 | 0xdf | 0xe7 | 0xef | 0xf7 | 0xff => true,
            // The repeating block instructions.
            0xed => matches!(
                self.bus.peek(pc.wrapping_add(1)),
                0xb0 | 0xb1 | 0xb2 | 0xb3 | 0xb8 | 0xb9 | 0xba | 0xbb
            ),
            _ => false,
        }
    }
}
