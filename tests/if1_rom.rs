//! The real Interface 1 ROM, driving the real microdrive.
//!
//! Everything else about the Interface 1 is tested against a stub ROM, which
//! says the interface answers the questions the tests thought to ask. This
//! file puts Sinclair's own 8K in the socket and lets it drive: it formats a
//! cartridge, saves to it, loads back off it, and reads what the machine
//! printed off the screen. Three bugs were found by it that nothing else
//! caught — the shadow ROM paging out a fetch too early, the status port's
//! lines being in the wrong bits, and a blank cartridge whose empty records
//! were not what the ROM's own FORMAT writes.
//!
//! It skips itself when `roms/if1.rom` is not there, like every other test
//! that needs a ROM.

use zx_rustrum::if1::{Drive, If1};
use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::microdrive::Cartridge;

/// The keyboard as the ULA reads it: eight half-rows of five keys.
const CAPS: (usize, u8) = (0, 0);
const SYM: (usize, u8) = (7, 1);
const ZERO: (usize, u8) = (4, 0);
const NINE: (usize, u8) = (4, 1);
const ONE: (usize, u8) = (3, 0);
const ENTER: (usize, u8) = (6, 0);

/// Hold some keys down, then let go. The ROM wants a key on two scans running
/// before it believes in it, so a press has to last a few frames.
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

/// Which key a character is on, and whether it wants SYMBOL SHIFT with it.
fn key_of(c: char) -> Option<((usize, u8), bool)> {
    const ROWS: [&str; 8] = [
        " ZXCV",
        "ASDFG",
        "QWERT",
        "12345",
        "09876",
        "POIUY",
        "\nLKJH",
        " \u{1}MNB",
    ];
    // The only symbol-shifted keys these tests type: the quote is on P, the
    // semicolon on O, and the star — `SAVE *` is what makes it a microdrive
    // command rather than a tape one — on B.
    let shifted = [('"', 'P'), (';', 'O'), ('*', 'B')];
    let up = c.to_ascii_uppercase();
    let (up, shift) = match shifted.iter().find(|(sym, _)| *sym == up) {
        Some((_, key)) => (*key, true),
        None => (up, false),
    };
    for (row, keys) in ROWS.iter().enumerate() {
        if let Some(bit) = keys.chars().position(|k| k == up) {
            return Some(((row, bit as u8), shift));
        }
    }
    None
}

fn type_text(spec: &mut Spectrum, text: &str) {
    for c in text.chars() {
        let Some((key, shift)) = key_of(c) else {
            continue;
        };
        if shift {
            hold(spec, &[SYM, key], 6);
        } else {
            hold(spec, &[key], 6);
        }
    }
}

