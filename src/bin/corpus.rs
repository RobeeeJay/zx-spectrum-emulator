//! Build a training corpus out of somebody else's annotations.
//!
//! Given a memory image and a symbol file describing what is in it, this reads
//! every named routine with the emulator's *own* feature extractor and writes
//! a row per routine. The extractor matters more than the format: a model
//! trained on features from a separate reimplementation would be asked
//! different questions at inference time than it was taught to answer.
//!
//! ```text
//! corpus roms/48.rom symbols-48.txt > rom48.csv
//! corpus game.z80 aticatac.symbols.txt > aticatac.csv
//! ```
//!
//! A `.rom` or `.bin` is loaded at $0000 unless `--org` says otherwise; a
//! `.sna` or `.z80` is loaded as the snapshot it is.

use zx_rustrum::autodoc::{self, Features};
use zx_rustrum::machine::Spectrum;
use zx_rustrum::snapshot;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut org = 0u16;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--org" => {
                org = iter
                    .next()
                    .and_then(|v| u16::from_str_radix(v.trim_start_matches('$'), 16).ok())
                    .unwrap_or(0);
            }
            other => positional.push(other.to_string()),
        }
    }
    let [image, symbols] = positional.as_slice() else {
        eprintln!("usage: corpus <image> <symbols.txt> [--org ADDR]");
        std::process::exit(2);
    };

    let memory = match load(image, org) {
        Ok(memory) => memory,
        Err(e) => {
            eprintln!("{image}: {e}");
            std::process::exit(1);
        }
    };
    let text = match std::fs::read_to_string(symbols) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{symbols}: {e}");
            std::process::exit(1);
        }
    };

    let (named, _) = autodoc::parse_symbols(&text);
    let peek = |a: u16| memory[a as usize];

    println!("{}", header());
    let mut written = 0;
    let mut skipped = 0;
    for (address, name, description) in named {
        // Only routines are worth a row: a symbol on a table of bytes would
        // teach the model that data looks like code.
        let features = autodoc::read_routine(&peek, address);
        if features.length < 2 {
            skipped += 1;
            continue;
        }
        println!("{}", row(address, &name, &description, &features));
        written += 1;
    }
    eprintln!("{written} routines written, {skipped} too short to be one");
}

/// Load whatever was given into a flat 64K image.
fn load(path: &str, org: u16) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let kind = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match kind.as_str() {
        "sna" | "z80" => {
            let model = snapshot::probe_model_bytes(&kind, &data)?;
            let mut spec = Spectrum::with_model(model);
            match kind.as_str() {
                "sna" => snapshot::load_sna(&mut spec, &data)?,
                _ => snapshot::load_z80(&mut spec, &data)?,
            }
            Ok((0..=u16::MAX).map(|a| spec.bus.peek_raw(a)).collect())
        }
        _ => {
            let mut memory = vec![0u8; 0x10000];
            let at = org as usize;
            let end = (at + data.len()).min(memory.len());
            memory[at..end].copy_from_slice(&data[..end - at]);
            Ok(memory)
        }
    }
}

fn header() -> String {
    [
        "address",
        "name",
        // What the rules make of it now, so the model can be compared against
        // the thing it is meant to improve on.
        "rule_label",
        "instructions",
        "calls",
        "constants",
        "ports_in",
        "ports_out",
        "block_move",
        "shifts",
        "indirect_writes",
        "masked_writes",
        "compares",
        "daa",
        "reads_r",
        "reads_rom",
        "next_scanline",
        "third_crossing",
        "attribute_address",
        "touches_screen",
        "touches_attrs",
        "description",
    ]
    .join(",")
}

fn row(address: u16, name: &str, description: &str, f: &Features) -> String {
    let (rule_label, _) = autodoc::describe(f);
    let screen = f.constants.iter().any(|c| (0x4000..0x5800).contains(c));
    let attrs = f.constants.iter().any(|c| (0x5800..0x5B00).contains(c));
    let fields: Vec<String> = vec![
        format!("{address:04X}"),
        quote(name),
        rule_label,
        f.length.to_string(),
        f.calls.len().to_string(),
        f.constants.len().to_string(),
        f.ports_in.len().to_string(),
        f.ports_out.len().to_string(),
        u8::from(f.ldir || f.lddr).to_string(),
        f.shifts.to_string(),
        f.indirect_writes.to_string(),
        f.masked_writes.to_string(),
        f.compares.to_string(),
        u8::from(f.daa).to_string(),
        u8::from(f.reads_r).to_string(),
        u8::from(f.reads_rom).to_string(),
        u8::from(f.next_scanline).to_string(),
        u8::from(f.third_crossing).to_string(),
        u8::from(f.attribute_address).to_string(),
        u8::from(screen).to_string(),
        u8::from(attrs).to_string(),
        quote(description),
    ];
    fields.join(",")
}

/// Comma-separated values, with the commas somebody wrote inside a description
/// kept out of the way.
fn quote(text: &str) -> String {
    let text = text.replace('"', "'");
    if text.contains(',') || text.contains('"') {
        format!("\"{text}\"")
    } else {
        text
    }
}
