//! The small SVG renderer, checked against the artwork it exists to draw.

use zx_rustrum::svg::{render, view_box, Transform};

const CASSETTE: &str = include_str!("../designs/cassette.svg");
const LEFT_COG: &str = include_str!("../designs/left-cog.svg");
const LEFT_REEL: &str = include_str!("../designs/left-reel.svg");

fn pixel(img: &zx_rustrum::svg::Image, x: usize, y: usize) -> [u8; 4] {
    let i = (y * img.width + x) * 4;
    [
        img.rgba[i],
        img.rgba[i + 1],
        img.rgba[i + 2],
        img.rgba[i + 3],
    ]
}

#[test]
fn the_view_box_is_read_from_the_document() {
    assert_eq!(view_box(CASSETTE), Some((0.0, 0.0, 500.0, 330.0)));
}

#[test]
fn the_shell_is_drawn_and_the_corners_are_left_clear() {
    let img = render(CASSETTE, 500, 330, &[], Transform::IDENTITY);
    assert_eq!(img.rgba.len(), 500 * 330 * 4);

    // The shell starts a few units in, so the very corner is untouched.
    assert_eq!(pixel(&img, 1, 1)[3], 0, "outside the shell should be clear");
    // The middle of the label is paper, near white.
    let paper = pixel(&img, 250, 60);
    assert!(
        paper[3] == 255 && paper[0] > 200,
        "expected label, got {paper:?}"
    );
}

#[test]
fn the_gradient_on_the_shell_runs_from_light_to_dark() {
    let img = render(CASSETTE, 500, 330, &[], Transform::IDENTITY);
    // Down the left edge, inside the shell but outside the label.
    let top = pixel(&img, 15, 20);
    let bottom = pixel(&img, 15, 310);
    assert!(top[3] == 255 && bottom[3] == 255);
    assert!(
        top[0] > bottom[0] + 8,
        "the shell should be lighter at the top: {top:?} against {bottom:?}"
    );
}

#[test]
fn the_red_stripe_is_where_the_artwork_puts_it() {
    let img = render(CASSETTE, 500, 330, &[], Transform::IDENTITY);
    // Left of the window bezel, which is drawn over the middle of the stripe.
    let red = pixel(&img, 60, 114);
    assert!(
        red[0] > 150 && red[1] < 60 && red[2] < 60,
        "expected the red stripe at y=114, got {red:?}"
    );
}

#[test]
fn the_window_is_a_hole_so_the_reels_show_through() {
    // The shell path has the window as a second subpath, filled even-odd. If
    // that were ignored the middle would be solid and the tape invisible.
    let img = render(CASSETTE, 500, 330, &["Right-Reel"], Transform::IDENTITY);
    let inside = pixel(&img, 250, 150);
    assert_eq!(
        inside[3], 0,
        "the window should be clear through to the layer below, got {inside:?}"
    );
}

#[test]
fn a_group_can_be_left_out() {
    let differing = |skip: &[&str]| {
        let with = render(CASSETTE, 500, 330, &[], Transform::IDENTITY);
        let without = render(CASSETTE, 500, 330, skip, Transform::IDENTITY);
        with.rgba
            .chunks(4)
            .zip(without.rgba.chunks(4))
            .filter(|(a, b)| a != b)
            .count()
    };
    assert!(
        differing(&["Label"]) > 10_000,
        "the label covers most of the front"
    );
    // The file has a reel drawn into it, which the emulator replaces with one
    // that moves. Nearly all of it is behind the shell, so only the sliver
    // showing through the window changes.
    assert!(differing(&["Right-Reel"]) > 0, "the reel should have gone");
}

