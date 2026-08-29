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
