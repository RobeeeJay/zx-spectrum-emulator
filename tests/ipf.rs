//! IPF disks: what the preservation people wrote down about a disk.

use zx_rustrum::ipf::{self, Platform};

/// An IPF built by hand, so what is tested is the reader rather than whatever
/// wrote the file.
fn image(platform: u32, tracks: &[(u32, u32, Vec<Vec<u8>>)]) -> Vec<u8> {
    fn record(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(name);
        out.extend_from_slice(&((body.len() + 12) as u32).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // the CRC, which nothing here checks
        out.extend_from_slice(body);
        out
    }
    let mut out = record(b"CAPS", &[]);

    let mut info = vec![0u8; 84];
    info[..4].copy_from_slice(&1u32.to_be_bytes()); // media type
    info[4..8].copy_from_slice(&2u32.to_be_bytes()); // encoder
    info[48..52].copy_from_slice(&platform.to_be_bytes());
    out.extend_from_slice(&record(b"INFO", &info));

    let mut datas = Vec::new();
    for (key, (cylinder, head, sectors)) in tracks.iter().enumerate() {
        let key = key as u32 + 1;
        let mut imge = vec![0u8; 68];
        imge[..4].copy_from_slice(&cylinder.to_be_bytes());
        imge[4..8].copy_from_slice(&head.to_be_bytes());
        imge[40..44].copy_from_slice(&(sectors.len() as u32).to_be_bytes());
        imge[52..56].copy_from_slice(&key.to_be_bytes());
        out.extend_from_slice(&record(b"IMGE", &imge));

        // The blocks: a descriptor each, then the streams they point at.
        let mut descriptors = vec![0u8; sectors.len() * 32];
        let mut streams = Vec::new();
        for (i, sector) in sectors.iter().enumerate() {
            let offset = descriptors.len() + streams.len();
            descriptors[i * 32 + 28..i * 32 + 32].copy_from_slice(&(offset as u32).to_be_bytes());
            streams.extend_from_slice(sector);
        }
        descriptors.extend_from_slice(&streams);
        datas.push((key, descriptors));
    }
    for (key, payload) in datas {
        let mut body = vec![0u8; 16];
        body[..4].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        body[12..16].copy_from_slice(&key.to_be_bytes());
        out.extend_from_slice(&record(b"DATA", &body));
        out.extend_from_slice(&payload);
    }
    out
}

/// The CRC a controller checks a field with, over the sync bytes and the
/// field: CCITT from $FFFF.
fn crc(mark_and_body: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for byte in [0xA1u8, 0xA1, 0xA1].iter().chain(mark_and_body) {
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

/// One sector's worth of stream: the sync, the identity, a gap, the sync
/// again, and the data — which is how a track is written.
fn sector_stream(c: u8, h: u8, r: u8, n: u8, mark: u8, data: &[u8], break_crc: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let element = |out: &mut Vec<u8>, kind: u8, bytes: &[u8]| {
        // One byte of length is enough for anything here; the width goes in
        // the top three bits of the header.
        out.push((1 << 5) | kind);
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    };
    element(&mut out, 1, &[0x44, 0x89, 0x44, 0x89, 0x44, 0x89]);
    let mut id = vec![0xFE, c, h, r, n];
    let id_crc = crc(&id);
    id.extend_from_slice(&id_crc.to_be_bytes());
    element(&mut out, 2, &id);
    element(&mut out, 3, &[0x4E; 8]);
    element(&mut out, 1, &[0x44, 0x89, 0x44, 0x89, 0x44, 0x89]);
    let mut field = vec![mark];
    field.extend_from_slice(data);
    let mut field_crc = crc(&field);
    if break_crc {
        field_crc ^= 0xFFFF;
    }
    field.extend_from_slice(&field_crc.to_be_bytes());
    // Two bytes of length, since a sector is longer than 255.
    out.push((2 << 5) | 2);
    out.extend_from_slice(&(field.len() as u16).to_be_bytes());
    out.extend_from_slice(&field);
    out.push(0); // the end of the stream
    out
}

/// A disk comes out of an IPF as tracks and sectors, with the identity in the
/// address marks and both CRCs checked.
#[test]
fn an_ipf_is_read_as_tracks_and_sectors() {
    let sectors: Vec<Vec<u8>> = (1..=9u8)
        .map(|r| sector_stream(0, 0, r, 2, 0xFB, &vec![r; 512], false))
        .collect();
    let bytes = image(5, &[(0, 0, sectors)]);
    assert!(ipf::is_ipf(&bytes));

    let read = ipf::parse(&bytes).expect("it should read");
    assert_eq!(read.platform, Platform::ZxSpectrum);
    assert_eq!(read.disk.tracks.len(), 1);
    let track = read.disk.track(0, 0).expect("track 0");
    assert_eq!(track.sectors.len(), 9);
    assert_eq!(
        track.sectors.iter().map(|s| s.r).collect::<Vec<_>>(),
        (1..=9).collect::<Vec<_>>()
    );
    assert_eq!(track.sectors[2].data.len(), 512);
    assert!(track.sectors[2].data.iter().all(|b| *b == 3));
    assert_eq!(read.deleted, 0);
    assert_eq!(read.bad_crc, 0);
}

/// The things a protected disk does, which are the reason the format exists:
/// a data field marked deleted, and a CRC that does not check out.
#[test]
fn the_marks_a_protection_leaves_are_kept() {
    let sectors = vec![
        sector_stream(0, 0, 1, 2, 0xFB, &vec![0; 512], false),
        sector_stream(0, 0, 2, 2, 0xF8, &vec![0; 512], false),
        sector_stream(0, 0, 3, 2, 0xFB, &vec![0; 512], true),
    ];
    let read = ipf::parse(&image(5, &[(0, 0, sectors)])).expect("it should read");
    let track = read.disk.track(0, 0).unwrap();

    assert_eq!(track.sectors[0].st2 & 0x40, 0, "an ordinary sector");
    assert_eq!(
        track.sectors[1].st2 & 0x40,
        0x40,
        "a deleted data mark is a control mark in ST2"
    );
    assert_eq!(read.deleted, 1);
    assert_eq!(
        track.sectors[2].st1 & 0x20,
        0x20,
        "a bad CRC is a data error in ST1"
    );
    assert_eq!(read.bad_crc, 1);
}

/// A sector whose identity cannot be read is not a sector: the controller
/// would not find it either.
#[test]
fn a_sector_with_an_unreadable_identity_is_left_out() {
    let mut good = sector_stream(0, 0, 1, 2, 0xFB, &vec![0; 512], false);
    let broken = {
        let mut stream = sector_stream(0, 0, 2, 2, 0xFB, &vec![0; 512], false);
        // Break the identity's CRC, which is the last byte of that element.
        let at = stream
            .windows(2)
            .position(|w| w == [0x21, 0x07])
            .map(|i| i + 8)
            .unwrap_or(12);
        stream[at] ^= 0xFF;
        stream
    };
    good.extend_from_slice(&broken);
    let read = ipf::parse(&image(5, &[(0, 0, vec![good, broken])])).expect("it should read");
    let track = read.disk.track(0, 0).unwrap();
    assert!(
        track.sectors.len() < 2,
        "the sector with the broken identity should be left out: {:?}",
        track.sectors.iter().map(|s| s.r).collect::<Vec<_>>()
    );
}

/// An IPF of somebody else's machine is refused, with what it is.
#[test]
fn an_ipf_for_another_machine_is_refused() {
    let sectors = vec![sector_stream(0, 0, 1, 2, 0xFB, &vec![0; 512], false)];
    let why = match ipf::parse(&image(1, &[(0, 0, sectors)])) {
        Ok(_) => panic!("an Amiga disk should be refused"),
        Err(why) => why,
    };
    assert!(why.contains("Amiga"), "{why}");
    assert!(
        why.contains("not the IBM format"),
        "and why it cannot be read: {why}"
    );
}

/// Something that is not an IPF at all.
#[test]
fn something_that_is_not_an_ipf_is_refused() {
    assert!(!ipf::is_ipf(b"MV - CPCEMU Disk-File"));
    assert!(ipf::parse(b"MV - CPCEMU Disk-File").is_err());
    assert!(ipf::parse(b"CAPS").is_err(), "a header and nothing else");
    // And a file that starts right and stops: nothing is read out of the
    // middle of a record.
    assert!(ipf::parse(b"CAPS\x00\x00\x00\x0c\x00\x00\x00\x00").is_err());
}

/// The real thing, if it is on this machine.
#[test]
fn a_real_ipf_reads_with_every_crc_checking_out() {
    let path = "/Users/robee/Downloads/Combat School.ipf";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("no {path}; skipping");
        return;
    };
    let read = ipf::parse(&bytes).expect("it should read");
    assert_eq!(read.platform, Platform::ZxSpectrum);
    let sectors: usize = read.disk.tracks.iter().map(|t| t.sectors.len()).sum();
    assert!(sectors > 150, "{sectors} sectors");
    assert_eq!(
        read.bad_crc, 0,
        "every sector's CRC should check out, which is what says the reading is right"
    );
    assert!(
        read.deleted > 100,
        "and it is a protected disk: {} deleted marks",
        read.deleted
    );
}
