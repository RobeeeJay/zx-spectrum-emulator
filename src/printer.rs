//! The ZX Printer, and the Alphacom 32 that answers on the same port.
//!
//! A ZX Printer burns dots off aluminium-coated paper with two styluses on a
//! belt, one of which is always crossing the paper. The machine does not send
//! it a line: it watches the encoder, which says when the stylus has reached
//! the next dot, and switches the stylus on or off in time. So the printer is
//! emulated as where the stylus has got to, worked out from how long the motor
//! has been running — 440 T-states a dot slow, 220 fast, 384 positions a line
//! of which 256 are on the paper. This is Fuse's `printer.c` (after Ian
//! Collier's xz80) moved onto the machine's own T-state clock.
//!
//! Port $FB, decoded on A2 alone. Read: bit 0 is the encoder (the stylus has
//! moved on since the last write), bit 6 low says a printer is there, bit 7
//! that the stylus is at the left edge. Write: bit 7 powers the stylus, bit 2
//! stops the motor and bit 1 slows it.
//!
//! The Alphacom 32 is a thermal printer that plugs in the same way and is
//! driven by the same ROM routines, so to the machine it is the same device.
//! Its own timing has not been measured here; it runs at the ZX Printer's.

/// Dots across the paper.
pub const DOTS: usize = 256;
/// Bytes a line takes, a bit a dot.
pub const LINE_BYTES: usize = DOTS / 8;

/// How long the motor may have been running before the paper that came out
/// is counted: without a limit, a program that starts the motor and forgets
/// it would feed out miles of blank paper at the next write.
const MAX_FRAMES: u64 = 400;

/// What the printout is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Paper {
    /// The ZX Printer's: silver, with the print where the coating was burnt
    /// away to the black underneath.
    #[default]
    Metallised,
    /// The Alphacom's: off-white, printed by heat in a blue-black that was
    /// never quite even and faded.
    Thermal,
}

#[derive(Clone, Debug)]
pub struct ZxPrinter {
    /// Everything printed, top line first.
    pub lines: Vec<[u8; LINE_BYTES]>,
    pub paper: Paper,
    /// 0 stopped, 1 slow, 2 fast.
    speed: i64,
    /// A speed asked for part-way through a line, taken up at the next one.
    new_speed: i64,
    /// When the current line's stylus set off, 64 positions before the paper.
    start: u64,
    /// The dot the stylus was at when last written to; -1 before the paper.
    pixel: i64,
    stylus: bool,
    line: [bool; DOTS],
}

impl ZxPrinter {
    pub fn new(paper: Paper) -> ZxPrinter {
        ZxPrinter {
            lines: Vec::new(),
            paper,
            speed: 0,
            new_speed: 0,
            start: 0,
            pixel: -1,
            stylus: false,
            line: [false; DOTS],
        }
    }

    /// Whether the port at this address is the printer's.
    pub fn decodes(port: u16) -> bool {
        port & 0x0004 == 0
    }

    pub fn running(&self) -> bool {
        self.speed != 0
    }

    fn elapsed(&self, now: u64, frame_t: u64) -> i64 {
        now.saturating_sub(self.start).min(MAX_FRAMES * frame_t) as i64
    }

    pub fn read(&self, now: u64, frame_t: u64) -> u8 {
        if self.speed == 0 {
            return 0x3E;
        }
        let mut cpp = 440 / self.speed;
        let mut x = self.elapsed(now, frame_t) / cpp - 64;
        let mut pix = self.pixel;
        let mut sp = self.new_speed;
        // On a later line than the last write: the stylus is wherever that
        // line has got to.
        while x > 320 {
            pix = -1;
            x -= 384;
            if sp != 0 {
                x = (x + 64) * cpp;
                cpp = 440 / sp;
                x = x / cpp - 64;
                sp = 0;
            }
        }
        let mut answer = if (x > -10 && x < 0) || self.stylus {
            0xBE
        } else {
            0x3E
        };
        if x > pix {
            answer |= 1;
        }
        answer
    }

