//! The ZX Interface 1, and the microdrives hanging off it.
//!
//! Two halves. The first is a ROM that is not in the machine's memory map
//! until the machine asks for it: the Interface 1 watches the address bus, and
//! when the CPU fetches from $0008 or $1708 — the ROM's error handler and its
//! close-files hook, which is where a program ends up after a `LOAD *` or a
//! `CLOSE` — it pages its own 8K over the bottom of the ROM. It pages out
//! again when the CPU fetches from $0700. That is the whole of the mechanism;
//! everything the microdrives can do is done by that ROM.
//!
//! The second half is the drives. A cartridge is a loop of tape that runs past
//! the head at about 76 sectors a second, and the ROM reads it by waiting for
//! a gap, then a sector header, then a record. What is here is that loop: a
//! motor that turns, a position that advances with the machine's clock, and
//! the bytes handed over one at a time as they pass.
//!
//! Ports: $E7 is the data register, and $EF the control and status one — read
//! for status, written to drive the motors. $F7 is the RS232 and network side,
//! which is not emulated: nothing is on the other end of it.

use crate::microdrive::{Cartridge, SECTOR_LEN};

/// How many drives an Interface 1 can address.
pub const MAX_DRIVES: usize = 8;

/// How long a sector takes to pass the head, in T-states.
///
/// A cartridge is a loop of about 200 sectors that goes round in something
/// like ten seconds, which is 76 sectors a second: 46,000 T-states each at
/// 3.5MHz. The ROM does not measure this — it waits for what it expects —
/// but the tape has to move at some speed for the gaps to be gaps.
pub const SECTOR_T: u64 = 46_000;

/// How much of a sector's time is the gap between it and the next.
const GAP: f32 = 0.12;

/// One drive: what is in it, and where the tape has got to.
#[derive(Clone)]
pub struct Drive {
    pub cartridge: Option<Cartridge>,
    /// Where writes go, if they go anywhere.
    pub path: Option<std::path::PathBuf>,
    /// Whether the emulator was told to keep the cartridge as it is. Kept
    /// apart from the cartridge's own write-protect tab: one is what the
    /// cartridge says, the other what the user said.
    pub read_only: bool,
    /// Which sector is under the head, and how far into it the tape has run.
    position: usize,
    started_at: u64,
}

impl Default for Drive {
    fn default() -> Self {
        Drive::empty()
    }
}

impl Drive {
    pub fn empty() -> Drive {
        Drive {
            cartridge: None,
            path: None,
            read_only: true,
            position: 0,
            started_at: 0,
        }
    }

    pub fn loaded(
        cartridge: Cartridge,
        path: Option<std::path::PathBuf>,
        read_only: bool,
    ) -> Drive {
        Drive {
            cartridge: Some(cartridge),
            path,
            read_only,
            position: 0,
            started_at: 0,
        }
    }

    /// Whether the drive will let the machine write: both the tab on the
    /// cartridge and what the emulator was told have to allow it.
    pub fn writable(&self) -> bool {
        !self.read_only && self.cartridge.as_ref().is_some_and(|c| !c.write_protected)
    }

    pub fn sector(&self) -> usize {
        self.position
    }
}

/// Where the head is within a sector: the gap first, then the header, then a
/// second gap, then the record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    Gap,
    Header(usize),
    Record(usize),
}

#[derive(Clone)]
pub struct If1 {
    /// The shadow ROM, once somebody has supplied one. Without it the
    /// interface is a box that pages in nothing, which is exactly what the
    /// machine would do with an empty socket.
    pub rom: Option<Vec<u8>>,
    /// Whether the shadow ROM is paged in over the machine's own.
    pub paged: bool,
    pub drives: Vec<Drive>,
    /// Which drive the last write to $EF selected, 1-based; 0 is none.
    pub selected: usize,
    /// The comms and write-protect bits of the control port, kept as written
    /// so a read gives back what the ROM expects to see.
    control: u8,
    /// The machine's clock, as the bus last said.
    now: u64,
    /// When the motor of the selected drive started, so the tape's position
    /// follows the machine's own time rather than a count of accesses.
    motor_since: Option<u64>,
    /// What the drive is handing over: which byte of which part of the sector.
    at: usize,
    /// Bytes written by the machine, waiting to go into the sector under the
    /// head.
    writing: Vec<u8>,
}

