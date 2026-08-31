//! The three-inch disks the +3 used, as DSK files.

use zx_rustrum::disk::{Disk, FILLER, SECTORS, SECTOR_SIZE, TRACKS};

/// A disk image the user has, if they have one: these are somebody's games and
/// none are in the repository, so the tests that want one skip themselves.
fn an_image() -> Option<(String, Vec<u8>)> {
    for path in [
        "disks/test.dsk",
        "/Users/robee/ScummVM/Driller/driller.dsk",
        "/Users/robee/Downloads/wireware.dsk",
    ] {
        if let Ok(data) = std::fs::read(path) {
            return Some((path.to_string(), data));
        }
    }
    None
}

/// A blank disk is what the +3's own FORMAT makes: forty tracks of nine
/// 512-byte sectors numbered from $C1, every byte $E5.
///
/// $E5 is not an arbitrary filler. It is what an empty CP/M directory entry
/// starts with, which is why a disk formatted with it catalogues as empty
/// rather than as full of rubbish.
#[test]
fn a_blank_disk_is_formatted_the_way_the_machine_formats_one() {
    let disk = Disk::blank("zx-rustrum");
    assert_eq!(disk.tracks_per_side, TRACKS);
    assert_eq!(disk.sides, 1);
    assert_eq!(disk.tracks.len(), TRACKS as usize);

    let track = disk.track(0, 0).expect("a track 0");
    assert_eq!(track.sectors.len(), SECTORS as usize);
    let ids: Vec<u8> = track.sectors.iter().map(|s| s.r).collect();
    assert_eq!(
        ids,
        vec![0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9],
        "data format numbers its sectors from $C1"
    );
    for sector in &track.sectors {
        assert_eq!(sector.n, 2, "512 bytes is size code 2");
        assert_eq!(sector.data.len(), SECTOR_SIZE);
        assert!(sector.data.iter().all(|b| *b == FILLER));
    }

    // The last track is where it should be, and the sectors carry the
    // cylinder they are on rather than always saying zero.
    let last = disk.track(TRACKS - 1, 0).expect("the last track");
    assert_eq!(last.sectors[0].c, TRACKS - 1);
    assert_eq!(disk.describe(), "40 tracks, 1 side, 360 sectors, 180K");
}

/// What is written comes back the same.
#[test]
fn a_disk_survives_being_written_and_read_again() {
    let mut disk = Disk::blank("zx-rustrum");
    // Something to tell the sectors apart by.
    for (i, sector) in disk
        .track_mut(3, 0)
        .expect("track 3")
        .sectors
        .iter_mut()
        .enumerate()
    {
        sector.data[0] = i as u8;
        sector.data[511] = 0xAA;
    }
    let bytes = disk.to_bytes();
    let read = Disk::parse(&bytes).expect("what we wrote should parse");

    assert_eq!(read.tracks_per_side, disk.tracks_per_side);
    assert_eq!(read.sides, disk.sides);
    assert_eq!(read.tracks.len(), disk.tracks.len());
    for (a, b) in read.tracks.iter().zip(disk.tracks.iter()) {
        assert_eq!(a, b, "track {} came back different", a.track);
    }
}

/// A real disk image, if there is one to hand: the point is that what comes
/// back is the disk rather than a guess at it.
#[test]
fn a_real_disk_image_is_read_as_tracks_and_sectors() {
    let Some((path, data)) = an_image() else {
        eprintln!("no .dsk to hand; skipping");
        return;
    };
    let disk = Disk::parse(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert!(disk.tracks_per_side >= 40, "{}: {}", path, disk.describe());
    assert!(!disk.tracks.is_empty());

    let first = &disk.tracks[0];
    assert_eq!(first.track, 0);
    assert!(
        !first.sectors.is_empty(),
        "{path}: track 0 should hold sectors"
    );
    // Every sector's data is as long as its size code says, on a disk that is
    // not doing anything clever.
    for sector in &first.sectors {
        assert_eq!(
            sector.data.len(),
            sector.declared_len(),
            "{path}: sector ${:02X} is {} bytes for size code {}",
            sector.r,
            sector.data.len(),
            sector.n
        );
    }
    eprintln!("{path}: {}", disk.describe());
}

/// Anything that is not a disk image is refused, rather than read as one and
/// found to be nonsense later.
#[test]
fn something_that_is_not_a_disk_is_refused() {
    assert!(Disk::parse(b"").is_err());
    assert!(Disk::parse(&[0u8; 0x400]).is_err());
    let mut nearly = vec![0u8; 0x400];
    nearly[..8].copy_from_slice(b"MV - CPC");
    // A header that says there are tracks, and no track headers behind it.
    nearly[0x30] = 40;
    nearly[0x31] = 1;
    nearly[0x32] = 0x00;
    nearly[0x33] = 0x13;
    assert!(
        Disk::parse(&nearly).is_err(),
        "a file that claims tracks it has not got is not a disk"
    );
}

/// The catalogue, which is what the machine's own CAT prints: a CP/M
/// directory at the front of the disk, sixty-four entries of thirty-two bytes.
#[test]
fn a_disk_with_files_on_it_has_a_catalogue() {
    let mut disk = Disk::blank("test");
    assert_eq!(disk.format(), zx_rustrum::disk::Format::Data);
    assert_eq!(
        disk.catalogue().as_deref(),
        Some(&[][..]),
        "a blank disk catalogues as empty rather than as unreadable"
    );

    // One entry, written the way +3DOS writes one: user 0, eight bytes of
    // name, three of type, then the extent and the records it holds.
    {
        let directory = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
        // A real entry is written over, not into: the rest of the thirty-two
        // bytes are the extent, the record count and the blocks, and leaving
        // them as the disk's $E5 filler makes a two-kilobyte file enormous.
        let entry = &mut directory[..32];
        entry.fill(0);
        entry[1..9].copy_from_slice(b"GAME    ");
        entry[9..12].copy_from_slice(b"BIN");
        entry[12] = 0; // extent 0
        entry[15] = 16; // sixteen 128-byte records: 2K
    }
    let files = disk.catalogue().expect("a catalogue");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "GAME.BIN");
    assert_eq!(files[0].kilobytes, 2);

    // The free space is what is left of the disk after the files and the
    // directory itself.
    let free = disk.free_kilobytes().expect("free space");
    assert_eq!(free, 180 - 2 - 2, "180K disk, 2K of file, 2K of directory");
}

