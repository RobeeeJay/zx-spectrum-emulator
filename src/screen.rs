//! ULA display rendering: 256x192 pixels plus border, into an RGBA buffer.

use crate::machine::SpectrumBus;

pub const BORDER_X: usize = 32;
pub const BORDER_Y: usize = 24;
pub const SCREEN_W: usize = 256;
pub const SCREEN_H: usize = 192;
pub const WIDTH: usize = SCREEN_W + BORDER_X * 2;
pub const HEIGHT: usize = SCREEN_H + BORDER_Y * 2;

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
fn put(out: &mut [u8], x: usize, y: usize, c: [u8; 3]) {
    let i = (y * WIDTH + x) * 4;
    out[i] = c[0];
    out[i + 1] = c[1];
    out[i + 2] = c[2];
    out[i + 3] = 0xff;
}

/// Render the live display. On a 128K this follows the shadow-screen bit, so
/// it draws whichever bank the ULA is showing. `flash_on` alternates every 16
/// frames, as the ULA does.
pub fn render(bus: &SpectrumBus, out: &mut [u8], flash_on: bool) {
    draw(out, flash_on, true, bus, &|offset| bus.video(offset));
}

/// Render 6912 bytes starting at logical address `base` as if they were video
/// RAM. Used for previewing a detected back buffer.
pub fn render_from(bus: &SpectrumBus, base: u16, out: &mut [u8], flash_on: bool, borders: bool) {
    draw(out, flash_on, borders, bus, &|offset| {
        bus.peek_raw(base.wrapping_add(offset))
    });
}

/// Shared drawing routine: `byte` supplies the display file, offset 0..6911.
fn draw(
    out: &mut [u8],
    flash_on: bool,
    borders: bool,
    bus: &SpectrumBus,
    byte: &dyn Fn(u16) -> u8,
) {
    for y in 0..HEIGHT {
        let color = if borders {
            border_at_line(bus, y)
        } else {
            bus.border
        };
        let c = PALETTE[(color & 7) as usize];
        for x in 0..WIDTH {
            put(out, x, y, c);
        }
    }

    for y in 0..SCREEN_H {
        let yy = y as u16;
        let row_off = ((yy & 0xc0) << 5) | ((yy & 0x07) << 8) | ((yy & 0x38) << 2);
        let attr_row = 0x1800 + (yy / 8) * 32;
        for cell in 0..32u16 {
            let bits = byte(row_off | cell);
            let attr = byte(attr_row + cell);
            let bright = (attr & 0x40) >> 3;
            let mut ink = (attr & 0x07) | bright;
            let mut paper = ((attr >> 3) & 0x07) | bright;
            if attr & 0x80 != 0 && flash_on {
                std::mem::swap(&mut ink, &mut paper);
            }
            let ink_c = PALETTE[ink as usize];
            let paper_c = PALETTE[paper as usize];
            for bit in 0..8usize {
                let on = bits & (0x80 >> bit) != 0;
                put(
                    out,
                    BORDER_X + cell as usize * 8 + bit,
                    BORDER_Y + y,
                    if on { ink_c } else { paper_c },
                );
            }
        }
    }
}

/// Border colour in effect when the ULA drew screen row `y`, so that mid-frame
/// `OUT (254),A` writes show up as horizontal bands.
fn border_at_line(bus: &SpectrumBus, y: usize) -> u8 {
    let t = bus.model.first_pixel_t() as i64
        + (y as i64 - BORDER_Y as i64) * bus.model.t_per_line() as i64;
    bus.border_at(t.max(0) as u32)
}
