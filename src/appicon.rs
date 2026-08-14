//! The application icon, shaped the way macOS shapes one.
//!
//! Apple's icons are not square pictures: the artwork sits inside a rounded
//! square — a squircle, near enough — with clear space around it, so that every
//! icon in the dock is the same visual size whatever shape its artwork is. The
//! proportions here are Apple's: on a 1024-point canvas the shape is 824 across
//! with a corner radius of 185.4, leaving 100 points of margin.
//!
//! Done here rather than to the file so that `icon.png` stays the artwork. Draw
//! a new one, export it square, and it is shaped to match without anybody
//! editing corners by hand.

/// Apple's proportions, as fractions of the canvas: the shape's width, and its
/// corner radius.
const ARTWORK: f32 = 824.0 / 1024.0;
const RADIUS: f32 = 185.4 / 1024.0;

/// How much of the shape the artwork itself takes up.
///
/// Artwork drawn to the edges of a square — a rainbow stripe running right
/// across, lettering that starts at the margin — loses its ends to the corners
/// when it is cut to a rounded square. Standing it back from the curve keeps
/// the lettering whole, and the shape is filled with the artwork's own
/// background so the inset does not read as a border.
const INSIDE: f32 = 0.88;

/// Shape an icon: scale the artwork into Apple's rounded square, centred on a
/// transparent canvas of `size` points.
///
/// Returns RGBA, ready to hand to the window system.
pub fn shaped(source: &[u8], width: u32, height: u32, size: u32) -> Vec<u8> {
    let canvas = size as f32;
    let art = (canvas * ARTWORK).round();
    let inset = ((canvas - art) / 2.0).round();
    let radius = canvas * RADIUS;

    // The colour behind the artwork, taken from its own corner: the shape is
    // filled with it so that standing the artwork back from the curve does not
    // leave a band of nothing around it.
    let (br, bg, bb, _) = sample(source, width, height, 0.5, 0.5);

    let art_inset = inset + art * (1.0 - INSIDE) / 2.0;
    let art_side = art * INSIDE;

    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            // How much of this pixel is inside the rounded square. Taken from
            // the distance to the shape's edge so the curve is smooth rather
            // than a staircase.
            let inside = coverage(x as f32 + 0.5, y as f32 + 0.5, inset, art, radius);
            if inside <= 0.0 {
                continue;
            }
            // Where that pixel sits in the artwork, which is scaled to sit
            // inside the shape rather than fill it to the curve.
            let u = (x as f32 - art_inset) / art_side * width as f32;
            let v = (y as f32 - art_inset) / art_side * height as f32;
            let (r, g, b) = if u >= 0.0 && v >= 0.0 && u < width as f32 && v < height as f32 {
                let (r, g, b, _) = sample(source, width, height, u, v);
                (r, g, b)
            } else {
                (br, bg, bb)
            };
            let at = ((y * size + x) * 4) as usize;
            out[at] = r;
            out[at + 1] = g;
            out[at + 2] = b;
            out[at + 3] = (255.0 * inside).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// How much of the pixel at (`x`, `y`) falls inside the rounded square.
fn coverage(x: f32, y: f32, inset: f32, side: f32, radius: f32) -> f32 {
    let (left, top) = (inset, inset);
    let (right, bottom) = (inset + side, inset + side);
    // Distance outside the shape, measured from the rounded rectangle: zero
    // within it, growing as it goes out.
    let dx = (left + radius - x).max(x - (right - radius)).max(0.0);
    let dy = (top + radius - y).max(y - (bottom - radius)).max(0.0);
    let distance = (dx * dx + dy * dy).sqrt() - radius;
    // A pixel straddling the edge is half covered, which is what makes the
    // curve look like a curve.
    (0.5 - distance).clamp(0.0, 1.0)
}

/// The artwork at a point, blended from the four pixels around it.
fn sample(source: &[u8], width: u32, height: u32, u: f32, v: f32) -> (u8, u8, u8, u8) {
    let channels = source.len() / (width * height).max(1) as usize;
    let at = |x: u32, y: u32, c: usize| -> f32 {
        let index = ((y.min(height - 1) * width + x.min(width - 1)) as usize) * channels + c;
        source.get(index).copied().unwrap_or(0) as f32
    };
    let (x0, y0) = (u.floor().max(0.0) as u32, v.floor().max(0.0) as u32);
    let (fx, fy) = (u - u.floor(), v - v.floor());
    let mix = |c: usize| -> u8 {
        let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1, y0, c) * fx;
        let bottom = at(x0, y0 + 1, c) * (1.0 - fx) + at(x0 + 1, y0 + 1, c) * fx;
        (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
    };
    let alpha = if channels == 4 { mix(3) } else { 255 };
    (mix(0), mix(1), mix(2), alpha)
}
