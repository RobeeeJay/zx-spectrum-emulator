//! Tape window: block list, transport controls and an oscilloscope showing
//! the EAR waveform as it is played.

use eframe::egui;
use egui::{Color32, Pos2, RichText, Sense, Stroke, Vec2};

use crate::ui::{theme, App};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Trigger {
    Rising,
    Falling,
    Off,
}

pub struct TapeWindowState {
    /// Width of the oscilloscope sweep, in microseconds.
    pub window_us: f32,
    pub trigger: Trigger,
    /// One-shot request to scroll to the current block (after a load or skip).
    pub scroll_to_current: bool,
    /// Block the list was showing last frame, to notice when it advances.
    pub last_block: Option<usize>,
    /// How far each cog has turned, in radians. They wind on only while the
    /// tape is playing, so the picture freezes when it stops.
    pub left_spin: f32,
    pub right_spin: f32,
    /// When they were last wound on, by the interface's clock.
    pub spun_at: f64,
    /// The block the list last asked to scroll into view, for tests and for
    /// anyone wondering why the list jumped.
    pub scroll_requested_for: Option<usize>,
}

impl Default for TapeWindowState {
    fn default() -> Self {
        TapeWindowState {
            window_us: 16000.0,
            trigger: Trigger::Rising,
            scroll_to_current: true,
            last_block: None,
            left_spin: 0.0,
            right_spin: 0.0,
            spun_at: 0.0,
            scroll_requested_for: None,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if app.tape_ref().is_none() {
        ui.heading("No tape loaded");
        ui.label("File ▸ Load tape… opens a .tzx or .tap file, or a ZX81 .p, .81 or .p81.");
        return;
    }

    // The window is a fixed width and any height, so the contents scroll
    // rather than being squeezed when it is made short.
    egui::ScrollArea::vertical()
        .id_salt("tape-window")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            transport(app, ui);
            ui.separator();
            // The deck itself, above the trace it produces.
            ui.vertical_centered(|ui| {
                crate::ui::cassette::ui(app, ui);
            });
            ui.separator();
            scope(app, ui);
            ui.separator();
            block_list(app, ui);
        });
}

fn transport(app: &mut App, ui: &mut egui::Ui) {
    let now = app.machine_t();
    let mut action: Option<i32> = None;
    let playing = app.tape_ref().is_some_and(|t| t.playing);

    ui.horizontal_wrapped(|ui| {
        if ui
            .button("|◀ Start")
            .on_hover_text("Back to the start of the tape")
            .clicked()
        {
            let t = app.tape_mut().unwrap();
            t.rewind();
            t.edges.clear();
        }
        if ui
            .button("◀◀ Rewind")
            .on_hover_text("Previous section")
            .clicked()
        {
            action = Some(-1);
        }
        if ui
            .button(if playing { "⏸ Pause" } else { "▶ Play" })
            .clicked()
        {
            let t = app.tape_mut().unwrap();
            if playing {
                t.stop();
            } else {
                t.play(now);
            }
        }
        if ui.button("■ Stop").clicked() {
            let t = app.tape_mut().unwrap();
            t.stop();
        }
        if ui
            .button("▶▶ Forward")
            .on_hover_text("Next section")
            .clicked()
        {
            action = Some(1);
        }
        ui.separator();
        let boost = app.tape_boost();
        if ui
            .selectable_label(boost, "Max speed")
            .on_hover_text("Runs the CPU at 8x while the tape moves, so loading is quick.")
            .clicked()
        {
            *app.tape_boost_mut() = !boost;
        }
    });

    if let Some(dir) = action {
        let t = app.tape_mut().unwrap();
        let target = t.next_data_block(dir);
        t.seek(target);
        if t.playing {
            t.play(now);
        }
        app.tape.scroll_to_current = true;
    }
}

