//! The Kempston mouse.
//!
//! Three ports and no ROM: two eight-bit counters that the mouse's movement
//! turns over, and a byte of buttons. A program reads the counters each frame
//! and moves its pointer by the difference, so the numbers mean nothing on
//! their own and wrap freely. The decoding and the button bits are Fuse's
//! `kempmouse.c`, and are written out in `tests/mouse.rs` so a change has to
//! be deliberate.

/// Where the mouse has got to, and what is held down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KempstonMouse {
    pub x: u8,
    pub y: u8,
    /// Active low: a button held down clears its bit.
    pub buttons: u8,
}

impl Default for KempstonMouse {
    fn default() -> Self {
        KempstonMouse {
            x: 0,
            y: 0,
            buttons: 0xFF,
        }
    }
}

/// The left button is bit 1 and the right bit 0.
const LEFT: u8 = 0x02;
const RIGHT: u8 = 0x01;

impl KempstonMouse {
    /// What the mouse answers at this port, if it is one of its three:
    /// $FADF for the buttons, $FBDF for X and $FFDF for Y, only partly
    /// decoded.
    pub fn io_read(&self, port: u16) -> Option<u8> {
        if port & 0x0121 == 0x0001 {
            Some(self.buttons)
        } else if port & 0x0521 == 0x0101 {
            Some(self.x)
        } else if port & 0x0521 == 0x0501 {
            Some(self.y)
        } else {
            None
        }
    }

    /// Move by this many of the machine's pixels. Y counts up the screen, the
    /// way the host's does not.
    pub fn move_by(&mut self, dx: i32, dy: i32) {
        self.x = self.x.wrapping_add(dx as u8);
        self.y = self.y.wrapping_sub(dy as u8);
    }

    pub fn set_buttons(&mut self, left: bool, right: bool) {
        self.buttons = 0xFF;
        if left {
            self.buttons &= !LEFT;
        }
        if right {
            self.buttons &= !RIGHT;
        }
    }
}

/// The AMX mouse, from Advanced Memory Systems: a Z80 PIO that interrupts the
/// machine once for every step the mouse moves, and a byte of buttons.
///
/// The PIO sits on A7 low, with A6 choosing data or control and A5 choosing
/// its port A (across) or B (up and down): $1F and $3F are the two directions,
/// read in bit 0 by the interrupt handler, and $5F and $7F are where the
/// program sets the PIO up. The buttons are at $DF, active low: left bit 7,
/// middle bit 6, right bit 5. The ports and buttons are from the Sinclair Wiki
/// and agree with dsp-emulator; which way each direction bit runs is from
/// dsp-emulator and zx84, which agree with each other. zx84 puts the left
/// button on bit 6, against the other two.
///
/// Only odd ports are answered. The four documented ones are, and an even port
/// with A7 low is the ULA's as well: the keyboard is worth more than a bus
/// fight nothing relies on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmxMouse {
    /// Active low: left bit 7, middle bit 6, right bit 5.
    pub buttons: u8,
    /// Steps the machine has not been told about yet: right and down are
    /// positive.
    pub pending_x: i32,
    pub pending_y: i32,
    /// What bit 0 of each direction port says: across, 0 right and 1 left;
    /// up and down, 0 up and 1 down.
    dir: [u8; 2],
    /// The vector each PIO port puts on the bus when it interrupts. The PIO
    /// can only hold even ones.
    vector: [u8; 2],
    /// A PIO comes up with its interrupts off, and a program turns them on.
    int_enabled: [bool; 2],
    expect: [Expect; 2],
    /// When the last step was delivered, for the spacing between them.
    last_at: Option<u64>,
    last: usize,
}

/// A PIO control word can say that the next byte is not a control word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Expect {
    #[default]
    Word,
    IoMask,
    IntMask,
}

/// How far apart the steps are delivered. A choice rather than a measurement:
/// close enough that a quick movement arrives within a frame or two, far
/// enough apart that the program gets on with something between handlers.
pub const AMX_STEP_T: u64 = 1000;
/// How many steps can be waiting. A mouse flung across the desk while the
/// machine had its interrupts off is not replayed for seconds afterwards.
const AMX_QUEUE: i32 = 255;

