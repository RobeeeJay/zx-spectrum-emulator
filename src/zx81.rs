//! ZX81, in 1K and 16K forms.
//!
//! The ZX81 has no video hardware worth the name: the picture is produced by
//! the CPU walking the display file, with the ULA watching the bus. Whenever an
//! opcode is fetched from an address with A15 set and bit 6 clear, the ULA feeds
//! the CPU a NOP instead and uses the byte it saw as a character code, fetching
//! that character's bitmap from the ROM during the refresh half of the same M1
//! cycle and shifting it out as eight pixels.
//!
//! Emulating it at that level — rather than drawing a character grid — is what
//! makes it cycle exact, and is why programs that abuse the mechanism for
//! high-resolution graphics work without any special handling.

use crate::tape::Tape;
use crate::z80::{Bus, Z80};

/// The ZX81's Z80A runs a little faster than a Spectrum's.
pub const CPU_HZ: f64 = 3_250_000.0;
/// T-states in one television line.
pub const LINE_T: u32 = 207;
/// Lines in a full frame. The ROM decides this, but a sane display is 312.
pub const LINES: u32 = 312;
/// Two pixels per T-state, as on the Spectrum.
pub const RASTER_W: usize = LINE_T as usize * 2;
/// A little taller than a nominal frame, so an overrunning program still has
/// somewhere to draw.
pub const RASTER_H: usize = 340;

/// How much memory is fitted.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Ram {
    /// The unexpanded machine: 1K, mirrored through the whole 16K page.
    K1,
    /// The usual RAM pack.
    K16,
}

impl Ram {
    pub fn size(&self) -> usize {
        match self {
            Ram::K1 => 1024,
            Ram::K16 => 16384,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Ram::K1 => "ZX81 1K",
            Ram::K16 => "ZX81 16K",
        }
    }
}

pub struct Zx81Bus {
    /// The ROM: 8K on a stock machine, mirrored into the second 8K of the
    /// bottom page. Some machines carry a 16K ROM that fills the page instead,
    /// so the mask follows whatever was loaded.
    pub rom: Vec<u8>,
    pub rom_mask: u16,
    pub ram: Vec<u8>,
    pub ram_mask: u16,

    /// T-states since power-on.
    pub tstates: u64,
    /// T-states since the last horizontal sync.
    pub t_in_line: u32,
    /// Television line since the last vertical sync.
    pub line: u32,
    /// Three-bit counter selecting the pixel row within a character.
    pub lcnt: u8,
    pub vsync: bool,
    /// The NMI generator, which the ROM uses to time the borders in SLOW mode.
    pub nmi_on: bool,
    pub nmi_pending: bool,
    pub frame: u64,

    /// One byte per pixel, 1 meaning black. The picture being drawn, and the
    /// one before it so a part-drawn frame still shows something.
    pub fb: Vec<u8>,
    pub fb_prev: Vec<u8>,

    /// Keyboard matrix, one byte per half-row, bit clear = key down.
    pub keys: [u8; 8],
    /// Copy of the CPU's I register, which points at the character set.
    pub i: u8,
    /// Whether the CPU is halted; the ULA blanks the rest of the line.
    pub halted: bool,
    /// Bytes the ULA turned into pixels this frame, for the tests and the UI.
    pub video_bytes: u32,

    /// The tape in the deck, if any. The ZX81 reads it on bit 7 of port $FE.
    pub tape: Option<Tape>,
    /// Run faster while the tape plays: the ZX81 loads at about 50 bytes a
    /// second, so a game is several minutes of real time.
    pub tape_boost: bool,
    /// Scratch space for edges on their way out of the tape.
    tape_edge_scratch: Vec<(u64, bool)>,
}