/// The screen read back through the ROM's own font, so the test sees what the
/// user would have seen rather than what the interface was asked.
fn screen_text(spec: &Spectrum, rom: &[u8]) -> Vec<String> {
    let font = &rom[0x3D00..0x3D00 + 96 * 8];
    let mut lines = Vec::new();
    for row in 0..24usize {
        let mut line = String::new();
        for col in 0..32usize {
            let mut cell = [0u8; 8];
            for (i, byte) in cell.iter_mut().enumerate() {
                let y = row * 8 + i;
                let addr = 0x4000 + ((y & 0xC0) << 5) + ((y & 0x07) << 8) + ((y & 0x38) << 2) + col;
                *byte = spec.bus.peek_raw(addr as u16);
            }
            let mut found = ' ';
            for c in 0..96usize {
                let glyph = &font[c * 8..c * 8 + 8];
                let inverted: Vec<u8> = glyph.iter().map(|b| !b).collect();
                if glyph == cell || inverted == cell {
                    found = (32 + c as u8) as char;
                    break;
                }
            }
            line.push(found);
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn printed(spec: &Spectrum, rom: &[u8]) -> String {
    screen_text(spec, rom)
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn roms() -> Option<(Vec<u8>, Vec<u8>)> {
    let rom48 = std::fs::read("roms/48.rom").ok()?;
    let if1 = std::fs::read("roms/if1.rom").ok()?;
    Some((rom48, if1))
}

/// A 48K with an Interface 1 on the back, booted to its prompt.
fn machine(rom48: &[u8], if1_rom: &[u8], cartridge: Option<Cartridge>) -> Spectrum {
    let mut spec = Spectrum::new();
    spec.set_model(Model::Spectrum48, rom48);
    spec.reset();
    let mut if1 = If1::new(1);
    if1.rom = Some(if1_rom.to_vec());
    if let Some(cart) = cartridge {
        if1.drives[0] = Drive::loaded(cart, None, false);
    }
    spec.bus.if1 = Some(if1);
    for _ in 0..200 {
        spec.run(FRAME_T);
    }
    spec
}

/// CAT is extended mode, then SYMBOL SHIFT with 9; FORMAT is the same with 0.
/// Neither is printed on a 48K's keyboard — they arrived with the interface.
fn keyword(spec: &mut Spectrum, key: (usize, u8)) {
    hold(spec, &[CAPS, SYM], 8);
    hold(spec, &[SYM, key], 8);
}

fn enter(spec: &mut Spectrum, frames: u32) {
    hold(spec, &[ENTER], 8);
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
}

/// What the ROM says about itself. The 8K comes from somebody else's archive,
/// so before anything is run out of it the test checks it is the ROM it claims
/// to be: the second edition, with Sinclair's copyright in it and the
/// microdrive messages it prints.
#[test]
fn the_rom_in_the_socket_is_sinclairs_interface_1_rom() {
    let Some((_, rom)) = roms() else {
        eprintln!("need roms/48.rom and roms/if1.rom; skipping");
        return;
    };
    assert_eq!(rom.len(), 8192, "the Interface 1's ROM is 8K");

    let text: String = rom.iter().map(|b| (b & 0x7F) as char).collect();
    for phrase in [
        "Microdrive not present",
        "Microdrive full",
        "Drive 'write' protected",
        "Invalid device expression",
        "Hook code error",
    ] {
        assert!(text.contains(phrase), "the ROM should print {phrase:?}");
    }
    assert!(
        text.contains("1983 Sinclair Research Ltd MJB"),
        "the second edition carries its author's initials"
    );

    // $0008 is the machine's error handler, and the first thing the shadow ROM
    // does there is pick up the address the machine came from.
    assert_eq!(&rom[0x0008..0x000B], &[0x2A, 0x5D, 0x5C], "LD HL,(CH_ADD)");
    // $0700 is the way back out, and it has to be a RET: the interface pages
    // itself out on that fetch, so this byte is the last of its own it runs.
    assert_eq!(rom[0x0700], 0xC9, "the way back to the machine's own ROM");
}

/// CAT 1, typed at the keyboard, catalogues the cartridge in drive 1.
///
/// The whole path: the keyword goes through the machine's error handler, the
/// interface pages its ROM in, turns the motor on, walks the tape looking for
/// gaps and sync, reads headers and record descriptors, and prints what it
/// found. A blank cartridge reads as its name and half a kilobyte per sector.
#[test]
fn the_machine_catalogues_a_cartridge_from_the_keyboard() {
    let Some((rom48, if1_rom)) = roms() else {
        eprintln!("need roms/48.rom and roms/if1.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &if1_rom, Some(Cartridge::blank("TESTCART", 180)));

    keyword(&mut spec, NINE);
    hold(&mut spec, &[ONE], 8);
    enter(&mut spec, 1500);

    let text = printed(&spec, &rom48);
    assert!(
        text.contains("TESTCART"),
        "the cartridge should name itself: {text}"
    );
    assert!(
        text.contains("90"),
        "and say how much of its 180 sectors is free: {text}"
    );
    assert!(text.contains("0 OK"), "with no error: {text}");
}

/// FORMAT, SAVE, NEW, LOAD, LIST: a cartridge written and read back by the
/// machine itself.
///
/// This is the test that says the microdrive works, and the only one that says
/// anything about writing. A program saved to a cartridge and loaded off it
/// again has been through every part of the interface — the motor chain, the
/// gap and sync lines, the twelve bytes of preamble in front of every block,
/// the checksums, and the ROM's own idea of where a file is.
#[test]
fn a_program_saved_to_a_cartridge_loads_back_off_it() {
    let Some((rom48, if1_rom)) = roms() else {
        eprintln!("need roms/48.rom and roms/if1.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &if1_rom, Some(Cartridge::blank("OLD", 180)));

    // FORMAT "m";1;"newcart" — the ROM writes every sector of the tape.
    keyword(&mut spec, ZERO);
    type_text(&mut spec, "\"m\";1;\"newcart\"");
    enter(&mut spec, 3000);
    assert!(
        printed(&spec, &rom48).contains("0 OK"),
        "FORMAT should finish: {}",
        printed(&spec, &rom48)
    );

    // A one-line program. REM is what the E key types at the start of a line.
    type_text(&mut spec, "10 ");
    hold(&mut spec, &[(2, 2)], 8);
    type_text(&mut spec, "hello");
    enter(&mut spec, 20);

    // SAVE *"m";1;"prog" — the star is what makes it the microdrive.
    hold(&mut spec, &[(1, 1)], 8);
    type_text(&mut spec, "*\"m\";1;\"prog\"");
    enter(&mut spec, 2000);
    let after_save = printed(&spec, &rom48);
    assert!(
        after_save.contains("0 OK"),
        "SAVE should finish without an error: {after_save}"
    );

    // The cartridge in the drive now holds the file, and the emulator's own
    // reading of it agrees with what the ROM wrote.
    let cart = spec.bus.if1.as_ref().unwrap().drives[0]
        .cartridge
        .as_ref()
        .unwrap();
    let names: Vec<String> = cart.catalogue().into_iter().map(|f| f.name).collect();
    assert!(
        names.iter().any(|n| n.trim() == "prog"),
        "the catalogue should hold the file the machine saved: {names:?}"
    );
    // Every sector adds up bar one: FORMAT writes a pattern of $FC to a
    // sector to check the tape and leaves it there, which is why the ROM
    // reports 89K free on a 180-sector cartridge rather than 90.
    let bad = cart.bad_checksums();
    assert!(
        bad.len() <= 1,
        "only the sector FORMAT used as scratch should fail: {bad:?}"
    );

    // NEW, then load it back and list it.
    hold(&mut spec, &[(1, 0)], 8);
    enter(&mut spec, 200);
    hold(&mut spec, &[(6, 3)], 8);
    type_text(&mut spec, "*\"m\";1;\"prog\"");
    enter(&mut spec, 2000);
    hold(&mut spec, &[(6, 2)], 8);
    enter(&mut spec, 60);

    let listed = printed(&spec, &rom48);
    assert!(
        listed.contains("10 REM hello"),
        "the program should come back off the cartridge: {listed}"
    );
}
