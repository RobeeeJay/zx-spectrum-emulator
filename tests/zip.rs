//! Getting a tape or a recording out of an archive.

use zx_rustrum::zip;

/// Build an archive by hand, so the reader is tested against bytes rather than
/// against whatever wrote them.
fn archive(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    use std::io::Write;

    let mut out: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    for (name, data, deflate) in files {
        let (method, packed) = if *deflate {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data).unwrap();
            (8u16, encoder.finish().unwrap())
        } else {
            (0u16, data.to_vec())
        };
        let at = out.len();

        // Local header, then the name, then the data.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0]); // version needed
        out.extend_from_slice(&[0, 0]); // flags
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // time and date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc, which nothing here checks
        out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);

        // And its entry in the directory at the end.
        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&[20, 0, 20, 0]); // versions
        directory.extend_from_slice(&[0, 0]); // flags
        directory.extend_from_slice(&method.to_le_bytes());
        directory.extend_from_slice(&[0; 4]); // time and date
        directory.extend_from_slice(&0u32.to_le_bytes()); // crc
        directory.extend_from_slice(&(packed.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&[0; 8]); // extra, comment, disk, internal
        directory.extend_from_slice(&[0; 4]); // external attributes
        directory.extend_from_slice(&(at as u32).to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }

    let directory_at = out.len();
    let count = files.len() as u16;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disk numbers
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&(directory_at as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

/// A stored file comes back as it went in, and so does a deflated one.
#[test]
fn a_file_comes_back_out_of_the_archive() {
    let tape: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
    let bytes = archive(&[("manic.tap", &tape, true), ("readme.txt", b"hello", false)]);

    assert!(zip::is_zip(&bytes), "it should be recognised as an archive");
    let entries = zip::entries(&bytes).expect("its directory should read");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "manic.tap");
    assert_eq!(entries[0].extension(), "tap");

    let (name, out) = zip::first_with_extension(&bytes, &["tap", "tzx"]).expect("the tape");
    assert_eq!(name, "manic.tap");
    assert_eq!(out, tape, "deflated on the way in and back out again");

    let (_, text) = zip::first_with_extension(&bytes, &["txt"]).expect("the stored file");
    assert_eq!(text, b"hello", "a stored file is copied out as it is");
}

/// An archive with nothing of the kind wanted gives nothing. Loading the
/// readme because it was the only file there would be worse than doing
/// nothing.
#[test]
fn an_archive_with_nothing_wanted_gives_nothing() {
    let bytes = archive(&[
        ("readme.txt", b"nothing here", false),
        ("scan.jpg", b"x", false),
    ]);
    assert!(zip::first_with_extension(&bytes, &["tap", "tzx", "rzx"]).is_none());
}

/// The first of its kind, not the best: choosing between two tapes is a
/// question for whoever made the archive.
#[test]
fn the_first_file_of_the_right_kind_is_the_one() {
    let bytes = archive(&[
        ("side b.tap", b"second", false),
        ("side a.tap", b"first", false),
    ]);
    let (name, data) = zip::first_with_extension(&bytes, &["tap"]).expect("a tape");
    assert_eq!(name, "side b.tap", "the one the directory lists first");
    assert_eq!(data, b"second");
}

/// Rubbish in is nothing out, rather than a panic: these files come off the
/// internet.
#[test]
fn a_broken_archive_is_refused_quietly() {
    assert!(!zip::is_zip(b"not a zip at all"));
    assert!(zip::entries(b"PK\x03\x04 and then nothing").is_err());
    assert!(zip::first_with_extension(b"", &["tap"]).is_none());

    // A directory that points past the end of the file.
    let mut bytes = archive(&[("game.tap", b"data", false)]);
    let len = bytes.len();
    bytes[len - 6..len - 2].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    assert!(zip::first_with_extension(&bytes, &["tap"]).is_none());
}

/// A tape inside an archive is loaded as if it had arrived on its own, and
/// the notes still go beside the archive: that is the file the user has.
#[test]
fn a_tape_inside_an_archive_loads() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    // A .tap is a length and a block: a header for a program called BOUNCE.
    let mut header = vec![0x00, 0x00];
    header.extend_from_slice(b"BOUNCE    ");
    header.extend_from_slice(&[0x1B, 0x00, 0x0A, 0x00, 0x1B, 0x00]);
    let mut block: Vec<u8> = vec![(header.len() + 1) as u8, 0x00];
    block.extend_from_slice(&header);
    block.push(header.iter().fold(0u8, |sum, byte| sum ^ byte));

    let scratch = std::env::temp_dir().join(format!("zxrs-zip-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let path = scratch.join("bounce.zip");
    std::fs::write(
        &path,
        archive(&[
            ("readme.txt", b"cracked by nobody", false),
            ("bounce.tap", &block, true),
        ]),
    )
    .unwrap();

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.load_path(&path);

    let tape = app
        .spec
        .bus
        .tape
        .as_ref()
        .expect("the tape inside should be in the deck");
    assert_eq!(
        tape.name, "bounce.tap",
        "named after the file in the archive"
    );
    assert_eq!(tape.blocks.len(), 1);
    assert_eq!(
        app.tape_path.as_deref(),
        Some(path.as_path()),
        "and the notes go beside the archive, which is the file on disk"
    );

    // An archive with nothing loadable in it leaves the deck alone.
    let empty = scratch.join("inlay.zip");
    std::fs::write(&empty, archive(&[("scan.jpg", b"x", false)])).unwrap();
    app.load_path(&empty);
    assert_eq!(
        app.spec.bus.tape.as_ref().map(|t| t.name.clone()),
        Some("bounce.tap".to_string()),
        "the tape that was in the deck should still be"
    );
}

/// The tape window's Load… takes a zip as the main window's does. It used to
/// hand the file straight to the tape reader, which found no tape in a zip and
/// left the deck empty while the main window's Load, given the same file,
/// mounted the tape inside.
#[test]
fn the_tape_windows_load_takes_a_zip_too() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{tape, App, Roms};

    let mut header = vec![0x00, 0x00];
    header.extend_from_slice(b"BOUNCE    ");
    header.extend_from_slice(&[0x1B, 0x00, 0x0A, 0x00, 0x1B, 0x00]);
    let mut block: Vec<u8> = vec![(header.len() + 1) as u8, 0x00];
    block.extend_from_slice(&header);
    block.push(header.iter().fold(0u8, |sum, byte| sum ^ byte));

    let scratch = std::env::temp_dir().join(format!("zxrs-zip-deck-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let path = scratch.join("bounce.zip");
    std::fs::write(&path, archive(&[("bounce.tap", &block, true)])).unwrap();

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    tape::load_chosen(&mut app, &path);
    let loaded = app
        .spec
        .bus
        .tape
        .as_ref()
        .map(|t| (t.name.clone(), t.blocks.len()));
    assert_eq!(
        loaded,
        Some(("bounce.tap".to_string(), 1)),
        "the tape inside the zip is in the deck; status said {:?}",
        app.status
    );
    let _ = std::fs::remove_dir_all(&scratch);
}
