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

/// The band behind the row you asked to be taken to. Dark enough that the
/// green text still reads, and nothing like the amber bar under the current
/// instruction, which is a different question being answered.
pub const MARK: Color32 = Color32::from_rgb(0x16, 0x3c, 0x52);

/// The bands behind the listing, one per block, so a routine and the table
/// next to it are told apart at a glance. Two shades per kind and taken in
/// turn, so neighbours differ; dark enough that the text over them is still
/// the text and not a colour scheme.
/// How tall every control is, and so how tall a row of the listing is.
pub const CONTROL_H: f32 = 22.0;

pub const CODE_BANDS: [Color32; 2] = [
    Color32::from_rgb(0x16, 0x33, 0x26),
    Color32::from_rgb(0x14, 0x2c, 0x40),
];
pub const DATA_BANDS: [Color32; 2] = [
    Color32::from_rgb(0x42, 0x2d, 0x16),
    Color32::from_rgb(0x38, 0x1e, 0x36),
];

/// The band behind a row of the listing: which block it is in, and what that
/// block holds. Taken in turn so that neighbours differ, and from a different
/// pair for code and for data so the two are told apart as well.
pub fn band(index: usize, kind: crate::blocks::Kind) -> Color32 {
    let bands = match kind {
        crate::blocks::Kind::Code => CODE_BANDS,
        crate::blocks::Kind::Data => DATA_BANDS,
    };
    bands[index % bands.len()]
}

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
    // egui expands the rect it paints by a point on each side under the
    // pointer. That is only the painting, not the layout, but the outline
    // still swells as the pointer crosses it. It does not here.
    w.hovered.expansion = 0.0;

    w.active.bg_fill = CASE_LIGHT;
    w.active.weak_bg_fill = CASE_LIGHT;
    w.active.bg_stroke = Stroke::new(1.0, EDGE);
    w.active.fg_stroke = Stroke::new(1.0, WHITE);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = CONTROL;
    w.open.weak_bg_fill = CONTROL;
    w.open.bg_stroke = Stroke::new(1.0, EDGE);
    w.open.fg_stroke = Stroke::new(1.0, INK);
    w.open.corner_radius = radius;
    w.open.expansion = 0.0;

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
        // Every control the same height. egui sizes a button to at least
        // `interact_size` and a toggle to its text plus padding, so a row of
        // them came out at 18, 20 and 22 points, each sitting at a different
        // height in the row. Nothing moved when the pointer arrived, but the
        // outline appeared two points off from its neighbours', which is what
        // "the buttons jump on hover" actually was.
        style.spacing.interact_size.y = CONTROL_H;
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

/// The colour of a slider's track: the green of the handle, taken right down,
/// so the part still to be dragged through reads as unlit rather than empty.
const SLIDER_TRACK: Color32 = Color32::from_rgb(0x12, 0x3a, 0x2c);

/// Style a slider as an instrument's control: a green handle running along a
/// sunken track, with the part behind it filled in.
///
/// Kept apart from [`slider`] so what the styling does can be asserted on
/// without a window to draw into.
pub fn slider_visuals(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = GREEN;
    visuals.slider_trailing_fill = true;
    for state in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        state.bg_fill = SLIDER_TRACK;
        state.fg_stroke = Stroke::new(1.5, GREEN);
    }
    // The handle takes its colour from the interacted state's fill, so it
    // lights up under the pointer instead of matching the track.
    visuals.widgets.hovered.bg_fill = GREEN;
    visuals.widgets.active.bg_fill = GREEN;
}

/// A slider in the emulator's own style: see [`slider_visuals`]. Every slider
/// the user reaches for goes through here, so they all look alike.
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    sunken()
        .show(ui, |ui| {
            slider_visuals(&mut ui.style_mut().visuals);
            ui.add(slider.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }))
        })
        .inner
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

/// A dropdown drawn as a button with a menu under it.
///
/// egui's own `ComboBox` builds itself inside a nested `Ui`, and a nested `Ui`
/// is placed at the top of the row rather than centred in it, so a combo sits
/// a couple of points below the buttons beside it however the row is laid out.
/// A button is one widget and lines up with the rest of the toolbar.
pub fn dropdown<R>(
    ui: &mut egui::Ui,
    width: f32,
    selected: impl Into<String>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    let text = format!("{}  \u{25be}", selected.into());
    let response = ui.add(egui::Button::new(text).min_size(size));
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(contents)
        .map(|inner| inner.inner)
}

/// What the run/pause button says in each state. Kept as constants so the
/// button can be measured against both.
pub const RUN_LABEL: &str = "▶ Run";
pub const PAUSE_LABEL: &str = "⏸ Pause";

/// The run/pause button, wide enough for whichever word is longer.
///
/// Sized for both states rather than the one being shown, so the controls
/// after it do not jump sideways every time the machine is paused.
pub fn run_pause_button(ui: &mut egui::Ui, running: bool) -> egui::Response {
    // Laid out the way the button itself lays its label out, rather than
    // through the painter: the two disagree about how wide the symbols are,
    // and measuring the wrong one leaves the button a few points short.
    let widest = [RUN_LABEL, PAUSE_LABEL]
        .into_iter()
        .map(|text| {
            egui::WidgetText::from(text)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .rect
                .width()
        })
        .fold(0.0_f32, f32::max);
    let size = egui::vec2(
        widest + 2.0 * ui.spacing().button_padding.x,
        ui.spacing().interact_size.y,
    );
    let label = if running { PAUSE_LABEL } else { RUN_LABEL };
    ui.add(egui::Button::new(label).min_size(size))
}

/// A toggle the same height as a button.
///
/// egui sizes a button to at least `interact_size`, and a selectable label to
/// its text plus padding, so a row of the two comes out at 22, 20 and 18
/// points and every one of them sits at a different height. Nothing moves when
/// the pointer arrives, but the outline that appears is a couple of points off
/// from its neighbours', which reads exactly like the button jumping.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool, text: &str) -> egui::Response {
    let mut response = selectable(ui, *on, text);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
}

/// A button that shows whether it is the one in force, without flipping a flag
/// of its own: one of a set where pressing one chooses it.
pub fn selectable(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let height = button_height(ui);
    ui.add(
        egui::Button::selectable(on, text)
            // Framed whether it is on or off. egui leaves the frame off a
            // selectable button while it is unselected and the pointer is
            // elsewhere, and puts it back the moment the pointer arrives: the
            // stroke is a point on each side, so the button grew by two and
            // shoved every control after it along the row. That is what "the
            // buttons move on hover" was.
            .frame_when_inactive(true)
            .min_size(egui::vec2(0.0, height)),
    )
}

/// How tall a button comes out, so anything sitting beside one can match it.
pub fn button_height(ui: &egui::Ui) -> f32 {
    // What a plain button comes out at: egui gives one a minimum size of
    // `interact_size`, and that is the height everything in a row of controls
    // has to match. Working it out from the text and the padding instead
    // overshoots by a point, which is just as visible as being short by two.
    ui.spacing().interact_size.y
}

/// A divider between groups of controls.
///
/// egui's own separator takes the height of the row it is in and grows it a
/// little as it goes, which walks everything placed afterwards downwards. This
/// one is a fixed height, so a row of controls stays level.
pub fn divider(ui: &mut egui::Ui) {
    let height = ui.spacing().interact_size.y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, height), egui::Sense::hover());
    let x = rect.center().x;
    ui.painter().line_segment(
        [
            egui::pos2(x, rect.top() + 2.0),
            egui::pos2(x, rect.bottom() - 2.0),
        ],
        Stroke::new(1.0, EDGE),
    );
}
