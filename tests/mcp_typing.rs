//! Typing a BASIC line at the machine, keywords and all.
//!
//! A Spectrum's keywords are one key each, not spelled out, so a tool that
//! types letter by letter cannot type `LOAD` — the L would give the keyword
//! and the rest would follow it as letters. These check against the machine:
//! what comes back is read out of the edit buffer, so the test sees what the
//! ROM made of the keys rather than what was sent.

use zx_rustrum::mcp::json::Json;
use zx_rustrum::mcp::tools::{Reply, Session};

fn call<const N: usize>(
    session: &mut Session,
    name: &str,
    args: [(&str, Json); N],
) -> Result<String, String> {
    session
        .call(name, &Json::obj(args))
        .map(|reply| match reply {
            Reply::Text(text) => text,
            Reply::Picture { text, .. } => text,
        })
}

/// What is in the edit buffer, with the ROM's tokens spelled out.
fn typed(session: &Session) -> String {
    let bus = &session.spec.bus;
    let start = u16::from(bus.peek_raw(0x5C59)) | (u16::from(bus.peek_raw(0x5C5A)) << 8);
    let rom = &bus.rom;
    let mut out = String::new();
    for i in 0..64u16 {
        let byte = bus.peek_raw(start + i);
        match byte {
            0x0D | 0x80 => break,
            0x20..=0x7E => out.push(byte as char),
            0xA5..=0xFF => {
                // The token table at $0095: each word ends with its last
                // letter's top bit set, and the first entry is the one before
                // the first token proper.
                let mut at = 0x0095;
                for _ in 0..(byte - 0xA4) {
                    while rom[at] & 0x80 == 0 {
                        at += 1;
                    }
                    at += 1;
                }
                loop {
                    let c = rom[at];
                    out.push((c & 0x7F) as char);
                    at += 1;
                    if c & 0x80 != 0 {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out.trim().to_string()
}

fn booted() -> Option<Session> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let mut session = Session::new();
    session
        .spec
        .set_model(zx_rustrum::machine::Model::Spectrum48, &rom);
    session.spec.reset();
    session.rom_loaded = true;
    for _ in 0..200 {
        session.spec.run(zx_rustrum::machine::FRAME_T);
    }
    Some(session)
}

/// A microdrive command types itself: the keyword, the star, the quotes and
/// the semicolons are all where the machine keeps them.
#[test]
fn a_microdrive_command_types_itself() {
    let Some(mut session) = booted() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    call(
        &mut session,
        "type_text",
        [
            ("text", Json::str("LOAD *\"m\";1;\"prog\"")),
            ("enter", Json::Bool(false)),
        ],
    )
    .expect("typed");
    assert_eq!(typed(&session), "LOAD *\"m\";1;\"prog\"");
}

/// The words that live in extended mode type themselves too, which is the only
/// way to reach them: CAT is not three letters, it is one key with both shifts
/// in front of it.
#[test]
fn the_extended_mode_words_type_themselves() {
    let Some(mut session) = booted() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    call(
        &mut session,
        "type_text",
        [("text", Json::str("CAT 1")), ("enter", Json::Bool(false))],
    )
    .expect("typed");
    assert_eq!(typed(&session), "CAT 1");
}

/// A word inside a string is a word, not a keyword. "info" holds IN.
#[test]
fn a_keyword_inside_a_string_is_just_letters() {
    let Some(mut session) = booted() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    call(
        &mut session,
        "type_text",
        [
            ("text", Json::str("SAVE *\"m\";1;\"info\"")),
            ("enter", Json::Bool(false)),
        ],
    )
    .expect("typed");
    assert_eq!(typed(&session), "SAVE *\"m\";1;\"info\"");
}

/// And a keyword that is only the start of a longer word is not a keyword:
/// PRINTER is a variable name, not PRINT followed by ER.
#[test]
fn a_keyword_that_starts_a_longer_word_is_not_one() {
    let Some(mut session) = booted() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    call(
        &mut session,
        "type_text",
        [
            ("text", Json::str("LET printer=1")),
            ("enter", Json::Bool(false)),
        ],
    )
    .expect("typed");
    // Lower case, because that is what the machine types: the case of the
    // text asked for says nothing, only the keys are sent.
    assert_eq!(typed(&session), "LET printer=1");
}

/// Asking for a letter where the machine wants a keyword is an error that says
/// which mode it is in, rather than typing a keyword nobody asked for.
#[test]
fn typing_a_letter_at_the_start_of_a_line_says_what_would_happen() {
    let Some(mut session) = booted() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    let err = call(
        &mut session,
        "type_text",
        [("text", Json::str("abc=1")), ("enter", Json::Bool(false))],
    )
    .expect_err("K mode");
    assert!(err.contains("K mode"), "{err}");
    assert!(err.contains("LET"), "and what to do instead: {err}");
}
