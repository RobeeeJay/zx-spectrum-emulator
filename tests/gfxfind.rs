//! Finding a block of the screen in memory, however it was stored.

use zx_rustrum::gfxfind::{search, Match, MAX_FOUND};

/// A block with a shape, and no symmetry to make its mirror image the same.
const BLOCK: [u8; 8] = [0x18, 0x3C, 0x7E, 0xDB, 0xFF, 0x24, 0x5A, 0x81 ^ 0x40];

fn memory_with(places: &[(u16, u16, [u8; 8])]) -> Vec<u8> {
    let mut memory = vec![0u8; 0x10000];
    for (addr, pitch, rows) in places {
        for (i, b) in rows.iter().enumerate() {
            memory[*addr as usize + i * *pitch as usize] = *b;
        }
    }
    memory
}

fn find(memory: &[u8], pattern: [u8; 8]) -> Vec<Match> {
    search(&|a| memory[a as usize], pattern)
        .expect("a block with a shape")
        .matches
}

/// As a character — eight bytes in a row — it is found with a pitch of one.
#[test]
fn a_character_is_eight_bytes_in_a_row() {
    let memory = memory_with(&[(0x9000, 1, BLOCK)]);
    assert_eq!(
        find(&memory, BLOCK),
        vec![Match {
            addr: 0x9000,
            pitch: 1,
            mirrored: false,
            inverted: false
        }]
    );
}

/// As a column of a sprite kept a row of pixels at a time, three bytes wide,
/// its rows are three bytes apart; with a mask byte beside each byte of a
/// two-byte sprite, four.
#[test]
fn part_of_a_wider_sprite_is_found_by_how_far_apart_its_rows_are() {
    let memory = memory_with(&[(0xA001, 3, BLOCK), (0xB002, 4, BLOCK)]);
    let found = find(&memory, BLOCK);
    let at = |addr| found.iter().find(|m| m.addr == addr).map(|m| m.pitch);
    assert_eq!(
        at(0xA001),
        Some(3),
        "a three-byte sprite's middle column: {found:?}"
    );
    assert_eq!(
        at(0xB002),
        Some(4),
        "a masked two-byte sprite's second byte: {found:?}"
    );
    assert_eq!(found.len(), 2, "and nothing else: {found:?}");
}

/// A sprite kept facing the other way is found mirrored, and a mask of it
/// inverted.
#[test]
fn mirrored_and_inverted_copies_are_found_and_said_to_be() {
    let mirrored = BLOCK.map(u8::reverse_bits);
    let inverted = BLOCK.map(|b| !b);
    let memory = memory_with(&[(0xC000, 1, mirrored), (0xD000, 2, inverted)]);
    let found = find(&memory, BLOCK);
    assert!(
        found.contains(&Match {
            addr: 0xC000,
            pitch: 1,
            mirrored: true,
            inverted: false
        }),
        "{found:?}"
    );
    assert!(
        found.contains(&Match {
            addr: 0xD000,
            pitch: 2,
            mirrored: false,
            inverted: true
        }),
        "{found:?}"
    );
    assert!(found[0].describe().contains("mirrored") || found[1].describe().contains("mirrored"));
}

/// A block with nothing in it, or the same byte on every row, would be found
/// everywhere: it is refused and the reason given.
#[test]
fn a_blank_block_is_refused() {
    let memory = vec![0u8; 0x10000];
    let blank = search(&|a| memory[a as usize], [0; 8]).unwrap_err();
    assert!(blank.contains("empty"), "{blank}");
    let stripes = search(&|a| memory[a as usize], [0xAA; 8]).unwrap_err();
    assert!(stripes.contains("same"), "{stripes}");
}

/// A block found everywhere stops at a count, and says there was more.
#[test]
fn a_block_found_everywhere_stops_at_a_count() {
    let memory: Vec<u8> = (0..0x10000).map(|a| BLOCK[a % 8]).collect();
    let result = search(&|a| memory[a as usize], BLOCK).unwrap();
    assert_eq!(result.matches.len(), MAX_FOUND);
    assert!(result.more);
}

/// Against the real ROM: the letter A of the font, at $3D00 + 33 x 8, is found
/// where it is, as a character, and said to be in the ROM.
#[test]
fn the_roms_letter_a_is_found_in_the_rom() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    let at = 0x3D00 + 33 * 8;
    let a: [u8; 8] = std::array::from_fn(|i| rom[at + i]);
    let mut memory = vec![0u8; 0x10000];
    memory[..0x4000].copy_from_slice(&rom[..0x4000]);
    let found = find(&memory, a);
    let hit = found
        .iter()
        .find(|m| m.addr as usize == at && m.pitch == 1)
        .unwrap_or_else(|| panic!("A at ${at:04X}: {found:?}"));
    assert!(hit.describe().contains("in the ROM"), "{}", hit.describe());
}
