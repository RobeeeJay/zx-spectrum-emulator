//! The +3's disk controller: a µPD765A, as far as +3DOS can tell.
//!
//! The chip is driven through two ports — a status register the program polls
//! and a data register it pushes bytes at — and every command is the same
//! three phases: the program writes a command and its parameters, then the
//! data goes one way or the other, then the program reads the result bytes
//! back. What is here is that state machine and the commands +3DOS uses.
//!
//! Nothing is timed. A real controller makes the program wait while the head
//! steps and the disk turns, and reports "not ready" until the motor is up to
//! speed; this one answers at once. +3DOS polls rather than counting, so it
//! cannot tell — but a program that measures the wait could, and that is worth
//! knowing before trusting this with a protected disk.

use crate::disk::Disk;

/// Main status register bits, as the datasheet names them.
pub const RQM: u8 = 0x80;
pub const DIO: u8 = 0x40;
pub const EXM: u8 = 0x20;
pub const CB: u8 = 0x10;

/// A disk in a drive, and what may be done to it.
#[derive(Clone)]
pub struct Drive {
    pub disk: Disk,
    /// Where it came from, and where writes go. None for a disk that has never
    /// been saved.
    pub path: Option<std::path::PathBuf>,
    /// A disk mounted read-only takes writes as far as the controller is
    /// concerned and refuses them at the surface, which is what a write-
    /// protected disk does: the program is told, rather than the write quietly
    /// going nowhere.
    pub write_protected: bool,
}

