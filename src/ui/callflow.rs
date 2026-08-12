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

use crate::detect::{self, Finding, Question};
use crate::loops::{self, Turn};
use crate::ui::{theme, App};

/// What has been worked out, and when.
#[derive(Default)]
pub struct CallFlowState {
    /// Whether it is watching the program at the moment. Watching costs a
    /// branch on every memory access, so it only runs while somebody is
    /// looking for something.
    pub looking: bool,
    /// Which question is being asked. One detector at a time, so what is in
    /// the list is the answer to something the reader chose.
    pub asking: Question,
    /// The finding waiting to be accepted or thrown away. Nothing is written
    /// against an address until somebody says so.
    pub offered: Option<Finding>,
    /// What the detectors have made of the program: one line per loop found,
    /// with how sure they are and what the answer rests on.
    pub findings: Vec<Finding>,
    /// When they last ran, so they can run again without being asked and
    /// without running every frame.
    pub looked_at: Option<std::time::Instant>,
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

/// How often the detectors run themselves. Sifting a few hundred thousand
/// calls is not free, and the answer does not change from one frame to the
/// next.
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_secs(3);

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);

    // Detection runs on its own, so the window is telling you what it thinks
    // rather than waiting to be asked.
    let due = app
        .callflow
        .looked_at
        .is_none_or(|when| when.elapsed() > LOOK_AGAIN);
    if due && app.callflow.looking {
        look(app);
    }

    findings(app, ui);
    ui.separator();

    let Some(turn) = app.callflow.turn.take() else {
        ui.label(
            RichText::new(
                "Press Main game loop, let the program run for a few seconds, \
                 and one turn of the loop it keeps coming back to is drawn here.",
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
        // The same button as the main window and the debugger carry. Looking
        // for something means watching the program run and then stopping it to
        // read what was found, and reaching for another window to do that
        // loses your place in this one.
        if theme::run_pause_button(ui, app.running).clicked() {
            app.running = !app.running;
        }
        theme::divider(ui);

        // Always clickable: pressing one is what starts the machine being
        // watched, so there is no switch to find first.
        for question in Question::all() {
            let asking = app.callflow.looking && app.callflow.asking == question;
            if theme::selectable(ui, asking, question.label())
                .on_hover_text(match question {
                    Question::MainGameLoop => {
                        "Watch the program and look for the routines it keeps \
                         coming back to. Nothing is written down until you say so."
                    }
                    Question::Keyboard => {
                        "Watch the program and look for the routines that read \
                         the keyboard on port $FE."
                    }
                    Question::Joystick => {
                        "Watch the program and look for the routines that read a \
                         joystick — a Kempston or a Fuller on its own port, or a \
                         Sinclair on the keyboard's rows."
                    }
                })
                .clicked()
            {
                start_looking(app, question);
            }
        }
        if app.callflow.looking {
            theme::divider(ui);
            if ui.button("Stop").clicked() {
                app.callflow.looking = false;
            }
        }

        theme::divider(ui);
        let calls = app.spec.bus.observer.steps().count();
        ui.label(
            RichText::new(if app.callflow.looking {
                format!("watching — {calls} calls so far")
            } else if calls > 0 {
                format!("stopped — {calls} calls watched")
            } else {
                "not watching".to_string()
            })
            .color(if app.callflow.looking {
                theme::LCD_FG
            } else {
                theme::DIM
            }),
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

/// Start watching from nothing, so what is found comes from now rather than
/// from whatever happened to be in the buffer.
fn start_looking(app: &mut App, question: Question) {
    app.spec.bus.observer.clear();
    app.callflow.asking = question;
    app.callflow.looking = true;
    app.callflow.looked_at = None;
    app.callflow.findings.clear();
    app.callflow.offered = None;
    app.callflow.turn = None;
    app.callflow.summary = format!(
        "Watching for the {}. Let the program run for a few seconds.",
        question.label().to_lowercase()
    );
}

/// What the detectors think, one line each.
fn findings(app: &mut App, ui: &mut egui::Ui) {
    if app.callflow.findings.is_empty() {
        ui.label(
            RichText::new(if app.callflow.looking {
                format!(
                    "Looking for the {}. Nothing found yet.",
                    app.callflow.asking.label().to_lowercase()
                )
            } else {
                "Press one of the buttons above to look for something.".to_string()
            })
            .color(theme::DIM),
        );
        return;
    }

    let findings = app.callflow.findings.clone();
    ui.label(
        RichText::new(format!(
            "{} candidate{} for the {}, the likeliest first.",
            findings.len(),
            if findings.len() == 1 { "" } else { "s" },
            app.callflow.asking.label().to_lowercase()
        ))
        .small()
        .color(theme::DIM),
    );

    let mut go_to = None;
    for finding in &findings {
        // Everything that can be pressed goes on one row, in front of the
        // prose. The button used to follow the sentence saying what the guess
        // rests on, and a row cannot wrap, so on a long sentence it was off
        // the edge of the window and could not be reached at all.
        ui.horizontal(|ui| {
            // The finding is a guess until somebody accepts it. Nothing is
            // written against the address before that.
            let already = app.notes.label(finding.address) == finding.label;
            if already {
                ui.add_enabled(
                    false,
                    egui::Button::new(RichText::new("Labelled").color(theme::GREEN)),
                );
            } else if ui
                .button("Label")
                .on_hover_text(format!(
                    "Write {} against ${:04X} in the listing, with what it \
                     rests on as the comment, marked as a guess",
                    finding.label, finding.address
                ))
                .clicked()
            {
                let comment = format!("{}: {}", finding.what.to_lowercase(), finding.because);
                app.notes.suggest(finding.address, &finding.label, &comment);
                if let Err(e) = app.notes.save_if_dirty() {
                    app.set_status(format!("Could not save notes: {e}"), true);
                } else {
                    app.set_status(
                        format!("Labelled ${:04X} as {}", finding.address, finding.label),
                        false,
                    );
                }
            }

            ui.label(RichText::new(finding.what).color(theme::LCD_FG));

            let name = app.notes.label(finding.address);
            let named = if name.is_empty() {
                format!("${:04X}", finding.address)
            } else {
                format!("{name}  ${:04X}", finding.address)
            };
            if ui
                .add(
                    egui::Label::new(RichText::new(named).monospace().color(theme::AMBER))
                        .sense(egui::Sense::click()),
                )
                .on_hover_text("Show it in the debugger")
                .clicked()
            {
                go_to = Some(finding.address);
            }

            // The word and the number behind it, because "likely" on its own
            // is not something anybody can argue with.
            ui.label(
                RichText::new(format!(
                    "{} ({:.0}%)",
                    finding.sure.label(),
                    finding.score * 100.0
                ))
                .color(match finding.sure {
                    detect::Sure::Certain => theme::GREEN,
                    detect::Sure::Likely => theme::LCD_FG,
                    detect::Sure::Possible => theme::DIM,
                }),
            );
        });
        ui.label(RichText::new(&finding.because).small().color(theme::DIM));
    }
    if let Some(address) = go_to {
        app.show_in_debugger(address);
    }
}

/// Run the detectors over what has been watched, and take a turn of whatever
/// loop they found.
///
/// Both at once: sifting the calls is the expensive part and they want the
/// same sift. Doing only the first would leave a finding sitting above an
/// empty chart, which reads as a failure rather than as a window waiting to be
/// asked.
fn look(app: &mut App) {
    app.callflow.looked_at = Some(std::time::Instant::now());
    let frame_t = app.spec.bus.frame_t();
    let steps: Vec<crate::observe::Step> = app.spec.bus.observer.steps().copied().collect();
    app.callflow.from_calls = steps.len();
    app.callflow.findings =
        detect::ask(app.callflow.asking, &steps, &app.spec.bus.observer, frame_t);

    let phases = loops::phases(&steps, frame_t, 64);
    if let Some(phase) = phases.iter().max_by_key(|phase| phase.iterations) {
        let head = app.notes.label(phase.head);
        let named = if head.is_empty() {
            format!("${:04X}", phase.head)
        } else {
            format!("{head} (${:04X})", phase.head)
        };
        // The chart is drawn whatever was asked for — it is what the window
        // is — but the line above it answers the question that was asked, not
        // the one the chart happens to be of.
        app.callflow.summary = match app.callflow.asking {
            Question::MainGameLoop => format!(
                "{} phase(s) watched. One turn of the loop on {named}: {} turns seen, \
                 {:.2} frames a turn.",
                phases.len(),
                phase.iterations,
                phase.frames_per_turn(frame_t),
            ),
            question => format!(
                "Watched {} routines over {} frames, looking for the {}. The chart \
                 below is one turn of the loop on {named}.",
                app.spec.bus.observer.routines.len(),
                app.spec.bus.observer.frames,
                question.label().to_lowercase(),
            ),
        };
        app.callflow.turn = loops::turn(&steps, phase, frame_t);
    }
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
        app.show_in_debugger(entry);
    }
}
