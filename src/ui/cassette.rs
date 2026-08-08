//! The cassette in the tape window, drawn from the artwork in `designs/`.
//!
//! The SVGs are translated into painter geometry rather than rasterised: it
//! keeps the picture sharp at any size, and the parts that move — the two cogs
//! turning and the tape packs winding from one side to the other — are then
//! just numbers rather than a pile of pre-rendered frames.
//!
//! All the coordinates below are in the artwork's own 500x330 space, taken
//! from the SVG paths with their transforms worked through, and mapped onto
//! wherever the widget ends up.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, Vec2};

use crate::ui::App;

/// The artwork's coordinate space.
pub const ART_W: f32 = 500.0;
pub const ART_H: f32 = 330.0;

/// How big the cassette is drawn, at most.
pub const MAX_W: f32 = 780.0;
/// What the tape window should be wide, to sit around it.
pub const WINDOW_W: f32 = MAX_W + 30.0;

/// Both reels are this big when full, in artwork units.
const REEL_R: f32 = 87.42;
/// Centres of the two reels, which the cogs share.
const LEFT_HUB: (f32, f32) = (144.0, 148.9);
const RIGHT_HUB: (f32, f32) = (356.4, 148.9);

/// How small a pack winds down to: an empty reel still has the tape's own
/// thickness wrapped round it.
const EMPTY: f32 = 0.6;

/// The window in the shell, through which the packs are seen.
const WINDOW: (f32, f32, f32, f32) = (193.4, 120.1, 305.7, 175.5);

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

/// How fast a hub turns, given how much tape is wound on it and whether the
/// tape is being played at speed. The tape moves at a constant rate, so a
/// small pack has to turn faster than a fat one — which is why a cassette's
/// hubs visibly change speed as it plays.
pub fn spin_rate(pack: f32, boosted: bool) -> f32 {
    /// Radians a second for a full reel, before the speed is taken into account.
    const BASE: f32 = 2.2;
    /// An unhurried tape turns at half that.
    const SLOW: f32 = 0.5;
    /// Boosted, it winds on half again as fast as the unhurried rate.
    const BOOSTED: f32 = 1.5;
    BASE * if boosted { BOOSTED } else { SLOW } / pack.max(EMPTY)
}

/// Draw the cassette, and return the rectangle it took.
pub fn ui(app: &mut App, ui: &mut egui::Ui) -> Rect {
    let (progress, playing, name) = match app.tape_ref() {
        Some(t) => (overall_progress(t), t.playing, written_name(&t.name)),
        None => return Rect::NOTHING,
    };

    // Wind the hubs on while the tape runs. Doing it from elapsed time rather
    // than from the tape position keeps the movement smooth whatever the data
    // on the tape happens to be.
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let (left_pack, right_pack) = reel_scales(progress);
    let boosted = app.tape_boost();
    if playing {
        app.tape.left_spin += dt * spin_rate(left_pack, boosted);
        app.tape.right_spin += dt * spin_rate(right_pack, boosted);
    }

    // The window is sized around this, so the width is what decides; the
    // height only comes into it if the window has been made short.
    let room = ui.available_height() * 0.62;
    let width = ui
        .available_width()
        .min(MAX_W)
        .min(room * ART_W / ART_H)
        .max(160.0);
    let size = Vec2::new(width, width * ART_H / ART_W);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let art = Art {
        origin: rect.min,
        scale: rect.width() / ART_W,
    };
    let painter = ui.painter_at(rect);

    shell(&painter, &art);
    label(&painter, &art, &name);
    packs(&painter, &art, left_pack, right_pack);
    window_bezel(&painter, &art);
    cog(&painter, &art, LEFT_HUB, app.tape.left_spin);
    cog(&painter, &art, RIGHT_HUB, app.tape.right_spin);
    fittings(&painter, &art);
    rect
}

/// What gets written on the label: the file's name without the extension,
/// as somebody would have written it in biro.
pub fn written_name(file: &str) -> String {
    let stem = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string());
    stem.trim().to_string()
}

/// How far through the whole tape, by playing time rather than by block, so
/// the reels wind on smoothly instead of jumping as each block goes by.
pub fn overall_progress(tape: &crate::tape::Tape) -> f32 {
    tape.progress()
}

