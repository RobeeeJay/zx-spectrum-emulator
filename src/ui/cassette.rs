//! The cassette in the tape window, drawn from the artwork in `designs/`.
//!
//! The SVGs are rendered by [`crate::svg`] rather than copied into drawing
//! code, so the picture is whatever the files say it is and stays that way
//! when they are re-exported. The shell is rasterised once per size and kept
//! as a texture; the reels are the flat discs the artwork says they are, drawn
//! directly so they can wind on; and the cogs are rasterised once and then
//! turned by rotating the quad they sit on, which costs nothing per frame.

use egui::{Color32, ColorImage, FontId, Mesh, Pos2, Rect, Shape, TextureHandle, Vec2};

use crate::svg::{self, Transform};
use crate::ui::App;

/// The artwork's coordinate space.
pub const ART_W: f32 = 500.0;
pub const ART_H: f32 = 330.0;

/// How big the cassette is drawn, at most.
pub const MAX_W: f32 = 512.0;
/// What the tape window should be wide, to sit around it: a third narrower
/// than it started out, which is enough for the cassette and its margins.
pub const WINDOW_W: f32 = MAX_W + 30.0;

const CASSETTE_SVG: &str = include_str!("../../designs/cassette.svg");
const LEFT_COG_SVG: &str = include_str!("../../designs/left-cog.svg");
const RIGHT_COG_SVG: &str = include_str!("../../designs/right-cog.svg");

/// The reel drawn into the shell artwork. The emulator supplies its own, which
/// has to wind on, so the one in the file is left out.
const BAKED_REEL: &str = "Right-Reel";

/// Both reels are this big when full, in artwork units, and this colour —
/// taken from `left-reel.svg`, which is a plain disc.
const REEL_R: f32 = 87.42;
const REEL_INK: Color32 = Color32::from_rgb(76, 36, 24);
/// Centres of the two reels, which the cogs share.
const LEFT_HUB: (f32, f32) = (144.0, 148.9);
const RIGHT_HUB: (f32, f32) = (356.4, 148.9);
/// How much of a cog's own artwork to cut out as its texture.
const COG_R: f32 = 40.0;

/// How small a pack winds down to: an empty reel still has the tape's own
/// thickness wrapped round it.
const EMPTY: f32 = 0.6;

/// The most a hub is wound on for in one frame. A window that has not been
/// drawn for a while — because the machine was busy, or the emulator was in
/// the background — should not have its reels lurch to catch up.
pub const MAX_STEP: f32 = 0.1;

/// The window in the shell, through which the packs are seen.
const WINDOW: (f32, f32, f32, f32) = (194.0, 121.0, 305.1, 174.0);

/// The ruled lines on the label, where the title goes.
const RULE_X: (f32, f32) = (80.1, 455.5);
const RULE_Y: [f32; 4] = [49.0, 64.0, 79.0, 93.0];

/// How big each pack is, as a fraction of a full reel, `progress` of the way
/// through the tape. The supply reel on the left empties into the take-up reel
/// on the right, so the two swap over.
pub fn reel_scales(progress: f32) -> (f32, f32) {
    let p = progress.clamp(0.0, 1.0);
    (1.0 - (1.0 - EMPTY) * p, EMPTY + (1.0 - EMPTY) * p)
}

/// How fast a hub turns, in radians a second, for a pack of that size.
///
/// Taken from the real thing rather than picked by eye: a compact cassette
/// runs at 1⅞ inches a second, and a C60's tape winds out to about 25.7 mm
/// from the hub. A full pack therefore turns about eighteen times a minute,
/// and a nearly empty one about thirty.
pub fn spin_rate(pack: f32, boosted: bool) -> f32 {
    /// Tape speed, in millimetres a second.
    const TAPE_MM_S: f32 = 47.6;
    /// Radius of a full pack, in millimetres.
    const FULL_MM: f32 = 25.7;
    /// Hurrying the tape along — Max CPU or Fastload — turns the hubs at
    /// double speed, the way a deck's do when the fast-forward is held down.
    const BOOST: f32 = 2.0;
    TAPE_MM_S / (pack.max(EMPTY) * FULL_MM) * if boosted { BOOST } else { 1.0 }
}

