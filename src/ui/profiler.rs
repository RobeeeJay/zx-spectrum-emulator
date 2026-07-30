//! Profiler window: start and stop runs, list them, and show where the
//! emulated time went as a bar graph of functions.

use eframe::egui;
use egui::{Color32, RichText, Sense, Stroke, Vec2};

use crate::machine::Slot;
use crate::profiler::{format_duration, FuncStats, Metric};
use crate::ui::App;

pub struct ProfilerWindowState {
    /// Width the bars are drawn in.
    pub bar_width: f32,
    /// How many functions to list.
    pub limit: usize,
}

impl Default for ProfilerWindowState {
    fn default() -> Self {
        ProfilerWindowState {
            bar_width: 220.0,
            limit: 200,
        }
    }
}

impl App {
    pub fn start_profiling(&mut self) {
        let now = self.spec.bus.total_t();
        let hz = self.spec.bus.model.cpu_hz();
        self.spec.profiler.start(now, hz);
        self.show_profiler = true;
        self.set_status("Profiling…".into(), false);
    }

    pub fn stop_profiling(&mut self) {
        if !self.spec.profiler.running {
            return;
        }
        let now = self.spec.bus.total_t();
        self.spec.profiler.stop(now);
        let summary = self
            .spec
            .profiler
            .runs
            .last()
            .map(|r| {
                format!(
                    "Profile: {} of emulated time, {} functions",
                    format_duration(r.seconds(r.emulated_t)),
                    r.funcs.len()
                )
            })
            .unwrap_or_default();
        self.set_status(summary, false);
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    ui.separator();

    // A resizable left panel for the runs, so the bars and their figures get
    // the rest of the width rather than a rigid half.
    egui::Panel::left("profile-runs")
        .resizable(true)
        .default_size(330.0)
        .show(ui, |ui| run_list(app, ui));

    match app.spec.profiler.selected {
        Some(i) if i < app.spec.profiler.runs.len() => bars(app, ui, i),
        _ => {
            ui.label("Select a run to see where its time went.");
        }
    }
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    let running = app.spec.profiler.running;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!running, egui::Button::new("● Start"))
            .on_hover_text("Begin a profiling run")
            .clicked()
        {
            app.start_profiling();
        }
        if ui
            .add_enabled(running, egui::Button::new("■ Stop"))
            .on_hover_text("End the run and total up the time")
            .clicked()
        {
            app.stop_profiling();
        }
        if ui
            .add_enabled(!running && !app.spec.profiler.runs.is_empty(), egui::Button::new("Clear runs"))
            .clicked()
        {
            app.spec.profiler.clear();
        }
        ui.separator();
        ui.label("Rank by:");
        ui.selectable_value(&mut app.spec.profiler.metric, Metric::SelfTime, "Self time")
            .on_hover_text("Time in the function itself, excluding what it called");
        ui.selectable_value(&mut app.spec.profiler.metric, Metric::Inclusive, "Inclusive")
            .on_hover_text("Time between entry and return, callees included");
    });

    if running {
        let depth = app.spec.profiler.depth();
        let inner = app
            .spec
            .profiler
            .innermost()
            .map(|e| format!("${e:04X}"))
            .unwrap_or_else(|| "—".into());
        let seen = app
            .spec
            .profiler
            .current()
            .map(|r| r.funcs.len())
            .unwrap_or(0);
        ui.label(
            RichText::new(format!(
                "recording…  call depth {depth}, in {inner}, {seen} functions seen"
            ))
            .monospace()
            .color(Color32::from_rgb(255, 170, 90)),
        );
    } else {
        ui.label(
            RichText::new("stopped — press Start to record a run").monospace(),
        );
    }
}

fn run_list(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Runs").strong());
    let running = app.spec.profiler.running;
    let rows: Vec<(usize, String, bool)> = app
        .spec
        .profiler
        .runs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let last = i + 1 == app.spec.profiler.runs.len();
            let in_progress = running && last;
            let text = if in_progress {
                format!("{}  {}   running…", i + 1, r.started_label)
            } else {
                let wall = r
                    .wall
                    .map(|w| format_duration(w.as_secs_f64()))
                    .unwrap_or_else(|| "—".into());
                format!(
                    "{}  {}   {}   ({} emulated)",
                    i + 1,
                    r.started_label,
                    wall,
                    format_duration(r.seconds(r.emulated_t))
                )
            };
            (i, text, in_progress)
        })
        .collect();

    if rows.is_empty() {
        ui.label("No runs yet.");
        return;
    }

    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("profile-runs")
        .max_height(ui.available_height() - 40.0)
        .show(ui, |ui| {
            for (i, text, in_progress) in rows {
                let mut rich = RichText::new(text).monospace();
                if in_progress {
                    rich = rich.color(Color32::from_rgb(255, 170, 90));
                }
                if ui
                    .selectable_label(app.spec.profiler.selected == Some(i), rich)
                    .clicked()
                {
                    clicked = Some(i);
                }
            }
        });
    if let Some(i) = clicked {
        app.spec.profiler.selected = Some(i);
    }
}