impl Zx81Bus {
    pub fn new(ram: Ram) -> Zx81Bus {
        Zx81Bus {
            rom: vec![0xff; 8192],
            rom_mask: 0x1fff,
            ram: vec![0; ram.size()],
            ram_mask: (ram.size() - 1) as u16,
            tstates: 0,
            t_in_line: 0,
            line: 0,
            lcnt: 0,
            vsync: false,
            nmi_on: false,
            nmi_pending: false,
            frame: 0,
            fb: vec![0; RASTER_W * RASTER_H],
            fb_prev: vec![0; RASTER_W * RASTER_H],
            keys: [0xff; 8],
            i: 0x1e,
            halted: false,
            video_bytes: 0,
            tape: None,
            tape_boost: true,
            tape_edge_scratch: Vec::new(),
        }
    }

    /// Load a ROM image. An 8K image is mirrored through the bottom page, as
    /// on a stock ZX81; a 16K one fills the page on its own.
    pub fn load_rom(&mut self, data: &[u8]) {
        let size = if data.len() > 8192 { 16384 } else { 8192 };
        self.rom = vec![0xff; size];
        let n = data.len().min(size);
        self.rom[..n].copy_from_slice(&data[..n]);
        self.rom_mask = (size - 1) as u16;
    }

    /// Memory is not decoded above A14: the ROM appears twice in the bottom
    /// page, RAM repeats through the top of its page, and the whole lot is
    /// mirrored again above $8000 — which is what lets the display routine
    /// execute the display file with A15 set.
    #[inline]
    pub fn mem(&self, addr: u16) -> u8 {
        if addr & 0x4000 == 0 {
            self.rom[(addr & self.rom_mask) as usize]
        } else {
            self.ram[(addr & self.ram_mask) as usize]
        }
    }

    #[inline]
    pub fn poke(&mut self, addr: u16, value: u8) {
        if addr & 0x4000 != 0 {
            let i = (addr & self.ram_mask) as usize;
            self.ram[i] = value;
        }
    }

    /// Advance the clock, generating horizontal sync — and an NMI with it when
    /// the generator is on.
    fn tick(&mut self, t: u32) {
        self.tstates += t as u64;
        self.t_in_line += t;
        while self.t_in_line >= LINE_T {
            self.t_in_line -= LINE_T;
            self.line += 1;
            if !self.vsync {
                self.lcnt = (self.lcnt + 1) & 7;
            }
            if self.nmi_on {
                self.nmi_pending = true;
            }
            if self.line as usize >= RASTER_H {
                // A program that never syncs would otherwise draw off the end.
                self.end_frame();
            }
        }
    }

    /// Finish the picture and start the next one.
    pub fn end_frame(&mut self) {
        std::mem::swap(&mut self.fb, &mut self.fb_prev);
        self.fb.iter_mut().for_each(|p| *p = 0);
        self.line = 0;
        self.frame += 1;
        self.video_bytes = 0;
    }

    /// Turn a character code into eight pixels at the current raster position.
    fn emit_character(&mut self, ch: u8) {
        // The ULA fetches the bitmap during the refresh half of the M1 cycle,
        // addressing it with I, the character code and the line counter.
        let addr = ((self.i as u16) << 8) | ((ch as u16 & 0x3f) << 3) | self.lcnt as u16;
        let mut bits = self.mem(addr);
        if ch & 0x80 != 0 {
            bits = !bits; // inverse video
        }

        let y = self.line as usize;
        let x0 = self.t_in_line as usize * 2;
        if y < RASTER_H {
            for bit in 0..8usize {
                let x = x0 + bit;
                if x < RASTER_W {
                    self.fb[y * RASTER_W + x] = u8::from(bits & (0x80 >> bit) != 0);
                }
            }
        }
        self.video_bytes += 1;
    }

    /// Keyboard: a read of port $FE returns the half-rows selected by the high
    /// address byte, and starts the vertical sync when the NMI generator is
    /// off — which is how the ROM times the picture.
    fn keyboard(&self, port: u16, tape: bool) -> u8 {
        let mut result = 0x1f;
        for row in 0..8 {
            if port & (1 << (8 + row)) == 0 {
                result &= self.keys[row] & 0x1f;
            }
        }
        // Bit 6 is the 50/60 Hz jumper, high for a 50 Hz machine. Bit 7 is the
        // tape input, which the ROM's loader tests with RLA at $035B.
        result | 0x40 | (u8::from(tape) << 7)
    }

