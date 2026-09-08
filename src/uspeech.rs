//! The Currah µSpeech: a 2K ROM, an SP0256-AL2 and one address that does
//! everything.
//!
//! The box has no ports of its own worth the name. Every access to `$0038` —
//! an opcode fetch, a memory read, a memory write, an `IN` or an `OUT` — turns
//! its ROM on, and the next one turns it off again. That is not an accident:
//! the ULA's interrupt lands on `$0038`, so the interface pages itself in for
//! the interrupt, its own handler runs, and jumping back to `$0038` pages it
//! out and leaves the machine's own handler to run. Fitting one to a machine
//! that is already running is enough; the next interrupt hands it the machine.
//!
//! While it is paged in the bottom 16K is the interface's, not the machine's:
//!
//! * `$0000-$07FF` is its ROM, mirrored again over `$0800-$0FFF`.
//! * `$1000-$1FFF` is the SP0256. Writing an allophone there says it; reading
//!   gives its busy line back in bit 0.
//! * `$3000-$3FFF` is the intonation: an even address is the low pitch and an
//!   odd one the high, about seven per cent above it. Only writes count.
//! * Nothing else answers, and the machine's own ROM is not there to be read.
//!
//! All of it works through `IN` and `OUT` as well, because the decoding is on
//! the address bus and does not care which kind of cycle put it there.
//!
//! The speech itself is the SP0256-AL2's, in `crate::sp0256`: this file is the
//! box the chip sits in, and the chip is what talks. The busy line read back
//! at `$1000` is the chip's own, so a program that polls it waits exactly as
//! long as the sound takes.
//!
//! The addresses, the mirroring and the busy bit are from Thomas Busse's
//! measurements of the real hardware at
//! <https://maziac.github.io/currah_uspeech_tests>, which is the only
//! description precise enough to work from; the allophone lengths are the
//! SP0256-AL2's published ones.

pub const ROM_LEN: usize = 2048;

#[derive(Clone)]
pub struct Uspeech {
    /// The 2K ROM, once somebody has supplied one. Without it there is nothing
    /// to page in and the interface is an empty socket.
    pub rom: Option<Vec<u8>>,
    /// Whether the interface's ROM and registers are over the bottom 16K.
    pub paged: bool,
    /// The allophone last written, which is what the chip is saying.
    pub allophone: u8,
    /// The higher of the two pitches, set by writing to an odd address in the
    /// $3000 block.
    pub high_pitch: bool,
    /// How many allophones have been spoken, so the window can say whether
    /// anything is reaching the chip at all.
    pub spoken: u64,
    /// How many of those were sounds rather than the five pauses. The driver
    /// writes a pause every interrupt whether or not anything is being said,
    /// so the pauses say nothing about whether the machine is talking.
    pub phonemes: u64,
}

/// What an access told the interface to do. The chip itself lives with the
/// mixer — it makes sound at its own rate, not the machine's — so what it is
/// told has to be passed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Told {
    Nothing,
    /// Say this allophone.
    Say(u8),
    /// Use the higher of the two pitches, or the lower.
    Pitch(bool),
}

impl Default for Uspeech {
    fn default() -> Self {
        Uspeech::new()
    }
}

impl Uspeech {
    pub fn new() -> Uspeech {
        Uspeech {
            rom: None,
            paged: false,
            allophone: 0,
            phonemes: 0,
            high_pitch: false,
            spoken: 0,
        }
    }

    pub fn ready(&self) -> bool {
        self.rom.is_some()
    }

    /// Any access at all to $0038 turns the interface on, and the next one
    /// turns it off. Reads, writes, `IN`, `OUT` and opcode fetches all count.
    pub fn touch(&mut self, addr: u16) -> bool {
        if addr != 0x0038 || !self.ready() {
            return false;
        }
        self.paged = !self.paged;
        true
    }

    /// What the interface answers at an address, if it is paged in. The busy
    /// line comes from the chip, which is the only thing that knows how long
    /// an allophone takes.
    pub fn mem(&self, addr: u16, busy: bool) -> Option<u8> {
        if !self.paged {
            return None;
        }
        match addr {
            // The 2K ROM, and its mirror in the next 2K.
            0x0000..=0x0FFF => self
                .rom
                .as_ref()
                .and_then(|rom| rom.get(addr as usize & 0x07FF))
                .copied(),
            // The SP0256. Bit 0 is the busy line; the rest are the chip's own
            // business and are not driven here.
            0x1000..=0x1FFF => Some(if busy { 0xFF } else { 0xFE }),
            // The machine's ROM is not readable while the interface is in.
            0x2000..=0x3FFF => Some(0xFF),
            _ => None,
        }
    }

    /// A write into the interface's space, and what it told the chip.
    pub fn poke(&mut self, addr: u16, value: u8) -> Option<Told> {
        if !self.paged {
            return None;
        }
        match addr {
            0x1000..=0x1FFF => {
                self.allophone = value & 0x3F;
                self.spoken += 1;
                if self.allophone > 4 {
                    self.phonemes += 1;
                }
                Some(Told::Say(self.allophone))
            }
            0x3000..=0x3FFF => {
                // The address says which pitch; the byte written means
                // nothing at all.
                self.high_pitch = addr & 0x0001 != 0;
                Some(Told::Pitch(self.high_pitch))
            }
            // Its ROM, and the space where the machine's would be.
            0x0000..=0x3FFF => Some(Told::Nothing),
            _ => None,
        }
    }

    /// An `IN`: the address bus is the address bus, whichever cycle it is.
    pub fn io_read(&mut self, port: u16, busy: bool) -> Option<u8> {
        if self.touch(port) {
            return None;
        }
        self.mem(port, busy)
    }

    /// An `OUT`, which can say an allophone or set the pitch just as a memory
    /// write can.
    pub fn io_write(&mut self, port: u16, value: u8) -> Option<Told> {
        if self.touch(port) {
            return None;
        }
        self.poke(port, value)
    }

    /// What the reset line does: the ROM is out of the way and the chip quiet.
    pub fn reset(&mut self) {
        self.paged = false;
        self.high_pitch = false;
    }
}