/// Draw the EAR waveform, triggered on an edge so the display stands still.
fn scope(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        // The sweep control sits in a frame, so it is taller than everything
        // else on the row. Claiming that height before anything is placed is
        // what lets the labels centre against it rather than sitting on top.
        ui.set_min_height(ui.spacing().interact_size.y + 8.0);
        theme::group_label(ui, "Scope");
        sweep_slider(&mut app.tape.window_us, ui);
        ui.separator();
        theme::group_label(ui, "Trigger");
        ui.selectable_value(&mut app.tape.trigger, Trigger::Rising, "Rising");
        ui.selectable_value(&mut app.tape.trigger, Trigger::Falling, "Falling");
        ui.selectable_value(&mut app.tape.trigger, Trigger::Off, "Free run");
    });

    let now = app.machine_t();
    let window_t = ((app.tape.window_us as f64) * app.cpu_hz() / 1_000_000.0).max(1.0) as u64;
    let tape = app.tape_ref().unwrap();

    let height = 150.0;
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::LCD_BG);

    let y_high = rect.top() + 18.0;
    let y_low = rect.bottom() - 18.0;
    let y_mid = (y_high + y_low) * 0.5;
    let grid = Stroke::new(1.0, theme::LCD_GRID);
    for i in 0..=10 {
        let x = rect.left() + rect.width() * i as f32 / 10.0;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            grid,
        );
    }
    // The trigger threshold: the EAR line is a single bit, so mid-scale.
    painter.line_segment(
        [
            Pos2::new(rect.left(), y_mid),
            Pos2::new(rect.right(), y_mid),
        ],
        Stroke::new(1.0, Color32::from_rgb(0x5a, 0x46, 0x14)),
    );

    // Pick the sweep start: the newest edge of the chosen slope that has a
    // whole sweep of signal after it, so the trace does not slide sideways.
    let want = match app.tape.trigger {
        Trigger::Rising => Some(true),
        Trigger::Falling => Some(false),
        Trigger::Off => None,
    };
    let triggered = want.and_then(|level| {
        tape.edges
            .iter()
            .rev()
            .find(|(t, l)| *l == level && t.saturating_add(window_t) <= now)
            .map(|(t, _)| *t)
    });
    let t0 = triggered.unwrap_or_else(|| now.saturating_sub(window_t));
    let t1 = t0 + window_t;

    let x_of = |t: u64| -> f32 {
        let frac = (t.saturating_sub(t0)) as f32 / window_t as f32;
        rect.left() + frac.clamp(0.0, 1.0) * rect.width()
    };
    let y_of = |level: bool| if level { y_high } else { y_low };

    // Level at the left edge of the sweep.
    let mut level = tape
        .edges
        .iter()
        .rev()
        .find(|(t, _)| *t <= t0)
        .map(|(_, l)| *l)
        .unwrap_or(false);

    let trace = Stroke::new(1.5, theme::LCD_FG);
    let mut x = rect.left();
    let mut drew = false;
    for &(t, l) in tape.edges.iter() {
        if t <= t0 {
            continue;
        }
        if t > t1 {
            break;
        }
        let ex = x_of(t);
        painter.line_segment(
            [Pos2::new(x, y_of(level)), Pos2::new(ex, y_of(level))],
            trace,
        );
        painter.line_segment([Pos2::new(ex, y_of(level)), Pos2::new(ex, y_of(l))], trace);
        level = l;
        x = ex;
        drew = true;
    }
    painter.line_segment(
        [
            Pos2::new(x, y_of(level)),
            Pos2::new(rect.right(), y_of(level)),
        ],
        trace,
    );

    // Trigger marker.
    if triggered.is_some() {
        painter.line_segment(
            [
                Pos2::new(rect.left() + 1.0, rect.top()),
                Pos2::new(rect.left() + 1.0, rect.bottom()),
            ],
            Stroke::new(1.0, theme::AMBER),
        );
        painter.text(
            Pos2::new(rect.left() + 4.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            "trig",
            egui::FontId::monospace(10.0),
            theme::AMBER,
        );
    }
    painter.text(
        Pos2::new(rect.right() - 4.0, rect.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.0} µs/div", app.tape.window_us / 10.0),
        egui::FontId::monospace(10.0),
        theme::GREEN,
    );
    if !drew && !tape.playing {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no signal",
            egui::FontId::monospace(12.0),
            Color32::from_rgb(0x2a, 0x5a, 0x3c),
        );
    }
}

/// Whether the current row should be scrolled into view: either because
/// something asked for it, or because it has gone off screen. The list always
/// follows the tape — a block list that does not show what is playing is not
/// worth having.
pub fn needs_scroll(forced: bool, row_visible: bool) -> bool {
    forced || !row_visible
}

