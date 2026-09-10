//! Recording what the machine sends out through MIC, and turning it back into
//! blocks.
//!
//! A Spectrum saves by flipping bit 3 of port $FE, and what comes out of the
//! MIC socket is a train of pulses: a long run of pilot, two short sync
//! pulses, and then every bit as a pair of pulses, short for 0 and long for 1.
//! What is kept here is when the line changed, in the machine's own T-states,
//! and when it has been quiet long enough for a block to have ended, the
//! pulses are read back into the bytes the machine sent.
//!
//! The reading is of the ROM's own timings — a pilot of 2,168 T, sync of 667
//! and 735, bits of 855 and 1,710 — with room either side, and what comes out
//! is a standard block: the flag, the data and the checksum exactly as the
//! machine sent them, which is what a `.tap` holds. A saver with timings of
//! its own is not read, and says so rather than producing a wrong block.

use crate::tape::Block;

/// How long MIC has to stay still for a block to be over: half a second of the
/// machine's time, which is longer than any gap inside a block and shorter
/// than the second the ROM waits between a header and its data.
pub const QUIET_T: u64 = 1_750_000;

/// The ROM's pulse lengths, and how far a pulse may stray and still be one:
/// the pilot is 2,168 T.
const SYNC_MAX: u64 = 1000;
const PILOT_MIN: u64 = 1800;
const PILOT_MAX: u64 = 2600;
/// A 0 is two pulses of 855 and a 1 is two of 1,710: a pair is one or the
/// other by which side of halfway between them its total falls.
const PAIR_SPLIT: u64 = 2565;
const BIT_MAX: u64 = 2200;
/// How much pilot there has to be before a run of pulses is a block at all.
const PILOT_PULSES: usize = 256;

/// What was heard, and what came of it.
#[derive(Clone, Default, Debug)]
pub struct Recorder {
    /// When MIC changed, since the last block was finished.
    edges: Vec<u64>,
    /// When the last block finished, for the pause the next one starts with.
    last_block_end: Option<u64>,
    /// How many runs of pulses could not be read as a standard block.
    pub unread: usize,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// The line changed.
    ///
    /// A clock that has gone backwards — a reset, a snapshot — is not the
    /// same save going on: what was half heard is dropped, or the length of
    /// the next pulse would be worked out across the jump.
    pub fn edge(&mut self, at: u64) {
        if self.edges.last().is_some_and(|last| at < *last) {
            self.edges.clear();
            self.last_block_end = None;
        }
        self.edges.push(at);
    }

    /// Whether anything is waiting to be read.
    pub fn pending(&self) -> bool {
        !self.edges.is_empty()
    }

    /// If MIC has been quiet long enough, read what was heard into a block.
    ///
    /// `None` while a block may still be arriving, and while there is nothing
    /// to read. A run of pulses that is not a standard block is counted in
    /// `unread` and dropped.
    pub fn finish_if_quiet(&mut self, now: u64) -> Option<Block> {
        let last = *self.edges.last()?;
        if now < last {
            // The clock went backwards under a half-heard block.
            self.edges.clear();
            self.last_block_end = None;
            return None;
        }
        if now - last < QUIET_T {
            return None;
        }
        let edges = std::mem::take(&mut self.edges);
        let block = read_block(&edges);
        if block.is_none() {
            self.unread += 1;
        }
        // The pause is the silence before this block started, as a tape keeps
        // it: the one after the previous block.
        let pause_ms = match (self.last_block_end, edges.first()) {
            (Some(end), Some(start)) => {
                ((start.saturating_sub(end)) / 3_500).clamp(100, 5_000) as u16
            }
            _ => 1000,
        };
        self.last_block_end = Some(last);
        block.map(|data| Block::Standard { pause_ms, data })
    }
}

/// Read a train of edges as a standard block: pilot, sync, then bytes.
fn read_block(edges: &[u64]) -> Option<Vec<u8>> {
    let pulses: Vec<u64> = edges.windows(2).map(|w| w[1] - w[0]).collect();

    // The pilot: a long run of pulses about 2,168 T long.
    let mut at = 0;
    let mut run = 0;
    while at < pulses.len() {
        if (PILOT_MIN..=PILOT_MAX).contains(&pulses[at]) {
            run += 1;
        } else if run >= PILOT_PULSES {
            break;
        } else {
            run = 0;
        }
        at += 1;
    }
    if run < PILOT_PULSES {
        return None;
    }

    // Two short sync pulses end it.
    if at + 1 >= pulses.len() || pulses[at] > SYNC_MAX || pulses[at + 1] > SYNC_MAX {
        return None;
    }
    at += 2;

    // Then the bits, a pair of pulses each, most significant first.
    let mut bytes = Vec::new();
    let mut byte = 0u8;
    let mut bits = 0;
    while at + 1 < pulses.len() {
        let (a, b) = (pulses[at], pulses[at + 1]);
        if a > BIT_MAX || b > BIT_MAX {
            break;
        }
        byte = (byte << 1) | u8::from(a + b > PAIR_SPLIT);
        bits += 1;
        if bits == 8 {
            bytes.push(byte);
            byte = 0;
            bits = 0;
        }
        at += 2;
    }
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Write a tape of standard blocks as a `.tap`: each block's length, then the
/// block. Anything that is not a standard block cannot go in one.
pub fn to_tap(blocks: &[Block]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Standard { data, .. } => {
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.extend_from_slice(data);
            }
            _ => {
                return Err(
                    "a .tap holds standard blocks only, and this tape has other kinds: \
                     save it as .tzx"
                        .into(),
                )
            }
        }
    }
    Ok(out)
}

/// Write a tape of standard blocks as a `.tzx`, which keeps the pauses as
/// well. Version 1.20, block $10 for each.
pub fn to_tzx(blocks: &[Block]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    out.extend_from_slice(b"ZXTape!\x1A");
    out.push(1);
    out.push(20);
    for block in blocks {
        match block {
            Block::Standard { pause_ms, data } => {
                out.push(0x10);
                out.extend_from_slice(&pause_ms.to_le_bytes());
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.extend_from_slice(data);
            }
            _ => {
                return Err(
                    "only standard blocks are written here, and this tape has other kinds".into(),
                )
            }
        }
    }
    Ok(out)
}