/// Which way a hub is drawn turning, given how far it has wound on.
///
/// Tape leaves the left hub and is taken up by the right one along the bottom
/// of the shell. A point at the bottom of a hub therefore travels to the
/// right, which is anticlockwise as the cassette is seen.
pub fn drawn_angle(spin: f32) -> f32 {
    -spin
}

/// The same, in revolutions a second, which is how a deck is measured.
pub fn revs_per_second(pack: f32, boosted: bool) -> f32 {
    spin_rate(pack, boosted) / std::f32::consts::TAU
}

/// The rasterised artwork, kept between frames.
#[derive(Default)]
pub struct Art {
    /// Width the textures were made for, so they are only remade on a resize.
    width: usize,
    shell: Option<TextureHandle>,
    left_cog: Option<TextureHandle>,
    right_cog: Option<TextureHandle>,
}

/// Draw the cassette, and return the rectangle it took.
pub fn ui(app: &mut App, ui: &mut egui::Ui) -> Rect {
    // With nothing in the deck the cassette's space is left empty rather than
    // the window rearranging itself around the gap.
    let loaded = app.tape_ref().is_some();
    let (progress, playing, name) = match app.tape_ref() {
        Some(t) => (t.progress(), t.playing, written_name(&t.name)),
        None => (0.0, false, String::new()),
    };

    // Wind the hubs on while the tape runs. The clock is read rather than the
    // frame time added up: egui can lay a window out more than once in a
    // frame, and adding a frame's worth of turn each time would have the hubs
    // running at two or three times the speed they should.
    let now = ui.input(|i| i.time);
    let dt = (now - app.tape.spun_at).clamp(0.0, MAX_STEP as f64) as f32;
    app.tape.spun_at = now;
    let (left_pack, right_pack) = reel_scales(progress);
    let boosted = app.tape_boost();
    // Only while the tape is actually moving. A paused machine passes no
    // T-states, so the deck stands still — and hubs that keep turning over a
    // stopped tape say the opposite of what has happened.
    if playing && app.running {
        app.tape.left_spin += dt * spin_rate(left_pack, boosted);
        app.tape.right_spin += dt * spin_rate(right_pack, boosted);
    }

    // The window is a fixed width and the contents scroll, so the width is
    // the only thing that decides how big the cassette is drawn.
    let width = ui.available_width().clamp(160.0, MAX_W);
    let size = Vec2::new(width, width * ART_H / ART_W);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let scale = rect.width() / ART_W;
    let at = |x: f32, y: f32| rect.min + Vec2::new(x, y) * scale;
    if !loaded {
        return rect;
    }
    let painter = ui.painter_at(rect);

    rasterise(app, ui.ctx(), rect.width().round() as usize);

    // Behind the window: the inside of the shell, then the two packs.
    let window = Rect::from_min_max(at(WINDOW.0, WINDOW.1), at(WINDOW.2, WINDOW.3));
    painter.rect_filled(window, 0.0, Color32::from_rgb(20, 20, 20));
    {
        let painter = painter.with_clip_rect(window);
        for (hub, pack) in [(LEFT_HUB, left_pack), (RIGHT_HUB, right_pack)] {
            painter.circle_filled(at(hub.0, hub.1), REEL_R * pack * scale, REEL_INK);
        }
    }

    // The shell over the top: the window is a hole in the artwork, so the
    // packs show through it.
    if let Some(shell) = &app.art.shell {
        painter.image(
            shell.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    title(&painter, rect, scale, &name);

    for (texture, hub, angle) in [
        (app.art.left_cog.clone(), LEFT_HUB, app.tape.left_spin),
        (app.art.right_cog.clone(), RIGHT_HUB, app.tape.right_spin),
    ] {
        let Some(texture) = texture else { continue };
        turned_image(
            &painter,
            &texture,
            at(hub.0, hub.1),
            COG_R * scale,
            drawn_angle(angle),
        );
    }
    rect
}

/// Rasterise the artwork, if the size has changed since last time.
fn rasterise(app: &mut App, ctx: &egui::Context, width: usize) {
    if app.art.width == width && app.art.shell.is_some() {
        return;
    }
    let height = (width as f32 * ART_H / ART_W).round() as usize;
    if width == 0 || height == 0 {
        return;
    }
    let load = |name: &str, image: svg::Image| {
        ctx.load_texture(
            name,
            ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba),
            egui::TextureOptions::LINEAR,
        )
    };
    app.art.shell = Some(load(
        "cassette",
        svg::render(
            CASSETTE_SVG,
            width,
            height,
            &[BAKED_REEL],
            Transform::IDENTITY,
        ),
    ));

    // Each cog is cut out of its own file as a square centred on its hub, so
    // it can be turned by rotating the quad it is drawn on rather than being
    // rasterised again every frame.
    let side = ((COG_R * 2.0 / ART_W) * width as f32).round().max(8.0) as usize;
    let cogs = [(LEFT_COG_SVG, LEFT_HUB), (RIGHT_COG_SVG, RIGHT_HUB)];
    let mut made = Vec::new();
    for (src, hub) in cogs {
        // render() fits the viewBox to the output, which for a square output
        // would squash the artwork; undoing that first leaves artwork units,
        // and the crop then centres the hub.
        let undo = Transform::scale(ART_W / side as f32, ART_H / side as f32);
        let per_unit = side as f32 / (COG_R * 2.0);
        let crop = Transform::scale(per_unit, per_unit)
            .then(Transform::translate(COG_R - hub.0, COG_R - hub.1));
        made.push(load(
            "cog",
            svg::render(src, side, side, &[], undo.then(crop)),
        ));
    }
    app.art.right_cog = made.pop();
    app.art.left_cog = made.pop();
    app.art.width = width;
}

/// Draw a square texture centred on a point, turned by `angle` radians.
fn turned_image(
    painter: &egui::Painter,
    texture: &TextureHandle,
    centre: Pos2,
    radius: f32,
    angle: f32,
) {
    let (sin, cos) = angle.sin_cos();
    let mut mesh = Mesh::with_texture(texture.id());
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (x, y) = (dx * radius, dy * radius);
        let at = Pos2::new(centre.x + x * cos - y * sin, centre.y + x * sin + y * cos);
        let uv = Pos2::new((dx + 1.0) / 2.0, (dy + 1.0) / 2.0);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: at,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// What gets written on the label: the file's name without the extension,
/// as somebody would have written it in biro.
pub fn written_name(file: &str) -> String {
    std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .unwrap_or_else(|| file.trim().to_string())
}

/// How far through the whole tape, by playing time rather than by block.
pub fn overall_progress(tape: &crate::tape::Tape) -> f32 {
    tape.progress()
}

/// The tape's name, written across the ruled lines. Long names are set smaller
/// rather than allowed to run off the label.
fn title(painter: &egui::Painter, rect: Rect, scale: f32, name: &str) {
    if name.is_empty() {
        return;
    }
    let room = (RULE_X.1 - RULE_X.0) * scale * 0.94;
    let mut size = 26.0 * scale;
    let ink = Color32::from_rgb(31, 63, 143);
    for _ in 0..8 {
        let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(size), ink);
        if galley.rect.width() <= room || size <= 9.0 * scale {
            let centre = rect.min + Vec2::new((RULE_X.0 + RULE_X.1) / 2.0, RULE_Y[2]) * scale;
            painter.galley(
                Pos2::new(
                    centre.x - galley.rect.width() / 2.0,
                    centre.y - galley.rect.height(),
                ),
                galley,
                ink,
            );
            return;
        }
        size *= 0.85;
    }
}