    // ---- tape --------------------------------------------------------------

    pub fn tape_playing(&self) -> bool {
        self.tape.as_ref().is_some_and(|t| t.playing)
    }

    /// Tape level now, advancing the tape to the present. The edges are
    /// collected but not mixed anywhere: the ZX81 has no sound hardware, so
    /// they only feed the tape window's oscilloscope.
    fn tape_level(&mut self) -> bool {
        let now = self.tstates;
        let Some(tape) = self.tape.as_mut() else {
            return false;
        };
        let level = tape.level_at(now);
        let mut edges = std::mem::take(&mut self.tape_edge_scratch);
        edges.clear();
        tape.take_pending_edges(&mut edges);
        self.tape_edge_scratch = edges;
        level
    }

    /// Keep the tape moving even while the CPU is not polling the port, so it
    /// does not stall between the loader's reads.
    pub fn tape_tick(&mut self) {
        if self.tape_playing() {
            self.tape_level();
        }
    }

    pub fn start_vsync(&mut self) {
        // While the sync is low the line counter is held in reset.
        self.vsync = true;
        self.lcnt = 0;
    }

    /// The interrupt marks the end of a scan line, and the ULA starts its
    /// horizontal sync from it — which is what keeps the characters of each
    /// row landing in the same place across the picture.
    pub fn hsync_from_interrupt(&mut self) {
        self.t_in_line = 0;
        self.line += 1;
        if !self.vsync {
            self.lcnt = (self.lcnt + 1) & 7;
        }
        if self.line as usize >= RASTER_H {
            self.end_frame();
        }
    }

    pub fn stop_vsync(&mut self) {
        if self.vsync {
            self.vsync = false;
            self.end_frame();
        }
    }
}

impl Bus for Zx81Bus {
    fn fetch_op(&mut self, addr: u16) -> u8 {
        let byte = self.mem(addr);
        // The ULA only interferes above $8000, and only for codes with bit 6
        // clear; a HALT (bit 6 set) is executed properly and ends the line.
        let is_display_byte = addr & 0x8000 != 0 && byte & 0x40 == 0 && !self.halted;
        if is_display_byte {
            self.emit_character(byte);
        }
        self.tick(4);
        if is_display_byte {
            0x00 // the CPU sees a NOP
        } else {
            byte
        }
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.tick(3);
        self.mem(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.tick(3);
        self.poke(addr, value);
    }

    fn contend(&mut self, _addr: u16, times: u32) {
        self.tick(times);
    }

    fn io_read(&mut self, port: u16) -> u8 {
        self.tick(4);
        if port & 1 == 0 {
            // Reading the keyboard also starts the vertical sync, unless the
            // NMI generator is running the show.
            if !self.nmi_on {
                self.start_vsync();
            }
            let tape = self.tape_level();
            self.keyboard(port, tape)
        } else {
            0xff
        }
    }

    fn io_write(&mut self, port: u16, _value: u8) {
        self.tick(4);
        // Any write ends the vertical sync.
        self.stop_vsync();
        if port & 2 == 0 {
            self.nmi_on = false; // $FD
        } else if port & 1 == 0 {
            self.nmi_on = true; // $FE
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        self.mem(addr)
    }
}

/// How much of the ZX81's raster to show. The picture the ROM draws is 256x192
/// in the middle of a 414x312 raster.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct View {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl View {
    /// The whole raster, for watching what a program is really doing.
    pub const OVERSCAN: View = View {
        x: 0,
        y: 0,
        w: RASTER_W,
        h: 312,
    };
    /// The picture plus a modest border.
    pub const CROPPED: View = View {
        x: 60,
        y: 24,
        w: 320,
        h: 240,
    };

    pub fn buffer_len(&self) -> usize {
        self.w * self.h * 4
    }
}

/// The ZX81 shows black on white, and has no colour to speak of.
pub const WHITE: [u8; 3] = [0xe8, 0xe8, 0xe8];
pub const BLACK: [u8; 3] = [0x10, 0x10, 0x10];

impl Zx81Bus {
    /// Draw the last completed picture into an RGBA buffer.
    pub fn render(&self, view: View, out: &mut [u8]) {
        for row in 0..view.h {
            for col in 0..view.w {
                let (sx, sy) = (view.x + col, view.y + row);
                let ink = sx < RASTER_W
                    && sy < RASTER_H
                    && self.fb_prev[sy * RASTER_W + sx] != 0;
                let c = if ink { BLACK } else { WHITE };
                let i = (row * view.w + col) * 4;
                out[i] = c[0];
                out[i + 1] = c[1];
                out[i + 2] = c[2];
                out[i + 3] = 0xff;
            }
        }
    }
}

pub struct Zx81 {
    pub cpu: Z80,
    pub bus: Zx81Bus,
    pub ram: Ram,
    pub breakpoints: Vec<u16>,
}

impl Zx81 {
    pub fn new(ram: Ram) -> Zx81 {
        let mut machine = Zx81 {
            cpu: Z80::new(),
            bus: Zx81Bus::new(ram),
            ram,
            breakpoints: Vec::new(),
        };
        machine.reset();
        machine
    }