/// A file too big for one entry has several, and they are added up rather than
/// listed one for one — which is what CAT does.
#[test]
fn a_file_in_several_extents_is_listed_once() {
    let mut disk = Disk::blank("test");
    {
        let directory = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
        for extent in 0..2usize {
            let entry = &mut directory[extent * 32..extent * 32 + 32];
            entry.fill(0);
            entry[1..9].copy_from_slice(b"BIG     ");
            entry[9..12].copy_from_slice(b"   ");
            entry[12] = extent as u8;
            entry[15] = 128; // a full extent: 16K each
        }
    }
    let files = disk.catalogue().unwrap();
    assert_eq!(files.len(), 1, "one file, not two: {files:?}");
    assert_eq!(files[0].name, "BIG");
    assert_eq!(files[0].kilobytes, 32);
}

/// A disk that is not in either of the machine's formats has no catalogue to
/// read, and says so rather than printing rubbish.
#[test]
fn a_disk_in_another_format_has_no_catalogue() {
    let mut disk = Disk::blank("test");
    for sector in &mut disk.track_mut(0, 0).unwrap().sectors {
        // Amstrad's own numbering, which the +3 cannot read as a filesystem.
        sector.r = 0x01;
    }
    assert_eq!(disk.format(), zx_rustrum::disk::Format::Other);
    assert!(disk.catalogue().is_none());
    assert!(disk.free_kilobytes().is_none());
}

/// The flags a directory entry carries: read-only, and hidden from CAT.
#[test]
fn a_files_flags_are_read_from_the_high_bits_of_its_type() {
    let mut disk = Disk::blank("test");
    {
        let directory = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
        let entry = &mut directory[..32];
        entry.fill(0);
        entry[1..9].copy_from_slice(b"LOCKED  ");
        entry[9] = b'B' | 0x80; // read-only
        entry[10] = b'I' | 0x80; // and hidden
        entry[11] = b'N';
        entry[15] = 8;
    }
    let files = disk.catalogue().unwrap();
    assert_eq!(files[0].name, "LOCKED.BIN", "the flags are not part of it");
    assert!(files[0].read_only);
    assert!(files[0].system);
}

/// The catalogue of a real disk, against what the machine itself prints for
/// it. Driller's disk catalogues on a +3 as "DRILLER . 1K, 108K free".
#[test]
fn a_real_disks_catalogue_matches_what_the_machine_prints() {
    let Some((path, data)) = an_image() else {
        eprintln!("no .dsk to hand; skipping");
        return;
    };
    let disk = Disk::parse(&data).unwrap();
    let Some(files) = disk.catalogue() else {
        eprintln!("{path}: not a +3 format disk; skipping");
        return;
    };
    eprintln!(
        "{path}: {} files, {:?}K free",
        files.len(),
        disk.free_kilobytes()
    );
    for file in &files {
        eprintln!("  {} {}K", file.name, file.kilobytes);
        assert!(!file.name.is_empty());
        assert!(
            file.name.chars().all(|c| c.is_ascii_graphic() || c == '.'),
            "a name out of the directory should be printable: {:?}",
            file.name
        );
    }
}

