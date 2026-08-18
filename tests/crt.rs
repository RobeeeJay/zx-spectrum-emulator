//! What a television makes of the picture.

use zx_rustrum::crt::{roll_per_frame, subcarrier_per_dot, televise, Crt, DOTS_PER_FRAME};
use zx_rustrum::ui::crt::{bulge, covered, curved, CURVE};

/// A flat field of one colour, as the ULA would put it out.
fn field(w: usize, h: usize, rgb: [u8; 3]) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        v.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    v
}

fn pixel(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// The interference is the machine's own arithmetic: PAL's colour subcarrier
/// against the Spectrum's dot clock, which is 0.6334 of a cycle per pixel.
/// Sampled once a pixel that aliases to a ripple every 2.7 pixels or so —
/// the herringbone anybody who used one on a television will remember.
#[test]
fn the_interference_is_the_subcarrier_beating_against_the_dot_clock() {
    let per_dot = subcarrier_per_dot();
    assert!(
        (per_dot - 0.633_374).abs() < 1e-5,
        "4.43MHz into 7MHz: {per_dot}"
    );
    let aliased = 1.0 / (1.0 - per_dot);
    assert!(
        (2.6..2.8).contains(&aliased),
        "which shows as a ripple every {aliased:.2} pixels"
    );
    // And it does not sit still: a frame is 139,776 dots, which is not a whole
    // number of cycles, so the pattern arrives somewhere else next time.
    assert_eq!(DOTS_PER_FRAME, 139_776.0);
    let roll = roll_per_frame();
    assert!(
        roll > 0.01 && roll < 0.99,
        "the pattern should move from frame to frame, not stand still: {roll}"
    );
}

/// The picture comes out with a line and a gap under it, and the gap is
/// darker: that is what a set's line structure is.
#[test]
fn every_line_gets_a_gap_under_it() {
    let (w, h) = (8, 4);
    let src = field(w, h, [200, 200, 200]);
    let mut out = Vec::new();
    let quiet = Crt {
        interference: 0.0,
        bleed: 0.0,
        scanlines: 0.5,
    };
    televise(&src, w, h, &mut out, quiet, 0, 0.0);
    assert_eq!(out.len(), w * h * 2 * 4, "twice the height");
    for y in 0..h {
        let line = pixel(&out, w, 3, y * 2);
        let gap = pixel(&out, w, 3, y * 2 + 1);
        assert_eq!(line[0], 200, "the line itself is the picture");
        assert!(
            (gap[0] as i32 - 100).abs() <= 1,
            "and the gap under it is half of it: {gap:?}"
        );
    }
}

/// Colour is smeared sideways and brightness is not. Composite video carries
/// the colour on a subcarrier with a fraction of the luminance's bandwidth, so
/// a red edge arrives late and soft while the brightness edge stays sharp.
#[test]
fn colour_bleeds_sideways_and_brightness_does_not() {
    let (w, h) = (16, 1);
    // Black on the left, red on the right.
    let mut src = field(w, h, [0, 0, 0]);
    for x in 8..w {
        let i = x * 4;
        src[i] = 255;
    }
    let mut out = Vec::new();
    let smeary = Crt {
        interference: 0.0,
        bleed: 3.0,
        scanlines: 0.0,
    };
    televise(&src, w, h, &mut out, smeary, 0, 0.0);

    let before = pixel(&out, w, 6, 0);
    assert!(
        before[0] > 5,
        "the red should have run back over the edge: {before:?}"
    );
    let luma = |p: [u8; 4]| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
    let src_luma = |x: usize| {
        let i = x * 4;
        0.299 * src[i] as f32 + 0.587 * src[i + 1] as f32 + 0.114 * src[i + 2] as f32
    };
    // Away from the edge the brightness is untouched.
    for x in [1usize, 14] {
        assert!(
            (luma(pixel(&out, w, x, 0)) - src_luma(x)).abs() < 2.0,
            "but the brightness should be where it was at {x}"
        );
    }
    // And over the edge the picture is still dark on the black side: the
    // colour has run, and the brightness has not gone with it. It does not
    // come back exactly, because taking the smeared luminance out again would
    // want light of a negative amount on the black side, and a tube has none
    // of that.
    assert!(
        luma(pixel(&out, w, 6, 0)) < luma(pixel(&out, w, 14, 0)) / 3.0,
        "the black side should still be dark: {} against {}",
        luma(pixel(&out, w, 6, 0)),
        luma(pixel(&out, w, 14, 0))
    );
}

/// The same frame twice gives the same picture, and the next frame does not:
/// the interference rolls.
#[test]
fn the_pattern_rolls_from_frame_to_frame() {
    let (w, h) = (32, 2);
    let src = field(w, h, [180, 180, 180]);
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut c = Vec::new();
    let crt = Crt {
        bleed: 0.0,
        ..Crt::default()
    };
    televise(&src, w, h, &mut a, crt, 100, 0.0);
    televise(&src, w, h, &mut b, crt, 100, 0.0);
    televise(&src, w, h, &mut c, crt, 101, 0.0);
    assert_eq!(a, b, "the same frame should look the same twice");
    assert_ne!(a, c, "and the next frame should not");
}

/// The glass is part of a sphere: the middle is where it was, the edges bow
/// out, and the corners go furthest.
#[test]
fn the_tube_bulges_most_at_the_corners() {
    let middle = bulge(egui::Vec2::ZERO, CURVE);
    assert_eq!(middle, egui::Vec2::ZERO, "the middle does not move");

    let edge = bulge(egui::vec2(1.0, 0.0), CURVE).x;
    let corner = bulge(egui::vec2(1.0, 1.0), CURVE).x;
    assert!(edge > 1.0, "the edge bows out: {edge}");
    assert!(
        corner > edge,
        "and the corner further: {corner} over {edge}"
    );

    // What the picture covers grows with it, so a bezel drawn round it is
    // drawn round the glass rather than through it.
    let flat = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(512.0, 384.0));
    let glass = covered(flat, CURVE);
    assert!(glass.width() > flat.width() && glass.height() > flat.height());
    assert_eq!(glass.center(), flat.center(), "and stays where it was");
}

