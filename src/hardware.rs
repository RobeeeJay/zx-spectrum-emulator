//! What is plugged into the back of the machine.
//!
//! Some of these are emulated and some are only fitted. The difference matters
//! and the window says which is which: a switch that turns on nothing is worse
//! than no switch, because it looks like the thing is working.
//!
//! Emulated: the Interface 1 and its microdrives (`src/if1.rs`), the Fuller
//! Audio Box, Cheetah's SpecDrum, the Kempston mouse, and the ZX Printer and
//! Alphacom 32.
//!
//! Fitted and not emulated: the Currah µSpeech, the RAM Music Machine, and the
//! three Multifaces. What each needs is written against it below — mostly a
//! ROM that cannot be shipped, and in µSpeech's case a speech chip that has to
//! be synthesised rather than played back.

/// The parts the Hardware window is in, in the order it lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Mice,
    Multiface,
    Printers,
    Audio,
    Joysticks,
    Drives,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Mice,
        Section::Multiface,
        Section::Printers,
        Section::Audio,
        Section::Joysticks,
        Section::Drives,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Section::Mice => "Mice",
            Section::Multiface => "Multiface",
            Section::Printers => "Printers",
            Section::Audio => "Audio",
            Section::Joysticks => "Joysticks",
            Section::Drives => "Drives",
        }
    }
}

/// Everything that can be plugged in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Peripheral {
    Interface1,
    Uspeech,
    Fuller,
    SpecDrum,
    MusicMachine,
    MultifaceOne,
    Multiface128,
    Multiface3,
    KempstonMouse,
    ZxPrinter,
    Alphacom32,
    AmxMouse,
    KempstonJoystick,
    DkTronicsJoystick,
}

