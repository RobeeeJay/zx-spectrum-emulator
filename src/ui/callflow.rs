//! The program's loop, drawn.
//!
//! One turn of whatever loop the machine was found going round: the routines
//! it calls, in the order it calls them, nested as they nest, each box sized
//! by how much work it does and labelled with what it was measured doing.
//! Nothing here counts frames — a game may take four of them over a turn, as
//! Manic Miner does — and nothing is read off the code.
//!
//! Finding the loop means sifting a few hundred thousand calls, so it is done
//! when asked for rather than every frame, and the answer is kept until asked
//! again.

use egui::{Color32, Pos2, Rect, RichText, Stroke, Vec2};

use crate::loops::{self, Turn};
use crate::ui::{theme, App};

/// What has been worked out, and when.
#[derive(Default)]
pub struct CallFlowState {
    pub turn: Option<Turn>,
    /// How the loop was described when it was found.
    pub summary: String,
    /// Calls watched at the moment it was worked out, so it is clear whether
    /// the picture is of the program as it is now.
    pub from_calls: usize,
}

/// How a routine is drawn.
const ROW_H: f32 = 34.0;
const INDENT: f32 = 26.0;
const BOX_W: f32 = 260.0;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    ui.separator();

    let Some(turn) = app.callflow.turn.take() else {
        ui.label(
            RichText::new(
                "Switch AutoDoc on in the debugger to watch the program, let it \
                 run for a few seconds, then find the loop.",
            )
            .color(theme::DIM),
        );
        return;
    };
    draw(app, ui, &turn);
    app.callflow.turn = Some(turn);
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Find the loop")
            .on_hover_text(
                "Sift what has been watched for the routine the program keeps \
                 coming back to, and take one turn of it",
            )
            .clicked()
        {
            find(app);
        }
        theme::divider(ui);
        let watching = app.spec.bus.observer.enabled;
        ui.label(
            RichText::new(if watching {
                format!(
                    "watching — {} calls so far",
                    app.spec.bus.observer.steps().count()
                )
            } else {
                "not watching; AutoDoc is off".to_string()
            })
            .color(if watching { theme::LCD_FG } else { theme::DIM }),
        );
    });
    if !app.callflow.summary.is_empty() {
        ui.label(
            RichText::new(&app.callflow.summary)
                .color(theme::DIM)
                .small(),
        );
    }
}

/// Work out the loop from what has been watched.
fn find(app: &mut App) {
    let frame_t = app.spec.bus.frame_t();
    let steps: Vec<crate::observe::Step> = app.spec.bus.observer.steps().copied().collect();
    app.callflow.from_calls = steps.len();

    let phases = loops::phases(&steps, frame_t, 64);
    let Some(phase) = phases.first() else {
        app.callflow.turn = None;
        app.callflow.summary = "Nothing has repeated often enough to call a loop yet.".to_string();
        return;
    };
    let head = app.notes.label(phase.head);
    let named = if head.is_empty() {
        format!("${:04X}", phase.head)
    } else {
        format!("{head} (${:04X})", phase.head)
    };
    app.callflow.summary = format!(
        "{} phase(s) found. Showing the loop on {named}: {} turns watched, \
         {:.2} frames a turn.",
        phases.len(),
        phase.iterations,
        phase.frames_per_turn(frame_t),
    );
    app.callflow.turn = loops::turn(&steps, phase, frame_t);
}

/// Draw one turn: a box per call, in order, indented as they nest.
fn draw(app: &mut App, ui: &mut egui::Ui, turn: &Turn) {
    let calls: Vec<&crate::observe::Step> = turn.steps.iter().filter(|step| step.enter).collect();
    if calls.is_empty() {
        return;
    }
    let deepest = calls.iter().map(|step| step.depth).max().unwrap_or(1) as f32;
    let size = Vec2::new(
        BOX_W + deepest * INDENT + 40.0,
        calls.len() as f32 * ROW_H + 20.0,
    );

    let mut go_to = None;
    egui::ScrollArea::both()
        .id_salt("callflow")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::LCD_BG);

            // The most work any one call does, so the bars can be compared.
            let most = calls
                .iter()
                .filter_map(|step| app.spec.bus.observer.routines.get(&step.entry))
                .map(|seen| seen.inclusive.total() / seen.calls.max(1))
                .max()
                .unwrap_or(1)
                .max(1);

            let mut previous_top: Option<(f32, f32)> = None;
            for (row, step) in calls.iter().enumerate() {
                let depth = (step.depth.saturating_sub(1)) as f32;
                let left = rect.left() + 12.0 + depth * INDENT;
                let top = rect.top() + 10.0 + row as f32 * ROW_H;
                let box_rect =
                    Rect::from_min_size(Pos2::new(left, top), Vec2::new(BOX_W, ROW_H - 8.0));

                // A line from the call before it, so the order reads as a
                // thread down the page rather than a list.
                if let Some((prev_x, prev_bottom)) = previous_top {
                    painter.line_segment(
                        [Pos2::new(prev_x, prev_bottom), Pos2::new(left + 6.0, top)],
                        Stroke::new(1.0, theme::EDGE),
                    );
                }
                previous_top = Some((left + 6.0, box_rect.bottom()));

                let seen = app.spec.bus.observer.routines.get(&step.entry);
                let work = seen
                    .map(|seen| seen.inclusive.total() / seen.calls.max(1))
                    .unwrap_or(0);

                // How much of the machine's work this call accounts for, drawn
                // as the width of the bar behind the name.
                let share = (work as f32 / most as f32).clamp(0.02, 1.0);
                painter.rect_filled(box_rect, 3.0, theme::CASE_DARK);
                painter.rect_filled(
                    Rect::from_min_size(
                        box_rect.min,
                        Vec2::new(box_rect.width() * share, box_rect.height()),
                    ),
                    3.0,
                    Color32::from_rgb(0x10, 0x33, 0x2c),
                );
                let hovered = response.hover_pos().is_some_and(|at| box_rect.contains(at));
                painter.rect_stroke(
                    box_rect,
                    3.0,
                    Stroke::new(1.0, if hovered { theme::AMBER } else { theme::EDGE }),
                    egui::StrokeKind::Inside,
                );

                let name = app.notes.label(step.entry);
                let label = if name.is_empty() {
                    format!("${:04X}", step.entry)
                } else {
                    format!("{name}  ${:04X}", step.entry)
                };
                painter.text(
                    Pos2::new(box_rect.left() + 8.0, box_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::monospace(12.0),
                    theme::LCD_FG,
                );
                painter.text(
                    Pos2::new(box_rect.right() - 8.0, box_rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{work} bytes"),
                    egui::FontId::monospace(11.0),
                    theme::DIM,
                );

                if hovered && response.clicked() {
                    go_to = Some(step.entry);
                }
            }
        });

    if let Some(entry) = go_to {
        app.dbg.view_addr = entry;
        app.dbg.follow_pc = false;
        app.show_debugger = true;
    }
}
