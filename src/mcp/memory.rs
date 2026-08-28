//! Memory: reading it, writing it, searching it, and seeing what changed.
//!
//! Searching and comparing are how a variable is found without being told
//! where it is. Save the machine, play a life away, ask what changed, and the
//! lives counter is in the handful of addresses that came back.

use crate::mcp::json::Json;
use crate::mcp::tools::{addr, count, Session};

pub fn read_memory(session: &mut Session, args: &Json) -> Result<String, String> {
    let start = addr(args, "address")?;
    let length = count(args, "length", 256)?.min(crate::mcp::tools::MAX_READ as u32) as usize;
    let bank = args.get("bank").and_then(|b| b.as_i64());
    let mut bytes = Vec::with_capacity(length);
    for i in 0..length {
        let at = start.wrapping_add(i as u16);
        bytes.push(match bank {
            Some(bank) => session.spec.bus.bank_byte(bank as usize, at),
            None => session.spec.bus.peek_raw(at),
        });
    }
    let mut out = match bank {
        Some(bank) => format!("{length} bytes from ${start:04X} in bank {bank}\n"),
        None => format!("{length} bytes from ${start:04X} as the CPU sees them\n"),
    };
    out.push_str(&hex_dump(start, &bytes));
    Ok(out)
}

/// Bytes given as hex in a string, or as a list of numbers.
pub fn bytes_argument(args: &Json) -> Result<Vec<u8>, String> {
    match args.get("bytes") {
        Some(Json::Str(text)) => parse_hex_bytes(text),
        Some(Json::Arr(items)) => items
            .iter()
            .map(|i| {
                i.as_i64()
                    .filter(|v| (0..=255).contains(v))
                    .map(|v| v as u8)
                    .ok_or_else(|| format!("{i} is not a byte"))
            })
            .collect::<Result<Vec<u8>, String>>(),
        _ => Err("write_memory needs bytes: a list, or a string of hex".into()),
    }
}

pub fn write_memory(session: &mut Session, args: &Json) -> Result<String, String> {
    let start = addr(args, "address")?;
    let bytes = bytes_argument(args)?;
    for (i, byte) in bytes.iter().enumerate() {
        session.spec.bus.poke(start.wrapping_add(i as u16), *byte);
    }
    Ok(format!(
        "Wrote {} bytes at ${start:04X}. This changes the machine, not the file.",
        bytes.len()
    ))
}

pub fn parse_hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let part = part.trim_start_matches('$').trim_start_matches("0x");
            u8::from_str_radix(part, 16).map_err(|_| format!("{part:?} is not a hex byte"))
        })
        .collect()
}

/// Sixteen bytes a line, with the printable ones beside them.
pub fn hex_dump(start: u16, bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let at = start.wrapping_add((row * 16) as u16);
        out.push_str(&format!("${at:04X}  "));
        for i in 0..16 {
            match chunk.get(i) {
                Some(b) => out.push_str(&format!("{b:02X} ")),
                None => out.push_str("   "),
            }
        }
        out.push(' ');
        for b in chunk {
            // The Spectrum's own character set is ASCII from 32 to 126; above
            // that are its block graphics and its keywords, which are not
            // letters and are not printed as any.
            out.push(if (0x20..0x7F).contains(b) {
                *b as char
            } else {
                '.'
            });
        }
        out.push('\n');
    }
    out
}