fn block_list(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Blocks").strong());

    let current = app.tape_ref().unwrap().block;
    // Scroll whenever playback moves on to another block.
    if app.tape.last_block != Some(current) {
        app.tape.last_block = Some(current);
        app.tape.scroll_to_current = true;
    }
    let rows: Vec<(usize, String, bool)> = app
        .tape_ref()
        .unwrap()
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.describe(), b.is_data()))
        .collect();

    // How far through the block being played, to shade its row.
    let within = app.tape_ref().and_then(|t| t.block_progress());
    let (elapsed, total) = {
        let t = app.tape_ref().unwrap();
        match t.blocks.get(t.block) {
            Some(b) => {
                let total = b.duration_t() as f64 / app.cpu_hz();
                (total * within.unwrap_or(0.0) as f64, total)
            }
            None => (0.0, 0.0),
        }
    };

    let mut clicked = None;
    app.tape.scroll_requested_for = None;
    egui::ScrollArea::vertical()
        .id_salt("tape-blocks")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, text, is_data) in rows {
                let is_current = i == current;
                let mut rich = RichText::new(format!("{:3}  {text}", i + 1)).monospace();
                if !is_data && !is_current {
                    rich = rich.color(theme::DIM);
                }
                // A selectable row rather than a label: it highlights the block
                // being played and behaves like the clickable thing it is.
                let resp = ui.selectable_label(is_current, rich);
                if is_current {
                    if let Some(fraction) = within {
                        played_so_far(ui, resp.rect, fraction);
                        resp.clone().on_hover_text(format!(
                            "{:.0}% — {} of {}",
                            fraction * 100.0,
                            crate::profiler::format_duration(elapsed),
                            crate::profiler::format_duration(total)
                        ));
                    }
                    let visible = ui.clip_rect().contains_rect(resp.rect);
                    if needs_scroll(app.tape.scroll_to_current, visible) {
                        resp.scroll_to_me(Some(egui::Align::Center));
                        app.tape.scroll_requested_for = Some(i);
                    }
                }
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
        });
    app.tape.scroll_to_current = false;

    if let Some(i) = clicked {
        let now = app.machine_t();
        let t = app.tape_mut().unwrap();
        t.seek(i);
        if t.playing {
            t.play(now);
        }
    }
}

/// The part of a row covering what has already gone past the head.
pub fn played_rect(row: egui::Rect, fraction: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        row.min,
        egui::vec2(row.width() * fraction.clamp(0.0, 1.0), row.height()),
    )
}

/// Shade the part of a block's row that has already gone past the head.
///
/// Drawn over the row rather than beside it: the list is the only place a
/// block is named, so its own row is where its progress belongs.
fn played_so_far(ui: &egui::Ui, row: egui::Rect, fraction: f32) {
    let done = played_rect(row, fraction);
    let painter = ui.painter_at(row);
    painter.rect_filled(done, 2.0, theme::CYAN.gamma_multiply(0.22));
    if fraction > 0.0 {
        // A line at the head position, so slow blocks still show movement.
        // Amber against the blue of the bar, so the head is easy to pick out.
        painter.line_segment(
            [done.right_top(), done.right_bottom()],
            egui::Stroke::new(1.5, theme::AMBER),
        );
    }
}

/// The sweep control: a green handle running along a sunken track, so it reads
/// as a knob on an instrument rather than as a line of text with a dot on it.
fn sweep_slider(window_us: &mut f32, ui: &mut egui::Ui) {
    theme::sunken().show(ui, |ui| {
        let visuals = &mut ui.style_mut().visuals;
        // The part behind the handle fills in as it is dragged.
        visuals.selection.bg_fill = theme::GREEN;
        visuals.slider_trailing_fill = true;
        let track = egui::Color32::from_rgb(0x12, 0x3a, 0x2c);
        for state in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            state.bg_fill = track;
            state.fg_stroke = egui::Stroke::new(1.5, theme::GREEN);
        }
        visuals.widgets.hovered.bg_fill = theme::GREEN;
        visuals.widgets.active.bg_fill = theme::GREEN;
        ui.add(
            egui::Slider::new(window_us, 50.0..=40000.0)
                .logarithmic(true)
                .suffix(" µs")
                .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
        );
    });
}
