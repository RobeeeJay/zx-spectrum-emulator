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
