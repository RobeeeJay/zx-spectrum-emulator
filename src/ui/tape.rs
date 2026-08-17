//! Tape window: block list, transport controls and an oscilloscope showing
//! the EAR waveform as it is played.

use eframe::egui;
use egui::{Color32, Pos2, RichText, Sense, Stroke, Vec2};

use crate::ui::{theme, App, Hurry};

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
    // An empty deck shows the same window rather than a different one: the
    // controls are there but inert, the cassette's space is left blank, and
    // the block list says what is missing.
    let loaded = app.tape_ref().is_some();

    // The window is a fixed width and any height, so the contents scroll
    // rather than being squeezed when it is made short.
    egui::ScrollArea::vertical()
        .id_salt("tape-window")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled_ui(loaded, |ui| transport(app, ui));
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

/// How fast the tape is got through, on a line of its own: the transport is
/// the deck's own buttons, and these two are about the emulator rather than
/// about the tape.
fn speeds(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        // No slider in this row, so nothing in it is taller than a button and
        // the row needs no height claimed in advance.
        theme::group_label(ui, "Speed");

        // One of three rather than two switches: they were never independent
        // anyway, since hurrying a tape means running the machine flat out as
        // well, and a pair of toggles left "Ludicrous without Max" to be
        // explained away.
        let (boost, flash) = (app.tape_boost(), app.tape_flash());
        let now = if flash {
            Hurry::Ludicrous
        } else if boost {
            Hurry::Max
        } else {
            Hurry::Normal
        };

        if theme::selectable(ui, now == Hurry::Normal, "Normal")
            .on_hover_text("Play the tape at the speed it was recorded at.")
            .clicked()
        {
            app.set_hurry(Hurry::Normal);
        }
        if theme::selectable(ui, now == Hurry::Max, "Max")
            .on_hover_text("Run the machine as fast as it will go while the tape moves.")
            .clicked()
        {
            app.set_hurry(Hurry::Max);
        }
        ui.add_enabled_ui(!app.on_zx81(), |ui| {
            if theme::selectable(ui, now == Hurry::Ludicrous, "Ludicrous")
                .on_hover_text(
                    "Hand each block straight to the ROM's loader instead of playing \
                     it, so a tape loads in the time it takes to copy it, and run the \
                     machine flat out for the blocks that cannot be handed over. Games \
                     with a loader of their own read the tape themselves: those load at \
                     whatever speed the machine is running at.",
                )
                .clicked()
            {
                app.set_hurry(Hurry::Ludicrous);
            }
        });

        // Which of the two things it is doing, since they are not the same
        // thing: a game with a loader of its own is read to, not handed to.
        if flash {
            let what = if app.loader_is_reading() {
                "the game's own loader is reading the tape"
            } else {
                "handing blocks to the ROM"
            };
            ui.label(egui::RichText::new(what).small().color(theme::DIM));
        }
    });
}

