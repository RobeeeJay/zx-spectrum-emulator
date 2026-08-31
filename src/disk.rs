//! Three-inch disks, as the +3 kept them: the DSK file both emulators settled
//! on, and the sectors inside it.
//!
//! A DSK holds what the disk controller would have read off the surface rather
//! than a filesystem: tracks, and in each track a list of sectors with the
//! identity the controller matches on — cylinder, head, record, size — and the
//! bytes. That is why a copy-protected disk works: its odd sector numbers and
//! deliberate errors are in the file the same as on the disk.
//!
//! Two versions of the file exist. The original writes one track length for
//! the whole disk; the extended one writes a length per track, which is what
//! anything with unusual tracks needs. Both are read here; what is written is
//! always the original, since everything this emulator makes is regular.

/// What a track header says it is.
const TRACK_MARK: &[u8] = b"Track-Info\r\n";
const STANDARD_MARK: &[u8] = b"MV - CPC";
const EXTENDED_MARK: &[u8] = b"EXTENDED CPC DSK File";

/// The filler byte a formatted disk is full of, and the byte an empty CP/M
/// directory entry starts with — which is why a disk formatted with it reads
/// as empty rather than as full of rubbish.
pub const FILLER: u8 = 0xE5;

/// The +3's own format: forty tracks, one side, nine 512-byte sectors, and
/// sector numbers starting at $C1. The other format the machine knows —
/// system format, numbered from $41 — carries CP/M and is not what FORMAT
/// makes.
pub const TRACKS: u8 = 40;
pub const SECTORS: u8 = 9;
pub const SECTOR_SIZE: usize = 512;
pub const FIRST_SECTOR_ID: u8 = 0xC1;

/// One sector: what the controller matches on, what it reads back, and the
/// bytes themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sector {
    /// Cylinder, head, record and size, as the address mark on the disk says
    /// them. A protected disk lies here on purpose, so these are kept as they
    /// were found rather than worked out from where the sector sits.
    pub c: u8,
    pub h: u8,
    pub r: u8,
    pub n: u8,
    /// The two status bytes the controller gives back after reading it. A
    /// sector with a deliberate CRC error carries it here.
    pub st1: u8,
    pub st2: u8,
    pub data: Vec<u8>,
}

impl Sector {
    /// How long the data should be for the size code, which is not always how
    /// long it is: a weak or over-long sector is written out at its real
    /// length and the size code stays whatever the disk says.
    pub fn declared_len(&self) -> usize {
        128usize << (self.n.min(6) as usize)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub track: u8,
    pub side: u8,
    /// Size code shared by the sectors when they were formatted together.
    pub sector_size: u8,
    pub gap3: u8,
    pub filler: u8,
    pub sectors: Vec<Sector>,
}

#[derive(Clone, Debug)]
pub struct Disk {
    pub creator: String,
    pub tracks_per_side: u8,
    pub sides: u8,
    /// Tracks in the order the file holds them: track 0 side 0, track 0 side
    /// 1, track 1 side 0 and so on.
    pub tracks: Vec<Track>,
    /// Whether anything has been written since it was mounted or last saved.
    pub dirty: bool,
    /// Bumped whenever a sector changes, so anything drawing the disk knows
    /// its picture is out of date without comparing every byte.
    pub revision: u64,
}

impl Disk {
    /// A blank disk, formatted the way the +3's own FORMAT command formats
    /// one: data format, nine sectors a track numbered from $C1, every byte
    /// $E5. An empty directory is what $E5 means to +3DOS, so this catalogues
    /// as an empty disk rather than as a disk that needs formatting.
    pub fn blank(creator: &str) -> Disk {
        let tracks = (0..TRACKS)
            .map(|track| Track {
                track,
                side: 0,
                sector_size: 2,
                gap3: 0x4E,
                filler: FILLER,
                sectors: (0..SECTORS)
                    .map(|i| Sector {
                        c: track,
                        h: 0,
                        r: FIRST_SECTOR_ID + i,
                        n: 2,
                        st1: 0,
                        st2: 0,
                        data: vec![FILLER; SECTOR_SIZE],
                    })
                    .collect(),
            })
            .collect();
        Disk {
            creator: creator.chars().take(14).collect(),
            tracks_per_side: TRACKS,
            sides: 1,
            tracks,
            dirty: false,
            revision: 0,
        }
    }

