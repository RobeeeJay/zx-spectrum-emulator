//! Where the routines and the data are, worked out from watching the program.
//!
//! A disassembly is a wall of instructions with nothing to say where one thing
//! ends and the next begins. What a reader wants is the shape: this run of
//! bytes is a routine, that run is a table it reads, and the next one is
//! something else again. That shape cannot be had by decoding forwards — a
//! table of graphics disassembles perfectly well into nonsense — so it is
//! taken from what the machine did.
//!
//! Code blocks come from routines that were entered: where each one starts is
//! where it was called, and where it ends is the furthest it reached before
//! returning. Data blocks come from addresses that were read and never
//! executed. Everything else is left alone, because nothing has been seen of
//! it and saying otherwise would be inventing.
//!
//! Blocks are kept in the notes file beside the labels and comments, so what a
//! long run of a recording worked out is still there next time, and can be
//! read and corrected by hand.

use std::collections::{BTreeMap, BTreeSet};

use crate::observe::Observer;

/// What a block holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Instructions that were executed.
    Code,
    /// Bytes that were read and never executed.
    Data,
}

impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Code => "CODE",
            Kind::Data => "DATA",
        }
    }

    fn from_label(text: &str) -> Option<Kind> {
        match text.to_ascii_uppercase().as_str() {
            "CODE" => Some(Kind::Code),
            "DATA" => Some(Kind::Data),
            _ => None,
        }
    }
}

/// One run of addresses that belongs together. `to` is the last byte in it,
/// not one past: a block of a single byte has `from == to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Block {
    pub from: u16,
    pub to: u16,
    pub kind: Kind,
}

impl Block {
    pub fn contains(&self, addr: u16) -> bool {
        self.from <= addr && addr <= self.to
    }

    pub fn length(&self) -> u32 {
        self.to as u32 - self.from as u32 + 1
    }
}

/// The shortest run of data worth calling a block. Below this it is a couple
/// of variables rather than a table, and the listing would be striped rather
/// than divided.
const SHORTEST_DATA: u16 = 8;

/// Work out the blocks from what has been watched.
///
/// Routines first: each one starts where it was entered and runs to the
/// furthest address reached inside it. Then the data — addresses read and
/// never executed — fills whatever is left between them.
pub fn work_out(observer: &Observer) -> Vec<Block> {
    // Everything executed is code, whether or not anything was seen to call
    // it. Working only from routines that were called leaves out the code the
    // machine was already running when watching started — which, playing back
    // a recording of a game, is the game.
    let mut boundaries: BTreeSet<u16> = observer.routines.keys().copied().collect();
    for seen in observer.routines.values() {
        // Where a routine ends, the next thing begins — after the whole
        // instruction, not one byte past its first, or the block is cut
        // through the middle of the JP that ends it.
        for after in &seen.after {
            boundaries.insert(*after);
        }
    }

    let code: Vec<(u16, u16)> = observer
        .code_runs()
        .into_iter()
        .flat_map(|(from, to)| split_at(from, to, &boundaries))
        .collect();
    let data: Vec<(u16, u16)> = observer
        .data_blocks(SHORTEST_DATA)
        .into_iter()
        .map(|(at, length)| (at, at.saturating_add(length.saturating_sub(1))))
        .collect();
    assemble(&code, &data)
}

/// Add what has just been worked out to what was known before.
///
/// A single run of a program sees a fraction of it — the title screen, or the
/// first level — so a second run should add to the picture rather than replace
/// it. Where code and data disagree about an address, code wins: something
/// that was executed is code whatever else was done to it.
pub fn merge(known: &[Block], found: &[Block]) -> Vec<Block> {
    let ranges = |kind: Kind| -> Vec<(u16, u16)> {
        known
            .iter()
            .chain(found)
            .filter(|block| block.kind == kind)
            .map(|block| (block.from, block.to))
            .collect()
    };
    assemble(&ranges(Kind::Code), &ranges(Kind::Data))
}

