//! ULA display rendering: 256x192 pixels plus border, into an RGBA buffer.
//!
//! The border is rasterised by T-state, not by scanline. The ULA emits two
//! pixels per T-state, so a program that writes port $FE in a tight loop can
//! draw in the border at that resolution — which is exactly what border-art
//! demos do. Sampling one colour per line would reduce all of it to stripes.

use eframe::egui;

use crate::machine::{SpectrumBus, Tint, Tints};

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

/// Display scales offered in the UI, as multiples of the Spectrum's pixels.
pub const SCALES: [f32; 6] = [0.5, 1.0, 1.5, 2.0, 3.0, 3.5];

/// Where to draw a picture of `size` so it sits in the middle of `available`,
/// with the same amount of space on every side. When it is larger than the
/// space, it overflows equally in each direction rather than sticking to a
/// corner.
pub fn centred(available: egui::Rect, size: egui::Vec2) -> egui::Rect {
    egui::Rect::from_center_size(available.center(), size)
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

/// How much darker the previous frame is drawn when racing the beam: a third
/// off, so it is clearly behind without being hard to read.
pub const STALE_BRIGHTNESS: f32 = 2.0 / 3.0;

/// Render the live display. On a 128K this follows the shadow-screen bit, so
/// it draws whichever bank the ULA is showing. `flash_on` alternates every 16
/// frames, as the ULA does.
pub fn render(bus: &SpectrumBus, view: View, out: &mut [u8], flash_on: bool) {
    // What the ULA painted, line by line, rather than what the display file
    // holds at this instant. A game that races the beam draws a line ahead of
    // the beam and rubs it out behind, so the display file at any one moment
    // is missing whatever it has just rubbed out — and reading it then makes
    // that line blink on and off.
    draw(
        out,
        view,
        flash_on,
        true,
        bus,
        Source {
            byte: &|offset, _| bus.video_painted(offset),
            beam: None,
            fade: None,
            tint: None,
        },
    );
}

/// Draw the frame being painted, fading as a phosphor does behind the beam.
///
/// The same picture [`render_painting`] draws, with every pixel dimmed by how
/// long ago the beam drew it. At full speed this would be a flicker nobody can
/// see; at five seconds a frame it is the beam's whole trail down the screen.
pub fn render_fading(
    bus: &SpectrumBus,
    view: View,
    out: &mut [u8],
    flash_on: bool,
    fade: Fade,
    tint: Option<Tinting<'_>>,
) {
    draw(
        out,
        view,
        flash_on,
        true,
        bus,
        Source {
            // What the beam put out, except where a write has been marked:
            // there, what the display file holds, since the mark is on its way
            // to the colour that write is going to show.
            byte: &|offset, live| {
                if live {
                    bus.video(offset)
                } else {
                    bus.video_painting(offset)
                }
            },
            beam: None,
            fade: Some(fade),
            tint,
        },
    );
}

/// Draw the frame the ULA is painting now: this frame above the beam, the one
/// before below it, which is what a television shows.
///
/// [`render`] holds the last finished frame instead, so that a repaint landing
/// half way through one never catches a picture half drawn. That is right at
/// full speed and wrong while the machine is crawling: the whole point of slow
/// draw is to watch the picture being built, and holding the finished frame
/// freezes it for as long as the frame takes.
pub fn render_painting(bus: &SpectrumBus, view: View, out: &mut [u8], flash_on: bool) {
    draw(
        out,
        view,
        flash_on,
        true,
        bus,
        Source {
            byte: &|offset, _| bus.video_painting(offset),
            beam: None,
            fade: None,
            tint: None,
        },
    );
}

/// Render with the beam part-way through the frame: everything up to T-state
/// `beam` is this frame, the rest is what was on the screen before, dimmed.
pub fn render_racing(bus: &SpectrumBus, view: View, out: &mut [u8], flash_on: bool, beam: u32) {
    draw(
        out,
        view,
        flash_on,
        true,
        bus,
        // Behind the beam, what the ULA actually put out — the bytes as they
        // were when it fetched them, which is where a raster effect lives.
        // Ahead of it, what the display file holds now: the picture as the
        // program has built it, which it has not been asked to show yet.
        Source {
            byte: &|offset, ahead| {
                if ahead {
                    bus.video(offset)
                } else {
                    bus.video_painting(offset)
                }
            },
            beam: Some(beam),
            fade: None,
            tint: None,
        },
    );
}

/// T-state at which the ULA emits the pixel at (`px`, `py`) of a rendered
/// frame. The inverse of what [`draw`] does, so the cursor and the raster
/// agree on where the beam is.
pub fn t_at_pixel(view: View, first_pixel_t: u32, t_per_line: u32, px: usize, py: usize) -> i64 {
    let line = py as i64 - view.border_top as i64;
    let x = px as i64 - view.border_x as i64;
    first_pixel_t as i64 + line * t_per_line as i64 + x.div_euclid(2) + DISPLAY_LEAD_T
}

/// Where the beam is at a given T-state: the other way round from
/// [`t_at_pixel`].
///
/// In the picture's own coordinates, border included, so it can be drawn
/// straight onto what is on screen. Two pixels go out per T-state, so a
/// T-state is half a pixel of accuracy and the beam is a pair of pixels wide.
/// The answer can be off the picture — during the top border the line is
/// negative, and during the flyback it is past the bottom — which is a fact
/// about where the beam is rather than something to clamp away.
pub fn pixel_at_t(view: View, first_pixel_t: u32, t_per_line: u32, t: u32) -> (i64, i64) {
    let since = t as i64 - first_pixel_t as i64 - DISPLAY_LEAD_T;
    let line = since.div_euclid(t_per_line as i64);
    let along = since.rem_euclid(t_per_line as i64);
    (
        along * 2 + view.border_x as i64,
        line + view.border_top as i64,
    )
}

/// Render 6912 bytes starting at logical address `base` as if they were video
/// RAM. Used for previewing a detected back buffer.
/// The attribute a bitmap-only view is drawn with: bright white on black.
///
/// A back buffer is a bitmap. Most programs keep 6,144 bytes of one and build
/// the colours somewhere else — or not at all — so the 768 bytes after it are
/// whatever happens to be in memory there, and drawing them as attributes
/// paints the preview in the colours of somebody else's variables.
pub const MONO_ATTR: u8 = 0x47;

/// The same as [`render_from`], but as a bitmap: every cell bright white on
/// black, whatever the bytes behind the screen say.
pub fn render_bitmap_from(bus: &SpectrumBus, view: View, base: u16, out: &mut [u8], borders: bool) {
    draw(
        out,
        view,
        false,
        borders,
        bus,
        Source {
            byte: &|offset, _| {
                if offset >= 0x1800 {
                    MONO_ATTR
                } else {
                    bus.peek_raw(base.wrapping_add(offset))
                }
            },
            beam: None,
            fade: None,
            tint: None,
        },
    );
}

pub fn render_from(
    bus: &SpectrumBus,
    view: View,
    base: u16,
    out: &mut [u8],
    flash_on: bool,
    borders: bool,
) {
    draw(
        out,
        view,
        flash_on,
        borders,
        bus,
        Source {
            byte: &|offset, _| bus.peek_raw(base.wrapping_add(offset)),
            beam: None,
            fade: None,
            tint: None,
        },
    );
}

/// The picture fading the way a phosphor does.
///
/// A television's picture starts to go out the moment the beam has passed, and
/// is at its dimmest just before the beam comes round to light it again. That
/// is a fiftieth of a second on a real machine and invisible; run the machine
/// at five seconds a frame and it is the clearest possible answer to "where is
/// the beam, and how long ago was this part of the picture drawn?".
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    /// Where the beam is, in T-states into the frame.
    pub now: u32,
    /// How bright a pixel is by the time the beam comes back to it, as a
    /// fraction of how bright it was when it was drawn.
    pub floor: f32,
}

