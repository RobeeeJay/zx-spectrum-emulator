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

use crate::callgraph;
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
    /// Which drawing is on show.
    pub view: View,
}

/// The three ways of looking at the same calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// One turn of the loop, in the order it happened, nested as it nests.
    #[default]
    Thread,
    /// Who calls whom, over everything watched: the shape of the program
    /// rather than one turn of it.
    Graph,
    /// One turn again, as nested bars whose width is the work done: what a
    /// turn is made of and where its time goes, in one picture.
    Flame,
    /// One turn against the frames it ran in: when each routine ran, and
    /// whether that was before or after the beam had been past.
    Timeline,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Thread => "Thread",
            View::Graph => "Graph",
            View::Flame => "Flame",
            View::Timeline => "Timeline",
        }
    }

    pub fn all() -> [View; 4] {
        [View::Thread, View::Graph, View::Flame, View::Timeline]
    }
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

    if app.callflow.view == View::Graph {
        graph(app, ui);
        return;
    }

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
    match app.callflow.view {
        View::Flame => flame(app, ui, &turn),
        View::Timeline => timeline(app, ui, &turn),
        _ => draw(app, ui, &turn),
    }
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
                    Question::ScreenClear => {
                        "Watch the program and look for the routines that fill \
                         the display file or the attributes with one value."
                    }
                    Question::SpriteUpdate => {
                        "Watch the program and look for the routines that draw \
                         the moving things: a sprite's worth of the screen at a \
                         time, over and over, out of data held elsewhere."
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
        theme::group_label(ui, "View");
        for view in View::all() {
            if theme::selectable(ui, app.callflow.view == view, view.label())
                .on_hover_text(match view {
                    View::Thread => {
                        "One turn of the loop in the order it happened, nested \
                         as it nests."
                    }
                    View::Graph => {
                        "Who calls whom over everything watched, in layers: a \
                         routine sits to the right of what calls it, and a \
                         thicker line is a call made more often."
                    }
                    View::Flame => {
                        "One turn as nested bars, each as wide as the work it \
                         does: what a turn is made of and where its time goes."
                    }
                    View::Timeline => {
                        "One turn against the frames it ran in, with the \
                         stretch where the ULA is drawing the picture shaded: \
                         what ran before the beam was past, and what ran after."
                    }
                })
                .clicked()
            {
                app.callflow.view = view;
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
            // A label names a routine, so it goes at the entry point; the
            // evidence is at the instruction, so the comment goes there. When
            // nothing was seen to call the code, the two are the same address.
            let name_at = finding.entry.unwrap_or(finding.address);
            let already = app.notes.label(name_at) == finding.label;
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
                app.notes.suggest(name_at, &finding.label, &comment);
                if name_at != finding.address {
                    app.notes.suggest(finding.address, "", &comment);
                }
                if let Err(e) = app.notes.save_if_dirty() {
                    app.set_status(format!("Could not save notes: {e}"), true);
                } else {
                    app.set_status(
                        format!("Labelled ${name_at:04X} as {}", finding.label),
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
                let figures = measured(seen);

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
                    figures.short(),
                    egui::FontId::monospace(11.0),
                    theme::DIM,
                );
                if hovered {
                    response.clone().on_hover_text(figures.long());
                }

                if hovered && response.clicked() {
                    go_to = Some(step.entry);
                }
            }
        });

    if let Some(entry) = go_to {
        app.show_in_debugger(entry);
    }
}

/// How a routine is drawn in the graph.
const NODE_W: f32 = 150.0;
const NODE_H: f32 = 26.0;
const LAYER_GAP: f32 = 78.0;
const ROW_GAP: f32 = 12.0;

