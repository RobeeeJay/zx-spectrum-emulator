//! What is on the tape, and who is reading it.
//!
//! A tape's block list is the memory map before there is one: a standard block
//! carries a 17-byte header saying where it loads and how long it is, and a
//! turbo block is a game's own loader having taken over. Which loader that is
//! can be told from the sampling loop the machine is sitting in, which is what
//! `flashload::CORES` recognises.

use crate::mcp::json::Json;
use crate::mcp::tools::{count, Session};

pub fn tape_blocks(session: &mut Session, args: &Json) -> Result<String, String> {
    let tape = session
        .spec
        .bus
        .tape
        .as_ref()
        .ok_or("no tape is in the deck: load_tape first")?;
    let limit = count(args, "limit", 60)? as usize;
    let mut out = format!(
        "{}: {} blocks. The deck is {}.\n",
        tape.name,
        tape.blocks.len(),
        if session.spec.bus.tape_playing() {
            "playing"
        } else {
            "stopped"
        }
    );
    out.push_str(
        "A standard block with a header says what it loads and where; a turbo block is \
         the game's own loader at work.\n",
    );
    for (i, block) in tape.blocks.iter().enumerate().take(limit) {
        let here = if i == tape.block { " <- here" } else { "" };
        out.push_str(&format!("{i:4}  {}{here}\n", block.describe()));
        // The header of a standard block carries the address it loads at,
        // which is the one fact worth pulling out of the bytes.
        if let crate::tape::Block::Standard { data, .. } = block {
            if data.len() == 19 && data[0] == 0x00 {
                let length = u16::from_le_bytes([data[12], data[13]]);
                let param = u16::from_le_bytes([data[14], data[15]]);
                out.push_str(
                    match data[1] {
                        0 => format!("        {length} bytes of BASIC, autostart line {param}\n"),
                        3 => format!("        {length} bytes, loading at ${param:04X}\n"),
                        _ => format!("        {length} bytes\n"),
                    }
                    .as_str(),
                );
            }
        }
    }
    if tape.blocks.len() > limit {
        out.push_str(&format!("({} more)\n", tape.blocks.len() - limit));
    }
    Ok(out)
}

/// Which loader the machine is in, if it is in one.
///
/// Read off the sampling loop the CPU is sitting in rather than off a list of
/// games: seven of them, taken from the tapes themselves. A game with a loader
/// of its own counts its own pulses and cannot be handed a block, which is why
/// this is worth asking before wondering why loading is slow.
pub fn loader(session: &mut Session, _args: &Json) -> Result<String, String> {
    let at = session.spec.cpu.pc;
    match crate::flashload::sampler(&session.spec) {
        Some(name) => Ok(format!(
            "The machine is in {name}'s sampling loop at ${at:04X} — it is counting pulses \
             off the tape itself."
        )),
        None => Ok(format!(
            "Not in a loader's sampling loop at ${at:04X}. Either nothing is loading, or \
             the loader is one nobody has read off a tape yet."
        )),
    }
}
