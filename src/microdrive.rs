//! Microdrive cartridges, as MDR files.
//!
//! A cartridge is a loop of tape with up to 254 sectors on it, and the file is
//! exactly that: each sector written out as the head would read it, header
//! then record, with a byte on the end saying whether the write-protect tab
//! has been broken off.
//!
//! A sector is 543 bytes. Fifteen of them are the header — a flag, the sector
//! number, the cartridge name and a checksum — and the other 528 are the
//! record: a flag, which record of the file this is, how many of its 512 bytes
//! are used, the file's name, and two more checksums. Everything is summed mod
//! 255, which is the arithmetic the Interface 1's ROM does.

/// What a sector takes up in the file, and the pieces it is made of.
pub const SECTOR_LEN: usize = 543;
pub const HEADER_LEN: usize = 15;
pub const RECORD_LEN: usize = 528;
/// The most sectors a cartridge can hold. A real one holds fewer — the tape is
/// as long as it is, and 180 to 200 is usual.
pub const MAX_SECTORS: usize = 254;
/// How much of a record is data.
pub const DATA_LEN: usize = 512;

/// One sector, as it sits on the tape.
///
/// Kept as the bytes rather than as fields: the Interface 1 reads it a byte at
/// a time and checks its own checksums, so anything this took apart it would
/// have to put back together exactly.
#[derive(Clone, PartialEq, Eq)]
pub struct Sector {
    pub header: [u8; HEADER_LEN],
    pub record: [u8; RECORD_LEN],
}

impl Sector {
    /// The sector's number on the tape, which is how the ROM finds its way
    /// round the loop.
    pub fn number(&self) -> u8 {
        self.header[1]
    }

    /// The cartridge's name, which every sector carries a copy of.
    pub fn cartridge_name(&self) -> String {
        text(&self.header[4..14])
    }

    /// The name of the file this record belongs to.
    pub fn file_name(&self) -> String {
        text(&self.record[4..14])
    }

    /// Which record of that file it is, and how many of its bytes are used.
    pub fn record_number(&self) -> u8 {
        self.record[1]
    }

    pub fn used(&self) -> usize {
        u16::from_le_bytes([self.record[2], self.record[3]]) as usize
    }

    /// Whether the record is in use at all. A sector the ROM has erased has
    /// its record flagged empty, and its data is nobody's.
    pub fn in_use(&self) -> bool {
        // A name of nothing printable is not a name. An erased sector keeps
        // whatever was in those bytes, and rendering them as dots made a file
        // called ".........." appear in the catalogue.
        let named = self.record[4..14].iter().any(|b| (0x21..0x7F).contains(b));
        self.record[0] & 0x02 == 0 && named
    }

    /// The three checksums the Interface 1 works out for itself, and whether
    /// each of them agrees with what is on the tape.
    ///
    /// Checking them is what says a cartridge has been read correctly rather
    /// than plausibly; a real cartridge that has been sitting in a drawer for
    /// forty years may well have a sector that does not add up, and that is a
    /// fact about the cartridge rather than about the reading.
    pub fn checksums(&self) -> (bool, bool, bool) {
        (
            checksum(&self.header[..14]) == self.header[14],
            checksum(&self.record[..14]) == self.record[14],
            checksum(&self.record[15..527]) == self.record[527],
        )
    }

    /// The data itself, as much of it as the record says is used.
    pub fn data(&self) -> &[u8] {
        let used = self.used().min(DATA_LEN);
        &self.record[15..15 + used]
    }
}

/// The Interface 1's checksum: everything added up, mod 255.
pub fn checksum(bytes: &[u8]) -> u8 {
    let mut sum = 0u32;
    for byte in bytes {
        sum = (sum + *byte as u32) % 255;
    }
    sum as u8
}

/// Ten bytes of name, as something printable.
fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if (0x20..0x7F).contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// A file on the cartridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// How many sectors it takes up.
    pub sectors: usize,
    /// And how many bytes of them are used.
    pub bytes: usize,
}

#[derive(Clone)]
pub struct Cartridge {
    pub sectors: Vec<Sector>,
    /// Whether the tab has been broken off. Kept apart from how the cartridge
    /// was mounted: this is what the cartridge says about itself, and the
    /// mounting is what the emulator was told to do with it.
    pub write_protected: bool,
    pub dirty: bool,
    /// Which cartridge this is, and how many times it has been written to —
    /// the same bargain as a disk, and for the same reason: a picture of it
    /// has to know when it is stale, and every cartridge starts at revision 0.
    pub id: u64,
    pub revision: u64,
}

