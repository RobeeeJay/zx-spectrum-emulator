//! IPF disks: what the preservation people wrote down about a disk.
//!
//! An IPF holds a disk as the head would have read it — the sync marks, the
//! gaps, the address marks and the data, track by track — rather than as a
//! filesystem. It is how a protected disk is kept: the deleted-data marks, the
//! odd sector numbering and the weak bits that a copier could not reproduce
//! are all in the file.
//!
//! What is read here is the container and the streams inside it. The streams
//! turn out to hold decoded bytes rather than flux — the sync elements carry
//! the MFM sync words, and the data elements the bytes between them — so a
//! disk written in the usual IBM format can be taken apart at byte level: an
//! address mark, four bytes of identity, a CRC; then a data mark, the sector,
//! and another CRC. Both CRCs are checked, which is what says the reading is
//! right rather than plausible.
//!
//! What is not done: the cell timing, the weak bits, and anything that needs
//! the flux rather than the bytes. A disk whose protection measures how long a
//! sector takes to come round will not be fooled by this.

use crate::disk::{Disk, Sector, Track};

/// The machines an IPF can be of. A file says which, and one for a machine
/// this emulator is not is refused rather than half-read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Amiga,
    AtariSt,
    Pc,
    AmstradCpc,
    ZxSpectrum,
    SamCoupe,
    Archimedes,
    C64,
    Atari8Bit,
    Unknown(u32),
}

impl Platform {
    fn of(value: u32) -> Platform {
        match value {
            1 => Platform::Amiga,
            2 => Platform::AtariSt,
            3 => Platform::Pc,
            4 => Platform::AmstradCpc,
            5 => Platform::ZxSpectrum,
            6 => Platform::SamCoupe,
            7 => Platform::Archimedes,
            8 => Platform::C64,
            9 => Platform::Atari8Bit,
            other => Platform::Unknown(other),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Platform::Amiga => "Amiga".into(),
            Platform::AtariSt => "Atari ST".into(),
            Platform::Pc => "PC".into(),
            Platform::AmstradCpc => "Amstrad CPC".into(),
            Platform::ZxSpectrum => "ZX Spectrum".into(),
            Platform::SamCoupe => "Sam Coupé".into(),
            Platform::Archimedes => "Archimedes".into(),
            Platform::C64 => "Commodore 64".into(),
            Platform::Atari8Bit => "Atari 8-bit".into(),
            Platform::Unknown(n) => format!("platform {n}"),
        }
    }

    /// Whether a +3 could have read it. The CPC's disks are the +3's disks,
    /// which is why both are let through.
    fn readable_here(&self) -> bool {
        matches!(self, Platform::ZxSpectrum | Platform::AmstradCpc)
    }
}

/// Is this an IPF at all?
pub fn is_ipf(data: &[u8]) -> bool {
    data.starts_with(b"CAPS")
}

/// One record of the container: a four-letter name and a body.
struct Record<'a> {
    name: [u8; 4],
    body: &'a [u8],
    /// A DATA record is followed by its payload, which is not part of the
    /// record's own length.
    payload: &'a [u8],
}

fn records(data: &[u8]) -> Vec<Record<'_>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 12 <= data.len() {
        let name = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        let length = be32(data, at + 4) as usize;
        if length < 12 || at + length > data.len() {
            break;
        }
        let body = &data[at + 12..at + length];
        let mut payload: &[u8] = &[];
        let mut next = at + length;
        if &name == b"DATA" && body.len() >= 16 {
            let size = be32(body, 0) as usize;
            let end = (next + size).min(data.len());
            payload = &data[next.min(data.len())..end];
            next = end;
        }
        out.push(Record {
            name,
            body,
            payload,
        });
        at = next;
    }
    out
}

fn be32(data: &[u8], at: usize) -> u32 {
    if at + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// What one track's image record says about it.
struct Image {
    cylinder: u32,
    head: u32,
    blocks: usize,
    key: u32,
}

/// The kinds of thing a stream is made of.
const SYNC: u8 = 1;
const DATA: u8 = 2;
const GAP: u8 = 3;

/// Walk the elements of a stream, giving each one's kind and bytes.
fn elements(buf: &[u8], from: usize) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut at = from;
    while at < buf.len() {
        let head = buf[at];
        if head == 0 {
            break;
        }
        let kind = head & 0x1F;
        let width = (head >> 5) as usize;
        if at + 1 + width > buf.len() {
            break;
        }
        let mut size = 0usize;
        for i in 0..width {
            size = (size << 8) | buf[at + 1 + i] as usize;
        }
        let start = at + 1 + width;
        // Sync, data and gap counts are bytes; the raw and fuzzy kinds count
        // bits, since what they describe is not whole bytes.
        let length = match kind {
            SYNC | DATA | GAP => size,
            _ => size.div_ceil(8),
        };
        let end = (start + length).min(buf.len());
        out.push((kind, &buf[start..end]));
        at = end;
        if length == 0 {
            break;
        }
    }
    out
}

/// The CRC a floppy controller checks a field with: CCITT, starting at $FFFF,
/// over the three sync bytes and everything after them.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn field_crc(mark_and_body: &[u8]) -> u16 {
    let mut with_sync = vec![0xA1, 0xA1, 0xA1];
    with_sync.extend_from_slice(mark_and_body);
    crc16(&with_sync)
}

