//! The joystick interfaces, which are four different ideas about the same
//! five switches.
//!
//! Nobody agreed on how a joystick should reach a Spectrum, so the machine has
//! no joystick port and every interface solved it differently:
//!
//! * **Kempston** put it on a port. Reading it gives a byte with a bit per
//!   direction, set while the stick is over. It is the one most games ask for.
//! * **Sinclair**, in Interface 2, wired the stick to five keys — so a game
//!   that reads the keyboard supports it without knowing. Two sticks: the
//!   first on 6 to 0, the second on 1 to 5.
//! * **Cursor** (also AGF and Protek) did the same with the keys the ROM's own
//!   cursor keys are on: 5, 6, 7, 8 and 0.
//! * **Fuller** put it on a port like Kempston, at $7F, but the other way up:
//!   its bits are clear while the stick is over.
//! * **DK'Tronics** made a box with two sockets and the stick in one of them:
//!   port No. 2 is Kempston's IN 31, and port No. 1 is wired to 6, 7, 8, 9 and
//!   0 like Interface 2's first — except that its own manual's test prints 6,
//!   7, 8, 9, 0 for left, right, up, down and fire, where Sinclair's has up on
//!   9 and down on 8. Spectrum Computing records the same swap as a known
//!   error. The manual does not say how IN 31 is decoded, so it is decoded as
//!   Kempston's.
//! * The **DK'Tronics Programmable** has one socket, wired to whichever five
//!   keys it has been taught. Its slider has two positions: at 2 it is being
//!   taught — hold the stick one way, press the key, let go of both — and at 1
//!   it plays. Its tape also programs the diagonals; taught by hand it does
//!   not. Here holding two directions presses both keys, which is what the
//!   tape's programming gives; how the box stores its diagonals is not known,
//!   so taught-by-hand's lack of them is not copied.
//!
//! Which of those a game wants is not something the game says, which is why an
//! emulator has to offer all of them and let somebody choose.
//!
//! The masks and the key mappings are Fuse's `joystick.c`, which is the
//! description everything else agrees with; the button order there is left,
//! right, up, down, fire, and it is kept here so the two can be compared.

/// The five switches in a stick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Way {
    Left,
    Right,
    Up,
    Down,
    Fire,
}

impl Way {
    pub const ALL: [Way; 5] = [Way::Left, Way::Right, Way::Up, Way::Down, Way::Fire];

    pub fn name(&self) -> &'static str {
        match self {
            Way::Left => "left",
            Way::Right => "right",
            Way::Up => "up",
            Way::Down => "down",
            Way::Fire => "fire",
        }
    }

    fn index(&self) -> usize {
        match self {
            Way::Left => 0,
            Way::Right => 1,
            Way::Up => 2,
            Way::Down => 3,
            Way::Fire => 4,
        }
    }
}

