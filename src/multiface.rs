//! The Multiface: a red button, 8K of ROM and 8K of RAM.
//!
//! Romantic Robot's box hangs off the expansion port and does one thing:
//! pressing its button pulls the CPU's /NMI, and the interface pages its own
//! ROM and RAM over the bottom 16K in time for the fetch from $0066. Whatever
//! was running is stopped where it stood, with every register still in it, and
//! the ROM's menu can save the lot — which is how a game with no save game got
//! one, and how most of the snapshots in the archives were made.
//!
//! Three models, and they differ in more than their menus:
//!
//! * The **Multiface One** is for a 48K. It pages in on `IN` from a port with
//!   A7 set — $9F, in the manual — and out again on one with A7 clear, $1F.
//! * The **Multiface 128** is the same idea with $BF and $3F, and it hands
//!   back a byte saying how the 128K's memory was paged when the button went
//!   in, so its ROM can put it back.
//! * The **Multiface 3** has the two the other way round — $3F pages in and
//!   $BF pages out — and watches every write to $1FFD, $3FFD, $5FFD and $7FFD
//!   so it can tell its ROM what the +3's paging was.
//!
//! Two latches decide whether a press does anything. One is set by the button
//! and cleared by the fetch from $0066, so the ROM is paged in exactly once
//! per press; the other is cleared by the button and set again by the next
//! `OUT` to the interface's own port, which is what the ROM does on its way
//! out — press the button twice without letting the menu finish and the second
//! press does nothing, as on the desk.
//!
//! The port decoding and the latch behaviour are Fuse's `multiface.c`, which
//! is the only description of these boxes precise enough to work from: the
//! manuals give one port number each and say nothing about which address lines
//! are actually decoded.

pub const ROM_LEN: usize = 0x2000;
pub const RAM_LEN: usize = 0x2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    One,
    OneTwentyEight,
    Three,
}

impl Model {
    pub fn name(&self) -> &'static str {
        match self {
            Model::One => "Multiface One",
            Model::OneTwentyEight => "Multiface 128",
            Model::Three => "Multiface 3",
        }
    }

    /// The names its ROM goes by, in the order they are looked for.
    pub fn rom_names(&self) -> &'static [&'static str] {
        match self {
            Model::One => &["multiface1.rom", "mf1.rom"],
            Model::OneTwentyEight => &["multiface128.rom", "mf128.rom"],
            Model::Three => &["multiface3.rom", "mf3.rom"],
        }
    }
}

#[derive(Clone)]
pub struct Multiface {
    pub model: Model,
    /// The 8K of ROM, once somebody has supplied one. Without it the box does
    /// nothing at all — it has nothing to run when the button is pressed.
    pub rom: Option<Vec<u8>>,
    /// Its own 8K of RAM, which is where the menu keeps its working copy of
    /// whatever it is about to save.
    pub ram: Vec<u8>,
    /// Whether the ROM and RAM are over the bottom 16K of the machine.
    pub paged: bool,
    /// The button has been pressed and the NMI not taken yet.
    pub pressed: bool,
    /// IC8b: cleared by the button, set again by an `OUT` to the interface.
    /// One press, one entry into the menu.
    armed: bool,
    /// The software switch. The 128 and the 3 can be told to keep out of the
    /// way by a program, and are put back in the way when they page in.
    enabled: bool,
    /// The last byte written to each of $1FFD, $3FFD, $5FFD and $7FFD, which
    /// is what the Multiface 3 hands its ROM so it can restore the paging.
    paging: [u8; 4],
    /// The last byte written to $7FFD, for the Multiface 128's status byte.
    banked: u8,
}

impl Multiface {
    pub fn new(model: Model) -> Multiface {
        Multiface {
            model,
            rom: None,
            ram: vec![0; RAM_LEN],
            paged: false,
            pressed: false,
            armed: true,
            enabled: true,
            paging: [0; 4],
            banked: 0,
        }
    }

    /// Whether there is a ROM in it to run.
    pub fn ready(&self) -> bool {
        self.rom.is_some()
    }

    /// The red button. Returns whether the machine should be interrupted:
    /// nothing happens with no ROM in the box, with the press before last not
    /// yet finished with, or — on the One — with its switch off.
    pub fn press(&mut self) -> bool {
        if !self.ready() || !self.armed {
            return false;
        }
        if self.model == Model::One && !self.enabled {
            return false;
        }
        self.armed = false;
        self.pressed = true;
        true
    }