/// What was made of an IPF: the disk, and what had to be said about it.
pub struct Read {
    pub disk: Disk,
    pub platform: Platform,
    /// Sectors whose data field carried a deleted-data mark, which is a thing
    /// protections do and ordinary disks do not.
    pub deleted: usize,
    /// Sectors whose CRC did not check out — deliberate errors, usually.
    pub bad_crc: usize,
    /// Streams holding weak bits, which cannot be represented here.
    pub fuzzy: usize,
}

/// Read an IPF into a disk.
pub fn parse(data: &[u8]) -> Result<Read, String> {
    if !is_ipf(data) {
        return Err("not an IPF: it does not start with CAPS".into());
    }
    let records = records(data);
    let info = records
        .iter()
        .find(|r| &r.name == b"INFO")
        .ok_or("no INFO record: this is not a disk image")?;
    let platform = (0..4)
        .map(|i| Platform::of(be32(info.body, 48 + i * 4)))
        .find(|p| *p != Platform::of(0))
        .unwrap_or(Platform::Unknown(0));
    if !platform.readable_here() {
        return Err(format!(
            "this is {} disk. Its tracks are written the way that machine wrote them, \
             which is not the IBM format a +3 reads, so there is nothing here to put in \
             the drive.",
            match platform {
                Platform::Amiga | Platform::Archimedes | Platform::AtariSt =>
                    format!("an {}", platform.name()),
                _ => format!("a {}", platform.name()),
            }
        ));
    }

    let mut images = Vec::new();
    for record in &records {
        if &record.name == b"IMGE" {
            images.push(Image {
                cylinder: be32(record.body, 0),
                head: be32(record.body, 4),
                blocks: be32(record.body, 40) as usize,
                key: be32(record.body, 52),
            });
        }
    }
    let mut payloads = std::collections::BTreeMap::new();
    for record in &records {
        if &record.name == b"DATA" {
            payloads.insert(be32(record.body, 12), record.payload);
        }
    }

    let mut tracks: Vec<Track> = Vec::new();
    let (mut deleted, mut bad_crc, mut fuzzy) = (0usize, 0usize, 0usize);
    for image in &images {
        let Some(payload) = payloads.get(&image.key) else {
            continue;
        };
        let mut sectors = Vec::new();
        for block in 0..image.blocks {
            let at = block * 32;
            if at + 32 > payload.len() {
                break;
            }
            let data_offset = be32(&payload[at..at + 32], 28) as usize;
            let pieces = elements(payload, data_offset);
            fuzzy += pieces.iter().filter(|(kind, _)| *kind == 5).count();
            let data: Vec<&[u8]> = pieces
                .iter()
                .filter(|(kind, _)| *kind == DATA)
                .map(|(_, bytes)| *bytes)
                .collect();
            // An identity field is a mark, four bytes and a CRC; the sector
            // itself is the next data element.
            let Some(id) = data.first().filter(|f| f.len() >= 7 && f[0] == 0xFE) else {
                continue;
            };
            if field_crc(&id[..5]) != u16::from_be_bytes([id[5], id[6]]) {
                // An identity nobody can read is not a sector: the controller
                // would not find it either.
                continue;
            }
            let (c, h, r, n) = (id[1], id[2], id[3], id[4]);
            let Some(field) = data.get(1).filter(|f| f.len() > 3) else {
                continue;
            };
            let mark = field[0];
            if mark != 0xFB && mark != 0xF8 {
                continue;
            }
            let body = &field[1..field.len() - 2];
            let stored = u16::from_be_bytes([field[field.len() - 2], field[field.len() - 1]]);
            let mut st1 = 0u8;
            let mut st2 = 0u8;
            if field_crc(&field[..field.len() - 2]) != stored {
                // A deliberate CRC error, which is how a disk says "this is
                // not for copying".
                st1 |= 0x20;
                st2 |= 0x20;
                bad_crc += 1;
            }
            if mark == 0xF8 {
                // Control Mark: the data field is marked deleted.
                st2 |= 0x40;
                deleted += 1;
            }
            sectors.push(Sector {
                c,
                h,
                r,
                n,
                st1,
                st2,
                data: body.to_vec(),
            });
        }
        if sectors.is_empty() {
            continue;
        }
        tracks.push(Track {
            track: image.cylinder as u8,
            side: image.head as u8,
            sector_size: sectors[0].n,
            gap3: 0x4E,
            filler: crate::disk::FILLER,
            sectors,
        });
    }
    if tracks.is_empty() {
        return Err(
            "no sectors could be read out of it. The tracks are there, but nothing in them \
             is in the format a +3's controller reads — a disk written cell by cell for a \
             protection, most likely."
                .into(),
        );
    }

    let tracks_per_side = tracks.iter().map(|t| t.track).max().unwrap_or(0) + 1;
    let sides = tracks.iter().map(|t| t.side).max().unwrap_or(0) + 1;
    Ok(Read {
        disk: Disk {
            creator: "IPF".into(),
            tracks_per_side,
            sides,
            tracks,
            dirty: false,
            revision: 0,
            id: crate::disk::next_id(),
        },
        platform,
        deleted,
        bad_crc,
        fuzzy,
    })
}