impl Drive {
    pub fn new(disk: Disk, path: Option<std::path::PathBuf>, write_protected: bool) -> Drive {
        Drive {
            disk,
            path,
            write_protected,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Waiting for a command byte, or for the parameters of one.
    Command,
    /// Handing bytes to the program.
    Reading,
    /// Taking bytes from it.
    Writing,
    /// Taking the four bytes an ID field is made of, per sector, while
    /// formatting.
    Formatting,
    /// Handing back the seven bytes that say how it went.
    Result,
}

#[derive(Clone)]
pub struct Fdc {
    /// A: and B:. The +3 has one drive built in and a socket for another.
    pub drives: [Option<Drive>; 2],
    /// Whether the motor is turning, from bit 3 of port $1FFD.
    pub motor: bool,
    /// Commands completed, and how many of them ended abnormally. Kept
    /// because "did the machine actually talk to the disk" is otherwise
    /// invisible from outside, and it is the first thing worth knowing when a
    /// disk will not load.
    pub commands: u32,
    pub errors: u32,
    /// The last command byte, for the same reason.
    pub last_command: Option<u8>,

    phase: Phase,
    /// The command and its parameters as they arrive.
    command: Vec<u8>,
    /// How many parameter bytes the command in hand still wants.
    wanted: usize,
    /// Bytes going to or from the program.
    buffer: Vec<u8>,
    at: usize,
    /// Where the heads are, per drive.
    pcn: [u8; 2],
    /// Which drive and head the command in hand is about.
    unit: usize,
    head: u8,
    /// Set by a seek or recalibrate, read back by SENSE INTERRUPT STATUS.
    seek_done: Option<u8>,
    /// While a read or write is going on: which sector is being transferred,
    /// and where it ends.
    sector: Option<(u8, u8, u8)>,
    last_sector: u8,
    /// The result bytes, once there is something to say.
    result: Vec<u8>,
    /// While formatting: the track being written and the sectors so far.
    formatting: Option<(u8, u8, u8, u8, u8)>,
    format_sectors: Vec<[u8; 4]>,
}

impl Default for Fdc {
    fn default() -> Self {
        Fdc::new()
    }
}

impl Fdc {
    pub fn new() -> Fdc {
        Fdc {
            drives: [None, None],
            motor: false,
            commands: 0,
            errors: 0,
            last_command: None,
            phase: Phase::Command,
            command: Vec::new(),
            wanted: 0,
            buffer: Vec::new(),
            at: 0,
            pcn: [0, 0],
            unit: 0,
            head: 0,
            seek_done: None,
            sector: None,
            last_sector: 0,
            result: Vec::new(),
            formatting: None,
            format_sectors: Vec::new(),
        }
    }

    /// The status register the program polls: whether the controller wants a
    /// byte, which way the data is going, and whether it is busy.
    pub fn status(&self) -> u8 {
        match self.phase {
            Phase::Command => RQM | if self.command.is_empty() { 0 } else { CB },
            Phase::Reading => RQM | DIO | EXM | CB,
            Phase::Writing | Phase::Formatting => RQM | EXM | CB,
            Phase::Result => RQM | DIO | CB,
        }
    }

    /// Whether a drive has a disk in it and the motor is turning. A drive with
    /// no disk is not ready, which is how +3DOS knows to ask for one.
    fn ready(&self, unit: usize) -> bool {
        self.motor && self.drives.get(unit).map(|d| d.is_some()).unwrap_or(false)
    }

    /// A byte written to the data register.
    pub fn write(&mut self, value: u8) {
        match self.phase {
            Phase::Command => self.take_command_byte(value),
            Phase::Writing => {
                // `at` holds how long the sector is; the buffer was made that
                // size when the write began.
                self.buffer.push(value);
                if self.buffer.len() == self.at {
                    self.finish_write();
                }
            }
            Phase::Formatting => {
                self.buffer.push(value);
                if self.buffer.len().is_multiple_of(4) {
                    let n = self.buffer.len();
                    let id = [
                        self.buffer[n - 4],
                        self.buffer[n - 3],
                        self.buffer[n - 2],
                        self.buffer[n - 1],
                    ];
                    self.format_sectors.push(id);
                    if let Some((_, _, sectors, _, _)) = self.formatting {
                        if self.format_sectors.len() >= sectors as usize {
                            self.finish_format();
                        }
                    }
                }
            }
            // A byte written while the controller is talking is thrown away,
            // which is what the chip does.
            Phase::Reading | Phase::Result => {}
        }
    }

    /// A byte read from the data register.
    pub fn read(&mut self) -> u8 {
        match self.phase {
            Phase::Reading => {
                let byte = self.buffer.get(self.at).copied().unwrap_or(0);
                self.at += 1;
                if self.at >= self.buffer.len() {
                    self.finish_read();
                }
                byte
            }
            Phase::Result => {
                let byte = self.result.first().copied().unwrap_or(0);
                if !self.result.is_empty() {
                    self.result.remove(0);
                }
                if self.result.is_empty() {
                    self.phase = Phase::Command;
                }
                byte
            }
            _ => 0,
        }
    }

    /// How many parameter bytes each command takes, and whether it is one this
    /// controller knows.
    fn parameters(command: u8) -> Option<usize> {
        Some(match command & 0x1F {
            0x02 => 8, // READ TRACK
            0x03 => 2, // SPECIFY
            0x04 => 1, // SENSE DRIVE STATUS
            0x05 => 8, // WRITE DATA
            0x06 => 8, // READ DATA
            0x07 => 1, // RECALIBRATE
            0x08 => 0, // SENSE INTERRUPT STATUS
            0x09 => 8, // WRITE DELETED DATA
            0x0A => 1, // READ ID
            0x0C => 8, // READ DELETED DATA
            0x0D => 5, // FORMAT TRACK
            0x0F => 2, // SEEK
            0x11 => 8, // SCAN EQUAL
            0x19 => 8, // SCAN LOW OR EQUAL
            0x1D => 8, // SCAN HIGH OR EQUAL
            _ => return None,
        })
    }

    fn take_command_byte(&mut self, value: u8) {
        if self.command.is_empty() {
            match Fdc::parameters(value) {
                Some(wanted) => {
                    self.command.push(value);
                    self.wanted = wanted;
                    if wanted == 0 {
                        self.execute();
                    }
                }
                // An invalid command is answered with $80 in ST0, which is
                // what the chip does rather than ignoring it.
                None => {
                    self.result = vec![0x80];
                    self.phase = Phase::Result;
                }
            }
            return;
        }
        self.command.push(value);
        if self.command.len() > self.wanted {
            self.execute();
        }
    }

    fn execute(&mut self) {
        let command = self.command[0];
        self.commands += 1;
        self.last_command = Some(command);
        let opcode = command & 0x1F;
        match opcode {
            0x03 => self.done_without_result(), // SPECIFY
            0x04 => self.sense_drive_status(),
            0x07 => self.recalibrate(),
            0x08 => self.sense_interrupt(),
            0x0A => self.read_id(),
            0x0F => self.seek(),
            0x05 | 0x09 => self.start_write(),
            0x06 | 0x0C | 0x02 => self.start_read(),
            0x0D => self.start_format(),
            _ => {
                // Something known but not implemented: say the command
                // finished abnormally rather than hanging the program.
                self.result = vec![0x40 | self.unit as u8, 0, 0, 0, 0, 0, 0];
                self.phase = Phase::Result;
                self.command.clear();
            }
        }
    }

    fn done_without_result(&mut self) {
        self.command.clear();
        self.phase = Phase::Command;
    }

    fn unit_and_head(&mut self) {
        let byte = self.command.get(1).copied().unwrap_or(0);
        self.unit = (byte & 0x03) as usize;
        self.head = (byte >> 2) & 1;
    }

    /// ST0: the unit, the head, and how the command ended.
    fn st0(&self, end: u8) -> u8 {
        end | ((self.head & 1) << 2)
            | (self.unit as u8 & 3)
            | if self.ready(self.unit) { 0 } else { 0x08 }
    }

    fn sense_drive_status(&mut self) {
        self.unit_and_head();
        let ready = self.ready(self.unit);
        let protected = self
            .drives
            .get(self.unit)
            .and_then(|d| d.as_ref())
            .map(|d| d.write_protected)
            .unwrap_or(false);
        // ST3: unit, head, two-sided, track 0, ready, write protected.
        let mut st3 = (self.unit as u8 & 3) | ((self.head & 1) << 2);
        if self.pcn[self.unit.min(1)] == 0 {
            st3 |= 0x10;
        }
        if ready {
            st3 |= 0x20;
        }
        if protected {
            st3 |= 0x40;
        }
        self.result = vec![st3];
        self.phase = Phase::Result;
        self.command.clear();
    }

    fn recalibrate(&mut self) {
        self.unit_and_head();
        self.pcn[self.unit.min(1)] = 0;
        // Seek End, and Equipment Check when there is no disk to find track 0
        // on: that is how the ROM knows the drive is empty.
        let end = if self.ready(self.unit) { 0x20 } else { 0x70 };
        self.seek_done = Some(self.st0(end));
        self.done_without_result();
    }

    fn seek(&mut self) {
        self.unit_and_head();
        let to = self.command.get(2).copied().unwrap_or(0);
        self.pcn[self.unit.min(1)] = to;
        let end = if self.ready(self.unit) { 0x20 } else { 0x60 };
        self.seek_done = Some(self.st0(end));
        self.done_without_result();
    }

    fn sense_interrupt(&mut self) {
        match self.seek_done.take() {
            Some(st0) => {
                let pcn = self.pcn[self.unit.min(1)];
                self.result = vec![st0, pcn];
            }
            // Nothing to report: $80, invalid, which is how a polling loop
            // knows to stop asking.
            None => self.result = vec![0x80],
        }
        self.phase = Phase::Result;
        self.command.clear();
    }

    fn read_id(&mut self) {
        self.unit_and_head();
        let track = self.pcn[self.unit.min(1)];
        let head = self.head;
        let found = self
            .drives
            .get(self.unit)
            .and_then(|d| d.as_ref())
            .and_then(|d| d.disk.track(track, head))
            .and_then(|t| t.sectors.first())
            .map(|s| (s.c, s.h, s.r, s.n));
        match found {
            Some((c, h, r, n)) => {
                let st0 = self.st0(0);
                self.result = vec![st0, 0, 0, c, h, r, n];
            }
            None => {
                // Missing address mark: there is nothing readable there.
                let st0 = self.st0(0x40);
                self.result = vec![st0, 0x01, 0, track, head, 0, 0];
            }
        }
        self.phase = Phase::Result;
        self.command.clear();
    }

    /// The eight parameters a read or write carries: unit/head, C, H, R, N,
    /// EOT, GPL, DTL.
    fn transfer_parameters(&mut self) -> (u8, u8, u8, u8, u8) {
        self.unit_and_head();
        let c = self.command.get(2).copied().unwrap_or(0);
        let h = self.command.get(3).copied().unwrap_or(0);
        let r = self.command.get(4).copied().unwrap_or(0);
        let n = self.command.get(5).copied().unwrap_or(0);
        let eot = self.command.get(6).copied().unwrap_or(r);
        (c, h, r, n, eot)
    }

    fn start_read(&mut self) {
        let (c, h, r, _n, eot) = self.transfer_parameters();
        self.last_sector = eot;
        match self.sector_data(c, h, r) {
            Some(data) => {
                self.buffer = data;
                self.at = 0;
                self.sector = Some((c, h, r));
                self.phase = Phase::Reading;
            }
            None => self.transfer_failed(c, h, r),
        }
    }

    fn sector_data(&self, c: u8, h: u8, r: u8) -> Option<Vec<u8>> {
        let drive = self.drives.get(self.unit)?.as_ref()?;
        let track = drive.disk.track(self.pcn[self.unit.min(1)], self.head)?;
        // Matched on the identity in the address mark, not on where the sector
        // sits: a disk that numbers its sectors oddly is read correctly, and
        // one that lies about its cylinder is not.
        let sector = track
            .sectors
            .iter()
            .find(|s| s.r == r && s.c == c && (s.h == h || h == 0))?;
        Some(sector.data.clone())
    }

    /// The seven result bytes a read or write ends with.
    fn transfer_result(&mut self, st1: u8, st2: u8, c: u8, h: u8, r: u8, n: u8) {
        let end = if st1 == 0 && st2 == 0 { 0 } else { 0x40 };
        if end != 0 {
            self.errors += 1;
        }
        let st0 = self.st0(end);
        self.result = vec![st0, st1, st2, c, h, r, n];
        self.phase = Phase::Result;
        self.command.clear();
        self.sector = None;
    }

    fn transfer_failed(&mut self, c: u8, h: u8, r: u8) {
        // No data: either the drive is empty or the sector is not there.
        let st1 = if self.ready(self.unit) { 0x04 } else { 0x00 };
        self.transfer_result(st1, 0, c, h, r, 2);
    }

    fn finish_read(&mut self) {
        let Some((c, h, r)) = self.sector else {
            self.phase = Phase::Command;
            return;
        };
        // A multi-sector read carries on to the next one until it reaches the
        // last the command named.
        if r < self.last_sector {
            let next = r + 1;
            if let Some(data) = self.sector_data(c, h, next) {
                self.buffer = data;
                self.at = 0;
                self.sector = Some((c, h, next));
                return;
            }
        }
        self.transfer_result(0, 0, c, h, r.wrapping_add(1), 2);
    }

    fn start_write(&mut self) {
        let (c, h, r, n, eot) = self.transfer_parameters();
        self.last_sector = eot;
        let protected = self
            .drives
            .get(self.unit)
            .and_then(|d| d.as_ref())
            .map(|d| d.write_protected)
            .unwrap_or(false);
        if protected {
            // ST1 bit 1 is Not Writable, which is what a write-protected disk
            // answers. The program is told rather than the write going
            // nowhere quietly.
            self.transfer_result(0x02, 0, c, h, r, n);
            return;
        }
        let length = self
            .sector_data(c, h, r)
            .map(|d| d.len())
            .unwrap_or(128usize << (n.min(6) as usize));
        if self.sector_data(c, h, r).is_none() {
            self.transfer_failed(c, h, r);
            return;
        }
        self.buffer = Vec::with_capacity(length);
        self.at = length;
        self.sector = Some((c, h, r));
        self.phase = Phase::Writing;
    }

    fn finish_write(&mut self) {
        let Some((c, h, r)) = self.sector else {
            self.phase = Phase::Command;
            return;
        };
        let track_no = self.pcn[self.unit.min(1)];
        let head = self.head;
        let bytes = std::mem::take(&mut self.buffer);
        if let Some(drive) = self.drives.get_mut(self.unit).and_then(|d| d.as_mut()) {
            let mut wrote = false;
            if let Some(track) = drive.disk.track_mut(track_no, head) {
                let filler = track.filler;
                if let Some(sector) = track
                    .sectors
                    .iter_mut()
                    .find(|s| s.r == r && s.c == c && (s.h == h || h == 0))
                {
                    // However many bytes the program handed over, the sector
                    // is the length it was: a short write leaves the rest of
                    // it as the disk was formatted.
                    let length = sector.data.len();
                    sector.data = bytes;
                    sector.data.resize(length, filler);
                    wrote = true;
                }
            }
            drive.disk.dirty |= wrote;
        }
        if r < self.last_sector {
            let next = r + 1;
            if let Some(data) = self.sector_data(c, h, next) {
                self.at = data.len();
                self.buffer = Vec::with_capacity(self.at);
                self.sector = Some((c, h, next));
                return;
            }
        }
        self.transfer_result(0, 0, c, h, r.wrapping_add(1), 2);
    }

    fn start_format(&mut self) {
        self.unit_and_head();
        let n = self.command.get(2).copied().unwrap_or(2);
        let sectors = self.command.get(3).copied().unwrap_or(9);
        let gap3 = self.command.get(4).copied().unwrap_or(0x4E);
        let filler = self.command.get(5).copied().unwrap_or(crate::disk::FILLER);
        let protected = self
            .drives
            .get(self.unit)
            .and_then(|d| d.as_ref())
            .map(|d| d.write_protected)
            .unwrap_or(false);
        if protected || !self.ready(self.unit) {
            let st1 = if protected { 0x02 } else { 0 };
            self.transfer_result(st1, 0, 0, 0, 0, n);
            return;
        }
        self.formatting = Some((n, sectors, sectors, gap3, filler));
        self.format_sectors.clear();
        self.buffer.clear();
        self.phase = Phase::Formatting;
    }

    fn finish_format(&mut self) {
        let Some((n, _, _, gap3, filler)) = self.formatting.take() else {
            self.phase = Phase::Command;
            return;
        };
        let track_no = self.pcn[self.unit.min(1)];
        let head = self.head;
        let ids = std::mem::take(&mut self.format_sectors);
        let length = 128usize << (n.min(6) as usize);
        if let Some(drive) = self.drives.get_mut(self.unit).and_then(|d| d.as_mut()) {
            let sectors: Vec<crate::disk::Sector> = ids
                .iter()
                .map(|id| crate::disk::Sector {
                    c: id[0],
                    h: id[1],
                    r: id[2],
                    n: id[3],
                    st1: 0,
                    st2: 0,
                    data: vec![filler; length],
                })
                .collect();
            match drive.disk.track_mut(track_no, head) {
                Some(track) => {
                    track.sector_size = n;
                    track.gap3 = gap3;
                    track.filler = filler;
                    track.sectors = sectors;
                }
                None => drive.disk.tracks.push(crate::disk::Track {
                    track: track_no,
                    side: head,
                    sector_size: n,
                    gap3,
                    filler,
                    sectors,
                }),
            }
            drive.disk.dirty = true;
        }
        self.buffer.clear();
        self.transfer_result(0, 0, track_no, head, 1, n);
    }
}
