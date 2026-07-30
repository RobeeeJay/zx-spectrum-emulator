//! Tape window: block list, transport controls and an oscilloscope showing
//! the EAR waveform as it is played.

use eframe::egui;
use egui::{Color32, Pos2, RichText, Sense, Stroke, Vec2};

use crate::machine::CPU_HZ;
use crate::ui::App;

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
    /// Off by default: loading a tape should not start it moving.
    pub auto_play_on_load: bool,
    /// Keep the block being played on screen as the tape advances.
    pub follow_current: bool,
    /// One-shot request to scroll to the current block (after a load or skip).
    pub scroll_to_current: bool,
    /// Block the list was showing last frame, to notice when it advances.
    pub last_block: Option<usize>,
    /// The block the list last asked to scroll into view, for tests and for
    /// anyone wondering why the list jumped.
    pub scroll_requested_for: Option<usize>,
}

impl Default for TapeWindowState {
    fn default() -> Self {
        TapeWindowState {
            window_us: 2000.0,
            trigger: Trigger::Rising,
            auto_play_on_load: false,
            follow_current: true,
            scroll_to_current: true,
            last_block: None,
            scroll_requested_for: None,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if app.spec.bus.tape.is_none() {
        ui.heading("No tape loaded");
        ui.label("File ▸ Load tape… opens a .tzx or .tap file.");
        return;
    }

    transport(app, ui);
    ui.separator();
    scope(app, ui);
    ui.separator();
    block_list(app, ui);
}

fn transport(app: &mut App, ui: &mut egui::Ui) {
    let now = app.spec.bus.total_t();
    let mut action: Option<i32> = None;
    let (name, playing, block, count, pulses, stopped_by_block) = {
        let t = app.spec.bus.tape.as_ref().unwrap();
        (
            t.name.clone(),
            t.playing,
            t.block,
            t.blocks.len(),
            t.pulses,
            t.stopped_by_block,
        )
    };

    ui.label(RichText::new(&name).strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("|◀ Start").on_hover_text("Back to the start of the tape").clicked() {
            let t = app.spec.bus.tape.as_mut().unwrap();
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
            let t = app.spec.bus.tape.as_mut().unwrap();
            if playing {
                t.stop();
            } else {
                t.play(now);
            }
        }
        if ui.button("■ Stop").clicked() {
            let t = app.spec.bus.tape.as_mut().unwrap();
            t.stop();
        }
        if ui
            .button("▶▶ Fast forward")
            .on_hover_text("Next section")
            .clicked()
        {
            action = Some(1);
        }
        ui.separator();
        ui.checkbox(&mut app.spec.bus.tape_boost, "Boost speed while playing")
            .on_hover_text("Runs the CPU at 8x while the tape moves, so loading is quick.");
        ui.checkbox(&mut app.tape.auto_play_on_load, "Play on load")
            .on_hover_text("Off by default: a freshly loaded tape waits for Play.");
    });

    if let Some(dir) = action {
        let t = app.spec.bus.tape.as_mut().unwrap();
        let target = t.next_data_block(dir);
        t.seek(target);
        if t.playing {
            t.play(now);
        }
        app.tape.scroll_to_current = true;
    }

    let progress = if count == 0 {
        0.0
    } else {
        block as f32 / count as f32
    };
    ui.add(
        egui::ProgressBar::new(progress)
            .text(format!("block {} / {count}", (block + 1).min(count))),
    );

    // And how far through the block itself.
    let (within, description, seconds) = {
        let t = app.spec.bus.tape.as_ref().unwrap();
        let within = t.block_progress();
        let (description, seconds) = match t.blocks.get(t.block) {
            Some(b) => (
                b.describe(),
                b.duration_t() as f64 / crate::machine::CPU_HZ,
            ),
            None => (String::new(), 0.0),
        };
        (within, description, seconds)
    };
    match within {
        Some(fraction) => {
            let left = seconds * (1.0 - fraction as f64);
            ui.add(egui::ProgressBar::new(fraction).text(format!(
                "{}  {:.0}%  ({} left)",
                description.split("  ").next().unwrap_or(&description).trim(),
                fraction * 100.0,
                crate::profiler::format_duration(left)
            )))
            .on_hover_text(format!(
                "{description} — {} in total",
                crate::profiler::format_duration(seconds)
            ));
        }
        None => {
            ui.add_enabled(
                false,
                egui::ProgressBar::new(0.0).text("this block takes no time to play"),
            );
        }
    }
    ui.label(
        RichText::new(format!(
            "{}   {} pulses played",
            if playing {
                "playing"
            } else if stopped_by_block {
                "stopped by tape block"
            } else {
                "stopped"
            },
            pulses
        ))
        .monospace(),
    );
}

/// Draw the EAR waveform, triggered on an edge so the display stands still.
fn scope(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Scope:");
        ui.add(
            egui::Slider::new(&mut app.tape.window_us, 50.0..=40000.0)
                .logarithmic(true)
                .text("µs/sweep"),
        );
        ui.selectable_value(&mut app.tape.trigger, Trigger::Rising, "Trigger rising");
        ui.selectable_value(&mut app.tape.trigger, Trigger::Falling, "Trigger falling");
        ui.selectable_value(&mut app.tape.trigger, Trigger::Off, "Free run");
    });

    let now = app.spec.bus.total_t();
    let window_t = ((app.tape.window_us as f64) * CPU_HZ / 1_000_000.0).max(1.0) as u64;
    let tape = app.spec.bus.tape.as_ref().unwrap();

    let height = 150.0;
    let (rect, _resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(8, 14, 10));

    let y_high = rect.top() + 18.0;
    let y_low = rect.bottom() - 18.0;
    let y_mid = (y_high + y_low) * 0.5;
    let grid = Stroke::new(1.0, Color32::from_rgb(24, 48, 32));
    for i in 0..=10 {
        let x = rect.left() + rect.width() * i as f32 / 10.0;
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], grid);
    }
    // The trigger threshold: the EAR line is a single bit, so mid-scale.
    painter.line_segment(
        [
            Pos2::new(rect.left(), y_mid),
            Pos2::new(rect.right(), y_mid),
        ],
        Stroke::new(1.0, Color32::from_rgb(90, 70, 20)),
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

    let trace = Stroke::new(1.5, Color32::from_rgb(120, 255, 140));
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
        [Pos2::new(x, y_of(level)), Pos2::new(rect.right(), y_of(level))],
        trace,
    );

    // Trigger marker.
    if triggered.is_some() {
        painter.line_segment(
            [
                Pos2::new(rect.left() + 1.0, rect.top()),
                Pos2::new(rect.left() + 1.0, rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgb(255, 190, 0)),
        );
        painter.text(
            Pos2::new(rect.left() + 4.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            "trig",
            egui::FontId::monospace(10.0),
            Color32::from_rgb(255, 190, 0),
        );
    }
    painter.text(
        Pos2::new(rect.right() - 4.0, rect.top() + 2.0),
        egui::Align2::RIGHT_TOP,
        format!("{:.0} µs/div", app.tape.window_us / 10.0),
        egui::FontId::monospace(10.0),
        Color32::from_rgb(120, 200, 140),
    );
    if !drew && !tape.playing {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no signal",
            egui::FontId::monospace(12.0),
            Color32::from_rgb(70, 110, 80),
        );
    }
}

