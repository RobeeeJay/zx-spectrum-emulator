//! The General Instrument SP0256-AL2: the chip that does the talking.
//!
//! It is not a sample player. Inside it is a 2K ROM holding a program for a
//! little microsequencer, and that program's data words are the coefficients
//! of a twelve-pole lattice filter — six two-pole stages — driven either by a
//! train of impulses at the pitch period or by white noise. Speech comes out
//! of the filter, not out of the ROM, which is why the 64 allophones fit in
//! two kilobytes: what is stored is the shape of a mouth over time.
//!
//! An allophone is spoken by writing its number to the address load register.
//! The sequencer jumps into the ROM at that entry, walks a chain of frames —
//! each one a set of coefficients and a repeat count — and halts when the
//! chain ends. While it is walking, the chip is busy, which is the line the
//! µSpeech's ROM polls before sending the next allophone.
//!
//! The microsequencer's opcodes, the two data tables and the coefficient
//! quantisation table are from Joseph Zbiciak's reverse engineering of the
//! chip, as published in MAME's `sp0256.cpp` (BSD-3-Clause, Joseph Zbiciak and
//! Tim Lindner). The implementation here is written against that description
//! rather than copied from it; the tables are the chip's own data and are
//! transcribed as they stand.
//!
//! Three things are worth knowing before changing anything here.
//!
//! The sequencer addresses its ROM from `$1000`, not from zero: the AL2's 2K
//! sits above where a host's own ROM would go. Loading a dump at zero leaves
//! every allophone jumping into empty space, and the chip halts one sample
//! later without a sound — which is exactly what it did at first.
//!
//! Bit order matters and cannot be guessed. Some dumps are stored
//! bit-reversed and some are not, and the way to tell is to run them: with the
//! bytes the right way round every one of the 64 allophones comes out within
//! about 3.5% of its published length, and the wrong way round the chip either
//! halts at once or runs for a second and a half. The dump here is used as it
//! stands.
//!
//! The filter's arithmetic is deliberately narrow — sixteen bits that wrap,
//! and eight bits out — because that is the chip's own. Widening it to stop
//! the overflow would make something that is not an SP0256.

/// How many clocks of the chip's oscillator make one sample: 6 × 4 × 13.
pub const CLOCK_DIVIDER: u32 = 312;

/// The pause and noise periods the sequencer uses in place of a pitch.
const PER_PAUSE: u8 = 64;
const PER_NOISE: i32 = 64;

/// Coefficient quantisation, from the SP0250's data sheet: an eight-bit
/// coefficient names one of these, and the sign says which way round.
const QTBL: [i16; 128] = [
    0, 9, 17, 25, 33, 41, 49, 57, 65, 73, 81, 89, 97, 105, 113, 121, 129, 137, 145, 153, 161, 169,
    177, 185, 193, 201, 209, 217, 225, 233, 241, 249, 257, 265, 273, 281, 289, 297, 301, 305, 309,
    313, 317, 321, 325, 329, 333, 337, 341, 345, 349, 353, 357, 361, 365, 369, 373, 377, 381, 385,
    389, 393, 397, 401, 405, 409, 413, 417, 421, 425, 427, 429, 431, 433, 435, 437, 439, 441, 443,
    445, 447, 449, 451, 453, 455, 457, 459, 461, 463, 465, 467, 469, 471, 473, 475, 477, 479, 481,
    482, 483, 484, 485, 486, 487, 488, 489, 490, 491, 492, 493, 494, 495, 496, 497, 498, 499, 500,
    501, 502, 503, 504, 505, 506, 507, 508, 509, 510, 511,
];