    /// The track for a physical position, if the disk has one there. A disk
    /// with fewer tracks than the head can reach is normal: seeking past the
    /// last one finds nothing, which is what the controller reports.
    pub fn track(&self, track: u8, side: u8) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|t| t.track == track && t.side == side)
    }

    pub fn track_mut(&mut self, track: u8, side: u8) -> Option<&mut Track> {
        self.tracks
            .iter_mut()
            .find(|t| t.track == track && t.side == side)
    }

    /// Read a DSK file, either version of it.
    pub fn parse(data: &[u8]) -> Result<Disk, String> {
        if data.len() < 0x100 {
            return Err("too short to be a disk image".into());
        }
        let extended = data.starts_with(EXTENDED_MARK);
        if !extended && !data.starts_with(STANDARD_MARK) {
            return Err("not a DSK file: no \"MV - CPC\" or \"EXTENDED CPC DSK File\"".into());
        }
        let creator: String = data[0x22..0x30]
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| *b as char)
            .collect();
        let tracks_per_side = data[0x30];
        let sides = data[0x31].max(1);
        let count = tracks_per_side as usize * sides as usize;

        // Where each track starts, worked out from the lengths: one length for
        // the whole disk in the old format, one per track in the new. A track
        // whose length is zero is not on the disk at all — an unformatted
        // track, which is how a protected disk says "nothing here".
        let mut offsets = Vec::with_capacity(count);
        let mut at = 0x100usize;
        for i in 0..count {
            let length = if extended {
                data.get(0x34 + i).map_or(0, |b| *b as usize * 256)
            } else {
                u16::from_le_bytes([data[0x32], data[0x33]]) as usize
            };
            if length == 0 {
                offsets.push(None);
                continue;
            }
            offsets.push(Some(at));
            at += length;
        }

        let mut tracks = Vec::new();
        for offset in offsets.into_iter().flatten() {
            if offset + 0x100 > data.len() {
                break;
            }
            let header = &data[offset..offset + 0x100];
            if !header.starts_with(TRACK_MARK) {
                return Err(format!("no track header at ${offset:X}"));
            }
            let track = header[0x10];
            let side = header[0x11];
            let sector_size = header[0x14];
            let sector_count = header[0x15] as usize;
            let gap3 = header[0x16];
            let filler = header[0x17];

            let mut sectors = Vec::with_capacity(sector_count);
            let mut data_at = offset + 0x100;
            for s in 0..sector_count {
                let info = &header[0x18 + s * 8..0x18 + s * 8 + 8];
                let (c, h, r, n) = (info[0], info[1], info[2], info[3]);
                // The extended format puts the real length here, which is the
                // whole point of it: a sector can be longer or shorter than
                // its size code claims.
                let length = if extended {
                    u16::from_le_bytes([info[6], info[7]]) as usize
                } else {
                    128usize << (n.min(6) as usize)
                };
                let end = (data_at + length).min(data.len());
                sectors.push(Sector {
                    c,
                    h,
                    r,
                    n,
                    st1: info[4],
                    st2: info[5],
                    data: data[data_at.min(data.len())..end].to_vec(),
                });
                data_at += length;
            }
            tracks.push(Track {
                track,
                side,
                sector_size,
                gap3,
                filler,
                sectors,
            });
        }
        Ok(Disk {
            creator,
            tracks_per_side,
            sides,
            tracks,
            dirty: false,
            revision: 0,
        })
    }

    /// Write the disk back out, in the original format.
    ///
    /// The extended format exists for disks whose tracks are different lengths;
    /// nothing this emulator writes is like that, and a file every other
    /// emulator can read is worth more than one that keeps a distinction
    /// nothing here makes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let track_len = 0x100
            + self
                .tracks
                .iter()
                .map(|t| t.sectors.iter().map(|s| s.data.len()).sum::<usize>())
                .max()
                .unwrap_or(0);
        let mut out = Vec::with_capacity(0x100 + self.tracks.len() * track_len);
        out.extend_from_slice(b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n");
        out.resize(0x22, 0);
        let creator = format!("{:14}", self.creator);
        out.extend_from_slice(&creator.as_bytes()[..14]);
        out.push(self.tracks_per_side);
        out.push(self.sides);
        out.extend_from_slice(&(track_len as u16).to_le_bytes());
        out.resize(0x100, 0);

        for track in &self.tracks {
            let start = out.len();
            out.extend_from_slice(TRACK_MARK);
            out.resize(start + 0x10, 0);
            out.push(track.track);
            out.push(track.side);
            out.push(0);
            out.push(0);
            out.push(track.sector_size);
            out.push(track.sectors.len() as u8);
            out.push(track.gap3);
            out.push(track.filler);
            for sector in &track.sectors {
                out.extend_from_slice(&[
                    sector.c, sector.h, sector.r, sector.n, sector.st1, sector.st2,
                ]);
                out.extend_from_slice(&(sector.data.len() as u16).to_le_bytes());
            }
            out.resize(start + 0x100, 0);
            for sector in &track.sectors {
                out.extend_from_slice(&sector.data);
            }
            out.resize(start + track_len, track.filler);
        }
        out
    }

    /// How big it is, in the terms the machine thinks in.
    pub fn describe(&self) -> String {
        let sectors: usize = self.tracks.iter().map(|t| t.sectors.len()).sum();
        let bytes: usize = self
            .tracks
            .iter()
            .flat_map(|t| t.sectors.iter())
            .map(|s| s.data.len())
            .sum();
        format!(
            "{} tracks, {} side{}, {sectors} sectors, {}K",
            self.tracks_per_side,
            self.sides,
            if self.sides == 1 { "" } else { "s" },
            bytes / 1024
        )
    }
}