/// Whether the current row should be scrolled into view: either because
/// something asked for it, or because following is on and it has gone
/// off screen.
pub fn needs_scroll(forced: bool, follow: bool, row_visible: bool) -> bool {
    forced || (follow && !row_visible)
}

fn block_list(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Blocks").strong());
        ui.checkbox(&mut app.tape.follow_current, "Follow playing block");
    });
    ui.small("Click a block to move the tape there.");

    let current = app.spec.bus.tape.as_ref().unwrap().block;
    // Scroll whenever playback moves on to another block.
    if app.tape.last_block != Some(current) {
        app.tape.last_block = Some(current);
        if app.tape.follow_current {
            app.tape.scroll_to_current = true;
        }
    }
    let rows: Vec<(usize, String, bool)> = app
        .spec
        .bus
        .tape
        .as_ref()
        .unwrap()
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, b.describe(), b.is_data()))
        .collect();

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
                    rich = rich.color(Color32::from_gray(140));
                }
                // A selectable row rather than a label: it highlights the block
                // being played and behaves like the clickable thing it is.
                let resp = ui.selectable_label(is_current, rich);
                if is_current {
                    let visible = ui.clip_rect().contains_rect(resp.rect);
                    if needs_scroll(
                        app.tape.scroll_to_current,
                        app.tape.follow_current,
                        visible,
                    ) {
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
        let now = app.spec.bus.total_t();
        let t = app.spec.bus.tape.as_mut().unwrap();
        t.seek(i);
        if t.playing {
            t.play(now);
        }
    }
}
