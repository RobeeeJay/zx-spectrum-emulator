//! What is printed on the keys, against what the machine types when they are
//! pressed.
//!
//! The words on a Spectrum's keys are not decoration: they are the only way to
//! find CAT, FORMAT, INVERSE or ASN, none of which can be typed letter by
//! letter. So the table in `src/keyboard.rs` is checked against the ROM rather
//! than against a picture of a keyboard — every legend here was read off the
//! machine by pressing the key and looking at the edit line, and this test is
//! that same reading, kept.
//!
//! It skips itself without `roms/48.rom`, like every other test that needs a
//! ROM.

use zx_rustrum::keyboard::SPECTRUM;
use zx_rustrum::machine::{Model, Spectrum, FRAME_T};

const CAPS: (usize, u8) = (0, 0);
const SYM: (usize, u8) = (7, 1);

fn hold(spec: &mut Spectrum, keys: &[(usize, u8)], frames: u32) {
    for (row, bit) in keys {
        spec.bus.keys[*row] &= !(1 << bit);
    }
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
    for (row, bit) in keys {
        spec.bus.keys[*row] |= 1 << bit;
    }
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
}

/// What is in the edit buffer, as characters. The ROM's tokens come back as
/// single bytes above 164, so they are expanded from the ROM's own table.
fn typed(spec: &Spectrum, rom: &[u8]) -> String {
    let start = u16::from(spec.bus.peek_raw(0x5C59)) | (u16::from(spec.bus.peek_raw(0x5C5A)) << 8);
    let mut out = String::new();
    for i in 0..8u16 {
        let byte = spec.bus.peek_raw(start + i);
        match byte {
            0x0D | 0x80 => break,
            0x7F => out.push('©'),
            0x20..=0x7E => out.push(byte as char),
            0xA5..=0xFF => out.push_str(&token(rom, byte)),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// A keyword out of the ROM's token table at $0095, where each word ends with
/// its last letter's top bit set.
///
/// The table's first entry is the one before the first token proper, so the
/// count starts at $A4 rather than the $A5 that `RND` is: counting from $A5
/// hands back the word before the one asked for, which reads as a keyboard
/// with every legend shifted by one key.
fn token(rom: &[u8], byte: u8) -> String {
    let mut at = 0x0095;
    for _ in 0..(byte - 0xA4) {
        while rom[at] & 0x80 == 0 {
            at += 1;
        }
        at += 1;
    }
    let mut word = String::new();
    loop {
        let c = rom[at];
        word.push((c & 0x7F) as char);
        at += 1;
        if c & 0x80 != 0 {
            break;
        }
    }
    word.trim().to_string()
}

/// Press a key in extended mode, with or without a shift, and say what the
/// machine typed.
fn extended(rom: &[u8], at: (usize, u8), shifted: bool) -> String {
    let mut spec = Spectrum::new();
    spec.set_model(Model::Spectrum48, rom);
    spec.reset();
    for _ in 0..200 {
        spec.run(FRAME_T);
    }
    // Both shifts together is extended mode, and it lasts for one key.
    hold(&mut spec, &[CAPS, SYM], 6);
    if shifted {
        hold(&mut spec, &[SYM, at], 6);
    } else {
        hold(&mut spec, &[at], 6);
    }
    for _ in 0..10 {
        spec.run(FRAME_T);
    }
    typed(&spec, rom)
}

/// Every word printed under a key is what that key types in extended mode with
/// a shift held: CAT on 9, FORMAT on 0, INVERSE on M.
#[test]
fn the_word_under_each_key_is_what_the_key_types() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    let mut checked = 0;
    for key in SPECTRUM.iter().filter(|k| !k.under.is_empty()) {
        let at = key.press[0];
        let got = extended(&rom, at, true);
        assert_eq!(
            got, key.under,
            "{}: the key is printed with {:?} and types {:?}",
            key.main, key.under, got
        );
        checked += 1;
    }
    assert_eq!(checked, 36, "every key but the shifts, ENTER and SPACE");
}

/// And the green word above each key is what it types in extended mode on its
/// own — the column that was already there, never checked against the machine.
#[test]
fn the_word_above_each_letter_is_what_that_key_types() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    // The digits are left out: what is printed above them is the CAPS SHIFT
    // job — EDIT, DELETE, the cursors — which does something rather than
    // typing something.
    for key in SPECTRUM
        .iter()
        .filter(|k| k.main.len() == 1 && k.main.chars().all(|c| c.is_ascii_alphabetic()))
    {
        let got = extended(&rom, key.press[0], false);
        assert_eq!(
            got, key.over,
            "{}: the key is printed with {:?} above it and types {:?}",
            key.main, key.over, got
        );
    }
}
