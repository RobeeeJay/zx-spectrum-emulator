//! ULA display rendering: 256x192 pixels plus border, into an RGBA buffer.
//!
//! The border is rasterised by T-state, not by scanline. The ULA emits two
//! pixels per T-state, so a program that writes port $FE in a tight loop can
//! draw in the border at that resolution — which is exactly what border-art
//! demos do. Sampling one colour per line would reduce all of it to stripes.

use crate::machine::SpectrumBus;

pub const SCREEN_W: usize = 256;
pub const SCREEN_H: usize = 192;

/// How much of the border to show.
///
/// [`View::OVERSCAN`] is everything the ULA draws that a generous monitor would
/// show — the area border-art demos use. [`View::CROPPED`] trims it to roughly
/// what a television actually displayed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct View {
    pub border_x: usize,
    pub border_top: usize,
    pub border_bottom: usize,
}

impl View {
    /// The full overscan area: 384x304. The top border really is longer than
    /// the bottom one — the ULA emits 64 lines before the display and 56
    /// after, of which the last 8 are off the edge of most screens.
    pub const OVERSCAN: View = View {
        border_x: 64,
        border_top: 64,
        border_bottom: 48,
    };
    /// A television-sized border: 304x240.
    pub const CROPPED: View = View {
        border_x: 24,
        border_top: 24,
        border_bottom: 24,
    };

    pub fn width(&self) -> usize {
        SCREEN_W + self.border_x * 2
    }
    pub fn height(&self) -> usize {
        SCREEN_H + self.border_top + self.border_bottom
    }
    /// Size of the RGBA buffer [`render`] needs.
    pub fn buffer_len(&self) -> usize {
        self.width() * self.height() * 4
    }
}

impl Default for View {
    fn default() -> Self {
        View::OVERSCAN
    }
}

/// Dimensions of the full overscan view, which is what the tests measure
/// against.
pub const BORDER_X: usize = View::OVERSCAN.border_x;
pub const BORDER_TOP: usize = View::OVERSCAN.border_top;
pub const BORDER_BOTTOM: usize = View::OVERSCAN.border_bottom;
pub const WIDTH: usize = SCREEN_W + BORDER_X * 2;
pub const HEIGHT: usize = SCREEN_H + BORDER_TOP + BORDER_BOTTOM;

/// Normal then bright, in ULA order: black, blue, red, magenta, green, cyan,
/// yellow, white.
pub const PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xd7],
    [0xd7, 0x00, 0x00],
    [0xd7, 0x00, 0xd7],
    [0x00, 0xd7, 0x00],
    [0x00, 0xd7, 0xd7],
    [0xd7, 0xd7, 0x00],
    [0xd7, 0xd7, 0xd7],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xff],
    [0xff, 0x00, 0x00],
    [0xff, 0x00, 0xff],
    [0x00, 0xff, 0x00],
    [0x00, 0xff, 0xff],
    [0xff, 0xff, 0x00],
    [0xff, 0xff, 0xff],
];

#[inline]
fn put(out: &mut [u8], width: usize, x: usize, y: usize, c: [u8; 3]) {
    let i = (y * width + x) * 4;
    out[i] = c[0];
    out[i + 1] = c[1];
    out[i + 2] = c[2];
    out[i + 3] = 0xff;
}

/// Render the live display. On a 128K this follows the shadow-screen bit, so
/// it draws whichever bank the ULA is showing. `flash_on` alternates every 16
/// frames, as the ULA does.
pub fn render(bus: &SpectrumBus, view: View, out: &mut [u8], flash_on: bool) {
    draw(out, view, flash_on, true, bus, &|offset| bus.video(offset));
}

/// Render 6912 bytes starting at logical address `base` as if they were video
/// RAM. Used for previewing a detected back buffer.
pub fn render_from(
    bus: &SpectrumBus,
    view: View,
    base: u16,
    out: &mut [u8],
    flash_on: bool,
    borders: bool,
) {
    draw(out, view, flash_on, borders, bus, &|offset| {
        bus.peek_raw(base.wrapping_add(offset))
    });
}

/// The ULA fetches two T-states ahead of the pixels it is putting out, so a
/// border write lands on screen two T-states before the fetch clock that
/// `first_pixel_t` counts. Measured against a real machine with Border Break.
pub const DISPLAY_LEAD_T: i64 = 2;

/// Shared drawing routine: `byte` supplies the display file, offset 0..6911.
///
/// The raster is walked in the order the ULA emits it, so the border colour
/// can be tracked with a single cursor through the frame's list of writes.
fn draw(
    out: &mut [u8],
    view: View,
    flash_on: bool,
    borders: bool,
    bus: &SpectrumBus,
    byte: &dyn Fn(u16) -> u8,
) {
    let (width, height) = (view.width(), view.height());
    let first = bus.first_pixel_t() as i64;
    let per_line = bus.model.t_per_line() as i64;
    let frame_t = bus.frame_t() as i64;
    // Colour per T-state, mixing this frame with the last one at the point the
    // ULA has reached — which is what a screen actually shows.
    let raster = if borders {
        bus.border_raster()
    } else {
        Vec::new()
    };

    for py in 0..height {
        let line = py as i64 - view.border_top as i64;
        let line_start = first + line * per_line;
        let on_display_line = (0..SCREEN_H as i64).contains(&line);

        // Cached attribute lookups for the line, so each cell is read once.
        let (row_off, attr_row) = if on_display_line {
            let yy = line as u16;
            (
                ((yy & 0xc0) << 5) | ((yy & 0x07) << 8) | ((yy & 0x38) << 2),
                0x1800 + (yy / 8) * 32,
            )
        } else {
            (0, 0)
        };
        let mut cell_bits = 0u8;
        let mut ink_c = PALETTE[0];
        let mut paper_c = PALETTE[0];

        for px in 0..width {
            let x = px as i64 - view.border_x as i64;
            if on_display_line && (0..SCREEN_W as i64).contains(&x) {
                let cell = (x / 8) as u16;
                if x % 8 == 0 {
                    cell_bits = byte(row_off | cell);
                    let attr = byte(attr_row + cell);
                    let bright = (attr & 0x40) >> 3;
                    let mut ink = (attr & 0x07) | bright;
                    let mut paper = ((attr >> 3) & 0x07) | bright;
                    if attr & 0x80 != 0 && flash_on {
                        std::mem::swap(&mut ink, &mut paper);
                    }
                    ink_c = PALETTE[ink as usize];
                    paper_c = PALETTE[paper as usize];
                }
                let on = cell_bits & (0x80 >> (x % 8)) != 0;
                put(out, width, px, py, if on { ink_c } else { paper_c });
                continue;
            }

            let colour = if borders {
                // Two pixels per T-state.
                let t = line_start + x.div_euclid(2) + DISPLAY_LEAD_T;
                raster[t.rem_euclid(frame_t) as usize]
            } else {
                bus.border
            };
            put(out, width, px, py, PALETTE[(colour & 7) as usize]);
        }
    }
}