/// Which interface the stick is plugged into.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    None,
    Kempston,
    Sinclair1,
    Sinclair2,
    Cursor,
    Fuller,
    DkTronicsKeys,
    DkTronicsKempston,
    DkTronicsProgrammable,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::None,
        Kind::Kempston,
        Kind::Sinclair1,
        Kind::Sinclair2,
        Kind::Cursor,
        Kind::Fuller,
        Kind::DkTronicsKeys,
        Kind::DkTronicsKempston,
        Kind::DkTronicsProgrammable,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Kind::None => "None",
            Kind::Kempston => "Kempston",
            Kind::Sinclair1 => "Sinclair 1",
            Kind::Sinclair2 => "Sinclair 2",
            Kind::Cursor => "Cursor",
            Kind::Fuller => "Fuller",
            Kind::DkTronicsKeys => "DK'Tronics port 1",
            Kind::DkTronicsKempston => "DK'Tronics port 2",
            Kind::DkTronicsProgrammable => "DK'Tronics programmable",
        }
    }

    /// The name it is saved under, which does not change when the label does.
    pub fn key(&self) -> &'static str {
        match self {
            Kind::None => "none",
            Kind::Kempston => "kempston",
            Kind::Sinclair1 => "sinclair1",
            Kind::Sinclair2 => "sinclair2",
            Kind::Cursor => "cursor",
            Kind::Fuller => "fuller",
            Kind::DkTronicsKeys => "dktronics1",
            Kind::DkTronicsKempston => "dktronics2",
            Kind::DkTronicsProgrammable => "dkprog",
        }
    }

    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// What a program does to read it, for the window to say.
    pub fn how(&self) -> &'static str {
        match self {
            Kind::None => "Nothing is plugged in.",
            Kind::Kempston => {
                "A port: IN 31 gives 000FUDLR, a bit set while the stick is over. The one \
                 most games ask for."
            }
            Kind::Sinclair1 => "Interface 2's first stick, wired to keys 6 7 8 9 0.",
            Kind::Sinclair2 => "Interface 2's second stick, wired to keys 1 2 3 4 5.",
            Kind::Cursor => {
                "AGF and Protek: the keys the ROM's own cursors are on — 5 6 7 8 and 0."
            }
            Kind::Fuller => "A port at $7F, like Kempston but with its bits the other way up.",
            Kind::DkTronicsKeys => {
                "DK'Tronics' keyed socket: 6 7 for left and right, then 8 for up and 9 for \
                 down — the other way round from Interface 2 — and 0 to fire."
            }
            Kind::DkTronicsKempston => "DK'Tronics' other socket: IN 31, as Kempston.",
            Kind::DkTronicsProgrammable => {
                "Wired to whichever five keys it has been taught. Slider at 2, hold a \
                 direction and press the key it should be; slider back to 1 to play."
            }
        }
    }

    /// The interface in the Hardware window that a stick plugged in this way
    /// needs, for the ones that are there to be fitted.
    pub fn interface(&self) -> Option<crate::hardware::Peripheral> {
        match self {
            Kind::Kempston => Some(crate::hardware::Peripheral::KempstonJoystick),
            Kind::DkTronicsKeys | Kind::DkTronicsKempston => {
                Some(crate::hardware::Peripheral::DkTronicsJoystick)
            }
            Kind::DkTronicsProgrammable => Some(crate::hardware::Peripheral::DkTronicsProgrammable),
            _ => None,
        }
    }

    /// The keys it pulls down, in Fuse's button order: left, right, up, down,
    /// fire. A port interface pulls none.
    fn keys(&self) -> Option<[(usize, u8); 5]> {
        match self {
            // 6 7 9 8 0
            Kind::Sinclair1 => Some([(4, 4), (4, 3), (4, 1), (4, 2), (4, 0)]),
            // 1 2 4 3 5
            Kind::Sinclair2 => Some([(3, 0), (3, 1), (3, 3), (3, 2), (3, 4)]),
            // 5 8 7 6 0
            Kind::Cursor => Some([(3, 4), (4, 2), (4, 3), (4, 4), (4, 0)]),
            // 6 7 8 9 0: up and down the other way round from Sinclair's
            Kind::DkTronicsKeys => Some([(4, 4), (4, 3), (4, 2), (4, 1), (4, 0)]),
            _ => None,
        }
    }
}

/// The bits a port interface sets, in the same button order.
const KEMPSTON_MASK: [u8; 5] = [0x02, 0x01, 0x08, 0x04, 0x10];
const FULLER_MASK: [u8; 5] = [0x04, 0x08, 0x01, 0x02, 0x80];

/// One stick, and what it is plugged into.
#[derive(Clone, Default)]
pub struct Joystick {
    pub kind: Kind,
    /// Which of the five are over, in `Way`'s own order.
    pressed: [bool; 5],
    /// What the DK'Tronics Programmable has been taught: a key of the matrix
    /// for each of the five, in `Way`'s order. Nothing until it is taught.
    program: [Option<(usize, u8)>; 5],
    /// Its slider: at 2 it is being taught, and presses nothing.
    pub programming: bool,
}

impl Joystick {
    pub fn new() -> Joystick {
        Joystick::default()
    }

    pub fn set(&mut self, way: Way, down: bool) {
        self.pressed[way.index()] = down;
    }

    pub fn is_down(&self, way: Way) -> bool {
        self.pressed[way.index()]
    }

    pub fn anything_down(&self) -> bool {
        self.pressed.iter().any(|down| *down)
    }

    /// Let go of everything, which is what changing interface or resetting the
    /// machine has to do: a direction held on an interface nobody is reading
    /// any more would be held for ever.
    pub fn release(&mut self) {
        self.pressed = [false; 5];
    }