impl Peripheral {
    pub const ALL: [Peripheral; 14] = [
        Peripheral::Interface1,
        Peripheral::Uspeech,
        Peripheral::Fuller,
        Peripheral::SpecDrum,
        Peripheral::MusicMachine,
        Peripheral::MultifaceOne,
        Peripheral::Multiface128,
        Peripheral::Multiface3,
        Peripheral::KempstonMouse,
        Peripheral::ZxPrinter,
        Peripheral::Alphacom32,
        Peripheral::AmxMouse,
        Peripheral::KempstonJoystick,
        Peripheral::DkTronicsJoystick,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Peripheral::Interface1 => "Interface 1",
            Peripheral::Uspeech => "Currah µSpeech",
            Peripheral::Fuller => "Fuller Audio Box",
            Peripheral::SpecDrum => "Cheetah SpecDrum",
            Peripheral::MusicMachine => "RAM Music Machine",
            Peripheral::MultifaceOne => "Multiface One",
            Peripheral::Multiface128 => "Multiface 128",
            Peripheral::Multiface3 => "Multiface 3",
            Peripheral::KempstonMouse => "Kempston mouse",
            Peripheral::ZxPrinter => "ZX Printer",
            Peripheral::Alphacom32 => "Alphacom 32",
            Peripheral::AmxMouse => "AMX mouse",
            Peripheral::KempstonJoystick => "Kempston Joystick Interface",
            Peripheral::DkTronicsJoystick => "DK'Tronics Joystick Interface",
        }
    }

    /// The name it is saved under, which does not change when the label does.
    pub fn key(&self) -> &'static str {
        match self {
            Peripheral::Interface1 => "if1",
            Peripheral::Uspeech => "uspeech",
            Peripheral::Fuller => "fuller",
            Peripheral::SpecDrum => "specdrum",
            Peripheral::MusicMachine => "music_machine",
            Peripheral::MultifaceOne => "multiface1",
            Peripheral::Multiface128 => "multiface128",
            Peripheral::Multiface3 => "multiface3",
            Peripheral::KempstonMouse => "kempston_mouse",
            Peripheral::ZxPrinter => "zx_printer",
            Peripheral::Alphacom32 => "alphacom32",
            Peripheral::AmxMouse => "amx_mouse",
            Peripheral::KempstonJoystick => "kempston_joystick",
            Peripheral::DkTronicsJoystick => "dktronics_joystick",
        }
    }

    pub fn from_key(key: &str) -> Option<Peripheral> {
        Peripheral::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Which part of the Hardware window it is listed in.
    pub fn section(&self) -> Section {
        match self {
            Peripheral::KempstonMouse | Peripheral::AmxMouse => Section::Mice,
            Peripheral::MultifaceOne | Peripheral::Multiface128 | Peripheral::Multiface3 => {
                Section::Multiface
            }
            Peripheral::ZxPrinter | Peripheral::Alphacom32 => Section::Printers,
            Peripheral::Uspeech
            | Peripheral::Fuller
            | Peripheral::SpecDrum
            | Peripheral::MusicMachine => Section::Audio,
            Peripheral::KempstonJoystick | Peripheral::DkTronicsJoystick => Section::Joysticks,
            Peripheral::Interface1 => Section::Drives,
        }
    }

    /// What it does, in a line.
    pub fn what(&self) -> &'static str {
        match self {
            Peripheral::Interface1 => {
                "Microdrives, a local network and an RS232 socket, on an 8K ROM that pages \
                 itself in when the machine asks for it"
            }
            Peripheral::Uspeech => {
                "An SP0256 speech chip in a wedge behind the machine: a program writes \
                 allophones at it and it talks"
            }
            Peripheral::Fuller => {
                "A sound chip and a joystick port, giving a 48K the AY it had not got"
            }
            Peripheral::SpecDrum => "An eight-bit converter fed drum samples from memory",
            Peripheral::MusicMachine => "A sampler, a sound chip and a MIDI socket in one box",
            Peripheral::MultifaceOne => "A button that stops the machine and saves what is in it",
            Peripheral::Multiface128 => "The same, for the 128K",
            Peripheral::Multiface3 => "The same, for the +2A and +3, with the disk in mind",
            Peripheral::KempstonMouse => {
                "A mouse on three ports: two counters and the buttons. Moved by the host's \
                 mouse over the screen"
            }
            Peripheral::ZxPrinter => {
                "Sinclair's spark printer: 256 dots across silver paper, for COPY, LPRINT \
                 and LLIST"
            }
            Peripheral::Alphacom32 => {
                "A thermal printer on the same port, driven the same way, on white paper"
            }
            Peripheral::AmxMouse => {
                "Advanced Memory Systems' mouse: a Z80 PIO that interrupts the machine for \
                 every step it moves, and three buttons"
            }
            Peripheral::KempstonJoystick => {
                "One socket on a port: IN 31 gives a bit for each direction and fire. The \
                 one most games ask for"
            }
            Peripheral::DkTronicsJoystick => {
                "Two sockets: one on the Kempston port, one wired to keys 6 to 0 as \
                 Interface 2's first. The stick is in one of them, chosen in the Input window"
            }
        }
    }

    /// Whether the emulator does anything with it, and what is missing when
    /// it does not.
    pub fn emulated(&self) -> Emulated {
        match self {
            Peripheral::Interface1 => Emulated::NeedsRom(
                "roms/if1.rom — the 8K shadow ROM. Everything the microdrives do is done \
                 by it, so without one the interface pages in nothing.",
            ),
            Peripheral::Fuller
            | Peripheral::SpecDrum
            | Peripheral::KempstonMouse
            | Peripheral::ZxPrinter
            | Peripheral::Alphacom32
            | Peripheral::AmxMouse
            | Peripheral::KempstonJoystick
            | Peripheral::DkTronicsJoystick => Emulated::Yes,
            // Two ROMs: Currah's, which is the interface, and the speech
            // chip's own, which is where the allophones live.
            Peripheral::Uspeech => Emulated::NeedsRom(
                "roms/uspeech.rom — Currah's own 2K — and roms/sp0256-al2.rom, the speech \
                 chip's, which holds the allophones as filter coefficients. With only the \
                 first the interface works and nothing is audible.",
            ),
            Peripheral::MusicMachine => Emulated::No(
                "Its port map has not been checked against a reference here, and inventing \
                 one would make a switch that looks as though it works.",
            ),
            // Each has its own 8K, and they are not interchangeable: the
            // three page in on different ports and their menus save to
            // different things.
            Peripheral::MultifaceOne => Emulated::NeedsRom(
                "roms/multiface1.rom — the 8K the red button runs. Everything the \
                 Multiface does is in it.",
            ),
            Peripheral::Multiface128 => Emulated::NeedsRom(
                "roms/multiface128.rom — the 128K's Multiface, which saves to microdrive \
                 as well as to tape.",
            ),
            Peripheral::Multiface3 => {
                Emulated::NeedsRom("roms/multiface3.rom — the +2A and +3's, which saves to disk.")
            }
        }
    }
}

/// How far a peripheral is emulated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emulated {
    Yes,
    /// It works, once somebody supplies the ROM named here.
    NeedsRom(&'static str),
    /// It is a switch and nothing behind it, for the reason given.
    No(&'static str),
}

/// What is plugged in, and the state of the ones that do something.
#[derive(Clone)]
pub struct Hardware {
    fitted: Vec<Peripheral>,
    /// How many microdrives are on the Interface 1's chain.
    pub if1_drives: usize,
    /// The register the Fuller Audio Box's sound chip has selected. Its own,
    /// not the machine's: a 48K with a Fuller in it has one sound chip and a
    /// 128K with one has two.
    pub fuller_register: u8,
}

impl Default for Hardware {
    fn default() -> Self {
        Hardware {
            fitted: Vec::new(),
            if1_drives: 1,
            fuller_register: 0,
        }
    }
}

impl Hardware {
    pub fn fitted(&self, what: Peripheral) -> bool {
        self.fitted.contains(&what)
    }

    pub fn fit(&mut self, what: Peripheral, yes: bool) {
        let there = self.fitted.iter().position(|p| *p == what);
        match (yes, there) {
            (true, None) => {
                self.fitted.push(what);
                self.fitted.sort();
            }
            (false, Some(at)) => {
                self.fitted.remove(at);
            }
            _ => {}
        }
    }

    /// Everything plugged in, for saving and for saying.
    pub fn all_fitted(&self) -> &[Peripheral] {
        &self.fitted
    }
}