impl Default for If1 {
    fn default() -> Self {
        If1::new(1)
    }
}

impl If1 {
    pub fn new(drives: usize) -> If1 {
        If1 {
            rom: None,
            paged: false,
            drives: (0..drives.clamp(1, MAX_DRIVES))
                .map(|_| Drive::empty())
                .collect(),
            selected: 0,
            control: 0xFF,
            now: 0,
            motor_since: None,
            at: 0,
            writing: Vec::new(),
        }
    }

    /// How many drives are on the chain.
    pub fn drive_count(&self) -> usize {
        self.drives.len()
    }

    /// Add or remove drives, keeping the cartridges in the ones that stay.
    pub fn set_drive_count(&mut self, count: usize) {
        let count = count.clamp(1, MAX_DRIVES);
        while self.drives.len() < count {
            self.drives.push(Drive::empty());
        }
        self.drives.truncate(count);
        if self.selected > count {
            self.selected = 0;
        }
    }

    /// The clock, and with it the tape's position.
    pub fn at(&mut self, now: u64) {
        // A clock that has gone backwards — a reset, a snapshot — is not time
        // passing, and the tape has not moved.
        if now < self.now {
            self.motor_since = Some(now);
        }
        self.now = now;
        self.advance();
    }

    /// Whether a drive is turning, which is what lights the lamp on the front.
    pub fn motor_on(&self) -> bool {
        self.selected > 0 && self.motor_since.is_some()
    }

    /// The drive the ROM has selected, if there is one.
    pub fn drive(&self) -> Option<&Drive> {
        self.selected
            .checked_sub(1)
            .and_then(|i| self.drives.get(i))
    }

    fn drive_mut(&mut self) -> Option<&mut Drive> {
        self.selected
            .checked_sub(1)
            .and_then(|i| self.drives.get_mut(i))
    }

    /// Where the head is in the sector under it.
    pub fn head(&self) -> Where {
        let Some(drive) = self.drive() else {
            return Where::Gap;
        };
        let Some(cartridge) = drive.cartridge.as_ref() else {
            return Where::Gap;
        };
        if cartridge.sectors.is_empty() {
            return Where::Gap;
        }
        // The gap first, so the ROM sees the tape stop and start rather than a
        // continuous stream of bytes.
        let through = self.through_sector();
        if through < GAP {
            return Where::Gap;
        }
        let after_gap = (through - GAP) / (1.0 - GAP);
        let byte = (after_gap * SECTOR_LEN as f32) as usize;
        if byte < crate::microdrive::HEADER_LEN {
            Where::Header(byte)
        } else {
            Where::Record(byte - crate::microdrive::HEADER_LEN)
        }
    }

    /// How far the tape has run into the sector under the head, 0 to 1.
    fn through_sector(&self) -> f32 {
        let Some(since) = self.motor_since else {
            return 0.0;
        };
        let elapsed = self.now.saturating_sub(since);
        (elapsed % SECTOR_T) as f32 / SECTOR_T as f32
    }

    /// Move the tape on, if the motor is turning.
    fn advance(&mut self) {
        let Some(since) = self.motor_since else {
            return;
        };
        let elapsed = self.now.saturating_sub(since);
        let sectors = (elapsed / SECTOR_T) as usize;
        if sectors == 0 {
            return;
        }
        self.motor_since = Some(self.now - elapsed % SECTOR_T);
        let selected = self.selected;
        if let Some(drive) = selected.checked_sub(1).and_then(|i| self.drives.get_mut(i)) {
            if let Some(cartridge) = drive.cartridge.as_ref() {
                if !cartridge.sectors.is_empty() {
                    drive.position = (drive.position + sectors) % cartridge.sectors.len();
                }
            }
        }
    }

    /// A write to $EF: the motor chain and the comms lines.
    ///
    /// Bit 0 is the motor line, and it is shifted along the chain: writing a 1
    /// then a 0 starts drive 1, and each further write moves the running drive
    /// one further down. That is how one bit selects one of eight.
    pub fn write_control(&mut self, value: u8) {
        let motor = value & 0x01 != 0;
        if motor && self.selected == 0 {
            self.selected = 1;
            self.motor_since = Some(self.now);
            self.at = 0;
        } else if motor {
            self.selected = (self.selected + 1).min(self.drives.len());
            self.motor_since = Some(self.now);
            self.at = 0;
        } else if !motor && value & 0x02 == 0 {
            // The comms clock low with no motor bit: the chain is cleared.
            self.selected = 0;
            self.motor_since = None;
        }
        self.control = value;
    }