/// Maps the artwork's coordinates onto the screen.
struct Art {
    origin: Pos2,
    scale: f32,
}

impl Art {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.scale
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.at(x0, y0), self.at(x1, y1))
    }
    fn len(&self, n: f32) -> f32 {
        n * self.scale
    }
    fn round(&self, n: f32) -> CornerRadius {
        CornerRadius::same(self.len(n).round().clamp(0.0, 255.0) as u8)
    }
    fn stroke(&self, width: f32, colour: Color32) -> Stroke {
        Stroke::new((self.len(width)).max(0.7), colour)
    }
}

fn grey(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

/// The plastic shell, with the moulded highlight inside its edge.
fn shell(painter: &egui::Painter, art: &Art) {
    let body = art.rect(4.4, 11.4, 495.5, 323.3);
    painter.rect_filled(body, art.round(14.0), grey(191));
    // The shell is lighter at the top than the bottom. The gradient is inset
    // by the corner radius so it cannot show outside the rounded edge.
    let inset = art.len(14.0);
    let inner = Rect::from_min_max(
        body.min + Vec2::new(inset, 0.0),
        body.max - Vec2::new(inset, 0.0),
    );
    gradient(painter, inner, grey(207), grey(168));
    painter.rect_stroke(
        body,
        art.round(14.0),
        art.stroke(1.5, grey(141)),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        art.rect(6.6, 14.1, 493.3, 322.1),
        art.round(11.0),
        art.stroke(1.0, Color32::from_white_alpha(100)),
        egui::StrokeKind::Inside,
    );
}

/// A vertical gradient, as a two-triangle mesh.
fn gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (i, (pos, colour)) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ]
    .into_iter()
    .enumerate()
    {
        mesh.colored_vertex(pos, colour);
        let _ = i;
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// The paper label: ruled lines with the tape's name written on them, the
/// brand stripes, and the printed small print.
fn label(painter: &egui::Painter, art: &Art, name: &str) {
    let paper = art.rect(30.2, 36.5, 467.7, 229.5);
    painter.rect_filled(paper, art.round(5.0), grey(233));
    let inset = art.len(5.0);
    gradient(
        painter,
        Rect::from_min_max(
            paper.min + Vec2::new(inset, 0.0),
            paper.max - Vec2::new(inset, 0.0),
        ),
        grey(244),
        grey(217),
    );
    painter.rect_stroke(
        paper,
        art.round(5.0),
        art.stroke(1.0, grey(169)),
        egui::StrokeKind::Inside,
    );

    for y in RULE_Y {
        painter.line_segment(
            [art.at(RULE_X.0, y), art.at(RULE_X.1, y)],
            art.stroke(1.2, Color32::from_rgb(157, 157, 153)),
        );
    }
    title(painter, art, name);

    // The brand stripes across the middle.
    painter.rect_filled(
        art.rect(30.2, 110.0, 467.7, 119.0),
        CornerRadius::ZERO,
        Color32::from_rgb(202, 0, 0),
    );
    painter.rect_filled(
        art.rect(30.2, 119.0, 467.7, 132.6),
        CornerRadius::ZERO,
        grey(17),
    );

    // Type and noise-reduction printing, in the label's lower half.
    let ink = Color32::from_rgb(44, 44, 44);
    let small = FontId::proportional(art.len(10.0));
    painter.text(
        art.at(40.9, 143.9),
        Align2::LEFT_TOP,
        "Normal Bias",
        small.clone(),
        ink,
    );
    painter.text(
        art.at(40.6, 154.7),
        Align2::LEFT_TOP,
        "EQ·120μs",
        small.clone(),
        ink,
    );
    painter.text(
        art.at(405.0, 141.9),
        Align2::LEFT_TOP,
        "Noise",
        small.clone(),
        ink,
    );
    painter.text(
        art.at(404.6, 151.9),
        Align2::LEFT_TOP,
        "Reduction",
        small,
        ink,
    );
    for (y, text) in [(155.7, "IN"), (171.7, "OUT")] {
        let box_ = art.rect(405.3, y, 415.6, y + 11.1);
        painter.rect_filled(box_, CornerRadius::ZERO, Color32::WHITE);
        painter.rect_stroke(
            box_,
            CornerRadius::ZERO,
            art.stroke(1.2, ink),
            egui::StrokeKind::Inside,
        );
        painter.text(
            art.at(419.6, y - 1.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(art.len(11.0)),
            ink,
        );
    }

    // The type and length, in the maker's own colours.
    painter.text(
        art.at(102.3, 222.3),
        Align2::LEFT_BOTTOM,
        "D",
        FontId::proportional(art.len(30.0)),
        grey(17),
    );
    painter.rect_filled(
        art.rect(129.2, 210.2, 136.3, 214.2),
        CornerRadius::ZERO,
        Color32::BLACK,
    );
    painter.text(
        art.at(141.3, 222.3),
        Align2::LEFT_BOTTOM,
        "C60",
        FontId::proportional(art.len(30.0)),
        Color32::from_rgb(0, 17, 255),
    );
    painter.text(
        art.at(208.7, 220.3),
        Align2::LEFT_BOTTOM,
        "MADE IN JAPAN",
        FontId::proportional(art.len(6.5)),
        Color32::from_rgb(68, 68, 68),
    );

    // The maker's diamond, a square stood on its corner.
    let d = art.at(311.9, 210.0);
    let arm = art.len(10.6);
    painter.add(Shape::closed_line(
        vec![
            d + Vec2::new(0.0, -arm),
            d + Vec2::new(arm, 0.0),
            d + Vec2::new(0.0, arm),
            d + Vec2::new(-arm, 0.0),
        ],
        art.stroke(3.0, grey(17)),
    ));
    painter.text(
        art.at(316.0, 221.7),
        Align2::LEFT_BOTTOM,
        "FEK",
        FontId::proportional(art.len(26.0)),
        grey(17),
    );
}

/// The tape's name, written across the ruled lines. Long names are set smaller
/// rather than allowed to run off the label.
fn title(painter: &egui::Painter, art: &Art, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let room = art.len(RULE_X.1 - RULE_X.0) * 0.94;
    let mut size = art.len(26.0);
    let ink = Color32::from_rgb(31, 63, 143);
    for _ in 0..8 {
        let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(size), ink);
        if galley.rect.width() <= room || size <= art.len(9.0) {
            let centre = art.at((RULE_X.0 + RULE_X.1) / 2.0, RULE_Y[2]);
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

/// The two tape packs, seen through the window. They are drawn as full discs
/// and clipped to the window, which is what the shell does to them.
fn packs(painter: &egui::Painter, art: &Art, left: f32, right: f32) {
    let window = art.rect(WINDOW.0, WINDOW.1, WINDOW.2, WINDOW.3);
    let painter = painter.with_clip_rect(window);
    painter.rect_filled(window, CornerRadius::ZERO, grey(20));
    for (hub, pack) in [(LEFT_HUB, left), (RIGHT_HUB, right)] {
        let centre = art.at(hub.0, hub.1);
        let r = art.len(REEL_R * pack);
        painter.circle_filled(centre, r, Color32::from_rgb(76, 36, 24));
        // A couple of turns showing on the outside of the pack.
        for ring in [0.94, 0.88] {
            painter.circle_stroke(
                centre,
                r * ring,
                Stroke::new(1.0, Color32::from_rgb(58, 27, 18)),
            );
        }
    }
}

/// The grey surround the window is cut into.
fn window_bezel(painter: &egui::Painter, art: &Art) {
    let outer = art.rect(101.7, 105.7, 397.8, 191.0);
    let inner = art.rect(WINDOW.0, WINDOW.1, WINDOW.2, WINDOW.3);
    // Drawn as a frame, so the packs behind stay visible through the middle.
    for part in [
        Rect::from_min_max(outer.min, Pos2::new(outer.max.x, inner.min.y)),
        Rect::from_min_max(Pos2::new(outer.min.x, inner.max.y), outer.max),
        Rect::from_min_max(
            Pos2::new(outer.min.x, inner.min.y),
            Pos2::new(inner.min.x, inner.max.y),
        ),
        Rect::from_min_max(
            Pos2::new(inner.max.x, inner.min.y),
            Pos2::new(outer.max.x, inner.max.y),
        ),
    ] {
        painter.rect_filled(part, CornerRadius::ZERO, grey(194));
    }
    painter.rect_stroke(
        outer,
        art.round(8.5),
        art.stroke(1.0, grey(160)),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        inner,
        art.round(3.0),
        art.stroke(1.0, grey(160)),
        egui::StrokeKind::Outside,
    );
}

/// A drive cog: the toothed hub the recorder turns. `angle` is in radians.
fn cog(painter: &egui::Painter, art: &Art, hub: (f32, f32), angle: f32) {
    let centre = art.at(hub.0, hub.1);
    painter.circle_filled(centre, art.len(32.9), grey(30));
    painter.circle_filled(centre, art.len(30.0), Color32::from_rgb(242, 242, 239));
    painter.circle_stroke(
        centre,
        art.len(30.0),
        art.stroke(1.0, Color32::from_rgb(194, 194, 190)),
    );
    painter.circle_filled(
        centre,
        art.len(26.9),
        Color32::from_rgba_unmultiplied(133, 133, 133, 97),
    );
    painter.circle_stroke(
        centre,
        art.len(32.9),
        art.stroke(2.5, Color32::from_rgb(132, 132, 127)),
    );

    // Six teeth, evenly spaced, turning with the hub.
    let (tw, th) = (art.len(4.9) / 2.0, art.len(6.9) / 2.0);
    let radius = art.len(24.5);
    for i in 0..6 {
        let a = angle + i as f32 * std::f32::consts::TAU / 6.0;
        let (sin, cos) = a.sin_cos();
        let mid = centre + Vec2::new(sin * radius, -cos * radius);
        // The tooth is a small rectangle, turned to face out from the centre.
        let along = Vec2::new(sin, -cos) * th;
        let across = Vec2::new(cos, sin) * tw;
        painter.add(Shape::convex_polygon(
            vec![
                mid - along - across,
                mid - along + across,
                mid + along + across,
                mid + along - across,
            ],
            Color32::from_rgb(242, 242, 239),
            Stroke::NONE,
        ));
    }
}

/// The parts the recorder grips: the write-protect side marking, the drive and
/// sensing holes, the case screws, and the outline of the moulding underneath.
fn fittings(painter: &egui::Painter, art: &Art) {
    let side = art.rect(41.6, 49.5, 69.9, 78.6);
    painter.rect_filled(side, art.round(3.5), grey(59));
    painter.rect_stroke(
        side,
        art.round(3.5),
        art.stroke(1.0, grey(36)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        side.center(),
        Align2::CENTER_CENTER,
        "A",
        FontId::proportional(art.len(22.0)),
        Color32::from_rgb(242, 242, 242),
    );

    for (x, y, r) in [
        (129.3, 305.2, 10.9),
        (368.3, 305.2, 10.9),
        (179.5, 297.7, 9.7),
        (318.5, 297.7, 9.7),
    ] {
        painter.circle_filled(art.at(x, y), art.len(r), grey(22));
    }

    for (x, y) in [
        (483.3, 308.3),
        (19.0, 309.3),
        (249.4, 271.0),
        (17.5, 25.4),
        (482.3, 25.1),
    ] {
        let centre = art.at(x, y);
        let r = art.len(10.3);
        painter.circle_filled(centre, r, grey(156));
        painter.circle_stroke(centre, r, art.stroke(1.0, grey(111)));
        let arm = r * 0.66;
        for (dx, dy) in [(arm, 0.0), (0.0, arm)] {
            painter.line_segment(
                [centre - Vec2::new(dx, dy), centre + Vec2::new(dx, dy)],
                art.stroke(1.2, grey(92)),
            );
        }
    }

    let outline = [
        art.at(424.5, 323.1),
        art.at(76.7, 323.1),
        art.at(93.9, 246.4),
        art.at(407.3, 246.4),
    ];
    painter.add(Shape::closed_line(
        outline.to_vec(),
        art.stroke(0.82, grey(199)),
    ));
}