/// The mesh is a grid of quads with the picture stretched over it, so the
/// pixels are the ULA's however the glass is shaped.
#[test]
fn the_picture_is_drawn_over_the_curve() {
    let flat = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(512.0, 384.0));
    let mesh = curved(egui::TextureId::default(), flat, CURVE);
    assert!(mesh.vertices.len() > 100, "a grid, not a quad");
    assert_eq!(
        mesh.indices.len() % 3,
        0,
        "and triangles, three indices at a time"
    );
    // The corners of the texture are still the corners of the picture.
    let uvs: Vec<egui::Pos2> = mesh.vertices.iter().map(|v| v.uv).collect();
    assert!(uvs.contains(&egui::pos2(0.0, 0.0)) && uvs.contains(&egui::pos2(1.0, 1.0)));
    // And the mesh reaches further than the flat picture would have.
    let widest = mesh
        .vertices
        .iter()
        .map(|v| v.pos.x)
        .fold(f32::MIN, f32::max);
    assert!(
        widest > flat.right(),
        "the glass should reach past the flat picture: {widest} against {}",
        flat.right()
    );
}

/// The switch is in the Video section of the main window, and it changes what
/// the picture is made of rather than only how it is drawn.
#[test]
fn the_crt_switch_televises_the_picture() {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    let mut h: Harness<'_, App> = Harness::builder()
        .with_size([1200.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(!h.state().crt, "a set is not switched on to start with");

    let flat = h.state().screen_texture().map(|t| t.size());
    h.get_by_label("CRT").click();
    h.run_steps(3);
    assert!(h.state().crt, "the CRT switch did nothing");

    // Twice the height, because every line has a gap under it now.
    let televised = h.state().screen_texture().map(|t| t.size());
    match (flat, televised) {
        (Some([fw, fh]), Some([tw, th])) => {
            assert_eq!(tw, fw, "the same width");
            assert_eq!(th, fh * 2, "and twice the height: {th} against {fh}");
        }
        other => panic!("there should be a picture either way: {other:?}"),
    }
}