/// One file in the disk's catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// How big it is, in kilobytes, as the machine's own CAT reports it:
    /// rounded up to the kilobyte blocks it occupies.
    pub kilobytes: u32,
    /// Whether it is marked read-only, and whether it is hidden from CAT.
    pub read_only: bool,
    pub system: bool,
}

/// What a disk's geometry is, once something has said so.
///
/// The two formats the machine makes are implied by their sector numbering;
/// anything else has to be written down, and +3DOS writes it in the first
/// sector of the first track. That is how a 720K disk works on a machine whose
/// own FORMAT only makes 180K ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spec {
    pub tracks: u8,
    pub sides: u8,
    pub sectors: u8,
    /// Size code: 512 bytes is 2.
    pub sector_size: u8,
    /// Tracks at the front the filesystem does not use.
    pub reserved: u8,
    /// The allocation unit, in bytes.
    pub block_size: u32,
    /// How many of those the directory takes.
    pub directory_blocks: u8,
}

/// What format the disk is in, as +3DOS tells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Sectors numbered from $C1: the disk FORMAT makes, all forty tracks
    /// available and the directory at the front.
    Data,
    /// Sectors numbered from $41: the disk the machine was sold with, whose
    /// first track is reserved for CP/M.
    System,
    /// A disk carrying its own specification in track 0, sector 1 — which is
    /// how anything that is not one of those two says what it is.
    Specified(Spec),
    /// Something else. There is no catalogue to read.
    Other,
}

impl Format {
    /// The geometry, whichever way it was arrived at.
    pub fn spec(&self, disk: &Disk) -> Option<Spec> {
        match self {
            Format::Data => Some(Spec {
                tracks: disk.tracks_per_side,
                sides: disk.sides,
                sectors: SECTORS,
                sector_size: 2,
                reserved: 0,
                block_size: 1024,
                directory_blocks: 2,
            }),
            Format::System => Some(Spec {
                tracks: disk.tracks_per_side,
                sides: disk.sides,
                sectors: SECTORS,
                sector_size: 2,
                reserved: 1,
                block_size: 1024,
                directory_blocks: 2,
            }),
            Format::Specified(spec) => Some(*spec),
            Format::Other => None,
        }
    }