    /// Whether a port read is this interface's, and what it answers.
    ///
    /// Kempston is decoded on A5, A6 and A7 being low, which is what makes it
    /// port 31; the Fuller is the whole of $7F. A machine with no joystick
    /// answers neither, and the floating bus has the port instead.
    pub fn io_read(&self, port: u16) -> Option<u8> {
        match self.kind {
            Kind::Kempston | Kind::DkTronicsKempston if port & 0x00E0 == 0x0000 => {
                let mut value = 0x00;
                for way in Way::ALL {
                    if self.is_down(way) {
                        value |= KEMPSTON_MASK[way.index()];
                    }
                }
                Some(value)
            }
            Kind::Fuller if port & 0x00FF == 0x007F => {
                // The other way up: a bit is clear while the stick is over.
                let mut value = 0xFF;
                for way in Way::ALL {
                    if self.is_down(way) {
                        value &= !FULLER_MASK[way.index()];
                    }
                }
                Some(value)
            }
            _ => None,
        }
    }

    /// The key the DK'Tronics Programmable has been taught for a direction.
    pub fn taught(&self, way: Way) -> Option<(usize, u8)> {
        self.program[way.index()]
    }

    pub fn teach(&mut self, way: Way, key: Option<(usize, u8)>) {
        self.program[way.index()] = key;
    }

    /// With the slider at 2, a direction held while one key is down is taught
    /// that key: the way the manual says to program it by hand. Two
    /// directions, two keys, or none teach nothing.
    pub fn learn(&mut self, keyboard: &[u8; 8]) {
        if self.kind != Kind::DkTronicsProgrammable || !self.programming {
            return;
        }
        let held: Vec<Way> = Way::ALL.into_iter().filter(|w| self.is_down(*w)).collect();
        let down: Vec<(usize, u8)> = (0..8)
            .flat_map(|row| {
                (0..5u8)
                    .filter(move |bit| keyboard[row] & (1 << bit) == 0)
                    .map(move |bit| (row, bit))
            })
            .collect();
        if let ([way], [key]) = (held.as_slice(), down.as_slice()) {
            self.program[way.index()] = Some(*key);
        }
    }

    /// The keyboard lines it is holding down, for the interfaces that are
    /// wired to keys rather than to a port.
    ///
    /// A game reading the keyboard cannot tell the difference, which was the
    /// whole idea: Sinclair's interface works with games that know nothing
    /// about it.
    pub fn matrix(&self) -> [u8; 8] {
        let mut keys = [0xFFu8; 8];
        if let Some(map) = self.kind.keys() {
            for way in Way::ALL {
                if self.is_down(way) {
                    let (row, bit) = map[way.index()];
                    keys[row] &= !(1 << bit);
                }
            }
        }
        if self.kind == Kind::DkTronicsProgrammable && !self.programming {
            for way in Way::ALL {
                if let (true, Some((row, bit))) = (self.is_down(way), self.taught(way)) {
                    keys[row] &= !(1 << bit);
                }
            }
        }
        keys
    }
}

/// What the DK'Tronics Programmable has been taught, as the preferences keep
/// it: `up:4.2,fire:4.0`, a direction and its key's row and bit.
pub fn program_to_text(stick: &Joystick) -> String {
    Way::ALL
        .iter()
        .filter_map(|way| {
            stick
                .taught(*way)
                .map(|(row, bit)| format!("{}:{row}.{bit}", way.name()))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// And back. A piece that means nothing is dropped rather than throwing the
/// rest away.
pub fn program_from_text(stick: &mut Joystick, text: &str) {
    for way in Way::ALL {
        stick.teach(way, None);
    }
    for piece in text.split(',') {
        let Some((name, key)) = piece.trim().split_once(':') else {
            continue;
        };
        let Some(way) = Way::ALL.into_iter().find(|w| w.name() == name) else {
            continue;
        };
        if let Some((row, bit)) = key.split_once('.') {
            if let (Ok(row), Ok(bit)) = (row.parse::<usize>(), bit.parse::<u8>()) {
                if row < 8 && bit < 5 {
                    stick.teach(way, Some((row, bit)));
                }
            }
        }
    }
}
