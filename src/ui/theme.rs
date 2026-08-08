//! The look of the thing: dark plastic, LCD readouts and the Spectrum's own
//! seven colours, following `zx-ux-mockup.html`.
//!
//! Everything the windows draw by hand — the oscilloscope trace, the heat map
//! legend, the profiler bars — takes its colours from here rather than from a
//! literal, so the palette stays in one place.

use egui::{Color32, CornerRadius, FontId, Frame, Margin, Shadow, Stroke, Visuals};

// The machine's own palette, as the mockup names them.
pub const INK: Color32 = Color32::from_rgb(0xe8, 0xe4, 0xd8);
pub const DIM: Color32 = Color32::from_rgb(0x9a, 0x95, 0x8a);
pub const CASE: Color32 = Color32::from_rgb(0x24, 0x22, 0x20);
pub const CASE_DARK: Color32 = Color32::from_rgb(0x17, 0x16, 0x15);
pub const CASE_LIGHT: Color32 = Color32::from_rgb(0x33, 0x2f, 0x2c);
pub const PANEL: Color32 = Color32::from_rgb(0x1c, 0x1a, 0x18);
/// Widget faces, and the near-black they are outlined in.
pub const CONTROL: Color32 = Color32::from_rgb(0x21, 0x1f, 0x1c);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x3a, 0x36, 0x32);
pub const EDGE: Color32 = Color32::from_rgb(0x10, 0x0f, 0x0e);

pub const LCD_BG: Color32 = Color32::from_rgb(0x0c, 0x1f, 0x14);
pub const LCD_FG: Color32 = Color32::from_rgb(0x5d, 0xff, 0x9e);
/// The dim grid drawn inside an LCD panel.
pub const LCD_GRID: Color32 = Color32::from_rgb(0x18, 0x30, 0x20);

pub const AMBER: Color32 = Color32::from_rgb(0xff, 0xb2, 0x38);
pub const RED: Color32 = Color32::from_rgb(0xe0, 0x43, 0x3c);
pub const BLUE: Color32 = Color32::from_rgb(0x20, 0x62, 0xff);
pub const MAGENTA: Color32 = Color32::from_rgb(0xff, 0x33, 0xe0);
pub const GREEN: Color32 = Color32::from_rgb(0x0f, 0xbb, 0x4d);
pub const CYAN: Color32 = Color32::from_rgb(0x22, 0xe0, 0xe0);
pub const YELLOW: Color32 = Color32::from_rgb(0xf4, 0xe2, 0x30);
pub const WHITE: Color32 = Color32::from_rgb(0xf4, 0xf2, 0xea);

/// Text on a lit control: dark, so cyan reads as a lamp rather than a glare.
pub const ON_LIT: Color32 = Color32::from_rgb(0x04, 0x21, 0x1f);

/// The seven border colours, in the order the mockup's rainbow uses them.
pub const RAINBOW: [Color32; 7] = [BLUE, RED, MAGENTA, GREEN, CYAN, YELLOW, WHITE];

/// Apply the theme to a context. Cheap enough to call once per frame, but the
/// app only does it when something could have reset it.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = PANEL;
    visuals.window_fill = CASE;
    visuals.extreme_bg_color = EDGE; // text edits, progress bar troughs
    visuals.faint_bg_color = CASE_LIGHT;
    visuals.code_bg_color = LCD_BG;
    visuals.window_stroke = Stroke::new(1.0, EDGE);
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);

    // A lit control is cyan with dark text, like the mockup's active segment.
    visuals.selection.bg_fill = CYAN;
    visuals.selection.stroke = Stroke::new(1.0, ON_LIT);
    visuals.hyperlink_color = CYAN;
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = RED;

    let w = &mut visuals.widgets;
    let radius = CornerRadius::same(6);
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = CONTROL;
    w.inactive.weak_bg_fill = CONTROL;
    w.inactive.bg_stroke = Stroke::new(1.0, EDGE);
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = CONTROL_HOVER;
    w.hovered.weak_bg_fill = CONTROL_HOVER;
    // The same outline in every state. Letting one appear on hover reads as
    // the control growing by a pixel under the pointer.
    w.hovered.bg_stroke = Stroke::new(1.0, EDGE);
    w.hovered.fg_stroke = Stroke::new(1.0, WHITE);
    w.hovered.corner_radius = radius;

    w.active.bg_fill = CASE_LIGHT;
    w.active.weak_bg_fill = CASE_LIGHT;
    w.active.bg_stroke = Stroke::new(1.0, EDGE);
    w.active.fg_stroke = Stroke::new(1.0, WHITE);
    w.active.corner_radius = radius;

    w.open.bg_fill = CONTROL;
    w.open.weak_bg_fill = CONTROL;
    w.open.bg_stroke = Stroke::new(1.0, EDGE);
    w.open.fg_stroke = Stroke::new(1.0, INK);
    w.open.corner_radius = radius;

    // The app is dark whatever the desktop is set to: it is a picture of a
    // machine, not a document.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        // The mockup is monospace throughout, which suits a machine whose own
        // display was a 32-column character grid.
        style.override_font_id = Some(FontId::monospace(12.0));
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.menu_margin = Margin::same(6);
        // No banded rows: the mockup's panels are flat, and stripes across an
        // LCD readout look like a fault rather than a decoration.
        style.visuals.striped = false;
    });
}

/// A sunken LCD panel, for readouts that stand in for the real machine's
/// displays: registers, the oscilloscope, the memory dump.
pub fn lcd() -> Frame {
    Frame::new()
        .fill(LCD_BG)
        .inner_margin(Margin::symmetric(10, 8))
        .corner_radius(CornerRadius::same(5))
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x04, 0x11, 0x08)))
}

/// A sunken well for a control to sit in, so a knob reads as being set into
/// the case rather than floating on it.
pub fn sunken() -> Frame {
    Frame::new()
        .fill(EDGE)
        .inner_margin(Margin::symmetric(6, 2))
        .corner_radius(CornerRadius::same(5))
        .stroke(Stroke::new(1.0, Color32::from_black_alpha(120)))
}

/// A raised slab of case plastic, for grouping controls.
pub fn slab() -> Frame {
    Frame::new()
        .fill(CASE)
        .inner_margin(Margin::symmetric(10, 8))
        .corner_radius(CornerRadius::same(6))
        .stroke(Stroke::new(1.0, EDGE))
}

/// The label above a group of controls: small, dim and shouty, as on a
/// machine's silk-screened front panel.
pub fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .color(DIM)
            .size(10.0),
    );
}

/// The seven-colour flash the machine wears on its case. Drawn small, at the
/// end of the status line, as a maker's mark rather than decoration.
pub fn rainbow(ui: &mut egui::Ui) {
    let bar = egui::vec2(7.0, 10.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(bar.x * RAINBOW.len() as f32, bar.y),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    for (i, colour) in RAINBOW.iter().enumerate() {
        let x = rect.left() + i as f32 * bar.x;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), bar),
            0.0,
            *colour,
        );
    }
}