/// How each field of a data block is taken out of the bit stream: length,
/// left shift, which register it lands in, and four flags — delta update,
/// field replace, clear the fifth stage, clear everything.
#[rustfmt::skip]
const DATAFMT: [u16; 177] = [
    0x8000, 0x8008, 0x0108, 0x0208, 0x0308, 0x0408, 0x0508, 0x0608, 0x0708, 0x0808, 0x0908,
    0x0A08, 0x0B08, 0x0C08, 0x0D08, 0x0E08, 0x0F08, 0x8026, 0x0108, 0x0834, 0x0926, 0x0A17,
    0x0B26, 0x0C08, 0x0D08, 0x8026, 0x0108, 0x0816, 0x0917, 0x0A08, 0x0B08, 0x0C08, 0x0D08,
    0x4000, 0x0026, 0x2926, 0x2B26, 0x2D08, 0x4000, 0x0026, 0x2917, 0x2B08, 0x2D08, 0x0000,
    0x0000, 0x1024, 0x1105, 0x1243, 0x1333, 0x1443, 0x1533, 0x1643, 0x1733, 0x1833, 0x1924,
    0x1A14, 0x1B24, 0x1C05, 0x1D05, 0x1024, 0x1105, 0x1214, 0x1324, 0x1414, 0x1524, 0x1614,
    0x1724, 0x1814, 0x1915, 0x1A05, 0x1B05, 0x1C05, 0x1D05, 0x4000, 0x0026, 0x2335, 0x2535,
    0x2735, 0x4000, 0x0026, 0x2326, 0x2526, 0x2726, 0x8026, 0x0108, 0x0243, 0x0335, 0x0443,
    0x0535, 0x0643, 0x0735, 0x0834, 0x0926, 0x0A17, 0x0B26, 0x0E05, 0x0F05, 0x8026, 0x0108,
    0x0216, 0x0326, 0x0416, 0x0526, 0x0616, 0x0726, 0x0816, 0x0917, 0x0A08, 0x0B08, 0x0E05,
    0x0F05, 0x1024, 0x1105, 0x1833, 0x1924, 0x1A14, 0x1B24, 0x1C05, 0x1D05, 0x1024, 0x1105,
    0x1814, 0x1915, 0x1A05, 0x1B05, 0x1C05, 0x1D05, 0x0026, 0x0108, 0x8026, 0x0108, 0x0243,
    0x0335, 0x0443, 0x0535, 0x0643, 0x0735, 0x0834, 0x0926, 0x0A17, 0x0B26, 0x0C08, 0x0D08,
    0x0E05, 0x0F05, 0x8026, 0x0108, 0x0216, 0x0326, 0x0416, 0x0526, 0x0616, 0x0726, 0x0816,
    0x0917, 0x0A08, 0x0B08, 0x0C08, 0x0D08, 0x0E05, 0x0F05, 0x4000, 0x0026, 0x0108, 0x2335,
    0x2535, 0x2735, 0x0E05, 0x0F05, 0x4000, 0x0026, 0x0108, 0x2326, 0x2526, 0x2726, 0x0E05,
    0x0F05
];

/// Where in `DATAFMT` each opcode's block starts and ends, by opcode and mode.
#[rustfmt::skip]
const DF_IDX: [i16; 128] = [
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 17, 22, 17, 24, 25, 30,
    25, 32, 83, 94, 129, 142, 97, 108, 145, 158, 83, 96, 129, 144, 97, 110, 145, 160, 73,
    77, 74, 77, 78, 82, 79, 82, 33, 36, 34, 37, 38, 41, 39, 42, 127, 128, 127, 128, 127,
    128, 127, 128, 1, 14, 1, 16, 1, 14, 1, 16, 45, 56, 45, 58, 59, 70, 59, 72, 161, 166,
    162, 166, 169, 174, 170, 174, 111, 116, 111, 118, 119, 124, 119, 126, 161, 168, 162,
    168, 169, 176, 170, 176, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    0, 0, 0, 0, 0, 0, 0, 0
];

fn bitrev32(v: u32) -> u32 {
    v.reverse_bits()
}

/// The twelve-pole filter and the sequencer that feeds it.
#[derive(Clone)]
pub struct Sp0256 {
    rom: Vec<u8>,
    // The microsequencer.
    pc: u32,
    stack: u32,
    page: u32,
    mode: u8,
    halted: bool,
    /// The load request line: high when the chip will take a new allophone.
    lrq: bool,
    ald: u32,
    silent: bool,
    // The filter.
    r: [u8; 16],
    b_coef: [i16; 6],
    f_coef: [i16; 6],
    z: [[i16; 2]; 6],
    per: u32,
    amp: i32,
    cnt: i32,
    rpt: i32,
    rng: u32,
    interp: bool,
}

impl Sp0256 {
    /// A chip with its ROM in it. Every dump in circulation is bit-reversed,
    /// which is how the chip's own address decoding reads it.
    pub fn new(rom: &[u8]) -> Sp0256 {
        // The sequencer addresses its ROM from $1000: the bottom of its map is
        // where a host's own ROM would go, and the AL2's 2K sits above it.
        // Loading the dump at zero instead leaves every allophone jumping into
        // nothing, and the chip halts a sample later without a sound.
        let mut image = vec![0u8; 0x1000];
        image.extend_from_slice(rom);
        let mut chip = Sp0256 {
            rom: image,
            pc: 0,
            stack: 0,
            page: 0x1000 << 3,
            mode: 0,
            halted: true,
            lrq: true,
            ald: 0,
            silent: true,
            r: [0; 16],
            b_coef: [0; 6],
            f_coef: [0; 6],
            z: [[0; 2]; 6],
            per: 0,
            amp: 0,
            cnt: 0,
            rpt: -1,
            rng: 1,
            interp: false,
        };
        chip.rom.resize(0x10000, 0);
        chip
    }