/// Who calls whom, in layers.
///
/// Everything watched rather than one turn: the thread and the flame are what
/// a turn does, and this is the shape of the program the turns are made of. A
/// routine sits one layer to the right of whatever calls it, the line between
/// them is thicker the more often the call is made, and a line going back to
/// the left is a call into something that has already been reached — which is
/// what a loop in the program looks like from here.
fn graph(app: &mut App, ui: &mut egui::Ui) {
    let edges: std::collections::BTreeMap<(u16, u16), u32> = app
        .spec
        .bus
        .observer
        .edges
        .iter()
        .map(|(pair, edge)| (*pair, edge.calls))
        .collect();
    let work: std::collections::BTreeMap<u16, u64> = app
        .spec
        .bus
        .observer
        .routines
        .iter()
        .map(|(at, seen)| (*at, seen.inclusive.total() as u64))
        .collect();
    if edges.is_empty() {
        ui.label(
            RichText::new(
                "Nothing has been watched yet. Press one of the questions above, \
                 let the program run, and the calls it makes are drawn here.",
            )
            .color(theme::DIM),
        );
        return;
    }
    let layout = callgraph::layout(&edges, &work);

    let size = Vec2::new(
        layout.layers as f32 * (NODE_W + LAYER_GAP) + 20.0,
        layout.widest as f32 * (NODE_H + ROW_GAP) + 20.0,
    );
    let mut go_to = None;
    egui::ScrollArea::both()
        .id_salt("callflow-graph")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::LCD_BG);

            let box_of = |node: &callgraph::Node| -> Rect {
                Rect::from_min_size(
                    Pos2::new(
                        rect.left() + 10.0 + node.layer as f32 * (NODE_W + LAYER_GAP),
                        rect.top() + 10.0 + node.row as f32 * (NODE_H + ROW_GAP),
                    ),
                    Vec2::new(NODE_W, NODE_H),
                )
            };

            // The lines first, so the boxes sit on top of them.
            for link in &layout.links {
                let (Some(from), Some(to)) = (layout.node(link.from), layout.node(link.to)) else {
                    continue;
                };
                let (a, b) = (box_of(from), box_of(to));
                let back = to.layer <= from.layer;
                let start = if back {
                    a.left_center()
                } else {
                    a.right_center()
                };
                let end = if back {
                    b.right_center()
                } else {
                    b.left_center()
                };
                // A curve rather than a straight line: two calls between the
                // same pair of layers would otherwise lie on top of each other
                // wherever their rows happen to line up.
                let reach = (end.x - start.x).abs().max(40.0) * 0.4;
                let bend = if back { -reach } else { reach };
                painter.add(egui::Shape::CubicBezier(
                    egui::epaint::CubicBezierShape::from_points_stroke(
                        [
                            start,
                            Pos2::new(start.x + bend, start.y),
                            Pos2::new(end.x - bend, end.y),
                            end,
                        ],
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(
                            1.0 + link.weight * 3.0,
                            if back {
                                theme::AMBER.gamma_multiply(0.5)
                            } else {
                                theme::EDGE
                            },
                        ),
                    ),
                ));
            }

            for node in &layout.nodes {
                let at = box_of(node);
                let hovered = response.hover_pos().is_some_and(|p| at.contains(p));
                painter.rect_filled(at, 3.0, theme::CASE_DARK);
                // How much of the machine's work goes through it, as the width
                // of the bar behind the name.
                painter.rect_filled(
                    Rect::from_min_size(
                        at.min,
                        Vec2::new(at.width() * node.weight.max(0.02), at.height()),
                    ),
                    3.0,
                    Color32::from_rgb(0x10, 0x33, 0x2c),
                );
                painter.rect_stroke(
                    at,
                    3.0,
                    Stroke::new(1.0, if hovered { theme::AMBER } else { theme::EDGE }),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    Pos2::new(at.left() + 6.0, at.center().y),
                    egui::Align2::LEFT_CENTER,
                    label_for(app, node.entry),
                    egui::FontId::monospace(11.0),
                    theme::LCD_FG,
                );
                if hovered && response.clicked() {
                    go_to = Some(node.entry);
                }
            }
        });
    if let Some(entry) = go_to {
        app.show_in_debugger(entry);
    }
}