/// The marks on the picture saying where each write landed relative to the
/// beam, and how far they have blended away.
#[derive(Clone, Copy)]
pub struct Tinting<'a> {
    pub tints: &'a Tints,
    /// Now, in T-states since the machine started.
    pub now: u64,
    /// How long a mark takes to blend away into the colour it should be, in
    /// T-states. The user is given it in seconds of their own time; how many
    /// T-states that is depends on how slowly the machine is being run.
    pub over: u64,
}

/// Written too late for this frame: it will not be seen until the next one.
pub const LATE: [u8; 3] = PALETTE[10];
/// Written in time to be shown this frame.
pub const EARLY: [u8; 3] = PALETTE[12];

impl Tinting<'_> {
    /// The mark on a byte: its colour and how far it has blended away, or
    /// nothing if there is none or it has had its time.
    fn mark(&self, offset: u16) -> Option<([u8; 3], f32)> {
        let (kind, when) = self.tints.at(offset);
        let colour = match kind {
            Tint::None => return None,
            Tint::Late => LATE,
            Tint::Early => EARLY,
        };
        // A mark lasts until the beam goes over it, however long that is; it
        // is only its colour that is on a two-second timer. Ending the mark
        // when the colour has finished blending would take the cell back to
        // what the beam put out — which for a write the beam has passed is
        // what was there before it — so the picture would pop back to the old
        // content two seconds after every late write.
        let age = self.now.saturating_sub(when);
        let done = if self.over == 0 {
            1.0
        } else {
            (age as f32 / self.over as f32).min(1.0)
        };
        Some((colour, done))
    }

    /// The mark on a cell: the later of its bitmap byte and its attribute,
    /// since whichever was written last is the change being watched.
    fn cell_mark(&self, bitmap: u16, attribute: u16) -> Option<([u8; 3], f32)> {
        match (self.mark(bitmap), self.mark(attribute)) {
            (Some(a), Some(b)) => {
                if self.tints.at(bitmap).1 >= self.tints.at(attribute).1 {
                    Some(a)
                } else {
                    Some(b)
                }
            }
            (a, b) => a.or(b),
        }
    }
}

