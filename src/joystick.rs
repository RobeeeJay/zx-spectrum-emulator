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
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::None,
        Kind::Kempston,
        Kind::Sinclair1,
        Kind::Sinclair2,
        Kind::Cursor,
        Kind::Fuller,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Kind::None => "None",
            Kind::Kempston => "Kempston",
            Kind::Sinclair1 => "Sinclair 1",
            Kind::Sinclair2 => "Sinclair 2",
            Kind::Cursor => "Cursor",
            Kind::Fuller => "Fuller",
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
            Kind::Kempston if port & 0x00E0 == 0x0000 => {
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
        keys
    }
}