/// Where a function's entry point lives, to help tell ROM from banked RAM.
fn where_label(app: &App, entry: u16) -> String {
    match app.spec.bus.slot_of(entry) {
        Slot::Rom(p) => {
            if app.spec.bus.rom_pages() > 1 {
                format!("ROM{p}")
            } else {
                "ROM".into()
            }
        }
        Slot::Ram(b) => {
            if app.spec.bus.model.has_paging() {
                format!("RAM{b}")
            } else {
                "RAM".into()
            }
        }
    }
}

fn bars(app: &mut App, ui: &mut egui::Ui, run_index: usize) {
    let metric = app.spec.profiler.metric;
    let (ranked, total, cpu_hz, outside, unfinished, instructions) = {
        let run = &app.spec.profiler.runs[run_index];
        (
            run.ranked(metric),
            run.total(metric).max(1),
            run.cpu_hz,
            run.outside_t,
            run.unfinished,
            run.instructions,
        )
    };

    ui.label(RichText::new(format!("Time by function ({})", metric.label())).strong());
    ui.label(
        RichText::new(format!(
            "{} functions, {} instructions, {} outside any call{}",
            ranked.len(),
            instructions,
            format_duration(outside as f64 / cpu_hz),
            if unfinished > 0 {
                format!(", {unfinished} still on the stack at stop")
            } else {
                String::new()
            }
        ))
        .monospace(),
    );
    ui.small("Click an entry point to disassemble it.");

    if ranked.is_empty() {
        ui.label("No calls were recorded.");
        return;
    }

    let biggest = ranked
        .first()
        .map(|f| f.time(metric))
        .unwrap_or(1)
        .max(1);
    let bar_width = app.profiler.bar_width;
    let limit = app.profiler.limit;
    let mut jump_to = None;

    egui::ScrollArea::vertical()
        .id_salt("profile-bars")
        .max_height(ui.available_height() - 20.0)
        .show(ui, |ui| {
            for f in ranked.iter().take(limit) {
                if row(app, ui, f, metric, biggest, total, cpu_hz, bar_width) {
                    jump_to = Some(f.entry);
                }
            }
        });

    if let Some(entry) = jump_to {
        app.dbg.follow_pc = false;
        app.dbg.view_addr = entry;
        app.show_debugger = true;
        app.set_status(format!("Disassembling ${entry:04X}"), false);
    }
}

/// One bar. Returns true if its entry point was clicked.
#[allow(clippy::too_many_arguments)]
fn row(
    app: &App,
    ui: &mut egui::Ui,
    f: &FuncStats,
    metric: Metric,
    biggest: u64,
    total: u64,
    cpu_hz: f64,
    bar_width: f32,
) -> bool {
    let time = f.time(metric);
    let share = time as f32 / biggest as f32;
    let percent = time as f64 * 100.0 / total as f64;

    let mut clicked = false;
    ui.horizontal(|ui| {
        // The entry point is the clickable part, so a bar can be followed
        // straight into the disassembler.
        if ui
            .selectable_label(false, RichText::new(format!("${:04X}", f.entry)).monospace())
            .on_hover_text(format!(
                "{} — {} calls, self {}, inclusive {}{}",
                where_label(app, f.entry),
                f.calls,
                format_duration(f.self_t as f64 / cpu_hz),
                format_duration(f.incl_t as f64 / cpu_hz),
                if f.max_depth > 1 {
                    format!(", nested {} deep", f.max_depth)
                } else {
                    String::new()
                }
            ))
            .clicked()
        {
            clicked = true;
        }

        let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, 14.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_gray(38));
        let filled = egui::Rect::from_min_size(
            rect.min,
            Vec2::new((rect.width() * share).max(1.0), rect.height()),
        );
        // Ramp from blue for the small fry to orange for the hot spots.
        let colour = Color32::from_rgb(
            (90.0 + 165.0 * share) as u8,
            (140.0 + 40.0 * share) as u8,
            (255.0 - 175.0 * share) as u8,
        );
        painter.rect_filled(filled, 2.0, colour);
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );

        ui.label(
            RichText::new(format!(
                "{:5.1}%  {:>9}  {:>7} calls",
                percent,
                format_duration(time as f64 / cpu_hz),
                f.calls
            ))
            .monospace(),
        );
    });
    clicked
}
