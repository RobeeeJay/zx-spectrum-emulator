//! Cycle-accurate Z80 CPU core.
//!
//! Timing is not counted inside the CPU: every bus access tells the [`Bus`]
//! implementation how many T-states it takes and at which address, so the
//! machine can apply ULA contention at the right moment inside an instruction.

pub mod alu;
pub mod exec;
pub mod tables;

pub mod flags {
    pub const CF: u8 = 0x01;
    pub const NF: u8 = 0x02;
    pub const PF: u8 = 0x04;
    pub const F3: u8 = 0x08;
    pub const HF: u8 = 0x10;
    pub const F5: u8 = 0x20;
    pub const ZF: u8 = 0x40;
    pub const SF: u8 = 0x80;
}

/// Everything the CPU can talk to. All methods are responsible for advancing
/// the machine's T-state counter by the documented amount.
pub trait Bus {
    /// M1 opcode fetch: 4 T-states, contended at `addr`.
    fn fetch_op(&mut self, addr: u16) -> u8;
    /// Normal memory read: 3 T-states, contended at `addr`.
    fn read(&mut self, addr: u16) -> u8;
    /// A byte of the instruction itself rather than of the data it works on:
    /// the `nn` of `LD HL,nn`, the displacement of `LD A,(IX+d)`. The Z80
    /// reads these without an M1 cycle, so they are timed exactly like a
    /// normal read and by default are one — but anything watching what is code
    /// and what is data needs telling them apart, or every immediate operand
    /// in the program reads back as a two-byte table.
    fn read_operand(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }
    /// Normal memory write: 3 T-states, contended at `addr`.
    fn write(&mut self, addr: u16, value: u8);
    /// Internal CPU cycles that still put `addr` on the address bus.
    /// Contention is applied `times` times, one T-state each.
    fn contend(&mut self, addr: u16, times: u32);
    /// IN from a port: 4 T-states with the 48K ULA's split contention pattern.
    fn io_read(&mut self, port: u16) -> u8;
    /// OUT to a port: 4 T-states with the same pattern.
    fn io_write(&mut self, port: u16, value: u8);
    /// The refresh half of an M1 cycle: the CPU puts I:R on the address bus
    /// while the opcode is decoded. Nothing is read, and on most machines
    /// nothing watches — but the Spectrum's ULA does, and an address pointing
    /// into the screen's own RAM is what makes it snow.
    fn refresh(&mut self, _addr: u16) {}
    /// Peek without timing or access tracking; used by the debugger.
    fn peek(&self, addr: u16) -> u8;
}

/// Which register pair the current instruction treats as "HL".
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Idx {
    Hl,
    Ix,
    Iy,
}

#[derive(Clone)]
pub struct Z80 {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,

    pub a_: u8,
    pub f_: u8,
    pub b_: u8,
    pub c_: u8,
    pub d_: u8,
    pub e_: u8,
    pub h_: u8,
    pub l_: u8,

    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,

    pub i: u8,
    /// Refresh register; bit 7 is kept separately by the real chip's counter.
    pub r: u8,
    pub r7: u8,

    pub iff1: bool,
    pub iff2: bool,
    pub im: u8,
    pub halted: bool,
    /// M1 cycles spent halted, for measuring how much of a frame a program
    /// spends waiting for the interrupt.
    pub halted_fetches: u64,

    /// MEMPTR / WZ, observable through `BIT n,(HL)` and block I/O.
    pub wz: u16,
    /// F as left by the previous instruction, or 0 if it did not touch flags.
    /// Drives the undocumented F3/F5 behaviour of SCF/CCF.
    pub q: u8,
    prev_q: u8,

    /// Set when an EI has just executed: interrupts stay blocked for one
    /// instruction so `EI : RET` cannot be interrupted between the two.
    pub defer_int: bool,

    /// Incremented once per executed instruction; handy for the debugger.
    pub instructions: u64,
}

impl Default for Z80 {
    fn default() -> Self {
        Self::new()
    }
}

impl Z80 {
    pub fn new() -> Self {
        Z80 {
            a: 0xff,
            f: 0xff,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            a_: 0xff,
            f_: 0xff,
            b_: 0,
            c_: 0,
            d_: 0,
            e_: 0,
            h_: 0,
            l_: 0,
            ix: 0xffff,
            iy: 0xffff,
            sp: 0xffff,
            pc: 0,
            i: 0,
            r: 0,
            r7: 0,
            iff1: false,
            iff2: false,
            im: 0,
            halted: false,
            halted_fetches: 0,
            wz: 0,
            q: 0,
            prev_q: 0,
            defer_int: false,
            instructions: 0,
        }
    }

    pub fn reset(&mut self) {
        let fresh = Z80::new();
        *self = fresh;
    }

    // ---- register pair accessors -------------------------------------------