    pub fn write(&mut self, now: u64, frame_t: u64, value: u8) {
        if self.speed == 0 {
            if value & 4 == 0 {
                self.speed = if value & 2 != 0 { 1 } else { 2 };
                self.start = now;
                self.stylus = value & 0x80 != 0;
                self.pixel = -1;
            }
            return;
        }
        let mut cpp = 440 / self.speed;
        let mut x = self.elapsed(now, frame_t) / cpp - 64;
        // The stylus has been doing what it was last told since the last
        // write, all the way to here.
        for i in self.pixel.max(0)..x.min(DOTS as i64) {
            self.line[i as usize] = self.stylus;
        }
        if x >= DOTS as i64 && self.pixel < DOTS as i64 {
            self.output_line();
        }
        while x >= 320 {
            self.start += (cpp * 384) as u64;
            x -= 384;
            if self.new_speed != 0 {
                self.speed = self.new_speed;
                self.new_speed = 0;
                x = (x + 64) * cpp;
                cpp = 440 / self.speed;
                x = x / cpp - 64;
            }
            for i in 0..x.clamp(0, DOTS as i64) {
                self.line[i as usize] = self.stylus;
            }
            if x >= DOTS as i64 {
                self.output_line();
            }
        }
        if x < 0 {
            x = -1;
        }
        if value & 4 != 0 {
            // The motor stops, and a line part-way across is finished as it
            // stood.
            if (0..DOTS as i64).contains(&x) {
                for i in x..DOTS as i64 {
                    self.line[i as usize] = self.stylus;
                }
                self.output_line();
            }
            self.speed = 0;
            self.stylus = false;
        } else {
            self.pixel = x;
            self.stylus = value & 0x80 != 0;
            let speed = if value & 2 != 0 { 1 } else { 2 };
            if x < 0 {
                self.speed = speed;
            } else {
                self.new_speed = if speed == self.speed { 0 } else { speed };
            }
        }
    }

    /// Stop the motor where it is, as a reset does. The clock is going back
    /// to zero, so nothing is worked out against it.
    pub fn halt(&mut self) {
        self.speed = 0;
        self.new_speed = 0;
        self.stylus = false;
        self.pixel = -1;
    }

    fn output_line(&mut self) {
        let mut out = [0u8; LINE_BYTES];
        for (i, byte) in out.iter_mut().enumerate() {
            for bit in 0..8 {
                if self.line[i * 8 + bit] {
                    *byte |= 0x80 >> bit;
                }
            }
        }
        self.lines.push(out);
    }

    /// Read the printout back as text, through a font of 96 characters from
    /// space, eight bytes each — the ROM's at $3D00.
    ///
    /// A row of text is eight lines of dots, but the paper between rows can
    /// be any height, so each place is tried: eight lines that read as
    /// characters are a row, and the reading moves on past them.
    pub fn text(&self, font: &[u8]) -> Vec<String> {
        let mut rows = Vec::new();
        let mut at = 0;
        while at + 8 <= self.lines.len() {
            let row: String = (0..LINE_BYTES)
                .map(|col| {
                    let cell: Vec<u8> = (0..8).map(|y| self.lines[at + y][col]).collect();
                    (0..96)
                        .find(|c| font.get(c * 8..c * 8 + 8) == Some(&cell[..]))
                        .map_or('?', |c| (32 + c as u8) as char)
                })
                .collect();
            let blank = row.chars().all(|c| c == ' ' || c == '?');
            if blank {
                at += 1;
            } else {
                rows.push(row.trim_end().to_string());
                at += 8;
            }
        }
        rows
    }
}

/// A small, fixed scatter for the paper's grain, so the same printout always
/// comes out the same.
fn grain(x: usize, y: usize) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Lines of a printout as RGBA, a pixel a dot, on the paper given. `first`
/// is where they start in the printout, so the paper's grain carries on
/// across pieces rendered separately.
pub fn render(lines: &[[u8; LINE_BYTES]], first: usize, paper: Paper) -> Vec<u8> {
    let mut out = Vec::with_capacity(lines.len() * DOTS * 4);
    for (i, line) in lines.iter().enumerate() {
        let y = first + i;
        for x in 0..DOTS {
            let dot = line[x / 8] & (0x80 >> (x % 8)) != 0;
            let g = grain(x, y);
            let rgb = match paper {
                Paper::Metallised => {
                    // Brushed along the feed: streaks that run down the paper,
                    // a little speckle over them.
                    let streak = (grain(x, 0) % 9) as f32 - 4.0;
                    let speck = (g % 5) as f32 - 2.0;
                    let base = 186.0 + streak + speck;
                    if dot {
                        let d = 34.0 + (g % 7) as f32;
                        [d as u8, d as u8, (d + 3.0) as u8]
                    } else {
                        [base as u8, (base + 2.0) as u8, (base + 5.0) as u8]
                    }
                }
                Paper::Thermal => {
                    // Off-white gone slightly yellow, and heat that did not
                    // reach every dot the same: each element of the head a
                    // little different, each line a little different again.
                    let paper = [238, 232, 214];
                    let tint = (g % 7) as f32 / 255.0;
                    let paper = mix(paper, [226, 218, 192], 0.15 + tint);
                    if dot {
                        let head = (grain(x, 1) % 100) as f32 / 100.0;
                        let pass = (grain(0, y) % 100) as f32 / 100.0;
                        let density = 0.62 + 0.22 * head + 0.1 * pass;
                        mix(paper, [30, 34, 96], density)
                    } else {
                        paper
                    }
                }
            };
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xFF]);
        }
    }
    out
}