    /// Say an allophone. A write while the chip is still busy is dropped, as
    /// the hardware drops it: the µSpeech's ROM polls the busy line first.
    pub fn speak(&mut self, allophone: u8) {
        if !self.lrq {
            return;
        }
        self.lrq = false;
        self.ald = u32::from(allophone & 0x3F) << 4;
    }

    /// Whether the chip is talking or has a command waiting.
    pub fn busy(&self) -> bool {
        !self.halted || !self.lrq
    }

    pub fn reset(&mut self) {
        self.halted = true;
        self.lrq = true;
        self.ald = 0;
        self.pc = 0;
        self.stack = 0;
        self.page = 0x1000 << 3;
        self.mode = 0;
        self.silent = true;
        self.rpt = -1;
        self.rng = 1;
        self.r = [0; 16];
        self.z = [[0; 2]; 6];
    }

    /// One sample, at the chip's own rate of clock/312.
    pub fn sample(&mut self) -> i16 {
        loop {
            if self.rpt <= 0 {
                self.micro();
                // A halted chip makes nothing at all, and `micro` has set the
                // repeat count to keep the filter quiet.
                if self.halted && self.silent {
                    return 0;
                }
            }
            if let Some(sample) = self.step() {
                return sample;
            }
        }
    }

    /// Take `len` bits out of the ROM at the program counter.
    fn getb(&mut self, len: u32) -> u32 {
        let idx0 = (self.pc >> 3) as usize;
        let idx1 = ((self.pc + 8) >> 3) as usize;
        let d0 = u32::from(self.rom[idx0 & 0xFFFF]);
        let d1 = u32::from(self.rom[idx1 & 0xFFFF]);
        let data = ((d1 << 8) | d0) >> (self.pc & 7);
        self.pc += len;
        data & ((1 << len) - 1)
    }

    /// One sample out of the filter, or `None` when the frame's repeat count
    /// has run out and the sequencer is wanted again.
    fn step(&mut self) -> Option<i16> {
        // Sixteen bits, and they wrap: the chip's own arithmetic overflows and
        // the sound of it is part of the sound of the chip. Widening this to
        // stop it wrapping makes something that is not an SP0256.
        let mut samp: i16;
        let mut do_int = false;

        if self.per != 0 {
            // A train of impulses at the pitch period: voiced sound.
            if self.cnt <= 0 {
                self.cnt += self.per as i32;
                samp = self.amp as i16;
                self.rpt -= 1;
                do_int = self.interp;
                self.z = [[0i16; 2]; 6];
            } else {
                samp = 0;
                self.cnt -= 1;
            }
        } else {
            // White noise: the unvoiced sounds, /SS/ and the rest.
            self.cnt -= 1;
            if self.cnt <= 0 {
                do_int = self.interp;
                self.cnt = PER_NOISE;
                self.rpt -= 1;
                self.z = [[0i16; 2]; 6];
            }
            let bit = self.rng & 1 != 0;
            self.rng = (self.rng >> 1) ^ if bit { 0x4001 } else { 0 };
            samp = if bit {
                self.amp as i16
            } else {
                -self.amp as i16
            };
        }

        if do_int {
            // The frame is walking towards the next one: amplitude and pitch
            // are nudged by their interpolation registers every period.
            self.r[0] = self.r[0].wrapping_add(self.r[14]);
            self.r[1] = self.r[1].wrapping_add(self.r[15]);
            self.amp = i32::from(self.r[0] & 0x1F) << ((self.r[0] & 0xE0) >> 5);
            self.per = u32::from(self.r[1]);
        }

        if self.rpt <= 0 {
            return None;
        }

        // Six two-pole stages, in the form the application manual gives.
        for j in 0..6 {
            let b = (i32::from(self.b_coef[j]) * i32::from(self.z[j][1])) >> 9;
            let f = (i32::from(self.f_coef[j]) * i32::from(self.z[j][0])) >> 8;
            samp = samp.wrapping_add(b as i16).wrapping_add(f as i16);
            self.z[j][1] = self.z[j][0];
            self.z[j][0] = samp;
        }

        // The chip's own arithmetic: eight bits out, and clipped rather than
        // wrapped.
        Some((samp >> 4).clamp(-128, 127) * 256)
    }

