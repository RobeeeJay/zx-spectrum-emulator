//! Names that came from somewhere other than this session: symbol files, and
//! the fingerprints of routines known by their first bytes.

use std::path::PathBuf;

use crate::machine::Model;
use crate::mcp::json::Json;
use crate::mcp::tools::{addr, Session};

/// The symbol files this machine would use, in the order they are read.
/// The paged machines have a file per ROM, since a name means nothing
/// without knowing which ROM is in.
pub fn symbol_files(session: &Session) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Some(dir) = crate::prefs::config_dir() else {
        return files;
    };
    let machine = match session.spec.bus.model {
        Model::Spectrum48 => "48",
        Model::Spectrum128 => "128",
        Model::Plus2A => "plus2a",
        Model::Plus3 => "plus3",
    };
    files.push(dir.join("symbols.txt"));
    files.push(dir.join(format!("symbols-{machine}.txt")));
    if session.spec.bus.rom_pages() > 1 {
        let rom = session.spec.bus.rom_in_use();
        files.push(dir.join(format!("symbols-{machine}-rom{rom}.txt")));
    }
    files
}

/// Read the symbol files, and build the signature table from the ROM in
/// the machine. `tools/rom-symbols.py` and `tools/skool-symbols.py` write
/// files of the right shape.
pub fn load_symbols(session: &mut Session, args: &Json) -> Result<String, String> {
    let mut text = String::new();
    let mut read = Vec::new();
    let paths: Vec<PathBuf> = match args.get("path").and_then(|p| p.as_str()) {
        Some(one) => vec![PathBuf::from(one)],
        None => symbol_files(session),
    };
    for path in &paths {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                text.push_str(&content);
                text.push('\n');
                read.push(path.display().to_string());
            }
            // A missing file is not a failure: most people have none, and
            // the ones they have are the ones they made.
            Err(_) => continue,
        }
    }
    session.symbols = crate::autodoc::Symbols::from_text(&text);
    session.signatures = crate::autodoc::Signatures::from_rom(&session.spec.bus.rom);
    session.signatures.add_from_text(&text);
    if read.is_empty() {
        return Ok(format!(
            "No symbol file found; looked at {}. {} routines are recognised by their \
             bytes from the ROM in the machine, which works wherever a copy of one \
             has been put. tools/rom-symbols.py and tools/skool-symbols.py write \
             symbol files.",
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            session.signatures.len()
        ));
    }
    Ok(format!(
        "{} names from {}, and {} routines recognisable by their bytes. \
         Names from a file are shown by disassemble and routines.",
        session.symbols.len(),
        read.join(", "),
        session.signatures.len()
    ))
}

pub fn list_symbols(session: &mut Session, args: &Json) -> Result<String, String> {
    if session.symbols.is_empty() {
        return Err("no symbols are loaded: call load_symbols".into());
    }
    let from = match args.get("from") {
        Some(_) => addr(args, "from")?,
        None => 0,
    };
    let to = match args.get("to") {
        Some(_) => addr(args, "to")?,
        None => 0xFFFF,
    };
    let mut lines = Vec::new();
    for at in from..=to {
        if let Some((name, comment)) = session.symbols.get(at) {
            lines.push(if comment.is_empty() {
                format!("${at:04X}  {name}")
            } else {
                format!("${at:04X}  {name}  — {comment}")
            });
        }
        if at == 0xFFFF {
            break;
        }
    }
    if lines.is_empty() {
        return Ok(format!("no names between ${from:04X} and ${to:04X}"));
    }
    let shown = lines.len().min(200);
    let mut out = lines[..shown].join("\n");
    if lines.len() > shown {
        out.push_str(&format!("\n({} more)", lines.len() - shown));
    }
    Ok(out)
}

/// What the code at an address is, if its first bytes are a routine that
/// is known. Games copy ROM routines into RAM; the same bytes hash the
/// same wherever they land, so a copy is named after the original.
pub fn identify(session: &mut Session, args: &Json) -> Result<String, String> {
    let at = addr(args, "address")?;
    if session.signatures.is_empty() {
        session.signatures = crate::autodoc::Signatures::from_rom(&session.spec.bus.rom);
    }
    let peek = |a: u16| session.spec.bus.peek_raw(a);
    match session.signatures.identify(&peek, at) {
        Some((name, comment)) => Ok(format!("${at:04X} is {name} — {comment}")),
        None => Ok(format!(
            "${at:04X} does not match any of the {} routines known by their bytes. \
             That is not evidence it is nothing; the table only holds what can be \
             checked against the ROM in the machine.",
            session.signatures.len()
        )),
    }
}