impl Default for AmxMouse {
    fn default() -> Self {
        AmxMouse {
            buttons: 0xFF,
            pending_x: 0,
            pending_y: 0,
            dir: [0; 2],
            vector: [0; 2],
            int_enabled: [false; 2],
            expect: [Expect::Word; 2],
            last_at: None,
            last: 1,
        }
    }
}

impl AmxMouse {
    fn pio(port: u16) -> bool {
        port & 0x81 == 0x01
    }

    pub fn io_read(&self, port: u16) -> Option<u8> {
        if port & 0xFF == 0xDF {
            return Some(self.buttons);
        }
        if !Self::pio(port) || port & 0x40 != 0 {
            return None;
        }
        Some(self.dir[((port >> 5) & 1) as usize])
    }

    /// A write to one of the PIO's control ports.
    pub fn io_write(&mut self, port: u16, value: u8) {
        if !Self::pio(port) || port & 0x40 == 0 {
            return;
        }
        let ch = ((port >> 5) & 1) as usize;
        if self.expect[ch] != Expect::Word {
            // The I/O mask or the interrupt mask: nothing this needs.
            self.expect[ch] = Expect::Word;
            return;
        }
        if value & 1 == 0 {
            self.vector[ch] = value & 0xFE;
        } else if value & 0x0F == 0x0F {
            // A mode word; mode 3 is followed by which lines are inputs.
            if value & 0xC0 == 0xC0 {
                self.expect[ch] = Expect::IoMask;
            }
        } else if value & 0x0F == 0x07 {
            // The interrupt control word: bit 7 turns them on, bit 4 says a
            // mask follows.
            self.int_enabled[ch] = value & 0x80 != 0;
            if value & 0x10 != 0 {
                self.expect[ch] = Expect::IntMask;
            }
        } else if value & 0x0F == 0x03 {
            // Interrupts on or off, and nothing else.
            self.int_enabled[ch] = value & 0x80 != 0;
        }
    }

    /// The mouse moved, in the machine's pixels: right and down are positive.
    pub fn queue(&mut self, dx: i32, dy: i32) {
        self.pending_x = (self.pending_x + dx).clamp(-AMX_QUEUE, AMX_QUEUE);
        self.pending_y = (self.pending_y + dy).clamp(-AMX_QUEUE, AMX_QUEUE);
    }

    pub fn set_buttons(&mut self, left: bool, middle: bool, right: bool) {
        self.buttons = 0xFF;
        for (held, bit) in [(left, 0x80), (middle, 0x40), (right, 0x20)] {
            if held {
                self.buttons &= !bit;
            }
        }
    }

    /// A step ready to go, as the PIO port it comes from and the vector it
    /// interrupts with. The two ports take turns, so a diagonal arrives as a
    /// diagonal rather than all of the across and then all of the down.
    pub fn wants_interrupt(&self, now: u64) -> Option<(usize, u8)> {
        // A clock that has gone backwards — a reset, a snapshot — is not a
        // reason to wait.
        if let Some(last) = self.last_at {
            if now >= last && now - last < AMX_STEP_T {
                return None;
            }
        }
        let ready = [
            self.int_enabled[0] && self.pending_x != 0,
            self.int_enabled[1] && self.pending_y != 0,
        ];
        let first = 1 - self.last;
        [first, self.last]
            .into_iter()
            .find(|ch| ready[*ch])
            .map(|ch| (ch, self.vector[ch]))
    }

    /// The machine took the interrupt: the step is delivered, and the port
    /// says which way it went for the handler to read.
    pub fn acknowledged(&mut self, ch: usize, now: u64) {
        let pending = if ch == 0 {
            &mut self.pending_x
        } else {
            &mut self.pending_y
        };
        let step = pending.signum();
        *pending -= step;
        self.dir[ch] = if ch == 0 {
            u8::from(step < 0)
        } else {
            u8::from(step > 0)
        };
        self.last = ch;
        self.last_at = Some(now);
    }

    /// The reset line reaches the PIO: its vectors and interrupts go, and what
    /// the buttons are doing stays, since that is the desk's.
    pub fn reset(&mut self) {
        *self = AmxMouse {
            buttons: self.buttons,
            ..AmxMouse::default()
        };
    }
}
