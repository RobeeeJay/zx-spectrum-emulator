//! The screen, as something a model can look at.
//!
//! Two ways: a PNG, which MCP carries as an image block and a model with eyes
//! can actually see, and a character-cell sketch for when it cannot. The
//! sketch is 32x24 — one character per attribute cell — because 256x192 of
//! anything is unreadable and the cells are what the machine thinks in.

use crate::machine::SpectrumBus;
use crate::screen::{self, View};

/// Base64, since there is no crate in the lock file for that either and MCP
/// carries image data as base64 in JSON.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The picture as a PNG, at one byte per Spectrum pixel.
pub fn png(bus: &SpectrumBus, view: View, flash_on: bool) -> Result<Vec<u8>, String> {
    let mut pixels = vec![0u8; view.buffer_len()];
    screen::render(bus, view, &mut pixels, flash_on);
    encode(&pixels, view.width(), view.height())
}

pub fn encode(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("writing the PNG header: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("writing the PNG: {e}"))?;
    }
    Ok(out)
}

/// The display as characters, one per attribute cell: how much ink is in the
/// cell decides which character, and the colours are named beside it.
///
/// It is a sketch rather than a picture — eight by eight pixels cannot be one
/// character honestly — but it says where things are, which is what a question
/// like "did that blank the sprite" is asking.
pub fn sketch(bus: &SpectrumBus) -> String {
    const SHADES: [char; 5] = [' ', '.', '+', '#', '@'];
    let mut out = String::from("   0123456789012345678901234567890\n");
    for cell_y in 0..24usize {
        out.push_str(&format!("{cell_y:2} "));
        for cell_x in 0..32usize {
            let mut lit = 0u32;
            for row in 0..8usize {
                let y = cell_y * 8 + row;
                // The display file's thirds and rows, which are not in the
                // order anybody would have chosen.
                let addr =
                    0x4000 + ((y & 0xC0) << 5) + ((y & 0x07) << 8) + ((y & 0x38) << 2) + cell_x;
                lit += bus.video(addr as u16 - 0x4000).count_ones();
            }
            // Any ink at all is worth a mark: a single lit row is eight bits
            // of sixty-four, and scaling that linearly rounds a sprite's edge
            // down to blank.
            out.push(if lit == 0 {
                SHADES[0]
            } else {
                SHADES[(1 + lit as usize * 3 / 64).min(SHADES.len() - 1)]
            });
        }
        out.push('\n');
    }
    out
}

/// What colours are on the screen, and how much of it each covers: the quick
/// way to tell a title screen from a playing field.
pub fn colours(bus: &SpectrumBus) -> String {
    const NAMES: [&str; 8] = [
        "black", "blue", "red", "magenta", "green", "cyan", "yellow", "white",
    ];
    let mut ink = [0u32; 8];
    let mut paper = [0u32; 8];
    let mut flashing = 0u32;
    let mut bright = 0u32;
    for cell in 0..768u16 {
        let attr = bus.video(0x1800 + cell);
        ink[(attr & 7) as usize] += 1;
        paper[((attr >> 3) & 7) as usize] += 1;
        if attr & 0x80 != 0 {
            flashing += 1;
        }
        if attr & 0x40 != 0 {
            bright += 1;
        }
    }
    let listed = |counts: &[u32; 8]| {
        let mut pairs: Vec<_> = counts
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(i, n)| format!("{} {n}", NAMES[i]))
            .collect();
        pairs.truncate(4);
        pairs.join(", ")
    };
    format!(
        "attributes: ink {} | paper {} | {flashing} cells flashing, {bright} bright",
        listed(&ink),
        listed(&paper)
    )
}

/// Memory read as graphics: eight bytes to a row, as the Spectrum's own
/// characters and sprites are stored.
///
/// This is how "where are the graphics" is answered — point it at a candidate
/// address and see whether letters, sprites or rubbish come out.
pub fn tiles(peek: impl Fn(u16) -> u8, start: u16, count: u16, across: u16) -> String {
    let mut out = String::new();
    let rows = count.div_ceil(across.max(1));
    for row in 0..rows {
        let first = start.wrapping_add(row * across * 8);
        out.push_str(&format!("${first:04X}\n"));
        for line in 0..8u16 {
            for tile in 0..across {
                if row * across + tile >= count {
                    break;
                }
                let byte = peek(first.wrapping_add(tile * 8 + line));
                for bit in (0..8).rev() {
                    out.push(if byte & (1 << bit) != 0 { '#' } else { '.' });
                }
                out.push(' ');
            }
            out.push('\n');
        }
    }
    out
}