/// Where a sequence of bytes appears in memory.
///
/// The way to find a sprite whose bytes you have, a string the game prints, or
/// the code you are looking at somewhere else. Text is matched as ASCII, which
/// is what the Spectrum's own character set is between 32 and 126.
pub fn find_bytes(session: &mut Session, args: &Json) -> Result<String, String> {
    let wanted: Vec<u8> = match (args.get("bytes"), args.get("text"), args.get("value")) {
        (Some(Json::Str(hex)), _, _) => parse_hex_bytes(hex)?,
        (Some(Json::Arr(items)), _, _) => items
            .iter()
            .map(|i| {
                i.as_i64()
                    .filter(|v| (0..=255).contains(v))
                    .map(|v| v as u8)
                    .ok_or_else(|| format!("{i} is not a byte"))
            })
            .collect::<Result<_, _>>()?,
        (_, Some(Json::Str(text)), _) => text.bytes().collect(),
        (_, _, Some(value)) => vec![value
            .as_i64()
            .filter(|v| (0..=255).contains(v))
            .map(|v| v as u8)
            .ok_or("value should be a byte")?],
        _ => return Err("find_bytes needs bytes, text or value".into()),
    };
    if wanted.is_empty() {
        return Err("nothing to look for".into());
    }
    let from = match args.get("from") {
        Some(_) => addr(args, "from")?,
        None => 0x4000,
    };
    let to = match args.get("to") {
        Some(_) => addr(args, "to")?,
        None => 0xFFFF,
    };
    let limit = count(args, "limit", 40)?.min(500) as usize;

    let mut found = Vec::new();
    let mut at = from;
    loop {
        if wanted
            .iter()
            .enumerate()
            .all(|(i, b)| session.spec.bus.peek_raw(at.wrapping_add(i as u16)) == *b)
        {
            found.push(at);
        }
        if at >= to || at == 0xFFFF {
            break;
        }
        at = at.wrapping_add(1);
    }
    if found.is_empty() {
        return Ok(format!(
            "nothing between ${from:04X} and ${to:04X} matches those {} bytes",
            wanted.len()
        ));
    }
    let shown: Vec<String> = found
        .iter()
        .take(limit)
        .map(|a| format!("${a:04X}"))
        .collect();
    Ok(format!(
        "{} matches between ${from:04X} and ${to:04X}: {}{}",
        found.len(),
        shown.join(", "),
        if found.len() > shown.len() {
            format!(" (and {} more)", found.len() - shown.len())
        } else {
            String::new()
        }
    ))
}

/// What has changed in memory since a snapshot was taken.
///
/// This is how a variable is found: save the machine, lose a life, ask what
/// changed, and the counter is among the handful of addresses that come back.
/// Narrow it by asking for the ones that now hold a particular value.
pub fn changed_since(session: &mut Session, args: &Json) -> Result<String, String> {
    let name = args
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("last")
        .to_string();
    let before = session
        .memories
        .get(&name)
        .ok_or_else(|| {
            format!(
                "no state called {name:?}; there is {}",
                if session.memories.is_empty() {
                    "nothing saved".to_string()
                } else {
                    session
                        .memories
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            )
        })?
        .clone();
    let from = match args.get("from") {
        Some(_) => addr(args, "from")?,
        None => 0x4000,
    };
    let to = match args.get("to") {
        Some(_) => addr(args, "to")?,
        None => 0xFFFF,
    };
    let wanted_now = match args.get("value") {
        Some(v) => Some(
            v.as_i64()
                .filter(|n| (0..=255).contains(n))
                .map(|n| n as u8)
                .ok_or("value should be a byte")?,
        ),
        None => None,
    };
    let limit = count(args, "limit", 60)?.min(2000) as usize;
    let screen_too = crate::mcp::tools::flag(args, "include_screen", false);

    let mut changed = Vec::new();
    for at in from..=to {
        // The display file changes every frame and says nothing about what a
        // variable is; it is left out unless asked for.
        if !screen_too && (0x4000..0x5B00).contains(&at) {
            continue;
        }
        let was = before[(at as usize) - 0x4000];
        let now = session.spec.bus.peek_raw(at);
        if was != now && wanted_now.is_none_or(|want| now == want) {
            changed.push((at, was, now));
        }
        if at == 0xFFFF {
            break;
        }
    }
    if changed.is_empty() {
        return Ok(format!(
            "nothing between ${from:04X} and ${to:04X} differs from {name:?}"
        ));
    }
    let mut out = format!(
        "{} addresses differ from {name:?}{}:\n",
        changed.len(),
        match wanted_now {
            Some(v) => format!(" and now hold ${v:02X}"),
            None => String::new(),
        }
    );
    for (at, was, now) in changed.iter().take(limit) {
        out.push_str(&format!("  ${at:04X}  ${was:02X} -> ${now:02X}\n"));
    }
    if changed.len() > limit {
        out.push_str(&format!(
            "  ({} more; narrow it with from, to or value)\n",
            changed.len() - limit
        ));
    }
    Ok(out)
}
