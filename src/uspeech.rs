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
//! What is *not* here is the speech. The sounds live in the SP0256-AL2's own
//! ROM as filter coefficients — the chip is a twelve-pole lattice filter, not
//! a sample player — and that ROM is inside the chip, not in the µSpeech's.
//! Everything the machine can see is emulated, so a program that drives the
//! interface runs and reads back what it should; what comes out of the speaker
//! is silence, and the Hardware window says so rather than pretending.
//!
//! The addresses, the mirroring and the busy bit are from Thomas Busse's
//! measurements of the real hardware at
//! <https://maziac.github.io/currah_uspeech_tests>, which is the only
//! description precise enough to work from; the allophone lengths are the
//! SP0256-AL2's published ones.

pub const ROM_LEN: usize = 2048;

/// How long each of the 64 allophones takes, in tenths of a millisecond.
///
/// The chip holds its busy line up for exactly this long, and a program that
/// wants to speak without gaps waits on that line rather than counting: these
/// are the measured lengths from the SP0256-AL2's data sheet, which are not
/// the round numbers the manual gives.
const LENGTHS: [u16; 64] = [
    64, 256, 448, 960, 1984, // the five pauses
    2912, 1729, 546, 768, 1472, 984, 1729, 455, 960, 1274, 546, 1820, 768, 1365, 1729, 2002, 455,
    637, 728, 637, 1274, 819, 896, 364, 1280, 728, 1729, 2548, 721, 1105, 1274, 721, 1984, 1341,
    819, 1088, 1344, 1152, 1486, 2002, 819, 1456, 2457, 1452, 910, 1472, 1092, 2093, 1729, 1820,
    640, 1365, 1260, 2366, 2002, 2457, 694, 1365, 502,
];

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
    /// The machine's clock, and when the chip will have finished.
    now: u64,
    busy_until: u64,
    cpu_hz: f64,
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
            now: 0,
            busy_until: 0,
            cpu_hz: 3_500_000.0,
        }
    }

    pub fn ready(&self) -> bool {
        self.rom.is_some()
    }

    /// The machine's clock. A clock that has gone backwards — a reset, a
    /// snapshot — would otherwise leave the chip busy for the age of the
    /// universe, which is the bug the disk controller had.
    pub fn at(&mut self, now: u64, cpu_hz: f64) {
        if now < self.now {
            self.busy_until = 0;
        }
        self.now = now;
        self.cpu_hz = cpu_hz;
    }

    /// Whether the chip is still saying the last allophone.
    pub fn busy(&self) -> bool {
        self.now < self.busy_until
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

    /// What the interface answers at an address, if it is paged in.
    pub fn mem(&self, addr: u16) -> Option<u8> {
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
            0x1000..=0x1FFF => Some(if self.busy() { 0xFF } else { 0xFE }),
            // The machine's ROM is not readable while the interface is in.
            0x2000..=0x3FFF => Some(0xFF),
            _ => None,
        }
    }

    /// A write into the interface's space. Returns whether it took it.
    pub fn poke(&mut self, addr: u16, value: u8) -> bool {
        if !self.paged {
            return false;
        }
        match addr {
            0x1000..=0x1FFF => {
                self.say(value);
                true
            }
            0x3000..=0x3FFF => {
                // The address says which pitch; the byte written means
                // nothing at all.
                self.high_pitch = addr & 0x0001 != 0;
                true
            }
            // Its ROM, and the space where the machine's would be.
            0x0000..=0x3FFF => true,
            _ => false,
        }
    }

    /// An `IN`: the address bus is the address bus, whichever cycle it is.
    pub fn io_read(&mut self, port: u16) -> Option<u8> {
        if self.touch(port) {
            return None;
        }
        self.mem(port)
    }

    /// An `OUT`, which can write an allophone or the pitch just as a memory
    /// write can.
    pub fn io_write(&mut self, port: u16, value: u8) -> bool {
        if self.touch(port) {
            return false;
        }
        self.poke(port, value)
    }

    /// Start an allophone. The chip is busy for as long as that one takes.
    fn say(&mut self, allophone: u8) {
        let which = allophone as usize & 0x3F;
        self.allophone = which as u8;
        self.spoken += 1;
        if which > 4 {
            self.phonemes += 1;
        }
        let tenths_ms = f64::from(LENGTHS[which]);
        // The higher intonation runs the chip's oscillator about seven per
        // cent faster, so everything it says is that much shorter.
        let pitch = if self.high_pitch { 1.07 } else { 1.0 };
        let t = tenths_ms / 10_000.0 / pitch * self.cpu_hz;
        self.busy_until = self.now + t as u64;
    }

    /// What the reset line does: the ROM is out of the way and the chip quiet.
    pub fn reset(&mut self) {
        self.paged = false;
        self.busy_until = 0;
        self.high_pitch = false;
    }
}