    /// The fetch from $0066, which is what pages the interface in.
    ///
    /// The hardware decodes the address rather than the interrupt: the latch
    /// the button set is clocked by /M1 with $0066 on the bus, so this is a
    /// fetch hook like the Interface 1's rather than something hung on the
    /// CPU's NMI.
    pub fn on_fetch(&mut self, addr: u16) -> bool {
        if addr != 0x0066 || !self.pressed {
            return false;
        }
        self.pressed = false;
        self.paged = true;
        // Paging in puts the software switch back on: the ROM is about to
        // need its own ports.
        self.enabled = true;
        true
    }

    /// Whether a port is the interface's own.
    ///
    /// The One answers `x001 xx1x` and the other two `x011 xx1x`, which is
    /// what makes $9F/$1F and $BF/$3F the numbers in the manuals.
    fn ours(&self, port: u16) -> bool {
        match self.model {
            Model::One => port & 0x0072 == 0x0012,
            _ => port & 0x0072 == 0x0032,
        }
    }

    /// An `IN` from one of the interface's ports: it is the reading that pages
    /// it in or out, and the byte handed back is a side issue.
    pub fn io_read(&mut self, port: u16) -> Option<u8> {
        if !self.ready() || !self.ours(port) {
            return None;
        }
        let a7 = port & 0x0080 != 0;
        match self.model {
            Model::One => {
                if a7 {
                    if self.enabled {
                        self.paged = true;
                    }
                } else {
                    self.paged = false;
                }
                Some(0xFF)
            }
            Model::OneTwentyEight => {
                if a7 {
                    if self.enabled {
                        self.paged = true;
                        // How the 128K was paged when the button went in.
                        return Some(if self.banked & 0x08 != 0 { 0xFF } else { 0x7F });
                    }
                } else {
                    self.paged = false;
                }
                Some(0xFF)
            }
            Model::Three => {
                if a7 {
                    self.paged = false;
                } else if self.enabled {
                    self.paged = true;
                }
                // Which of the four paging ports is being asked about is in
                // the address, not in a register.
                let which = ((port >> 13) & 0x03) as usize;
                Some(self.paging[which] | 0xF0)
            }
        }
    }

    /// An `OUT`, which is how the ROM arms the button again on its way out.
    /// It is also how the 128 and the 3 are told to keep out of the way.
    pub fn io_write(&mut self, port: u16, value: u8) {
        if !self.ready() {
            return;
        }
        // The paging ports, watched rather than answered: the Multiface 3
        // hands these back to its ROM, and the 128 wants to know which ROM the
        // machine had.
        if self.model == Model::Three && port & 0x90FF == 0x10FD {
            self.paging[((port >> 13) & 0x03) as usize] = value & 0x0F;
        }
        if self.model == Model::OneTwentyEight && port & 0x8002 == 0x0000 {
            self.banked = value;
        }
        if !self.ours(port) {
            return;
        }
        if self.model != Model::One && self.paged {
            self.enabled = port & 0x0080 != 0;
        }
        self.armed = true;
    }

    /// The byte the interface has at an address, if it is paged in. Its ROM is
    /// over the bottom 8K and its RAM over the next.
    pub fn mem(&self, addr: u16) -> Option<u8> {
        if !self.paged {
            return None;
        }
        match addr {
            0x0000..=0x1FFF => self
                .rom
                .as_ref()
                .and_then(|r| r.get(addr as usize))
                .copied(),
            0x2000..=0x3FFF => self.ram.get(addr as usize - ROM_LEN).copied(),
            _ => None,
        }
    }

    /// A write into the interface's RAM. Writes into its ROM go nowhere, as
    /// they do on the machine's own.
    pub fn poke(&mut self, addr: u16, value: u8) -> bool {
        if !self.paged {
            return false;
        }
        match addr {
            0x0000..=0x1FFF => true,
            0x2000..=0x3FFF => {
                self.ram[addr as usize - ROM_LEN] = value;
                true
            }
            _ => false,
        }
    }

    /// What the reset line does. The RAM keeps what is in it — the box is not
    /// on the machine's reset line — but it is out of the way and the button
    /// is ready again.
    pub fn reset(&mut self) {
        self.paged = false;
        self.pressed = false;
        self.armed = true;
        self.enabled = true;
    }
}