/// Cut one run of code where a routine starts or another ended.
fn split_at(from: u16, to: u16, boundaries: &BTreeSet<u16>) -> Vec<(u16, u16)> {
    let mut pieces = Vec::new();
    let mut start = from;
    for cut in boundaries.range(from.saturating_add(1)..=to) {
        pieces.push((start, cut - 1));
        start = *cut;
    }
    pieces.push((start, to));
    pieces
}

/// Turn overlapping ranges into blocks that divide the address space.
///
/// Code keeps its boundaries and data does not, which is the difference
/// between the two. Where one routine starts is a fact about the program, so
/// two routines side by side stay two blocks and are drawn in two colours;
/// two runs of data side by side are one table read twice, and joining them
/// says so. Code seen twice with different extents keeps the longer.
fn assemble(code: &[(u16, u16)], data: &[(u16, u16)]) -> Vec<Block> {
    // One entry per start: the same routine watched twice is one block, as
    // far as it was ever seen to reach.
    let mut starts: BTreeMap<u16, u16> = BTreeMap::new();
    for (from, to) in code {
        let end = starts.entry(*from).or_insert(*to);
        *end = (*end).max(*to);
    }

    // A routine gives way where the next one starts. Nothing here decides
    // which of two overlapping routines owns the middle: the later start keeps
    // its start, which is the boundary a reader is looking for.
    let mut blocks: Vec<Block> = Vec::new();
    let bounds: Vec<u16> = starts.keys().copied().collect();
    for (index, (from, to)) in starts.iter().enumerate() {
        let mut end = *to;
        if let Some(next) = bounds.get(index + 1) {
            end = end.min(next.saturating_sub(1));
        }
        if end >= *from {
            blocks.push(Block {
                from: *from,
                to: end,
                kind: Kind::Code,
            });
        }
    }

    // Data goes wherever code is not. A byte that was executed is code however
    // often it was also read: a routine copied into place is read by the
    // copier and run afterwards, and calling it data describes the copy rather
    // than the program.
    let mut claimed: Vec<bool> = vec![false; 1 << 16];
    for block in &blocks {
        for addr in block.from..=block.to {
            claimed[addr as usize] = true;
        }
    }
    let mut is_data: Vec<bool> = vec![false; 1 << 16];
    for (from, to) in data {
        for addr in *from..=*to {
            if !claimed[addr as usize] {
                is_data[addr as usize] = true;
            }
        }
    }
    let mut run: Option<(u16, u16)> = None;
    for addr in 0..=u16::MAX {
        match (is_data[addr as usize], run) {
            (true, Some((from, _))) => run = Some((from, addr)),
            (true, None) => run = Some((addr, addr)),
            (false, Some((from, to))) => {
                blocks.push(Block {
                    from,
                    to,
                    kind: Kind::Data,
                });
                run = None;
            }
            (false, None) => {}
        }
    }
    if let Some((from, to)) = run {
        blocks.push(Block {
            from,
            to,
            kind: Kind::Data,
        });
    }

    blocks.sort();
    blocks
}

/// Which block an address is in, if any.
pub fn at(blocks: &[Block], addr: u16) -> Option<usize> {
    blocks
        .binary_search_by(|block| {
            if block.to < addr {
                std::cmp::Ordering::Less
            } else if block.from > addr {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()
}

/// How a block is written in the notes file: `block CODE 8000-80FF`.
pub fn to_line(block: &Block) -> String {
    format!(
        "block {} {:04X}-{:04X}",
        block.kind.label(),
        block.from,
        block.to
    )
}

/// Read one back. Anything that cannot be understood is skipped rather than
/// throwing the file away: these files are meant to be edited by hand.
pub fn from_line(line: &str) -> Option<Block> {
    let rest = line.trim().strip_prefix("block ")?;
    let (kind, range) = rest.trim().split_once(char::is_whitespace)?;
    let kind = Kind::from_label(kind)?;
    let (from, to) = range.trim().split_once('-')?;
    let from = u16::from_str_radix(from.trim().trim_start_matches('$'), 16).ok()?;
    let to = u16::from_str_radix(to.trim().trim_start_matches('$'), 16).ok()?;
    if to < from {
        return None;
    }
    Some(Block { from, to, kind })
}