/// How well the deck is behaving: the motor's steadiness and the head's
/// alignment, both of which a real one only ever had so much of.
///
/// Two lines rather than one. The window is a fixed width and the switches and
/// their sliders do not fit across it, and a row that wraps puts a slider
/// under the switch it has nothing to do with.
fn quality(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.set_min_height(theme::ROW_H);
        ui.spacing_mut().slider_width = 96.0;
        theme::group_label(ui, "Quality");
        theme::toggle(ui, &mut app.quality.speed, "Speed").on_hover_text(
            "Let the motor waver, as a real one does: wow over a turn of the \
             reel and a little flutter over the top of it, wandering across \
             seconds rather than shaking. Loaders measure the tape against \
             their own clock, so enough of this and they lose it.",
        );
        ui.add_enabled_ui(app.quality.speed, |ui| {
            theme::slider(
                ui,
                egui::Slider::new(&mut app.quality.speed_wobble, 0.0..=0.25)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
        });
        theme::toggle(ui, &mut app.quality.noise, "Noise").on_hover_text(
            "Tape hiss, there from the moment the head goes down: under the \
             signal, through the silence between blocks, and on a tape held \
             at pause. Stop lifts the head and it goes. Turned up past what \
             the reader calls an edge, the machine starts hearing it.",
        );
        ui.add_enabled_ui(app.quality.noise, |ui| {
            theme::slider(
                ui,
                egui::Slider::new(&mut app.quality.noise_level, 0.0..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            )
            .on_hover_text("How loud the hiss is");
        });
    });

    ui.horizontal_wrapped(|ui| {
        ui.set_min_height(theme::ROW_H);
        ui.spacing_mut().slider_width = 96.0;
        theme::toggle(ui, &mut app.quality.alignment, "Alignment").on_hover_text(
            "Put the head out of square with the tape. It then reads the top \
             of the track a moment before the bottom, and the two cancel each \
             other the shorter the wavelength gets — a low-pass whose corner \
             comes down the further out it is. The edges creep late first and \
             then start going missing, so the quick loaders go before the \
             slow ones.",
        );
        ui.add_enabled_ui(app.quality.alignment, |ui| {
            // The slider is how far out of square the head is; what that is
            // worth knowing as is where the corner lands, so it says that.
            let at = app.machine_t();
            let corner = app.quality.cutoff(at);
            theme::slider(
                ui,
                egui::Slider::new(&mut app.quality.alignment_offset, 0.0..=1.0)
                    .custom_formatter(move |_, _| format!("{:.1}kHz", corner / 1000.0)),
            )
            .on_hover_text("Where the corner sits");
            theme::slider(
                ui,
                egui::Slider::new(&mut app.quality.alignment_wobble, 0.0..=0.5)
                    .custom_formatter(|v, _| format!("±{:.0}%", v * 100.0)),
            )
            .on_hover_text("How far the corner wanders as the tape runs");
        });
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
            if let Some(t) = app.tape_mut() {
                t.rewind();
                t.edges.clear();
            }
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
                t.pause();
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
    });

    speeds(app, ui);
    quality(app, ui);

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
    // With nothing in the deck the screen stays as it is: a grid and no trace.
    let Some(tape) = app.tape_ref() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no signal",
            egui::FontId::proportional(12.0),
            theme::LCD_GRID,
        );
        return;
    };

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

    // The reader's own idea of the signal, faintly: it is what the machine
    // acts on, and with the head out of square it is not the same shape as
    // what arrived. Only while the tape is moving — a deck standing still is
    // not reading anything, and holding the last block's level across the
    // screen drew a line at the top or the bottom that meant nothing.
    let squares = Stroke::new(1.0, theme::LCD_GRID);
    let mut x = rect.left();
    let mut drew = false;
    for &(t, l) in tape.edges.iter().filter(|_| tape.playing) {
        if t <= t0 {
            continue;
        }
        if t > t1 {
            break;
        }
        let ex = x_of(t);
        painter.line_segment(
            [Pos2::new(x, y_of(level)), Pos2::new(ex, y_of(level))],
            squares,
        );
        painter.line_segment(
            [Pos2::new(ex, y_of(level)), Pos2::new(ex, y_of(l))],
            squares,
        );
        level = l;
        x = ex;
        drew = true;
    }
    if tape.playing {
        painter.line_segment(
            [
                Pos2::new(x, y_of(level)),
                Pos2::new(rect.right(), y_of(level)),
            ],
            squares,
        );
    }

    // And the signal itself over the top, which is where the head's doing
    // shows: a corner brought down rounds the squares off, and when the
    // rounding no longer reaches the reader's threshold an edge goes missing.
    let trace = Stroke::new(1.5, theme::LCD_FG);
    let y_of_signal = |y: f32| y_mid - y.clamp(-1.0, 1.0) * (y_low - y_high) * 0.5;
    // A sample for every pixel across the screen, since the hiss has a value
    // at every instant and drawing between two of them would smooth it away.
    // A deck with nothing on it draws a flat line down the middle rather than
    // leaving the last thing it saw on the screen.
    let columns = (rect.width().round() as usize).clamp(2, 2048);
    let shape: Vec<Pos2> = tape
        .scope_samples(t0, t1, columns)
        .into_iter()
        .map(|(t, y)| Pos2::new(x_of(t), y_of_signal(y)))
        .collect();
    painter.add(egui::Shape::line(shape, trace));
    if tape.playing {
        drew = true;
    }

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
            Pos2::new(rect.center().x, y_mid - 10.0),
            egui::Align2::CENTER_BOTTOM,
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

    // An empty deck still gets a list, with one row saying so, rather than
    // the window changing shape when a tape is taken out.
    let Some(tape) = app.tape_ref() else {
        egui::ScrollArea::vertical()
            .id_salt("tape-blocks")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_enabled_ui(false, |ui| {
                    let _ = ui
                        .selectable_label(false, RichText::new("  1  No tape loaded").monospace());
                });
            });
        return;
    };
    let current = tape.block;
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
    let mut insert_before: Option<usize> = None;
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

                // A way of stopping the deck where the tape's author did not.
                // It is on the row rather than in a menu because the row is
                // where the block is, and it appears on hover so the list
                // stays a list. Drawn into a child ui rather than allocated,
                // or every row would grow a button's worth of height.
                let row = resp.rect;
                let strip = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), row.top()),
                    egui::pos2(ui.max_rect().right(), row.bottom()),
                );
                // Where the pointer is, rather than what egui thinks is under
                // it: the list is inside a scroll area whose layer is not the
                // one the pointer is reckoned against, so `rect_contains_
                // pointer` says no over every row.
                let pointer = ui.input(|i| i.pointer.latest_pos());
                let over_row =
                    pointer.is_some_and(|at| strip.contains(at) && ui.clip_rect().contains(at));
                if over_row {
                    let mut over = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(strip.shrink2(egui::vec2(2.0, 0.0)))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    if over
                        .button(RichText::new("⏸ Pause before").size(11.0))
                        .on_hover_text(
                            "Put a stop-the-tape block in front of this one, \
                             so the deck stops here and waits to be started \
                             again",
                        )
                        .clicked()
                    {
                        insert_before = Some(i);
                    }
                }
            }
        });
    app.tape.scroll_to_current = false;

    if let Some(i) = insert_before {
        app.tape_mut().unwrap().insert_stop_before(i);
        return;
    }

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
/// The styling lives in the theme now, because the slow-draw and volume
/// sliders in the main window are the same control.
fn sweep_slider(window_us: &mut f32, ui: &mut egui::Ui) {
    theme::slider(
        ui,
        egui::Slider::new(window_us, 50.0..=40000.0)
            .logarithmic(true)
            .suffix(" µs"),
    );
}