/// Blend `rgb` `done` of the way out of `mark`.
fn blend(mark: [u8; 3], rgb: [u8; 3], done: f32) -> [u8; 3] {
    [
        (mark[0] as f32 * (1.0 - done) + rgb[0] as f32 * done) as u8,
        (mark[1] as f32 * (1.0 - done) + rgb[1] as f32 * done) as u8,
        (mark[2] as f32 * (1.0 - done) + rgb[2] as f32 * done) as u8,
    ]
}

#[inline]
fn scaled(c: [u8; 3], by: f32) -> [u8; 3] {
    [
        (c[0] as f32 * by) as u8,
        (c[1] as f32 * by) as u8,
        (c[2] as f32 * by) as u8,
    ]
}

#[inline]
fn dim(c: [u8; 3]) -> [u8; 3] {
    [
        (c[0] as f32 * STALE_BRIGHTNESS) as u8,
        (c[1] as f32 * STALE_BRIGHTNESS) as u8,
        (c[2] as f32 * STALE_BRIGHTNESS) as u8,
    ]
}

/// The ULA fetches two T-states ahead of the pixels it is putting out, so a
/// border write lands on screen two T-states before the fetch clock that
/// `first_pixel_t` counts. Measured against a real machine with Border Break.
pub const DISPLAY_LEAD_T: i64 = 2;

/// Shared drawing routine: `byte` supplies the display file, offset 0..6911.
///
/// The raster is walked in the order the ULA emits it, so the border colour
/// can be tracked with a single cursor through the frame's list of writes.
/// Where the picture comes from and what is done to it on the way out.
struct Source<'a> {
    /// The display file, offset 0..6911. Told whether it is being asked for a
    /// point ahead of the beam or behind it, which are two different pictures.
    byte: &'a dyn Fn(u16, bool) -> u8,
    /// Where to split this frame from the last, when racing.
    beam: Option<u32>,
    /// How the picture fades behind the beam, if it does.
    fade: Option<Fade>,
    /// The marks on the writes, if they are being shown.
    tint: Option<Tinting<'a>>,
}