#[test]
fn the_root_transform_turns_and_scales_what_it_draws() {
    // The cog has six teeth, so a sixth of a turn lands it on itself and a
    // twelfth puts the teeth squarely between where they were.
    let turn = |fraction: f32| {
        render(
            LEFT_COG,
            1000,
            660,
            &[],
            Transform::rotate_about(std::f32::consts::TAU * fraction, 144.0, 148.9),
        )
    };
    let plain = turn(0.0);
    let count = |other: &zx_rustrum::svg::Image| {
        plain
            .rgba
            .chunks(4)
            .zip(other.rgba.chunks(4))
            .filter(|(a, b)| (a[0] as i32 - b[0] as i32).abs() > 40)
            .count()
    };
    let sixth = count(&turn(1.0 / 6.0));
    let twelfth = count(&turn(1.0 / 12.0));
    assert!(
        twelfth > sixth * 3,
        "half a tooth out should differ far more than a whole one: {twelfth} against {sixth}"
    );
}

#[test]
fn a_reel_can_be_wound_down_to_its_hub() {
    let full = render(LEFT_REEL, 500, 330, &[], Transform::IDENTITY);
    let small = render(
        LEFT_REEL,
        500,
        330,
        &[],
        Transform::scale_about(0.6, 144.0, 148.9),
    );
    let ink = |img: &zx_rustrum::svg::Image| img.rgba.chunks(4).filter(|p| p[3] > 128).count();
    let (a, b) = (ink(&full), ink(&small));
    assert!(a > 0 && b > 0);
    let ratio = b as f32 / a as f32;
    assert!(
        (ratio - 0.36).abs() < 0.03,
        "area should go with the square of the radius, got {ratio}"
    );
}

/// Writes the artwork out so it can be looked at. Ignored by default.
#[test]
#[ignore]
fn dump() {
    let (w, h) = (1000, 660);
    let mut img = render(CASSETTE, w, h, &["Right-Reel"], Transform::IDENTITY);
    // Composite the moving parts the way the tape window does.
    for (src, t) in [
        (LEFT_REEL, Transform::IDENTITY),
        (
            include_str!("../designs/right-reel.svg"),
            Transform::scale_about(0.6, 356.4, 148.9),
        ),
    ] {
        let layer = render(src, w, h, &[], t);
        under(&mut img, &layer);
    }
    for (src, angle) in [
        (LEFT_COG, 0.4f32),
        (include_str!("../designs/right-cog.svg"), -0.9),
    ] {
        let centre = if angle > 0.0 { 144.0 } else { 356.4 };
        let layer = render(
            src,
            w,
            h,
            &[],
            Transform::rotate_about(angle, centre, 148.9),
        );
        over(&mut img, &layer);
    }
    write_png("/tmp/cassette-render.png", &img);
}

fn over(base: &mut zx_rustrum::svg::Image, top: &zx_rustrum::svg::Image) {
    for (b, t) in base.rgba.chunks_mut(4).zip(top.rgba.chunks(4)) {
        let a = t[3] as f32 / 255.0;
        for k in 0..4 {
            b[k] = (b[k] as f32 * (1.0 - a) + t[k] as f32 * a) as u8;
        }
    }
}

fn under(base: &mut zx_rustrum::svg::Image, below: &zx_rustrum::svg::Image) {
    for (b, u) in base.rgba.chunks_mut(4).zip(below.rgba.chunks(4)) {
        let a = b[3] as f32 / 255.0;
        for k in 0..4 {
            b[k] = (u[k] as f32 * (1.0 - a) + b[k] as f32 * a) as u8;
        }
    }
}

fn write_png(path: &str, img: &zx_rustrum::svg::Image) {
    let mut raw = Vec::new();
    for y in 0..img.height {
        raw.push(0u8);
        for x in 0..img.width {
            let i = (y * img.width + x) * 4;
            // On white, as the window shows it.
            let a = img.rgba[i + 3] as f32 / 255.0;
            for k in 0..3 {
                raw.push((img.rgba[i + k] as f32 * a + 255.0 * (1.0 - a)) as u8);
            }
        }
    }
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        let body: Vec<u8> = kind.iter().chain(data).copied().collect();
        let mut crc = 0xffff_ffffu32;
        for b in &body {
            crc ^= *b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        out.extend_from_slice(&body);
        out.extend_from_slice(&(!crc).to_be_bytes());
        out
    }
    let mut ihdr = (img.width as u32).to_be_bytes().to_vec();
    ihdr.extend_from_slice(&(img.height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut z = vec![0x78u8, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    std::fs::write(path, out).unwrap();
}
