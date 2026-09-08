//! Microdrive cartridges, as MDR files.

use zx_rustrum::microdrive::{checksum, Cartridge, DATA_LEN, MAX_SECTORS, SECTOR_LEN};

/// A cartridge somebody has, if they have one: these are somebody's games and
/// none are in the repository, so the tests that want one skip themselves.
fn a_cartridge() -> Option<(String, Vec<u8>)> {
    for path in [
        "cartridges/test.mdr",
        "/Users/robee/MinnaMicroZ80/samples/mdr/Hewson.mdr",
        "/Users/robee/MinnaMicroZ80/samples/mdr/Robocop.mdr",
    ] {
        if let Ok(data) = std::fs::read(path) {
            return Some((path.to_string(), data));
        }
    }
    None
}

/// The checksum the Interface 1 works out: everything added up, mod 255.
///
/// Not mod 256. A sector whose bytes add to 255 checksums as 0, and getting
/// that wrong would fail one sector in every few hundred — which reads as a
/// worn cartridge rather than as a bug.
#[test]
fn the_checksum_is_the_sum_of_the_bytes_mod_255() {
    assert_eq!(checksum(&[]), 0);
    assert_eq!(checksum(&[1, 2, 3]), 6);
    assert_eq!(checksum(&[255]), 0, "255 wraps to nothing");
    assert_eq!(checksum(&[200, 200]), 145, "400 mod 255");
    assert_eq!(checksum(&[0xFF; 10]), 0);
}

/// A blank cartridge is formatted: every sector numbered, named, checksummed,
/// and empty.
#[test]
fn a_blank_cartridge_is_formatted_and_empty() {
    let cart = Cartridge::blank("Games", 180);
    assert_eq!(cart.sectors.len(), 180);
    assert_eq!(cart.name(), "Games");
    assert_eq!(cart.catalogue(), vec![], "nothing on it");
    assert_eq!(cart.free_sectors(), 180, "and every sector free");
    assert!(!cart.write_protected);
    assert_eq!(
        cart.bad_checksums(),
        vec![],
        "and everything it wrote adds up"
    );

    // The sectors are numbered so the ROM can find its way round the loop, and
    // no two share a number.
    let mut numbers: Vec<u8> = cart.sectors.iter().map(|s| s.number()).collect();
    numbers.sort_unstable();
    numbers.dedup();
    assert_eq!(numbers.len(), 180, "every sector has its own number");

    // A cartridge cannot hold more than the tape does.
    assert_eq!(Cartridge::blank("Big", 500).sectors.len(), MAX_SECTORS);
}

/// What is written comes back the same, write-protect tab and all.
#[test]
fn a_cartridge_survives_being_written_and_read_again() {
    let mut cart = Cartridge::blank("Work", 20);
    cart.write_protected = true;
    cart.sectors[3].record[0] = 0x04;
    cart.sectors[3].record[2] = 0x10;
    cart.sectors[3].record[15] = 0xAB;

    let bytes = cart.to_bytes();
    assert_eq!(bytes.len(), 20 * SECTOR_LEN + 1, "sectors and the tab byte");
    let read = Cartridge::parse(&bytes).expect("what we wrote should parse");
    assert!(read.write_protected, "the tab is part of the file");
    assert_eq!(read.sectors.len(), 20);
    for (a, b) in read.sectors.iter().zip(cart.sectors.iter()) {
        assert!(a == b, "a sector came back different");
    }
}

/// A file without the tab byte is still a cartridge: the byte is optional, and
/// plenty of files in the wild have not got it.
#[test]
fn the_write_protect_byte_is_optional() {
    let cart = Cartridge::blank("Loose", 10);
    let mut bytes = cart.to_bytes();
    bytes.pop();
    let read = Cartridge::parse(&bytes).expect("without the tab byte");
    assert_eq!(read.sectors.len(), 10);
    assert!(!read.write_protected);
}

/// Anything that is not a cartridge is refused rather than read as one.
#[test]
fn something_that_is_not_a_cartridge_is_refused() {
    assert!(Cartridge::parse(&[]).is_err());
    assert!(
        Cartridge::parse(&[0u8; 900]).is_err(),
        "a length that is not whole sectors"
    );
    assert!(
        Cartridge::parse(&[0u8; SECTOR_LEN * 5]).is_err(),
        "the right length, and no headers in it"
    );
    assert!(
        Cartridge::parse(&[0u8; SECTOR_LEN * 300]).is_err(),
        "more sectors than a tape holds"
    );
}

/// A real cartridge, if there is one to hand: what matters is that the
/// checksums add up, because that is what says it was read as the Interface 1
/// would read it rather than merely plausibly.
#[test]
fn a_real_cartridge_reads_with_its_checksums_intact() {
    let Some((path, data)) = a_cartridge() else {
        eprintln!("no .mdr to hand; skipping");
        return;
    };
    let cart = Cartridge::parse(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    let bad = cart.bad_checksums();
    assert!(
        bad.len() * 20 < cart.sectors.len(),
        "{path}: {} of {} sectors do not add up, which is too many to be the tape's \
         own wear: {:?}",
        bad.len(),
        cart.sectors.len(),
        &bad[..bad.len().min(4)]
    );
    assert!(!cart.name().is_empty(), "{path}: it should have a name");

    let files = cart.catalogue();
    assert!(!files.is_empty(), "{path}: and something on it");
    for file in &files {
        assert!(file.sectors > 0);
        assert!(
            file.bytes <= file.sectors * DATA_LEN,
            "{path}: {} claims {} bytes in {} sectors",
            file.name,
            file.bytes,
            file.sectors
        );
    }
    eprintln!("{path}: {} — {}", cart.name(), cart.describe());
    for file in files.iter().take(6) {
        eprintln!(
            "   {:<12} {:>3} sectors, {:>6} bytes",
            file.name, file.sectors, file.bytes
        );
    }
}

/// A sector nobody is using is not a file, whatever is left in its name bytes.
///
/// An erased sector keeps whatever was written there, and rendering those
/// bytes as dots put a file called ".........." in the catalogue of a real
/// cartridge.
#[test]
fn an_erased_sector_is_not_a_file() {
    let mut cart = Cartridge::blank("Test", 8);
    // A record marked empty, with rubbish left in its name.
    cart.sectors[0].record[0] = 0x02;
    cart.sectors[0].record[4..14].copy_from_slice(&[0x00, 0xFF, 0x01, 0, 0, 0, 0, 0, 0, 0]);
    // And one that is not marked empty but has no name to speak of either.
    cart.sectors[1].record[0] = 0x04;
    cart.sectors[1].record[4..14].copy_from_slice(&[0x00; 10]);
    // A real file, for contrast.
    cart.sectors[2].record[0] = 0x04;
    cart.sectors[2].record[4..14].copy_from_slice(b"Exolon    ");
    cart.sectors[2].record[2] = 0x00;
    cart.sectors[2].record[3] = 0x02; // 512 bytes

    let files = cart.catalogue();
    assert_eq!(files.len(), 1, "one file, not three: {files:?}");
    assert_eq!(files[0].name, "Exolon");
    assert_eq!(files[0].bytes, 512);
    assert_eq!(cart.free_sectors(), 7);
}