fn draw(
    out: &mut [u8],
    view: View,
    flash_on: bool,
    borders: bool,
    bus: &SpectrumBus,
    source: Source<'_>,
) {
    let Source {
        byte,
        beam,
        fade,
        tint,
    } = source;
    let (width, height) = (view.width(), view.height());
    let first = bus.first_pixel_t() as i64;
    let per_line = bus.model.t_per_line() as i64;
    let frame_t = bus.frame_t() as i64;
    // Colour per T-state, mixing this frame with the last one at the point the
    // ULA has reached — which is what a screen actually shows.
    let raster = if borders {
        // When racing the beam the split between this frame and the last is
        // wherever the cursor is, not wherever the emulator has got to.
        match beam {
            Some(t) => bus.border_raster_at(t),
            None => bus.border_raster(),
        }
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
        let mut cell_stale = false;

        for px in 0..width {
            let x = px as i64 - view.border_x as i64;
            let t = line_start + x.div_euclid(2) + DISPLAY_LEAD_T;
            // How far this pixel has faded since the beam drew it: none at
            // all where the beam has just been, all the way to the floor a
            // frame later, just before it is drawn again.
            let lit = |rgb: [u8; 3]| match fade {
                Some(f) => {
                    let age = (f.now as i64 - t).rem_euclid(frame_t) as f32;
                    scaled(rgb, 1.0 - (1.0 - f.floor) * (age / frame_t as f32))
                }
                None => rgb,
            };
            // Past the beam, the screen still shows the frame before this one.
            let stale = beam.is_some_and(|b| t > b as i64);

            if on_display_line && (0..SCREEN_W as i64).contains(&x) {
                let cell = (x / 8) as u16;
                // A marked cell is drawn from the display file rather than
                // from what the beam put out: the mark is there to say what
                // the write did, and it blends into the colour the write is
                // going to show — which for a write the beam has passed is
                // nothing that is on the screen yet.
                let mark = tint.and_then(|t| t.cell_mark(row_off | cell, attr_row + cell));
                if x % 8 == 0 || stale != cell_stale {
                    cell_stale = stale;
                    // Ahead of the beam, what is shown is what the display
                    // file holds now — the picture as the program has built
                    // it, not the one the ULA painted a frame ago. Behind the
                    // beam it is what the ULA actually put there, attribute
                    // changes and all, which is what makes a raster effect
                    // visible: the two halves of the screen are the program's
                    // intention and the machine's execution of it.
                    let read = |o: u16| byte(o, stale || mark.is_some());
                    cell_bits = read(row_off | cell);
                    let attr = read(attr_row + cell);
                    let bright = (attr & 0x40) >> 3;
                    let mut ink = (attr & 0x07) | bright;
                    let mut paper = ((attr >> 3) & 0x07) | bright;
                    if attr & 0x80 != 0 && flash_on {
                        std::mem::swap(&mut ink, &mut paper);
                    }
                    ink_c = PALETTE[ink as usize];
                    paper_c = PALETTE[paper as usize];
                    // A byte just written shows as red or green and blends
                    // into the colour it is going to be.
                    if let Some((colour, done)) = mark {
                        ink_c = blend(colour, ink_c, done);
                        paper_c = blend(colour, paper_c, done);
                    }
                    if stale {
                        ink_c = dim(ink_c);
                        paper_c = dim(paper_c);
                    }
                }
                let on = cell_bits & (0x80 >> (x % 8)) != 0;
                put(out, width, px, py, lit(if on { ink_c } else { paper_c }));
                continue;
            }

            let colour = if !borders {
                bus.border
            } else if stale {
                // Ahead of the beam the border is simply the colour the
                // program has set: nothing has been painted with it yet, so
                // there is no history to show.
                bus.border
            } else {
                // Behind it, the colour at each T-state, which is where a
                // border effect lives.
                raster[t.rem_euclid(frame_t) as usize]
            };
            let rgb = PALETTE[(colour & 7) as usize];
            put(out, width, px, py, lit(if stale { dim(rgb) } else { rgb }));
        }
    }
}