    /// What to call it.
    pub fn describe(&self) -> String {
        match self {
            Format::Data => "+3 data format, sectors from $C1".into(),
            Format::System => "+3 system format, sectors from $41, first track reserved".into(),
            Format::Specified(spec) => format!(
                "a disk with its own specification: {} tracks, {} side{}, {} sectors a track, \
                 {} reserved, {}K blocks",
                spec.tracks,
                spec.sides,
                if spec.sides == 1 { "" } else { "s" },
                spec.sectors,
                spec.reserved,
                spec.block_size / 1024
            ),
            Format::Other => "not a +3 format — its sectors are numbered some other way".into(),
        }
    }
}

/// Which machine's files are on a disk, as far as their headers say.
///
/// The +3 and the Amstrad CPC use the same disks, the same controller and the
/// same filesystem, so a CPC disk mounts, catalogues and reads perfectly well
/// on a +3 — and then does not load, because the files in it are for another
/// machine. Telling somebody that is worth more than letting them wonder
/// whether the emulator is broken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MadeFor {
    /// A +3DOS header: the eight letters of "PLUS3DOS" and a soft end-of-file.
    Spectrum,
    /// An AMSDOS header: a name, a type, lengths, and a checksum of the first
    /// sixty-seven bytes that has to add up.
    Amstrad,
    /// Neither, which is what a file saved without a header looks like — a
    /// game's own loader reading its own data.
    Headerless,
}

impl Disk {
    /// The bytes of one of the disk's kilobyte blocks, as CP/M counts them:
    /// two sectors, after whatever tracks the format reserves.
    pub fn block(&self, index: u16) -> Option<Vec<u8>> {
        let spec = self.format().spec(self)?;
        let sector_bytes = 128u32 << spec.sector_size.min(6);
        let per_block = (spec.block_size / sector_bytes).max(1);
        let per_track = spec.sectors as u32;
        let mut out = Vec::with_capacity(spec.block_size as usize);
        for part in 0..per_block {
            let logical = index as u32 * per_block + part;
            // CP/M counts in tracks of its own, and on a double-sided disk a
            // side is one of them: unit 0 is track 0 side 0, unit 1 is track 0
            // side 1, unit 2 is track 1 side 0. The reserved count is in those
            // units too — skipping a whole physical track instead put the
            // directory of a 720K disk on the wrong side, and read somebody
            // else's data as filenames.
            let sides = spec.sides.max(1) as u32;
            let unit = spec.reserved as u32 + logical / per_track;
            let track = unit / sides;
            let side = (unit % sides) as u8;
            let sector = self
                .track(track as u8, side)?
                .sectors
                .get((logical % per_track) as usize)?;
            out.extend_from_slice(&sector.data);
        }
        Some(out)
    }

    /// Whose files these are, from the header on the first one.
    ///
    /// One file is enough: a disk does not mix them, and the first is the one
    /// a loader would go for.
    pub fn made_for(&self) -> Option<MadeFor> {
        let files = self.catalogue()?;
        let _ = files.first()?;
        // The directory entry's first block, which is where the file starts.
        let directory = self.directory()?;
        let entry = directory
            .chunks(32)
            .find(|entry| entry.len() == 32 && entry[0] == 0 && entry[12] == 0)?;
        let block = u16::from(entry[16]);
        let head = self.block(block)?;
        if head.starts_with(b"PLUS3DOS") {
            return Some(MadeFor::Spectrum);
        }
        // AMSDOS: the first sixty-seven bytes add up to the word at 67, and
        // nothing else is that lucky by accident.
        if head.len() >= 69 {
            let sum: u32 = head[..67].iter().map(|b| *b as u32).sum();
            let stored = u16::from_le_bytes([head[67], head[68]]) as u32;
            if sum == stored && stored != 0 {
                return Some(MadeFor::Amstrad);
            }
        }
        Some(MadeFor::Headerless)
    }

    /// Which format this is: one of the two the machine makes, or whatever the
    /// disk says of itself.
    pub fn format(&self) -> Format {
        let first = self.track(0, 0).and_then(|t| t.sectors.first());
        match first.map(|s| s.r) {
            Some(0xC1) => Format::Data,
            Some(0x41) => Format::System,
            _ => match first.and_then(|s| specification(&s.data)) {
                Some(spec) => Format::Specified(spec),
                None => Format::Other,
            },
        }
    }

