//! The Currah µSpeech, with Currah's own ROM in it.
//!
//! Everything the machine can see of this box is emulated: the ROM that turns
//! over at $0038, the speech chip's register at $1000 and its busy line, and
//! the two pitches at $3000. What is not here is the sound — the SP0256-AL2
//! keeps its allophones inside itself as filter coefficients — so what these
//! tests check is that the machine drives the chip, not that anything is
//! audible.
//!
//! Skips itself when the ROMs are not there.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::uspeech::Uspeech;

const SYM: (usize, u8) = (7, 1);
const ENTER: (usize, u8) = (6, 0);

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

/// Which key a character is on, and whether SYMBOL SHIFT goes with it.
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
    let shifted = [('"', 'P'), ('$', '4'), ('=', 'L')];
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

fn roms() -> Option<(Vec<u8>, Vec<u8>)> {
    Some((
        std::fs::read("roms/48.rom").ok()?,
        std::fs::read("roms/uspeech.rom").ok()?,
    ))
}

fn machine(rom48: &[u8], speech: &[u8]) -> Spectrum {
    let mut spec = Spectrum::new();
    spec.set_model(Model::Spectrum48, rom48);
    spec.reset();
    let mut uspeech = Uspeech::new();
    uspeech.rom = Some(speech.to_vec());
    spec.bus.uspeech = Some(uspeech);
    for _ in 0..250 {
        spec.run(FRAME_T);
    }
    spec
}

/// A machine with a µSpeech on the back comes up saying so.
///
/// Nothing has to be typed to bring the interface in: the ULA's interrupt is a
/// fetch from $0038, which is the address that turns the box over, so its ROM
/// gets the machine once a frame from the moment it is plugged in. Its own
/// sign-on above Sinclair's is the proof that its ROM is running.
#[test]
fn a_machine_with_one_fitted_comes_up_with_currahs_sign_on() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let spec = machine(&rom48, &speech);
    let text = screen_text(&spec, &rom48).join(" / ");
    assert!(
        text.contains("CURRAH"),
        "the interface should sign on: {text}"
    );
    assert!(
        text.contains("1982 Sinclair Research"),
        "over the machine's own: {text}"
    );

    // And it is out of the way again by the time anything else runs: the
    // handler pages it in at the interrupt and out on its way back.
    assert!(
        !spec.bus.uspeech.as_ref().unwrap().paged,
        "the machine's own ROM is what BASIC runs in"
    );
}

/// `LET s$="hello"` is how the Currah is spoken to, and the allophones come
/// out of it one at a time.
///
/// The driver writes a pause to the chip every interrupt whether or not there
/// is anything to say, so the thing to count is the allophones that are
/// sounds. Nothing is audible — the SP0256's own ROM is not here — but every
/// byte the machine sends the chip is.
#[test]
fn what_is_put_in_s_dollar_reaches_the_speech_chip() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &speech);
    let quiet = spec.bus.uspeech.as_ref().unwrap().phonemes;

    // LET is on the L key, and the rest is typed as it reads.
    hold(&mut spec, &[(6, 1)], 8);
    type_text(&mut spec, "s$=\"hello\"");
    hold(&mut spec, &[ENTER], 8);
    for _ in 0..200 {
        spec.run(FRAME_T);
    }

    let uspeech = spec.bus.uspeech.as_ref().unwrap();
    assert!(
        uspeech.phonemes > quiet,
        "the machine should have said something: {} sounds and {} allophones \
         in all, where before it had said {quiet}",
        uspeech.phonemes,
        uspeech.spoken
    );

    let text = screen_text(&spec, &rom48).join(" / ");
    assert!(
        !text.contains("Nonsense in BASIC"),
        "and the line should have run: {text}"
    );
}

/// The busy line is what a program waits on between allophones, and it is up
/// for as long as the allophone takes.
#[test]
fn the_busy_line_lasts_as_long_as_the_allophone_does() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &speech);

    // Reach in and drive the chip the way a program would: turn the interface
    // on with a read of $0038, say /OY/ — the longest allophone there is, at
    // 291ms — and watch the line.
    let start = spec.bus.total_t();
    let uspeech = spec.bus.uspeech.as_mut().unwrap();
    uspeech.at(start, 3_500_000.0);
    uspeech.touch(0x0038);
    assert!(uspeech.paged, "a read of $0038 turns it on");
    uspeech.poke(0x1000, 0x05);
    assert!(
        uspeech.busy(),
        "and it is busy the moment it is told to talk"
    );

    // A tenth of a second in it is still going; half a second later it is not.
    uspeech.at(start + 350_000, 3_500_000.0);
    assert!(
        uspeech.busy(),
        "/OY/ takes 291ms, so 100ms in it is talking"
    );
    uspeech.at(start + 1_750_000, 3_500_000.0);
    assert!(!uspeech.busy(), "and by half a second it has finished");

    // The next access to $0038 puts the machine's own ROM back.
    uspeech.touch(0x0038);
    assert!(!uspeech.paged);
    assert_eq!(uspeech.mem(0x0000), None, "and the box answers nothing");
}
