//! What a television makes of the picture.

use zx_rustrum::crt::{roll_per_frame, subcarrier_per_dot, televise, Crt, DOTS_PER_FRAME};
use zx_rustrum::ui::crt::{curved, sample_at, CURVE};

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
        line_gaps: true,
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
        line_gaps: false,
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

/// The glass is part of a sphere, so the picture swells in the middle: the
/// further out a point is, the further out it reaches for its colour, which
/// squeezes what is at the edges and stretches what is in the middle.
///
/// It was the other way round to begin with — the corners of the mesh pushed
/// outwards, which is a pincushion: the shape of a badly adjusted monitor
/// rather than the shape of a tube.
#[test]
fn the_picture_swells_in_the_middle_like_a_tube() {
    let middle = sample_at(egui::Vec2::ZERO, CURVE);
    assert_eq!(middle, egui::Vec2::ZERO, "the middle stays where it is");

    let half = sample_at(egui::vec2(0.5, 0.0), CURVE).x;
    assert!(
        half < 0.5,
        "half way out should read from further out than half way, which is \
         what swells the middle: {half}"
    );

    // Nothing is read from outside the picture, or the edge pixels would be
    // smeared out to fill the corners.
    for at in [
        egui::vec2(1.0, 1.0),
        egui::vec2(-1.0, 1.0),
        egui::vec2(1.0, -1.0),
        egui::vec2(-1.0, -1.0),
    ] {
        let from = sample_at(at, CURVE);
        assert!(
            from.x.abs() <= 1.0001 && from.y.abs() <= 1.0001,
            "the corner should read from inside the picture: {from:?}"
        );
    }

    // The corners reach furthest out, which is what tucks them back.
    let corner = sample_at(egui::vec2(1.0, 1.0), CURVE).x;
    let edge = sample_at(egui::vec2(1.0, 0.0), CURVE).x;
    assert!(
        corner > edge,
        "a corner reads from further out than the middle of an edge: {corner} \
         against {edge}"
    );
}

/// The mesh is a grid of quads filling the picture, each reading from a place
/// the curve moved: the pixels are still the ULA's, they are just read from
/// somewhere else.
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
    // The quads stay inside the picture: the curve is in what they read, not
    // in where they are, so nothing pokes out through the bezel.
    for v in &mesh.vertices {
        assert!(
            flat.expand(0.01).contains(v.pos),
            "a vertex outside the picture: {:?}",
            v.pos
        );
        assert!(
            (-0.0001..=1.0001).contains(&v.uv.x) && (-0.0001..=1.0001).contains(&v.uv.y),
            "and one reading from outside it: {:?}",
            v.uv
        );
    }
    // The middle of the top edge reads from inside the picture rather than
    // from its very top, which is the swell.
    let middle_top = mesh
        .vertices
        .iter()
        .find(|v| (v.pos.x - flat.center().x).abs() < 0.01 && v.pos.y == flat.top())
        .expect("the grid has a vertex at the middle of the top edge");
    assert!(
        middle_top.uv.y > 0.0,
        "the top of the picture is read from inside it: {:?}",
        middle_top.uv
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

/// The aerial lead is a switch of its own.
///
/// A monitor fed RGB had the tube's line structure and none of the composite
/// artefacts, so the two are separate: the CRT switch is the glass, and the
/// Composite switch is what came down the lead.
#[test]
fn composite_is_switched_apart_from_the_tube() {
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
    let flat = h.state().screen_texture().map(|t| t.size()).unwrap();

    // Composite on its own: the same picture, the same size, treated.
    h.get_by_label("Composite").click();
    h.run_steps(3);
    assert!(h.state().composite && !h.state().crt);
    assert_eq!(
        h.state().screen_texture().map(|t| t.size()),
        Some(flat),
        "the aerial lead does not give the picture line gaps"
    );

    let set = h.state().crt_settings();
    assert!(
        set.bleed > 0.0 && set.interference > 0.0 && !set.line_gaps,
        "the lead smears the colour and beats against the dot clock, and does \
         not give the picture line gaps: {set:?}"
    );

    // And the tube on top of it doubles the height, as the tube does.
    h.get_by_label("CRT").click();
    h.run_steps(3);
    assert_eq!(
        h.state().screen_texture().map(|t| t.size()),
        Some([flat[0], flat[1] * 2]),
        "and the tube does"
    );

    // And the lines are only drawn where there is room for them: at 1x there
    // is one row of screen for each row of picture, and a gap under every
    // line would be a pattern of its own rather than a line structure.
    h.state_mut().scale = 1.0;
    h.run_steps(2);
    let small = h.state().crt_settings();
    assert!(
        !small.line_gaps && small.scanlines == 0.0,
        "no room for the line structure at 1x: {small:?}"
    );
    h.state_mut().scale = 2.0;
    h.run_steps(2);

    // The tube on its own is the glass and nothing of the lead.
    h.get_by_label("Composite").click();
    h.run_steps(3);
    let set = h.state().crt_settings();
    assert!(
        set.line_gaps && set.scanlines > 0.0,
        "the tube gives the picture its line structure: {set:?}"
    );
    assert_eq!(
        (set.bleed, set.interference),
        (0.0, 0.0),
        "and none of what the aerial lead does: {set:?}"
    );
}

/// The set's picture is sampled smoothly and the machine's is not.
///
/// A tube has no pixel edges, and drawing one through a nearest sample beats
/// against the screen it is shown on: the line gaps and the herringbone are
/// both about a pixel across, so a picture scaled by anything but a whole
/// number takes some of them twice and some not at all, which is a moiré over
/// the whole screen. With the set off, a pixel is a hard square again.
#[test]
fn the_set_is_sampled_smoothly_and_the_machine_is_not() {
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
    assert_eq!(
        h.state().picture_filter(),
        egui::TextureOptions::NEAREST,
        "the ULA's pixels are squares"
    );

    for switch in ["CRT", "Composite"] {
        h.get_by_label(switch).click();
        h.run_steps(2);
        assert_eq!(
            h.state().picture_filter(),
            egui::TextureOptions::LINEAR,
            "{switch} should soften the picture"
        );
        h.get_by_label(switch).click();
        h.run_steps(2);
        assert_eq!(
            h.state().picture_filter(),
            egui::TextureOptions::NEAREST,
            "and switching it off should give the squares back"
        );
    }
}
