//! What a television made of the picture.
//!
//! The emulator's screen buffer is what the ULA put out: exact pixels, exact
//! colours, sharp to the sample. A set on the other end of an aerial lead did
//! not show that. It carried the picture as composite video, where the colour
//! rides on a subcarrier the set has to separate out again and never quite
//! does; it drew it in lines with gaps between them; and it showed the whole
//! thing on a tube with a curve in it.
//!
//! This is the first two of those, done to the pixels. The curve is geometry
//! and belongs to whatever draws the picture — see `ui::crt`.
//!
//! Nothing here is guessed at. The interference is the beat between the
//! machine's dot clock and PAL's colour subcarrier, worked out from both
//! numbers, and it rolls because the two do not divide.

/// The Spectrum's dot clock: two pixels a T-state, at 3.5MHz.
pub const DOT_HZ: f64 = 7_000_000.0;

/// PAL's colour subcarrier.
pub const SUBCARRIER_HZ: f64 = 4_433_618.75;

/// Dots in a 48K line: 224 T-states, two pixels each.
pub const DOTS_PER_LINE: f64 = 448.0;

/// Dots in a 48K frame: 312 lines of them.
pub const DOTS_PER_FRAME: f64 = DOTS_PER_LINE * 312.0;

/// Cycles of subcarrier in one dot, which is what makes the pattern.
///
/// 0.6334 of a cycle a dot, sampled once a dot, aliases to a ripple with a
/// period of about 2.7 pixels — the herringbone anybody who used a Spectrum
/// on a television will remember. It is not a texture laid over the picture:
/// it is the same arithmetic the set was doing.
pub fn subcarrier_per_dot() -> f64 {
    SUBCARRIER_HZ / DOT_HZ
}

/// How far the pattern moves from one frame to the next, in cycles.
///
/// A frame is 139,776 dots and the subcarrier does not fit a whole number of
/// cycles into that, so the pattern arrives somewhere else each time and the
/// whole thing crawls up the screen. On a real machine this is what makes the
/// interference roll rather than stand still.
pub fn roll_per_frame() -> f64 {
    (subcarrier_per_dot() * DOTS_PER_FRAME).fract()
}

/// How the picture is spoiled, and by how much.
///
/// Two separate things, and they were two separate things on the day as well:
/// the signal — colour on a subcarrier, and what that beats against — and the
/// tube it was shown on. A monitor fed RGB had the tube's line structure and
/// none of the composite artefacts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crt {
    /// How much of the picture's brightness the interference swings, 0 to 1.
    ///
    /// A few per cent. It is a pattern about two and a half pixels across, so
    /// any more of it and what shows is not the herringbone a set had but the
    /// beat between that and whatever the picture is being scaled by.
    pub interference: f32,
    /// How far the colour is smeared sideways, in pixels. Composite video
    /// carries colour on a subcarrier with a fraction of the luminance's
    /// bandwidth, so an edge between two colours arrives late and soft while
    /// the brightness edge stays where it is.
    pub bleed: f32,
    /// How dark the gap between two lines is, 0 for none and 1 for black.
    pub scanlines: f32,
    /// Whether the picture is drawn with a gap under every line, which is what
    /// doubles its height.
    pub line_gaps: bool,
}

impl Default for Crt {
    fn default() -> Self {
        Self {
            interference: 0.035,
            bleed: 2.5,
            scanlines: 0.35,
            line_gaps: true,
        }
    }
}

/// Turn the ULA's pixels into what a set showed.
///
/// With the line structure on that is twice the height — a line and the gap
/// under it — and without it, the same size as it came in.
///
/// `frame` is the machine's frame counter, which is what rolls the
/// interference; `x0` is the dot the left edge of the view starts at, so the
/// pattern sits still against the picture when the border is shown or hidden
/// rather than jumping sideways.
pub fn televise(src: &[u8], w: usize, h: usize, out: &mut Vec<u8>, crt: Crt, frame: u64, x0: f64) {
    let rows = if crt.line_gaps { 2 } else { 1 };
    out.resize(w * h * rows * 4, 0);
    let per_dot = subcarrier_per_dot();
    let bleed = crt.bleed.max(0.0);
    let taps = bleed.ceil() as isize;
    for y in 0..h {
        // The subcarrier's phase at the start of this line. A line is 448
        // dots, which is 283.75 cycles: the quarter left over is why the
        // pattern leans over rather than standing in columns.
        let line_phase = (frame as f64 * DOTS_PER_FRAME + y as f64 * DOTS_PER_LINE) * per_dot;
        for x in 0..w {
            // Colour, smeared. The luminance is left where it is: it is the
            // chrominance that is band-limited, which is why a red caption on
            // black bleeds and a white one does not.
            let (mut r, mut g, mut b) = (0.0f32, 0.0, 0.0);
            let (mut yy, mut weight) = (0.0f32, 0.0f32);
            for tap in -taps..=taps {
                let at = (x as isize + tap).clamp(0, w as isize - 1) as usize;
                let i = (y * w + at) * 4;
                let (sr, sg, sb) = (
                    src[i] as f32 / 255.0,
                    src[i + 1] as f32 / 255.0,
                    src[i + 2] as f32 / 255.0,
                );
                // A triangular window, which is what a one-pole filter run
                // both ways comes to and is enough for a smear.
                let k = if taps == 0 {
                    1.0
                } else {
                    1.0 - (tap.abs() as f32 / (taps as f32 + 1.0))
                };
                r += sr * k;
                g += sg * k;
                b += sb * k;
                weight += k;
                if tap == 0 {
                    yy = luma(sr, sg, sb);
                }
            }
            r /= weight;
            g /= weight;
            b /= weight;
            // Put the sharp luminance back over the soft colour.
            let smeared = luma(r, g, b);
            let (mut r, mut g, mut b) =
                (r + (yy - smeared), g + (yy - smeared), b + (yy - smeared));

            // The beat between the dot clock and the subcarrier.
            let phase = line_phase + (x0 + x as f64) * per_dot;
            let ripple = 1.0 + crt.interference * (phase * std::f64::consts::TAU).sin() as f32;
            r *= ripple;
            g *= ripple;
            b *= ripple;

            let a = src[(y * w + x) * 4 + 3];
            let line = ((y * rows) * w + x) * 4;
            put(out, line, r, g, b, a);
            if crt.line_gaps {
                // The gap under it, which is what a set's line structure looks
                // like once there is room to see it.
                let dim = 1.0 - crt.scanlines;
                put(out, line + w * 4, r * dim, g * dim, b * dim, a);
            }
        }
    }
}

fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

fn put(out: &mut [u8], at: usize, r: f32, g: f32, b: f32, a: u8) {
    out[at] = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
    out[at + 1] = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
    out[at + 2] = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
    out[at + 3] = a;
}
