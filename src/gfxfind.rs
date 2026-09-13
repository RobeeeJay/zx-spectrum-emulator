//! Looking for a graphic in memory, from an 8x8 block of the screen.
//!
//! What is on the screen was copied there from somewhere, and the bytes of one
//! character cell are the obvious thing to look for. How they were stored is
//! the question, so each way is tried: eight bytes in a row, which is a
//! character or a column of a sprite stored top to bottom; one byte in every
//! `pitch`, which is a row of a sprite that many bytes wide — or half as wide,
//! with a mask byte beside each; and each of those mirrored, for a sprite kept
//! facing the other way, and inverted, for a mask. A sprite drawn at a pixel
//! position that is not a multiple of eight straddles two cells and is not
//! found this way, and nor is one stored upside down.

/// The widest row looked for: a sprite of thirty-two bytes, with a mask byte
/// beside each.
pub const MAX_PITCH: u16 = 64;
/// Beyond this many the answer is not "here", it is "everywhere".
pub const MAX_FOUND: usize = 200;

/// A place the block was found, and how it was stored there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub addr: u16,
    /// How far apart its eight rows are: 1 for a character.
    pub pitch: u16,
    pub mirrored: bool,
    pub inverted: bool,
}

impl Match {
    /// What it probably is, in a line.
    pub fn describe(&self) -> String {
        let shape = match self.pitch {
            1 => "8 bytes in a row: a character, or a column of a sprite".to_string(),
            p if p % 2 == 0 => format!(
                "rows {p} bytes apart: a sprite {p} bytes wide, or {} with masks",
                p / 2
            ),
            p => format!("rows {p} bytes apart: a sprite {p} bytes wide"),
        };
        let mut out = format!("${:04X}  {shape}", self.addr);
        if self.mirrored {
            out.push_str(", mirrored");
        }
        if self.inverted {
            out.push_str(", inverted");
        }
        match self.addr {
            0x0000..=0x3FFF => out.push_str(" — in the ROM"),
            0x4000..=0x5AFF => out.push_str(" — on the screen"),
            _ => {}
        }
        out
    }
}

/// What a search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub matches: Vec<Match>,
    /// There were more than `MAX_FOUND`.
    pub more: bool,
}

/// A block picked off the screen, and what looking for it found.
#[derive(Clone, Debug)]
pub struct Picked {
    /// The character cell, across and down.
    pub cell: (u16, u16),
    pub pattern: [u8; 8],
    pub result: Result<Search, String>,
}

/// Look for eight rows of a block in the 64K the machine can see.
pub fn search(peek: &dyn Fn(u16) -> u8, pattern: [u8; 8]) -> Result<Search, String> {
    if pattern.iter().all(|b| *b == pattern[0]) {
        return Err(if pattern[0] == 0 {
            "that block is empty: pick one with something drawn in it".to_string()
        } else {
            "every row of that block is the same, so it would be found wherever the byte \
             repeats: pick one with a shape"
                .to_string()
        });
    }
    let memory: Vec<u8> = (0..=0xFFFFu16).map(peek).collect();
    let mut variants: Vec<(bool, bool, [u8; 8])> = Vec::new();
    for (mirrored, inverted) in [(false, false), (true, false), (false, true), (true, true)] {
        let form = pattern.map(|b| {
            let b = if mirrored { b.reverse_bits() } else { b };
            if inverted {
                !b
            } else {
                b
            }
        });
        // A symmetrical block reads the same mirrored: once is enough.
        if !variants.iter().any(|(_, _, f)| *f == form) {
            variants.push((mirrored, inverted, form));
        }
    }
    let mut matches = Vec::new();
    for (mirrored, inverted, form) in variants {
        for pitch in 1..=MAX_PITCH {
            let last = 0xFFFF - 7 * pitch as usize;
            for addr in 0..=last {
                if (0..8).all(|i| memory[addr + i * pitch as usize] == form[i]) {
                    if matches.len() == MAX_FOUND {
                        return Ok(Search {
                            matches,
                            more: true,
                        });
                    }
                    matches.push(Match {
                        addr: addr as u16,
                        pitch,
                        mirrored,
                        inverted,
                    });
                }
            }
        }
    }
    Ok(Search {
        matches,
        more: false,
    })
}