/// One turn as nested bars, each as wide as the work it does.
///
/// The thread says what was called and in what order; this says what a turn is
/// made of. A routine's bar spans the work done between going in and coming
/// out again, and the routines it called sit under it filling that span, so a
/// wide bar with nothing under it is where the time actually goes.
fn flame(app: &mut App, ui: &mut egui::Ui, turn: &Turn) {
    // Instructions run, from the observer, rather than anything counted here:
    // a call's width is what it cost including its callees.
    let cost = |entry: u16| -> f32 {
        app.spec
            .bus
            .observer
            .routines
            .get(&entry)
            .map(|seen| (seen.inclusive.total() / seen.calls.max(1)).max(1) as f32)
            .unwrap_or(1.0)
    };

    let calls: Vec<&crate::observe::Step> = turn.steps.iter().filter(|step| step.enter).collect();
    if calls.is_empty() {
        return;
    }
    // Each depth is a row; a call's width is its share of the turn's work, and
    // its left edge is where the calls before it at that depth ended.
    let deepest = calls.iter().map(|step| step.depth).max().unwrap_or(1) as f32;
    let total: f32 = calls
        .iter()
        .filter(|step| step.depth <= 1)
        .map(|step| cost(step.entry))
        .sum::<f32>()
        .max(1.0);

    let height = (deepest + 1.0) * (NODE_H + 4.0) + 20.0;
    let mut go_to = None;
    egui::ScrollArea::both()
        .id_salt("callflow-flame")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width().max(400.0) - 20.0;
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::LCD_BG);

            // Where the next bar at each depth starts, in work.
            let mut filled: Vec<f32> = vec![0.0; deepest as usize + 2];
            for step in &calls {
                let depth = step.depth.max(1) as usize;
                let share = cost(step.entry) / total;
                // A call starts no earlier than its caller did: the bars under
                // a bar are what it spent its time on.
                let start = filled[depth].max(filled[depth - 1] - share.min(1.0));
                let left = rect.left() + 10.0 + start * (rect.width() - 20.0);
                let bar = Rect::from_min_size(
                    Pos2::new(
                        left,
                        rect.top() + 10.0 + (depth as f32 - 1.0) * (NODE_H + 4.0),
                    ),
                    Vec2::new((share * (rect.width() - 20.0)).max(2.0), NODE_H),
                );
                filled[depth] = start + share;

                let hovered = response.hover_pos().is_some_and(|p| bar.contains(p));
                painter.rect_filled(
                    bar,
                    2.0,
                    if hovered {
                        Color32::from_rgb(0x1c, 0x55, 0x49)
                    } else {
                        Color32::from_rgb(0x10, 0x33, 0x2c)
                    },
                );
                painter.rect_stroke(
                    bar,
                    2.0,
                    Stroke::new(1.0, theme::EDGE),
                    egui::StrokeKind::Inside,
                );
                if bar.width() > 40.0 {
                    let clipped = painter.with_clip_rect(bar);
                    clipped.text(
                        Pos2::new(bar.left() + 4.0, bar.center().y),
                        egui::Align2::LEFT_CENTER,
                        label_for(app, step.entry),
                        egui::FontId::monospace(11.0),
                        theme::LCD_FG,
                    );
                }
                if hovered && response.clicked() {
                    go_to = Some(step.entry);
                }
            }
        });
    if let Some(entry) = go_to {
        app.show_in_debugger(entry);
    }
}

/// What to call a routine: its name if it has one, and its address either way.
fn label_for(app: &App, entry: u16) -> String {
    let name = app.notes.label(entry);
    if name.is_empty() {
        format!("${entry:04X}")
    } else {
        format!("{name}  ${entry:04X}")
    }
}

/// How a lane is drawn in the timeline.
const LANE_H: f32 = 20.0;
const LANE_GAP: f32 = 4.0;
const NAMES_W: f32 = 150.0;

