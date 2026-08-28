//! Typing at the machine.
//!
//! The keyboard is the same eight rows of five on every Spectrum, and the ROM
//! scans it once a frame: what makes a keypress work is holding it long enough
//! to be scanned twice, not sending an event.

use crate::machine::{Model, FRAME_T};
use crate::mcp::json::Json;
use crate::mcp::tools::{count, flag, text, Session};

/// Hold keys down, run, and let go.
///
/// The ROM scans the keyboard once a frame and wants a key on two scans
/// running before it believes in it, so a key held for one frame types
/// nothing. Ten frames is a fifth of a second, which is a person pressing
/// a key rather than a machine pretending to.
pub fn press_keys(session: &mut Session, args: &Json) -> Result<String, String> {
    let names: Vec<String> = match args.get("keys") {
        Some(Json::Arr(items)) => items
            .iter()
            .map(|i| {
                i.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| format!("{i} is not a key name"))
            })
            .collect::<Result<_, _>>()?,
        Some(Json::Str(one)) => vec![one.clone()],
        _ => return Err("press_keys needs keys: a name, or a list of them".into()),
    };
    let hold = count(args, "frames", 10)?.clamp(1, 600);
    let after = count(args, "then_frames", 10)?.min(600);

    let mut pressed = Vec::new();
    for name in &names {
        pressed.push(key_named(name, session.spec.bus.model)?);
    }
    for (row, bit) in &pressed {
        session.spec.bus.keys[*row] &= !(1 << bit);
    }
    for _ in 0..hold {
        session.spec.run(FRAME_T);
    }
    for (row, bit) in &pressed {
        session.spec.bus.keys[*row] |= 1 << bit;
    }
    for _ in 0..after {
        session.spec.run(FRAME_T);
    }
    Ok(format!(
        "Held {} for {hold} frames, then ran {after} more. {}",
        names.join(" + "),
        session.registers()
    ))
}

/// Type a line the way somebody at the keyboard would: a letter at a
/// time, with ENTER at the end unless told otherwise.
///
/// Only what can be typed without shifts: letters, digits, space and
/// ENTER. A Spectrum's punctuation is behind SYMBOL SHIFT and its keywords
/// behind a mode, and pretending otherwise would type something else.
pub fn type_text(session: &mut Session, args: &Json) -> Result<String, String> {
    let line = text(args, "text")?;
    let hold = count(args, "frames", 6)?.clamp(1, 60);
    for c in line.chars() {
        let name = match c {
            ' ' => "SPACE".to_string(),
            c if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
            other => {
                return Err(format!(
                    "{other:?} cannot be typed without a shift; press_keys takes \
                     SYMBOL SHIFT and a key together"
                ))
            }
        };
        let (row, bit) = key_named(&name, session.spec.bus.model)?;
        session.spec.bus.keys[row] &= !(1 << bit);
        for _ in 0..hold {
            session.spec.run(FRAME_T);
        }
        session.spec.bus.keys[row] |= 1 << bit;
        for _ in 0..hold {
            session.spec.run(FRAME_T);
        }
    }
    if flag(args, "enter", true) {
        let (row, bit) = key_named("ENTER", session.spec.bus.model)?;
        session.spec.bus.keys[row] &= !(1 << bit);
        for _ in 0..hold {
            session.spec.run(FRAME_T);
        }
        session.spec.bus.keys[row] |= 1 << bit;
        for _ in 0..hold {
            session.spec.run(FRAME_T);
        }
    }
    Ok(format!("Typed {line:?}. {}", session.registers()))
}

/// Where a key is in the matrix, by the name written on it.
///
/// The keyboard is the same eight rows of five on every Spectrum and on the
/// ZX81; what differs is the words printed on the keys, which is why this asks
/// the layout rather than holding a table of its own.
pub fn key_named(name: &str, _model: Model) -> Result<(usize, u8), String> {
    let wanted = name.trim().to_ascii_uppercase();
    let wanted = match wanted.as_str() {
        // What people call them, against what is printed on them.
        "CAPS" | "SHIFT" | "CAPSSHIFT" | "CAPS_SHIFT" => "CAPS SHIFT".to_string(),
        "SYMBOL" | "SYM" | "SYMBOLSHIFT" | "SYMBOL_SHIFT" => "SYMBOL SHIFT".to_string(),
        "RETURN" | "NEWLINE" | "CR" => "ENTER".to_string(),
        other => other.to_string(),
    };
    crate::keyboard::SPECTRUM
        .iter()
        .find(|key| key.main.eq_ignore_ascii_case(&wanted))
        .map(|key| key.press[0])
        .ok_or_else(|| {
            format!(
                "no key called {name:?}. The forty are 0-9, A-Z, ENTER, SPACE, \
                 CAPS SHIFT and SYMBOL SHIFT."
            )
        })
}