    /// Decode the register set into the filter's working values.
    fn regdec(&mut self) {
        self.amp = i32::from(self.r[0] & 0x1F) << ((self.r[0] & 0xE0) >> 5);
        self.cnt = 0;
        self.per = u32::from(self.r[1]);
        let iq = |x: u8| -> i16 {
            if x & 0x80 != 0 {
                QTBL[(0x7F & x.wrapping_neg()) as usize]
            } else {
                -QTBL[x as usize]
            }
        };
        for i in 0..6 {
            self.b_coef[i] = iq(self.r[2 + 2 * i]);
            self.f_coef[i] = iq(self.r[3 + 2 * i]);
        }
        self.interp = self.r[14] != 0 || self.r[15] != 0;
    }

    /// The microsequencer: run instructions until a frame is loaded with a
    /// repeat count, or until the chip halts.
    fn micro(&mut self) {
        while self.rpt <= 0 {
            // A command waiting, and nothing running: jump to its entry.
            if self.halted && !self.lrq {
                self.pc = self.ald | (0x1000 << 3);
                self.halted = false;
                self.lrq = true;
                self.ald = 0;
                self.r = [0; 16];
            }
            if self.halted {
                self.rpt = 1;
                self.lrq = true;
                self.ald = 0;
                self.r = [0; 16];
                self.silent = true;
                return;
            }

            let immed4 = self.getb(4);
            let opcode = self.getb(4) as u8;
            let mut repeat = 0u32;
            let mut ctrl_xfer = false;

            match opcode {
                // RTS, HALT, or SETPAGE.
                0x0 => {
                    if immed4 != 0 {
                        self.page = bitrev32(immed4) >> 13;
                    } else {
                        let target = self.stack;
                        self.stack = 0;
                        if target == 0 {
                            self.halted = true;
                            self.pc = 0;
                        } else {
                            self.pc = target;
                        }
                        ctrl_xfer = true;
                    }
                }
                // JMP and JSR.
                0xE | 0xD => {
                    let low = self.getb(8);
                    let target = self.page | (bitrev32(immed4) >> 17) | (bitrev32(low) >> 21);
                    ctrl_xfer = true;
                    if opcode == 0xD {
                        self.stack = (self.pc + 7) & !7;
                    }
                    self.pc = target;
                }
                // SETMODE: the mode bits and the top of the repeat count.
                0x1 => {
                    self.mode = (((immed4 & 8) >> 2) | (immed4 & 4) | ((immed4 & 3) << 4)) as u8;
                }
                // Everything else loads a frame, and its repeat count is in
                // the instruction.
                _ => repeat = immed4 | u32::from(self.mode & 0x30),
            }
            if opcode != 1 {
                self.mode &= 0xF;
            }
            if ctrl_xfer {
                continue;
            }
            if repeat == 0 {
                continue;
            }

            self.rpt = repeat as i32 + 1;
            let i = ((opcode as usize) << 3) | (self.mode as usize & 6);
            let (idx0, idx1) = (DF_IDX[i], DF_IDX[i + 1]);
            if idx0 < 0 || idx1 < idx0 {
                // An opcode with no data block behind it: nothing to load.
                continue;
            }

            for entry in idx0..=idx1 {
                let cr = DATAFMT[entry as usize];
                let len = u32::from(cr & 15);
                let shf = u32::from((cr >> 4) & 15);
                let prm = ((cr >> 8) & 15) as usize;
                let delta = cr & 0x1000 != 0;
                let field = cr & 0x2000 != 0;
                let clr5 = cr & 0x4000 != 0;
                let clra = cr & 0x8000 != 0;

                if clra {
                    self.r = [0; 16];
                    self.silent = true;
                }
                if clr5 {
                    self.r[12] = 0;
                    self.r[13] = 0;
                }
                if len == 0 {
                    continue;
                }

                let mut value = self.getb(len) as u8;
                if delta && value & (1 << (len - 1)) != 0 {
                    // Sign-extend into the byte.
                    value |= 0xFFu8 << len;
                }
                if shf != 0 {
                    value = value.wrapping_shl(shf);
                }
                self.silent = false;

                if field {
                    // Replace the top of the register, keeping the low bits.
                    self.r[prm] &= !(0xFFu8.wrapping_shl(shf));
                    self.r[prm] |= value;
                } else if delta {
                    self.r[prm] = self.r[prm].wrapping_add(value);
                } else {
                    self.r[prm] = value;
                }
            }

            // A pause has no pitch of its own, so it is given one to time it.
            if opcode == 0xF {
                self.silent = true;
                self.r[1] = PER_PAUSE;
            }

            self.regdec();
            break;
        }
    }
}
