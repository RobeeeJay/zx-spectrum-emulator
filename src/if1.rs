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
//! The second half is the drives. A cartridge is a loop of tape in 543-byte
//! sectors — fifteen bytes of header, then the 528-byte record — and the ROM
//! reads one by waiting for a gap, then for sync, then taking the block a byte
//! at a time. What is here is that loop, driven by the ROM's own accesses
//! rather than by the clock: see `GAP_READS` for why.
//!
//! Ports: $E7 is the data register, and $EF the control and status one — read
//! for status, written to drive the motors. $F7 is the RS232 and network side,
//! which is not emulated: nothing is on the other end of it.
//!
//! Which bit of the status port is which, and which way up the motor line is,
//! were read off the Interface 1's own ROM — its sector-finding loop at $165A
//! and its write-protect test at $136C — after the first version of this file
//! guessed and nothing worked with the real thing in the socket.

use crate::microdrive::{Cartridge, SECTOR_LEN};

/// How many drives an Interface 1 can address.
pub const MAX_DRIVES: usize = 8;

/// How many bytes of a block pass the head between one look at the status port
/// and the next.
///
/// The tape does not move on the clock here; it moves as the ROM reads it. A
/// microdrive block is read with `INIR` — 21 T-states a byte — where the tape
/// itself hands over a byte every 170 or so, and a tape that ran at its own
/// speed would give the same byte to a dozen reads in a row. On the hardware
/// the interface paces the CPU; here the reads pace the tape, which comes to
/// the same thing from the ROM's side and is what Fuse does as well.
const GAP_READS: u8 = 15;

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
    /// Whether this drive's motor is turning. One bit walks down the chain,
    /// so more than one can be on at once and the ROM makes sure it is not.
    pub motor_on: bool,
    /// The byte under the head, counted across the whole cartridge: sector
    /// times 543, plus the offset into it.
    head: usize,
    /// How many bytes of the block under the head have been handed over.
    transfered: usize,
    /// How long the block under the head is: 15 for a header, 528 for a
    /// record.
    max_bytes: usize,
    /// The gap and sync lines, counted down in reads of the status port.
    gap: u8,
    sync: u8,
    /// The last byte handed over, which is what the port keeps saying once the
    /// block has run out.
    last: u8,
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
            motor_on: false,
            head: 0,
            transfered: 0,
            max_bytes: crate::microdrive::HEADER_LEN,
            gap: GAP_READS,
            sync: GAP_READS,
            last: 0xFF,
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
            ..Drive::empty()
        }
    }

    /// Whether the drive will let the machine write: both the tab on the
    /// cartridge and what the emulator was told have to allow it.
    pub fn writable(&self) -> bool {
        !self.read_only && self.cartridge.as_ref().is_some_and(|c| !c.write_protected)
    }

    /// Which sector is under the head.
    pub fn sector(&self) -> usize {
        self.head / SECTOR_LEN
    }

    /// Where in that sector the head is, for the window to draw.
    pub fn head_at(&self) -> Where {
        if self.gap > 0 {
            return Where::Gap;
        }
        let off = self.head % SECTOR_LEN;
        if off < crate::microdrive::HEADER_LEN {
            Where::Header(off)
        } else {
            Where::Record(off - crate::microdrive::HEADER_LEN)
        }
    }

    /// The tape as a flat run of bytes, which is how the head reads it.
    fn byte(&self, at: usize) -> u8 {
        let Some(cartridge) = self.cartridge.as_ref() else {
            return 0xFF;
        };
        if cartridge.sectors.is_empty() {
            return 0xFF;
        }
        let sector = &cartridge.sectors[(at / SECTOR_LEN) % cartridge.sectors.len()];
        let off = at % SECTOR_LEN;
        if off < crate::microdrive::HEADER_LEN {
            sector.header[off]
        } else {
            sector.record[off - crate::microdrive::HEADER_LEN]
        }
    }

    fn set_byte(&mut self, at: usize, value: u8) {
        let Some(cartridge) = self.cartridge.as_mut() else {
            return;
        };
        if cartridge.sectors.is_empty() {
            return;
        }
        let n = cartridge.sectors.len();
        let sector = &mut cartridge.sectors[(at / SECTOR_LEN) % n];
        let off = at % SECTOR_LEN;
        if off < crate::microdrive::HEADER_LEN {
            sector.header[off] = value;
        } else {
            sector.record[off - crate::microdrive::HEADER_LEN] = value;
        }
        cartridge.dirty = true;
        cartridge.revision += 1;
    }

    fn advance(&mut self) {
        let len = self
            .cartridge
            .as_ref()
            .map_or(1, |c| c.sectors.len().max(1));
        self.head = (self.head + 1) % (len * SECTOR_LEN);
    }

    /// Put the head at the start of the next block, which is what a write to
    /// the control port does: the ROM has finished with whatever it was
    /// reading and is looking for the next thing.
    fn restart(&mut self) {
        while !self.head.is_multiple_of(SECTOR_LEN)
            && self.head % SECTOR_LEN != crate::microdrive::HEADER_LEN
        {
            self.advance();
        }
        self.transfered = 0;
        self.max_bytes = if self.head.is_multiple_of(SECTOR_LEN) {
            crate::microdrive::HEADER_LEN
        } else {
            crate::microdrive::RECORD_LEN
        };
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
    /// The comms clock, whose falling edge walks the motor bit down the chain.
    comms_clk: bool,
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
            comms_clk: false,
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

    /// The machine's clock. Nothing here moves with it any more — the tape
    /// moves as the ROM reads it — but the bus still says the time, and a
    /// snapshot or a reset may put it back.
    pub fn at(&mut self, _now: u64) {}

    /// Whether a drive is turning, which is what lights the lamp on the front.
    pub fn motor_on(&self) -> bool {
        self.drives.iter().any(|d| d.motor_on)
    }

    /// The drive the ROM has selected, if there is one.
    pub fn drive(&self) -> Option<&Drive> {
        self.drives.iter().find(|d| d.motor_on)
    }

    /// Where the head is in the sector under it.
    pub fn head(&self) -> Where {
        self.drive().map_or(Where::Gap, |d| d.head_at())
    }

    /// A write to $EF: the motor chain and the comms lines.
    ///
    /// Bit 0 is the motor line and bit 1 the comms clock. On the clock's
    /// falling edge every drive takes its neighbour's motor state and drive 1
    /// takes the motor bit — which is *low* for a motor that is to run, as the
    /// ROM's own writes say ($EE to start one). That is how one bit selects
    /// one of eight: the ROM clocks it along the chain until it is under the
    /// drive it wants.
    pub fn write_control(&mut self, value: u8) {
        let clock = value & 0x02 != 0;
        if !clock && self.comms_clk {
            for i in (1..self.drives.len()).rev() {
                self.drives[i].motor_on = self.drives[i - 1].motor_on;
            }
            self.drives[0].motor_on = value & 0x01 == 0;
            self.selected = self
                .drives
                .iter()
                .position(|d| d.motor_on)
                .map_or(0, |i| i + 1);
        }
        self.comms_clk = clock;
        self.control = value;
        // Every write to the control port sends the head to the start of a
        // block: the ROM writes here when it has finished with one and is
        // looking for the next.
        for drive in &mut self.drives {
            if drive.motor_on {
                drive.restart();
            }
        }
    }

    /// A read of $EF: what the drive says about itself.
    ///
    /// Which bit is which was read off the Interface 1's own ROM rather than
    /// off a table: the sector-finding loop at $165A waits for eight reads
    /// with bit 2 **set**, then six with it clear, then for bit 1 to go low —
    /// so bit 2 is the gap line, high while the gap between two sectors runs
    /// past the head, and bit 1 the sync line, low once the block's preamble
    /// is under it. Bit 0 is the write-protect tab, and the ROM refuses to
    /// write when it reads *clear* ($136C). The rest are the comms lines
    /// nothing here drives.
    pub fn read_status(&mut self) -> u8 {
        let mut status = 0xFF;
        for drive in self.drives.iter_mut().filter(|d| d.motor_on) {
            if drive.cartridge.is_none() {
                continue;
            }
            if drive.gap > 0 {
                // Tape between two blocks: both lines high.
                drive.gap -= 1;
            } else {
                status &= !0x06;
                if drive.sync > 0 {
                    drive.sync -= 1;
                } else {
                    drive.gap = GAP_READS;
                    drive.sync = GAP_READS;
                }
            }
            if !drive.writable() {
                status &= !0x01;
            }
        }
        status
    }

    /// A read of $E7: the byte under the head, and the tape moves on.
    pub fn read_data(&mut self) -> u8 {
        let mut byte = 0xFF;
        for drive in self.drives.iter_mut().filter(|d| d.motor_on) {
            if drive.cartridge.is_none() {
                continue;
            }
            if drive.transfered < drive.max_bytes {
                drive.last = drive.byte(drive.head);
                drive.advance();
            }
            drive.transfered += 1;
            byte &= drive.last;
        }
        byte
    }

    /// A write to $E7: a byte going onto the tape.
    ///
    /// The ROM writes twelve bytes of preamble before every block — ten zeros
    /// and two $FFs, which is what the head finds sync on — and they are not
    /// part of the block, so they are counted and thrown away. What follows
    /// goes onto the tape a byte at a time under the head.
    pub fn write_data(&mut self, value: u8) {
        const PREAMBLE: usize = 12;
        for drive in self.drives.iter_mut().filter(|d| d.motor_on) {
            if !drive.writable() {
                continue;
            }
            if drive.transfered >= PREAMBLE && drive.transfered < drive.max_bytes + PREAMBLE {
                let at = drive.head;
                drive.set_byte(at, value);
                drive.advance();
            }
            drive.transfered += 1;
        }
    }

    /// Whether a fetch from this address pages the shadow ROM in, before the
    /// byte is read.
    ///
    /// $0008 is the ROM's error handler and $1708 its close-files hook: a
    /// program that has just used a microdrive command lands on one of them,
    /// and that is the moment the interface takes over. The byte executed
    /// there has to be the interface's own, so this is taken before the fetch.
    pub fn on_fetch(&mut self, addr: u16) -> bool {
        if self.rom.is_none() {
            return false;
        }
        let was = self.paged;
        if matches!(addr, 0x0008 | 0x1708) {
            self.paged = true;
        }
        was != self.paged
    }

    /// And the fetch that pages it out, taken after the byte has been read.
    ///
    /// $0700 in the shadow ROM is a `RET`, and that is how a microdrive
    /// routine hands back to the machine's own ROM: it jumps there, the `RET`
    /// runs out of the shadow ROM, and the ROM is gone by the time the return
    /// address is fetched. Paging out before the read instead runs whatever
    /// the machine's ROM happens to hold at $0700 — $71 on a 48K, the middle
    /// of an unrelated routine — and the Interface 1's initialisation goes
    /// round for ever.
    pub fn after_fetch(&mut self, addr: u16) -> bool {
        if self.rom.is_none() || addr != 0x0700 {
            return false;
        }
        let was = self.paged;
        self.paged = false;
        was
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
        self.control = 0xFF;
        self.comms_clk = false;
        for drive in &mut self.drives {
            drive.motor_on = false;
            drive.head = 0;
            drive.transfered = 0;
            drive.max_bytes = crate::microdrive::HEADER_LEN;
            drive.gap = GAP_READS;
            drive.sync = GAP_READS;
        }
    }
}