impl Cartridge {
    /// A formatted cartridge with nothing on it.
    ///
    /// What the Interface 1's own FORMAT command leaves behind cannot be
    /// checked here without its ROM, so this is the format as it is written
    /// down: every sector numbered, named and checksummed, and every record
    /// flagged empty.
    pub fn blank(name: &str, sectors: usize) -> Cartridge {
        let sectors = sectors.clamp(1, MAX_SECTORS);
        let mut out = Vec::with_capacity(sectors);
        for i in 0..sectors {
            let mut header = [0u8; HEADER_LEN];
            header[0] = 0x01;
            // Numbered from the top down, the way a cartridge is written.
            header[1] = (sectors - i) as u8;
            write_name(&mut header[4..14], name);
            header[14] = checksum(&header[..14]);

            let mut record = [0u8; RECORD_LEN];
            // Bit 1 set is an empty record: nothing of anybody's here.
            record[0] = 0x02;
            record[14] = checksum(&record[..14]);
            record[527] = checksum(&record[15..527]);
            out.push(Sector { header, record });
        }
        Cartridge {
            sectors: out,
            write_protected: false,
            dirty: false,
            id: next_id(),
            revision: 0,
        }
    }

    pub fn parse(data: &[u8]) -> Result<Cartridge, String> {
        // A cartridge is whole sectors, with or without the write-protect byte
        // on the end. Anything else is not one.
        let (count, protected) = match (data.len() / SECTOR_LEN, data.len() % SECTOR_LEN) {
            (0, _) => return Err("too short to be a cartridge".into()),
            (n, 0) => (n, false),
            (n, 1) => (n, data[data.len() - 1] != 0),
            (_, spare) => {
                return Err(format!(
                    "not a whole number of sectors: {} bytes leaves {spare} over",
                    data.len()
                ))
            }
        };
        if count > MAX_SECTORS {
            return Err(format!(
                "{count} sectors, and a cartridge holds at most {MAX_SECTORS}"
            ));
        }
        let mut sectors = Vec::with_capacity(count);
        for i in 0..count {
            let at = i * SECTOR_LEN;
            let mut header = [0u8; HEADER_LEN];
            let mut record = [0u8; RECORD_LEN];
            header.copy_from_slice(&data[at..at + HEADER_LEN]);
            record.copy_from_slice(&data[at + HEADER_LEN..at + SECTOR_LEN]);
            sectors.push(Sector { header, record });
        }
        // A file of the right length that is not a cartridge would read as one
        // of nothing but rubbish, so the headers have to say they are headers.
        let headers = sectors.iter().filter(|s| s.header[0] & 0x01 != 0).count();
        if headers * 2 < count {
            return Err(
                "the sector headers do not look like headers: this is the right length for \
                 a cartridge but not the right shape"
                    .into(),
            );
        }
        Ok(Cartridge {
            sectors,
            write_protected: protected,
            dirty: false,
            id: next_id(),
            revision: 0,
        })
    }

    /// Write it back out, with the write-protect byte the format allows.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sectors.len() * SECTOR_LEN + 1);
        for sector in &self.sectors {
            out.extend_from_slice(&sector.header);
            out.extend_from_slice(&sector.record);
        }
        out.push(if self.write_protected { 1 } else { 0 });
        out
    }

    /// The cartridge's name, from the sectors that carry it. Taken by vote:
    /// one damaged header should not rename the cartridge.
    pub fn name(&self) -> String {
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for sector in &self.sectors {
            *counts.entry(sector.cartridge_name()).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(name, _)| name)
            .unwrap_or_default()
    }

    /// What is on it: the files, with how much room each takes.
    pub fn catalogue(&self) -> Vec<Entry> {
        let mut files: Vec<Entry> = Vec::new();
        for sector in &self.sectors {
            if !sector.in_use() {
                continue;
            }
            let name = sector.file_name();
            match files.iter_mut().find(|f| f.name == name) {
                Some(entry) => {
                    entry.sectors += 1;
                    entry.bytes += sector.used();
                }
                None => files.push(Entry {
                    name,
                    sectors: 1,
                    bytes: sector.used(),
                }),
            }
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        files
    }

    /// How many sectors nothing is using.
    pub fn free_sectors(&self) -> usize {
        self.sectors.iter().filter(|s| !s.in_use()).count()
    }

    /// Which sectors do not add up, and which of their three checksums failed.
    pub fn bad_checksums(&self) -> Vec<(usize, (bool, bool, bool))> {
        self.sectors
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.checksums()))
            .filter(|(_, (h, d, r))| !h || !d || !r)
            .collect()
    }

    pub fn describe(&self) -> String {
        let files = self.catalogue().len();
        format!(
            "{} sectors, {} free, {files} file{}{}",
            self.sectors.len(),
            self.free_sectors(),
            if files == 1 { "" } else { "s" },
            if self.write_protected {
                ", write-protected"
            } else {
                ""
            }
        )
    }
}

fn write_name(into: &mut [u8], name: &str) {
    for (slot, byte) in into.iter_mut().zip(
        name.bytes()
            .filter(|b| (0x20..0x7F).contains(b))
            .chain(std::iter::repeat(b' ')),
    ) {
        *slot = byte;
    }
}

/// The next cartridge's identity, for anything that has to know one has been
/// swapped for another.
pub fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
