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

/// How much brighter a lit line is drawn than a picture with no line
/// structure at all, at most.
///
/// A phosphor is only lit where the beam went, and it is lit *harder* there
/// than a flat panel showing the same picture: the light a television gives
/// out comes from the lines, not from the gaps between them. Drawing the line
/// at the picture's own brightness and the gap darker gives away that
/// difference and the picture comes out dim — which is what "the CRT lines
/// need to be brighter" was.
///
/// The gain is worked out rather than dialled: enough to put back what the
/// gaps take away, capped so a picture cannot be made brighter than the tube
/// can go.
const MOST_GAIN: f32 = 1.6;

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
    /// How hard the dot crawl bites where the colour is, 0 to 1.
    ///
    /// It is not spread evenly over the picture: crawl is the colour and the
    /// brightness getting into each other's way, so it is strongest where the
    /// colour changes and absent on a grey field. That is why this can be a
    /// fifth of the picture's brightness where it lands without the whole
    /// screen turning into a beat pattern.
    pub interference: f32,
    /// How far the colour is smeared sideways, in pixels. Composite video
    /// carries colour on a subcarrier with a fraction of the luminance's
    /// bandwidth, so an edge between two colours arrives late and soft while
    /// the brightness edge stays where it is.
    pub bleed: f32,
    /// How dark the gap between two lines is, 0 for none and 1 for black.
    ///
    /// The lit line is brightened to make up for it — see [`Crt::line_gain`] —
    /// so this is how much *contrast* there is between a line and the gap
    /// under it rather than how much of the picture is thrown away.
    pub scanlines: f32,
    /// Whether the picture is drawn with a gap under every line, which is what
    /// doubles its height.
    pub line_gaps: bool,
}

impl Crt {
    /// How much the lit line is brightened by.
    ///
    /// A line and its gap between them average `(1 + (1 - scanlines)) / 2` of
    /// the picture's brightness, so the line is multiplied by the reciprocal
    /// of that and the two together come out where the picture started. At the
    /// default darkness that is a fifth brighter; a gap as dark as the tube
    /// goes would want twice, which is past what the phosphor has to give and
    /// is capped.
    pub fn line_gain(&self) -> f32 {
        if !self.line_gaps {
            return 1.0;
        }
        let average = (2.0 - self.scanlines.clamp(0.0, 1.0)) / 2.0;
        (1.0 / average.max(0.01)).min(MOST_GAIN)
    }
}

impl Default for Crt {
    fn default() -> Self {
        Self {
            interference: 0.22,
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

            // The beat between the dot clock and the subcarrier, where there
            // is colour for it to bite on.
            //
            // Dot crawl is the colour and the brightness being carried on one
            // wire and not coming apart again cleanly at the other end. Where
            // there is no colour there is nothing to cross-talk, and where the
            // colour changes from one pixel to the next there is most of it —
            // which is why it is seen crawling along the edges of coloured
            // blocks and not over a grey screen. Spread evenly instead, it
            // shows as a beat against whatever the picture is being scaled by
            // rather than as anything a set did.
            let here = chroma(src, w, x, y);
            let edge = (here - chroma(src, w, x.saturating_sub(1), y))
                .abs()
                .max((here - chroma(src, w, (x + 1).min(w - 1), y)).abs());
            let bite = (here * 0.6 + edge * 2.0).min(1.4);
            let phase = line_phase + (x0 + x as f64) * per_dot;
            let ripple =
                1.0 + crt.interference * bite * (phase * std::f64::consts::TAU).sin() as f32;
            r *= ripple;
            g *= ripple;
            b *= ripple;

            let a = src[(y * w + x) * 4 + 3];
            let line = ((y * rows) * w + x) * 4;
            // The lit line carries the light the gaps are not carrying.
            let gain = crt.line_gain();
            let (r, g, b) = (r * gain, g * gain, b * gain);
            put(out, line, r, g, b, a);
            if crt.line_gaps {
                // The gap under it, which is what a set's line structure looks
                // like once there is room to see it. Dimmed from the lit line
                // rather than from the picture, so that the line and its gap
                // together come out at the brightness the picture came in at.
                let dim = 1.0 - crt.scanlines;
                put(out, line + w * 4, r * dim, g * dim, b * dim, a);
            }
        }
    }
}

/// How much colour a pixel carries, as against how bright it is: nothing on
/// black, white or any grey between them, and most on a saturated colour.
fn chroma(src: &[u8], w: usize, x: usize, y: usize) -> f32 {
    let i = (y * w + x) * 4;
    let (r, g, b) = (
        src[i] as f32 / 255.0,
        src[i + 1] as f32 / 255.0,
        src[i + 2] as f32 / 255.0,
    );
    r.max(g).max(b) - r.min(g).min(b)
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