    pub fn load_rom(&mut self, data: &[u8]) {
        self.bus.load_rom(data);
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.cpu.im = 1;
        self.bus.ram.iter_mut().for_each(|b| *b = 0);
        self.bus.tstates = 0;
        self.bus.t_in_line = 0;
        self.bus.line = 0;
        self.bus.lcnt = 0;
        self.bus.vsync = false;
        self.bus.nmi_on = false;
        self.bus.nmi_pending = false;
        self.bus.fb.iter_mut().for_each(|p| *p = 0);
        self.bus.fb_prev.iter_mut().for_each(|p| *p = 0);
    }

    /// Execute one instruction, with the interrupt and NMI the ULA generates.
    pub fn step_instruction(&mut self) {
        // The ULA needs to know where the character set is and whether the CPU
        // is sitting in a HALT.
        self.bus.i = self.cpu.i;
        self.bus.halted = self.cpu.halted;

        if self.bus.nmi_pending {
            self.bus.nmi_pending = false;
            self.cpu.nmi(&mut self.bus);
            self.bus.halted = self.cpu.halted;
        }

        // The ZX81 takes its interrupt from bit 6 of the refresh register
        // falling, which is how the ROM counts out a character row.
        let r_before = self.cpu.r_full();
        self.cpu.step(&mut self.bus);
        let r_after = self.cpu.r_full();
        if r_before & 0x40 != 0 && r_after & 0x40 == 0 {
            self.bus.i = self.cpu.i;
            if self.cpu.interrupt(&mut self.bus) {
                self.bus.hsync_from_interrupt();
            }
        }
    }

    /// Run for roughly `budget` T-states, stopping early on a breakpoint.
    pub fn run(&mut self, budget: u64) -> Option<u16> {
        let end = self.bus.tstates + budget;
        while self.bus.tstates < end {
            self.step_instruction();
            let pc = self.cpu.pc;
            if !self.breakpoints.is_empty() && self.breakpoints.contains(&pc) {
                return Some(pc);
            }
        }
        None
    }

    /// T-states in a frame at the ZX81's clock, for pacing.
    pub fn frame_t(&self) -> u64 {
        (LINE_T * LINES) as u64
    }

    /// Load a `.p` file: a straight image of memory from $4009 upwards.
    pub fn load_p(&mut self, data: &[u8]) -> Result<(), String> {
        if data.len() < 16 {
            return Err("that is too short to be a .p file".into());
        }
        if self.ram == Ram::K1 && data.len() > 1024 {
            return Err(format!(
                "this program needs {} bytes; fit the 16K RAM pack",
                data.len()
            ));
        }
        for (i, b) in data.iter().enumerate() {
            let addr = 0x4009u32 + i as u32;
            if addr > 0x7fff {
                break;
            }
            self.bus.poke(addr as u16, *b);
        }
        Ok(())
    }
}
