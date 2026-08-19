//! A routine written out as flow rather than as bytes in address order.
//!
//! A disassembly listing is a wall of instructions in memory order. What it
//! hides is the shape: which parts are loops, how many times they went round,
//! which lines are the way out, and where a call actually goes. All of that is
//! known here — the loops and their trip counts were measured while the
//! program ran — and none of it was being said.
//!
//! What comes out is the same instructions with the shape put back:
//!
//! ```text
//! 8A75  LD IX,$5E00
//! 8A7E  CALL $8A8A          -> into the routine below
//! 8A8C  loop, 7 times round
//!   8A96    CPIR
//!   8A9D    loop, 8 times round
//!     8A9E      LD (DE),A
//!     8AA0      INC D               ; down one pixel row
//!   8AA6    JP NZ,$8A8C     back to the loop above
//! 8AAE  RET NZ              way out
//! ```

use std::collections::{BTreeMap, BTreeSet};

use crate::autodoc::ends_routine;
use crate::disasm;

/// How far to follow a routine before giving up.
const MAX_INSTRUCTIONS: usize = 300;

/// Write a routine out with its loops and exits marked.
///
/// `trips` is what each loop was measured doing, by the address jumped back
/// to — the observer's `loops` map. Without it the shape is still shown, just
/// without the counts.
pub fn flow<F: Fn(u16) -> u8>(
    peek: &F,
    entry: u16,
    trips: &BTreeMap<u16, u32>,
    name_of: &dyn Fn(u16) -> Option<String>,
) -> Vec<String> {
    let instructions = decode(peek, entry);
    let (starts, ends) = extent(&instructions);
    let back_edges = back_edges(&instructions, starts, ends);

    // Where each loop begins, so its body can be indented.
    let heads: BTreeSet<u16> = back_edges.values().copied().collect();

    let mut out = Vec::new();
    let mut depth = 0usize;
    for (addr, text, _) in &instructions {
        if heads.contains(addr) {
            let times = trips.get(addr).copied();
            out.push(match times {
                Some(n) => format!(
                    "{:indent$}{addr:04X}  loop, {n} times round",
                    "",
                    indent = depth * 2
                ),
                None => format!("{:indent$}{addr:04X}  loop", "", indent = depth * 2),
            });
            depth += 1;
        }

        let mut note = String::new();
        if let Some(target) = back_edges.get(addr) {
            note = format!("   back to the loop at ${target:04X}");
        } else if let Some(target) = call_target(text) {
            note = match name_of(target) {
                Some(name) => format!("   -> {name}"),
                None if (entry..=ends).contains(&target) => "   -> into this routine".to_string(),
                None => String::new(),
            };
        } else if text.starts_with("RET") && !ends_routine(text) {
            note = "   way out".to_string();
        } else if text == "INC H" || text == "DEC H" {
            note = "   ; down one pixel row of the display file".to_string();
        } else if text.starts_with("LDIR") || text.starts_with("LDDR") {
            note = "   ; block copy".to_string();
        }

        out.push(format!(
            "{:indent$}{addr:04X}  {text}{note}",
            "",
            indent = depth * 2
        ));

        // A back-edge closes the loop it belongs to.
        if back_edges.contains_key(addr) {
            depth = depth.saturating_sub(1);
        }
    }
    out
}

/// The instructions of a routine, in memory order.
fn decode<F: Fn(u16) -> u8>(peek: &F, entry: u16) -> Vec<(u16, String, u8)> {
    let mut at = entry;
    let mut out = Vec::new();
    for _ in 0..MAX_INSTRUCTIONS {
        let insn = disasm::disasm(peek, at);
        let len = insn.len.max(1);
        out.push((at, insn.text.clone(), len));
        if ends_routine(&insn.text) || insn.text.starts_with("JP $") {
            break;
        }
        at = at.wrapping_add(len as u16);
    }
    out
}

/// The first and last address of a routine.
fn extent(instructions: &[(u16, String, u8)]) -> (u16, u16) {
    let first = instructions.first().map(|(a, _, _)| *a).unwrap_or(0);
    let last = instructions
        .last()
        .map(|(a, _, len)| a.wrapping_add(*len as u16))
        .unwrap_or(first);
    (first, last)
}

/// Jumps that go backwards within the routine: the loops.
fn back_edges(instructions: &[(u16, String, u8)], first: u16, last: u16) -> BTreeMap<u16, u16> {
    let mut edges = BTreeMap::new();
    for (addr, text, _) in instructions {
        let Some(target) = jump_target(text) else {
            continue;
        };
        if target < *addr && (first..=last).contains(&target) {
            edges.insert(*addr, target);
        }
    }
    edges
}

fn jump_target(text: &str) -> Option<u16> {
    let rest = text
        .strip_prefix("JP ")
        .or_else(|| text.strip_prefix("JR "))
        .or_else(|| text.strip_prefix("DJNZ "))?;
    parse_hex(rest.rsplit(',').next()?)
}

fn call_target(text: &str) -> Option<u16> {
    let rest = text.strip_prefix("CALL ")?;
    parse_hex(rest.rsplit(',').next()?)
}

fn parse_hex(text: &str) -> Option<u16> {
    u16::from_str_radix(text.trim().strip_prefix('$')?, 16).ok()
}