    /// The catalogue, as CAT would print it.
    ///
    /// +3DOS keeps a CP/M directory: sixty-four entries of thirty-two bytes at
    /// the front of the disk, each a user number, a name, a type, and which
    /// kilobyte blocks it occupies. A file longer than sixteen blocks has more
    /// than one entry — the extent number says which — so the entries are
    /// added up by name rather than listed one for one.
    ///
    /// $E5 in the first byte is a deleted or never-used entry, which is why a
    /// disk formatted with $E5 catalogues as empty.
    pub fn catalogue(&self) -> Option<Vec<Entry>> {
        let bytes = self.directory()?;

        let mut files: Vec<Entry> = Vec::new();
        // A directory is recognised as one, not assumed. A game disk often
        // carries a specification and then keeps its own data where the
        // catalogue would be; reading that as filenames produced seventy
        // files called things like ". H" of nineteen megabytes each.
        let mut plausible = 0usize;
        let mut rubbish = 0usize;
        for entry in bytes.chunks(32) {
            if entry.len() < 32 || entry[0] != 0 {
                // Only user 0, which is where +3DOS puts everything; $E5 is an
                // empty slot.
                continue;
            }
            let printable = entry[1..12]
                .iter()
                .all(|b| (0x20..=0x7E).contains(&(b & 0x7F)));
            if !printable {
                rubbish += 1;
                continue;
            }
            plausible += 1;
            let name: String = entry[1..9]
                .iter()
                .map(|b| (b & 0x7F) as char)
                .collect::<String>()
                .trim_end()
                .to_string();
            let extension: String = entry[9..12]
                .iter()
                .map(|b| (b & 0x7F) as char)
                .collect::<String>()
                .trim_end()
                .to_string();
            if name.is_empty() {
                continue;
            }
            let full = if extension.is_empty() {
                name.clone()
            } else {
                format!("{name}.{extension}")
            };
            // The high bits of the type are the flags: read-only, and hidden
            // from the catalogue.
            let read_only = entry[9] & 0x80 != 0;
            let system = entry[10] & 0x80 != 0;
            // How far into the file this extent reaches, in 128-byte records:
            // the extent number counts the 16K before it, and the record count
            // is what this one holds. The extents of one file are not added
            // up — the last one already says how long the file is, and adding
            // them made a 32K file 48K.
            let extent = (entry[12] as u32 & 0x1F) + (entry[14] as u32 & 0x3F) * 32;
            let records = extent * 128 + entry[15] as u32;
            let kilobytes = records.div_ceil(8);
            match files.iter_mut().find(|f| f.name == full) {
                Some(existing) => existing.kilobytes = existing.kilobytes.max(kilobytes),
                None => files.push(Entry {
                    name: full,
                    kilobytes,
                    read_only,
                    system,
                }),
            }
        }
        // More rubbish than names means this is not a directory: something
        // else lives where one would be.
        if rubbish > plausible {
            return None;
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Some(files)
    }

    /// How much room is left, in kilobytes: the blocks nothing has claimed.
    pub fn free_kilobytes(&self) -> Option<u32> {
        let spec = self.format().spec(self)?;
        let files = self.catalogue()?;
        let used: u32 = files.iter().map(|f| f.kilobytes).sum();
        let sector_bytes = 128u32 << spec.sector_size.min(6);
        // The whole disk, less the tracks the format reserves and the blocks
        // the directory itself takes.
        let total = (spec.tracks as u32 - spec.reserved as u32)
            * spec.sides.max(1) as u32
            * spec.sectors as u32
            * sector_bytes
            / 1024;
        let directory = spec.directory_blocks as u32 * spec.block_size / 1024;
        Some(total.saturating_sub(used + directory))
    }

    /// Which block of the filesystem a sector belongs to, if the disk has a
    /// filesystem and the sector is part of it.
    ///
    /// The inverse of [`Disk::block`]: the same units, the same order.
    pub fn block_of(&self, track: u8, side: u8, index: usize) -> Option<u16> {
        let spec = self.format().spec(self)?;
        let sides = spec.sides.max(1) as u32;
        let unit = track as u32 * sides + side as u32;
        let unit = unit.checked_sub(spec.reserved as u32)?;
        let logical = unit * spec.sectors as u32 + index as u32;
        let sector_bytes = 128u32 << spec.sector_size.min(6);
        let per_block = (spec.block_size / sector_bytes).max(1);
        u16::try_from(logical / per_block).ok()
    }

    /// Which file a sector holds part of, if any does.
    ///
    /// A directory entry lists the blocks it owns; on a disk with more than
    /// 255 of them they are pairs of bytes rather than single ones, which is
    /// what makes a 720K disk's allocation different from a 180K one's.
    pub fn file_at(&self, track: u8, side: u8, index: usize) -> Option<String> {
        let spec = self.format().spec(self)?;
        let block = self.block_of(track, side, index)?;
        let directory = self.directory()?;
        if block < spec.directory_blocks as u16 {
            return Some("the catalogue".into());
        }
        let sector_bytes = 128u32 << spec.sector_size.min(6);
        let blocks = (spec.tracks as u32 - spec.reserved as u32)
            * spec.sides.max(1) as u32
            * spec.sectors as u32
            * sector_bytes
            / spec.block_size;
        let wide = blocks > 255;
        for entry in directory.chunks(32) {
            if entry.len() < 32 || entry[0] != 0 {
                continue;
            }
            let owns = if wide {
                entry[16..32]
                    .chunks(2)
                    .any(|pair| u16::from_le_bytes([pair[0], pair[1]]) == block)
            } else {
                entry[16..32].iter().any(|b| *b as u16 == block)
            };
            if !owns {
                continue;
            }
            let name: String = entry[1..9]
                .iter()
                .map(|b| (b & 0x7F) as char)
                .collect::<String>()
                .trim_end()
                .to_string();
            let kind: String = entry[9..12]
                .iter()
                .map(|b| (b & 0x7F) as char)
                .collect::<String>()
                .trim_end()
                .to_string();
            return Some(if kind.is_empty() {
                name
            } else {
                format!("{name}.{kind}")
            });
        }
        None
    }

    /// The directory: the blocks at the front of the disk that hold the
    /// catalogue, however many of them this format has.
    fn directory(&self) -> Option<Vec<u8>> {
        let spec = self.format().spec(self)?;
        let mut out = Vec::new();
        for block in 0..spec.directory_blocks as u16 {
            out.extend_from_slice(&self.block(block)?);
        }
        Some(out)
    }
}

/// The disk specification +3DOS writes in the first sector of track 0.
///
/// Ten bytes: what sort of disk it is, how many tracks and sectors, how many
/// tracks are reserved, how big an allocation block is and how much of the
/// disk the directory takes. A disk without one is one of the two formats the
/// machine makes, told apart by their sector numbering.
///
/// Every field is checked against what a disk could actually be. The first
/// sector of a data-format disk is $E5 all through, which would otherwise read
/// as a specification for a disk of two hundred and twenty-nine tracks.
fn specification(sector: &[u8]) -> Option<Spec> {
    if sector.len() < 10 {
        return None;
    }
    let kind = sector[0];
    let sides = match sector[1] & 0x03 {
        0 => 1,
        _ => 2,
    };
    let tracks = sector[2];
    let sectors = sector[3];
    let sector_size = sector[4];
    let reserved = sector[5];
    let block_shift = sector[6];
    let directory_blocks = sector[7];
    let plausible = kind <= 3
        && (1..=100).contains(&tracks)
        && (1..=32).contains(&sectors)
        && sector_size <= 6
        && reserved < tracks
        && (3..=7).contains(&block_shift)
        && (1..=64).contains(&directory_blocks);
    if !plausible {
        return None;
    }
    Some(Spec {
        tracks,
        sides,
        sectors,
        sector_size,
        reserved,
        block_size: 128u32 << block_shift,
        directory_blocks,
    })
}
