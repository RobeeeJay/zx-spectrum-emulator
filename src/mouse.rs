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