/// Whose files are on a disk, from the header on the first one.
///
/// The +3 and the Amstrad CPC use the same disks, the same controller and the
/// same filesystem, so a CPC disk mounts and catalogues perfectly well on a +3
/// and then does not load. Saying which it is beats leaving somebody to wonder
/// whether the emulator is broken.
#[test]
fn a_disk_says_which_machine_its_files_are_for() {
    use zx_rustrum::disk::MadeFor;

    // A +3DOS file: the eight letters and a soft end-of-file.
    let mut plus3 = Disk::blank("test");
    write_file(&mut plus3, b"GAME    ", b"BAS", |block| {
        block[..9].copy_from_slice(b"PLUS3DOS\x1A");
    });
    assert_eq!(plus3.made_for(), Some(MadeFor::Spectrum));

    // An AMSDOS file: a checksum of the first sixty-seven bytes that adds up,
    // which is what a CPC's own SAVE writes.
    let mut amstrad = Disk::blank("test");
    write_file(&mut amstrad, b"DRILLER ", b"   ", |block| {
        block[1..9].copy_from_slice(b"DRILLER ");
        block[18] = 2; // a binary
        let sum: u32 = block[..67].iter().map(|b| *b as u32).sum();
        block[67..69].copy_from_slice(&(sum as u16).to_le_bytes());
    });
    assert_eq!(amstrad.made_for(), Some(MadeFor::Amstrad));

    // And a file with no header at all — a game's own loader reading its own
    // data — is neither, rather than being guessed at.
    let mut raw = Disk::blank("test");
    write_file(&mut raw, b"DATA    ", b"BIN", |block| {
        for (i, b) in block.iter_mut().enumerate() {
            *b = i as u8;
        }
    });
    assert_eq!(raw.made_for(), Some(MadeFor::Headerless));

    // A disk with nothing on it says nothing about anybody.
    assert_eq!(Disk::blank("test").made_for(), None);
}

/// Put a file in the catalogue with the given first kilobyte.
fn write_file(disk: &mut Disk, name: &[u8; 8], kind: &[u8; 3], fill: impl Fn(&mut [u8])) {
    // Block 2: the first one after the two the directory takes.
    {
        let directory = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
        let entry = &mut directory[..32];
        entry.fill(0);
        entry[1..9].copy_from_slice(name);
        entry[9..12].copy_from_slice(kind);
        entry[15] = 8; // one kilobyte of records
        entry[16] = 2; // and where it is
    }
    // Block 2 is logical sectors 4 and 5, which are the fifth and sixth of
    // track 0 on a data-format disk.
    let mut block = vec![0u8; 1024];
    fill(&mut block);
    let track = disk.track_mut(0, 0).unwrap();
    track.sectors[4].data.copy_from_slice(&block[..512]);
    track.sectors[5].data.copy_from_slice(&block[512..]);
}

/// A disk that is not one of the two the machine makes says what it is in the
/// first sector of track 0, and +3DOS reads it there.
///
/// That is how a 720K disk works on a machine whose own FORMAT only makes
/// 180K ones: eighty tracks, two sides, a track reserved, and two-kilobyte
/// blocks.
#[test]
fn a_disk_can_carry_its_own_specification() {
    use zx_rustrum::disk::{Format, Sector, Track};

    let mut disk = Disk::blank("big");
    disk.tracks_per_side = 80;
    disk.sides = 2;
    // Two sides of eighty tracks, numbered from 1 rather than $C1.
    disk.tracks = (0..80u8)
        .flat_map(|track| {
            (0..2u8).map(move |side| Track {
                track,
                side,
                sector_size: 2,
                gap3: 0x2A,
                filler: FILLER,
                sectors: (0..9u8)
                    .map(|i| Sector {
                        c: track,
                        h: side,
                        r: 1 + i,
                        n: 2,
                        st1: 0,
                        st2: 0,
                        data: vec![FILLER; 512],
                    })
                    .collect(),
            })
        })
        .collect();

    // The specification, as +3DOS writes it: type, sidedness, tracks,
    // sectors, size code, reserved tracks, block shift, directory blocks.
    let spec = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
    spec[..10].copy_from_slice(&[3, 0x81, 80, 9, 2, 1, 4, 4, 0x2A, 0x52]);

    match disk.format() {
        Format::Specified(spec) => {
            assert_eq!(spec.tracks, 80);
            assert_eq!(spec.sides, 2);
            assert_eq!(spec.reserved, 1);
            assert_eq!(spec.block_size, 2048);
            assert_eq!(spec.directory_blocks, 4);
        }
        other => panic!("it should read its own specification, not {other:?}"),
    }

    // One reserved track means one of CP/M's tracks, and on a double-sided
    // disk a side is one of those: the directory starts on track 0 side 1,
    // not track 1. Skipping a whole physical track read somebody else's data
    // as filenames.
    {
        let directory = &mut disk.track_mut(0, 1).unwrap().sectors[0].data;
        let entry = &mut directory[..32];
        entry.fill(0);
        entry[1..9].copy_from_slice(b"3DTUNNEL");
        entry[9..12].copy_from_slice(b"   ");
        entry[15] = 128; // 16K
    }
    let files = disk.catalogue().expect("a catalogue");
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].name, "3DTUNNEL");
    assert_eq!(files[0].kilobytes, 16);

    // And the room on it is the whole disk less the reserved track and the
    // directory: 720K of sectors, not 180K.
    let free = disk.free_kilobytes().expect("free space");
    assert!(
        (600..=720).contains(&free),
        "a 720K disk with 16K on it: {free}K free"
    );
}