    /// A read of $EF: what the drive says about itself.
    ///
    /// Bit 0 is the gap line and bit 1 the sync line — the ROM waits for a gap
    /// and then for sync to find a sector — bit 2 is the write-protect tab,
    /// and the rest are the comms lines nothing here drives.
    pub fn read_status(&mut self) -> u8 {
        let mut status = 0xFF;
        let head = self.head();
        let Some(drive) = self.drive() else {
            // Nothing selected: no gap, no sync, which is what an empty chain
            // looks like.
            return status;
        };
        if drive.cartridge.is_none() {
            return status;
        }
        // The lines are active low, as they are on the hardware.
        match head {
            Where::Gap => status &= !0x01,
            _ => status &= !0x02,
        }
        if drive.writable() {
            status |= 0x04;
        } else {
            status &= !0x04;
        }
        status
    }

    /// A read of $E7: the byte under the head.
    pub fn read_data(&mut self) -> u8 {
        let head = self.head();
        let Some(drive) = self.drive() else {
            return 0xFF;
        };
        let Some(cartridge) = drive.cartridge.as_ref() else {
            return 0xFF;
        };
        let sector = &cartridge.sectors[drive.position % cartridge.sectors.len()];
        match head {
            Where::Gap => 0xFF,
            Where::Header(at) => sector.header.get(at).copied().unwrap_or(0xFF),
            Where::Record(at) => sector.record.get(at).copied().unwrap_or(0xFF),
        }
    }

    /// A write to $E7: a byte going onto the tape.
    ///
    /// Held until a sector's worth has arrived rather than written a byte at a
    /// time, because what the ROM is writing is a whole record and half of one
    /// on the tape is a sector nobody can read.
    pub fn write_data(&mut self, value: u8) {
        if !self.drive().is_some_and(|d| d.writable()) {
            return;
        }
        self.writing.push(value);
        if self.writing.len() < crate::microdrive::RECORD_LEN {
            return;
        }
        let bytes = std::mem::take(&mut self.writing);
        let position = self.drive().map(|d| d.position).unwrap_or(0);
        if let Some(drive) = self.drive_mut() {
            if let Some(cartridge) = drive.cartridge.as_mut() {
                if !cartridge.sectors.is_empty() {
                    let at = position % cartridge.sectors.len();
                    cartridge.sectors[at]
                        .record
                        .copy_from_slice(&bytes[..crate::microdrive::RECORD_LEN]);
                    cartridge.dirty = true;
                    cartridge.revision += 1;
                }
            }
        }
    }

    /// Whether a fetch from this address pages the shadow ROM in or out.
    ///
    /// $0008 is the ROM's error handler and $1708 its close-files hook: a
    /// program that has just used a microdrive command lands on one of them,
    /// and that is the moment the interface takes over. $0700 is where its own
    /// ROM returns to the machine's.
    pub fn on_fetch(&mut self, addr: u16) -> bool {
        if self.rom.is_none() {
            return false;
        }
        let was = self.paged;
        match addr {
            0x0008 | 0x1708 => self.paged = true,
            0x0700 => self.paged = false,
            _ => {}
        }
        was != self.paged
    }

    /// The byte the shadow ROM has at an address, if it is paged in and covers
    /// it. The interface's ROM is 8K over the bottom of the machine's.
    pub fn rom_byte(&self, addr: u16) -> Option<u8> {
        if !self.paged || addr >= 0x2000 {
            return None;
        }
        self.rom
            .as_ref()
            .and_then(|rom| rom.get(addr as usize))
            .copied()
    }

    /// What the reset line does: the ROM is paged out and the drives stop.
    pub fn reset(&mut self) {
        self.paged = false;
        self.selected = 0;
        self.motor_since = None;
        self.control = 0xFF;
        self.at = 0;
        self.writing.clear();
        self.now = 0;
        for drive in &mut self.drives {
            drive.position = 0;
            drive.started_at = 0;
        }
    }
}