/// One turn of the loop against the frames it ran in.
///
/// The thread says what was called and the flame says what it cost; this says
/// when. On this machine that is the whole question — the ULA is drawing the
/// picture while the program runs, so a routine that writes to the display
/// file above the beam is seen this frame and one that writes below it is seen
/// next frame. The shaded band is the display being drawn; the lines are frame
/// boundaries, which is where the interrupt lands.
fn timeline(app: &mut App, ui: &mut egui::Ui, turn: &Turn) {
    let frame_t = app.spec.bus.frame_t();
    let laid = crate::timeline::lay_out(&turn.steps, frame_t);
    if laid.bars.is_empty() {
        ui.label(RichText::new("Nothing ran in this turn.").color(theme::DIM));
        return;
    }

    let height = laid.lanes.len() as f32 * (LANE_H + LANE_GAP) + 30.0;
    let mut go_to = None;
    egui::ScrollArea::both()
        .id_salt("callflow-timeline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width().max(500.0) - 20.0;
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::LCD_BG);

            let plot = Rect::from_min_max(
                Pos2::new(rect.left() + NAMES_W, rect.top() + 8.0),
                Pos2::new(rect.right() - 8.0, rect.bottom() - 8.0),
            );
            let x_of = |t: u64| -> f32 {
                let along = (t.saturating_sub(laid.from)) as f32 / laid.span() as f32;
                plot.left() + along.clamp(0.0, 1.0) * plot.width()
            };

            // The frames the turn covers, with the display period shaded: a
            // write inside the band is a write the beam may already have gone
            // past.
            let (first, last) = (laid.from / frame_t as u64, laid.to / frame_t as u64);
            let (start, end) = crate::timeline::display_window(
                app.spec.bus.first_pixel_t(),
                192,
                app.spec.bus.model.t_per_line(),
            );
            for frame in first..=last {
                let base = frame * frame_t as u64;
                let band = Rect::from_min_max(
                    Pos2::new(x_of(base + start as u64), plot.top()),
                    Pos2::new(x_of(base + end as u64), plot.bottom()),
                );
                painter.rect_filled(band, 0.0, Color32::from_rgb(0x0d, 0x1d, 0x1a));
                let edge = x_of(base);
                painter.line_segment(
                    [Pos2::new(edge, plot.top()), Pos2::new(edge, plot.bottom())],
                    Stroke::new(1.0, theme::AMBER.gamma_multiply(0.5)),
                );
            }

            for (lane, entry) in laid.lanes.iter().enumerate() {
                let y = plot.top() + lane as f32 * (LANE_H + LANE_GAP);
                painter.text(
                    Pos2::new(rect.left() + 8.0, y + LANE_H / 2.0),
                    egui::Align2::LEFT_CENTER,
                    label_for(app, *entry),
                    egui::FontId::monospace(11.0),
                    theme::LCD_FG,
                );
                painter.line_segment(
                    [
                        Pos2::new(plot.left(), y + LANE_H + LANE_GAP / 2.0),
                        Pos2::new(plot.right(), y + LANE_H + LANE_GAP / 2.0),
                    ],
                    Stroke::new(1.0, theme::EDGE.gamma_multiply(0.4)),
                );
            }

            for bar in &laid.bars {
                let y = plot.top() + bar.lane as f32 * (LANE_H + LANE_GAP);
                let at = Rect::from_min_max(
                    Pos2::new(x_of(bar.from), y + 2.0),
                    Pos2::new((x_of(bar.to)).max(x_of(bar.from) + 2.0), y + LANE_H - 2.0),
                );
                let hovered = response.hover_pos().is_some_and(|p| at.contains(p));
                painter.rect_filled(
                    at,
                    2.0,
                    if hovered {
                        theme::AMBER
                    } else {
                        Color32::from_rgb(0x14, 0x44, 0x3a)
                    },
                );
                if hovered && response.clicked() {
                    go_to = Some(bar.entry);
                }
            }

            painter.text(
                Pos2::new(plot.left(), rect.bottom() - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{} frames, shaded where the ULA is drawing the picture",
                    last - first + 1
                ),
                egui::FontId::monospace(10.0),
                theme::DIM,
            );
        });
    if let Some(entry) = go_to {
        app.show_in_debugger(entry);
    }
}

/// The three numbers a routine is described by.
///
/// How big it is, how much it wrote and how much it read — all measured while
/// the program ran rather than worked out from the code. Reads and writes are
/// per call and count what the routines it called did as well: a routine whose
/// whole job is to call the drawing routine does nothing itself, and saying so
/// is the wrong thing to say about it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    /// How many bytes of code the routine covers: the distance between the
    /// lowest and highest address seen executing inside it.
    pub size: u32,
    /// Bytes written per call, its callees included.
    pub wrote: u32,
    /// Bytes read per call, its callees included.
    pub read: u32,
}

impl Measured {
    /// What fits on the row.
    pub fn short(&self) -> String {
        format!("{}B  {}w  {}r", self.size, self.wrote, self.read)
    }

    /// What the row says when it is hovered.
    pub fn long(&self) -> String {
        format!(
            "{} bytes of code, and per call it writes {} bytes and reads {}, \
             counting what it calls",
            self.size, self.wrote, self.read
        )
    }
}

/// Read the three figures off what was observed.
pub fn measured(seen: Option<&crate::observe::Observed>) -> Measured {
    let Some(seen) = seen else {
        return Measured::default();
    };
    let calls = seen.calls.max(1);
    Measured {
        // Where the routine actually reaches, which is not the same as where
        // it starts: a routine that jumps over a table of data reaches past
        // the table, so this is where to start looking rather than a promise.
        size: seen
            .spans
            .map(|(low, high)| high.saturating_sub(low) as u32 + 1)
            .unwrap_or(0),
        wrote: seen.inclusive.total() / calls,
        read: seen.inclusive_reads.total() / calls,
    }
}
