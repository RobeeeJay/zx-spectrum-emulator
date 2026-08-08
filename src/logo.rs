//! The application's mark: the Spectrum's seven-colour flash on a dark
//! shell, drawn as pixels so the window manager has an icon to show without
//! anything having to be shipped alongside the binary.

/// The seven border colours, in the order the machine wore them.
pub const RAINBOW: [[u8; 3]; 7] = [
    [0x20, 0x62, 0xff],
    [0xe0, 0x43, 0x3c],
    [0xff, 0x33, 0xe0],
    [0x0f, 0xbb, 0x4d],
    [0x22, 0xe0, 0xe0],
    [0xf4, 0xe2, 0x30],
    [0xf4, 0xf2, 0xea],
];

/// The mark at `size` pixels square, as RGBA.
///
/// Drawn rather than loaded: the shapes are a rounded square and seven sheared
/// bars, which is less code than decoding an image would be, and it comes out
/// right at whatever size the platform asks for.
pub fn rgba(size: usize) -> Vec<u8> {
    let n = size as f32;
    let radius = n * 0.19;
    let mut out = vec![0u8; size * size * 4];

    // The bars run across the lower half, leaning to the right.
    let bar_top = n * 0.30;
    let bar_bottom = n * 0.86;
    let left = n * 0.13;
    let bar_w = n * 0.74 / RAINBOW.len() as f32;
    let shear = n * 0.14;

    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if !inside_rounded(fx, fy, n, radius) {
                continue;
            }
            let mut colour = [0x14, 0x16, 0x1a];
            if (bar_top..=bar_bottom).contains(&fy) {
                // Higher up a bar is further right, so the flash leans.
                let lean = (bar_bottom - fy) / (bar_bottom - bar_top) * shear;
                let along = fx - left - lean;
                if along >= 0.0 {
                    let index = (along / bar_w) as usize;
                    if index < RAINBOW.len() {
                        colour = RAINBOW[index];
                    }
                }
            }
            let i = (y * size + x) * 4;
            out[i..i + 3].copy_from_slice(&colour);
            out[i + 3] = 0xff;
        }
    }
    out
}

/// Is the point inside a rounded square filling the whole canvas?
fn inside_rounded(x: f32, y: f32, side: f32, radius: f32) -> bool {
    let cx = x.clamp(radius, side - radius);
    let cy = y.clamp(radius, side - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}