    #[inline]
    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f])
    }
    #[inline]
    pub fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = v as u8;
    }
    #[inline]
    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }
    #[inline]
    pub fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }
    #[inline]
    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }
    #[inline]
    pub fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }
    #[inline]
    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }
    #[inline]
    pub fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }
    #[inline]
    pub fn ir(&self) -> u16 {
        u16::from_be_bytes([self.i, (self.r & 0x7f) | self.r7])
    }
    #[inline]
    pub fn r_full(&self) -> u8 {
        (self.r & 0x7f) | self.r7
    }

    /// Value of the index register selected by `idx`.
    #[inline]
    pub fn idx_val(&self, idx: Idx) -> u16 {
        match idx {
            Idx::Hl => self.hl(),
            Idx::Ix => self.ix,
            Idx::Iy => self.iy,
        }
    }
    #[inline]
    pub fn set_idx(&mut self, idx: Idx, v: u16) {
        match idx {
            Idx::Hl => self.set_hl(v),
            Idx::Ix => self.ix = v,
            Idx::Iy => self.iy = v,
        }
    }

    // ---- flag helpers ------------------------------------------------------

    #[inline]
    pub fn flag(&self, mask: u8) -> bool {
        self.f & mask != 0
    }
    /// Record that this instruction wrote the flags (for SCF/CCF's F3/F5).
    #[inline]
    pub fn touched_flags(&mut self) {
        self.q = self.f;
    }

    // ---- fetch helpers -----------------------------------------------------

    #[inline]
    fn inc_r(&mut self) {
        self.r = (self.r & 0x80) | ((self.r.wrapping_add(1)) & 0x7f);
    }

    #[inline]
    pub fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let op = bus.fetch_op(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.refresh(bus);
        self.inc_r();
        op
    }

    /// Tell the bus what is on the address bus while the opcode is decoded:
    /// the I register as the high byte and R as the low one, before R is
    /// stepped on.
    ///
    /// Only when I points into the lower 16K, which is the only case any bus
    /// here cares about — a Spectrum's ULA sharing that RAM. The test is a
    /// register compare against a call and a division on every instruction
    /// the machine executes; without it a screenful of NOPs runs a fifth
    /// slower for a thing that almost never happens.
    #[inline]
    fn refresh(&self, bus: &mut impl Bus) {
        if self.i & 0xc0 == 0x40 {
            bus.refresh(((self.i as u16) << 8) | self.r as u16);
        }
    }

    #[inline]
    pub fn imm8(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read_operand(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    #[inline]
    pub fn imm16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.imm8(bus) as u16;
        let hi = self.imm8(bus) as u16;
        (hi << 8) | lo
    }

    #[inline]
    pub fn read16(&mut self, bus: &mut impl Bus, addr: u16) -> u16 {
        let lo = bus.read(addr) as u16;
        let hi = bus.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    #[inline]
    pub fn write16(&mut self, bus: &mut impl Bus, addr: u16, v: u16) {
        bus.write(addr, v as u8);
        bus.write(addr.wrapping_add(1), (v >> 8) as u8);
    }

    #[inline]
    pub fn push16(&mut self, bus: &mut impl Bus, v: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, v as u8);
    }

    #[inline]
    pub fn pop16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    /// F left by the previous instruction, or 0 if it left flags alone.
    #[inline]
    pub fn q_prev(&self) -> u8 {
        self.prev_q
    }

    /// Execute one instruction (including any prefixes).
    pub fn step(&mut self, bus: &mut impl Bus) {
        self.prev_q = self.q;
        self.q = 0;
        self.defer_int = false;

        if self.halted {
            self.halted_fetches += 1;
            // The halt state is not a re-run of the HALT opcode: the CPU keeps
            // performing M1 cycles so refresh continues, with PC — the address
            // *after* the HALT — on the bus. That matters on a Spectrum, where
            // a HALT at $7FFF refreshes from the uncontended $8000 while one at
            // $4000 is contended on every cycle.
            bus.fetch_op(self.pc);
            self.refresh(bus);
            self.inc_r();
            self.instructions = self.instructions.wrapping_add(1);
            return;
        }

        let op = self.fetch(bus);
        exec::execute(self, bus, op, Idx::Hl);
        self.instructions = self.instructions.wrapping_add(1);
    }

    /// Offer a maskable interrupt. Returns true if it was accepted.
    pub fn interrupt(&mut self, bus: &mut impl Bus) -> bool {
        if !self.iff1 || self.defer_int {
            return false;
        }
        // Leaving the halt state costs nothing: PC already points at the
        // instruction after the HALT, which is what gets pushed.
        self.halted = false;
        self.iff1 = false;
        self.iff2 = false;
        self.inc_r();

        match self.im {
            // IM 0 on a Spectrum sees 0xFF on the bus, which is RST 38h.
            0 | 1 => {
                bus.contend(self.ir(), 7);
                self.push16(bus, self.pc);
                self.pc = 0x0038;
                self.wz = 0x0038;
            }
            _ => {
                bus.contend(self.ir(), 7);
                self.push16(bus, self.pc);
                // Vector byte is whatever is floating on the bus: 0xFF here.
                let vector = ((self.i as u16) << 8) | 0x00ff;
                let target = self.read16(bus, vector);
                self.pc = target;
                self.wz = target;
            }
        }
        self.prev_q = self.q;
        self.q = 0;
        true
    }

    /// Non-maskable interrupt: 11 T-states, always taken.
    pub fn nmi(&mut self, bus: &mut impl Bus) {
        self.halted = false;
        self.iff2 = self.iff1;
        self.iff1 = false;
        self.inc_r();
        bus.contend(self.ir(), 5);
        self.push16(bus, self.pc);
        self.pc = 0x0066;
        self.wz = 0x0066;
    }
}
