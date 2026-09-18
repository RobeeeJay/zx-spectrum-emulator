//! The Training window: a game set up for a network to learn, the learning
//! started and stopped, and how it is going.
//!
//! The network sees the screen and nothing else — see `crate::training`. The
//! reward is read out of memory by the judge, which is told here where the
//! game keeps its score and its lives; what it reads goes no further than
//! the reward. The set-up is kept beside the tape, and the network in a
//! directory beside it, so a game set up once stays set up.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::joystick::Kind;
use crate::machine::Spectrum;
use crate::training::inputs::Action;
use crate::training::inputs::InputSet;
use crate::training::judge::Number;
use crate::training::model::SMALLEST;
use crate::training::player::Player;
use crate::training::ppo::Progress;
use crate::training::search::{Candidate, Search, Test};
use crate::training::setup::Setup;
use crate::training::sight::{changes, Area, Sight};
use crate::training::worker::{self, Preview, Processor, Start, Worker};
use crate::ui::{theme, App};

/// What every game of a run starts as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum From {
    /// The machine as it is when Start is pressed.
    #[default]
    Now,
    /// A quicksave.
    Slot(usize),
    /// The machine the kept network's games started from.
    Kept,
}

/// The window's state: the set-up being edited, the text of what is typed,
/// and the run, if there is one.
pub struct Training {
    pub setup: Setup,
    /// How the controls were put together, so the same choices can be shown.
    pub interface: Kind,
    pub diagonals: bool,
    pub fire_moving: bool,
    /// The keys of a keys-only set, by their legends.
    pub keys: String,
    /// Controls read from a file that none of the choices here would make.
    /// They are kept as they are until a choice is changed.
    pub custom: bool,
    pub score: String,
    pub lives: String,
    pub processor: Processor,
    pub from: From,
    /// Go on from the network kept beside the tape rather than a fresh one.
    pub resume: bool,
    pub worker: Option<Worker>,
    /// Two updates being run to see how long one takes, and what for.
    timing: Option<(Worker, Setup, Processor)>,
    /// How long an update took, the set-up and the processor it was of.
    pub timed: Option<(f32, Setup, Processor)>,
    /// A kept network with the machine's controls.
    pub player: Option<Player>,
    /// Looking for where the game keeps a number, and the number to look for.
    pub search: Option<Search>,
    /// The number as the screen shows it, for Is now.
    pub search_is: String,
    /// Let it take its favourite action rather than draw one.
    pub greedy: bool,
    started: Option<Instant>,
    /// The file the set-up was last read for, so a new tape brings its own.
    read_for: Option<Option<PathBuf>>,
    preview: Option<egui::TextureHandle>,
    /// One texture per frame of the stack the network is shown.
    seen: Vec<egui::TextureHandle>,
    /// The last thing to report, and whether it went wrong.
    pub message: Option<(String, bool)>,
}

impl Default for Training {
    fn default() -> Self {
        let mut training = Training {
            setup: Setup::default(),
            interface: Kind::Kempston,
            diagonals: false,
            fire_moving: true,
            keys: String::new(),
            custom: false,
            score: String::new(),
            lives: String::new(),
            processor: Processor::default(),
            from: From::Now,
            resume: false,
            worker: None,
            timing: None,
            timed: None,
            player: None,
            search: None,
            search_is: String::new(),
            greedy: false,
            started: None,
            read_for: None,
            preview: None,
            seen: Vec::new(),
            message: None,
        };
        training.adopt(Setup::default());
        training
    }
}

fn number(text: &str) -> Result<Option<Number>, String> {
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        Number::from_text(text).map(Some)
    }
}

impl Training {
    pub fn is_running(&self) -> bool {
        self.worker.as_ref().is_some_and(Worker::is_running)
    }

    pub fn is_timing(&self) -> bool {
        self.timing.is_some()
    }

    /// Once the timing run is over, take the second update's time: the
    /// first includes setting the network up and, on the graphics card,
    /// compiling its programs.
    fn finish_timing(&mut self) {
        if self.timing.as_ref().is_none_or(|(w, _, _)| w.is_running()) {
            return;
        }
        let (worker, setup, processor) = self.timing.take().expect("just seen");
        let shared = worker.shared();
        match (shared.history.get(1), &shared.error) {
            (Some(p), _) => self.timed = Some((p.seconds, setup, processor)),
            (None, Some(e)) => self.message = Some((e.clone(), true)),
            (None, None) => {
                self.message = Some(("Timing stopped before its two updates".into(), true))
            }
        }
    }

    /// Take a set-up over, showing its controls as the choices that make
    /// them where there are such choices.
    pub fn adopt(&mut self, setup: Setup) {
        let inputs = &setup.env.inputs;
        self.custom = false;
        let stick = [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .find(|(d, f)| *inputs == InputSet::joystick(inputs.interface, *d, *f));
        let names: Vec<&str> = inputs
            .actions
            .iter()
            .skip(1)
            .map(|a| a.name.as_str())
            .collect();
        if let (Some((d, f)), true) = (stick, inputs.interface != Kind::None) {
            (self.diagonals, self.fire_moving) = (d, f);
        } else if inputs.interface == Kind::None && InputSet::keys(&names).as_ref() == Ok(inputs) {
            self.keys = names.join(", ");
        } else {
            self.custom = true;
        }
        self.interface = inputs.interface;
        let judge = &setup.env.judge;
        self.score = judge.score.map(|n| n.to_text()).unwrap_or_default();
        self.lives = judge.lives.map(|n| n.to_text()).unwrap_or_default();
        self.setup = setup;
    }

    /// Put what has been typed into the set-up, and say what stands in the
    /// way of starting.
    pub fn gather(&mut self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.custom {
            let inputs = if self.interface == Kind::None {
                let names: Vec<&str> = self
                    .keys
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                if names.is_empty() {
                    Err("Name the keys it may press, with commas between: O, P, SPACE".into())
                } else {
                    InputSet::keys(&names)
                }
            } else {
                Ok(InputSet::joystick(
                    self.interface,
                    self.diagonals,
                    self.fire_moving,
                ))
            };
            match inputs {
                Ok(set) => self.setup.env.inputs = set,
                Err(e) => problems.push(e),
            }
        }
        let judge = &mut self.setup.env.judge;
        match number(&self.score) {
            Ok(n) => judge.score = n,
            Err(e) => problems.push(format!("Score: {e}")),
        }
        match number(&self.lives) {
            Ok(n) => judge.lives = n,
            Err(e) => problems.push(format!("Lives: {e}")),
        }
        if judge.score.is_none() && judge.lives.is_none() && judge.per_step == 0.0 {
            problems.push(
                "Nothing to reward: give the score's address, the lives', or a reward for \
                 each step survived"
                    .into(),
            );
        }
        let sight = &self.setup.env.sight;
        if sight.width() < SMALLEST || sight.height() < SMALLEST {
            problems.push(format!(
                "A {}x{} picture is too small for the network: shrink it less",
                sight.width(),
                sight.height()
            ));
        }
        problems
    }

    fn read(&mut self, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let setup = Setup::from_text(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.adopt(setup);
        Ok(())
    }
}

/// A tape with a set-up beside it brings it with it.
fn follow_source(app: &mut App) {
    let source = app.notes_source();
    let t = &mut app.training;
    if t.read_for.as_ref() == Some(&source) {
        return;
    }
    t.read_for = Some(source.clone());
    let Some(path) = source.as_deref().map(Setup::sidecar) else {
        return;
    };
    if path.exists() {
        t.message = Some(match t.read(&path) {
            Ok(()) => (format!("Set-up read from {}", path.display()), false),
            Err(e) => (e, true),
        });
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    follow_source(app);
    app.training.finish_timing();
    let running = app.training.is_running();
    if running || app.training.is_timing() {
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
    let problems = if running {
        Vec::new()
    } else {
        app.training.gather()
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled_ui(!running, |ui| {
                controls(&mut app.training, ui);
                ui.add_space(6.0);
                sight(&mut app.training, ui);
                ui.add_space(6.0);
                reward(&mut app.training, ui);
                find(app, ui);
                ui.add_space(6.0);
                learning(app, ui);
            });
            ui.add_space(8.0);
            buttons(app, ui, running, &problems);
            ui.add_space(8.0);
            progress(&app.training, ui);
            ui.add_space(6.0);
            watching(&mut app.training, ui);
            ui.add_space(8.0);
            play(app, ui);
        });
}

fn note(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).small().color(theme::DIM));
}

fn interface_name(kind: Kind) -> &'static str {
    if kind == Kind::None {
        "Keys only"
    } else {
        kind.name()
    }
}

fn controls(t: &mut Training, ui: &mut egui::Ui) {
    theme::group_label(ui, "What it may press");
    ui.horizontal_wrapped(|ui| {
        let mut interface = t.interface;
        theme::dropdown(ui, 190.0, interface_name(interface), |ui| {
            for kind in Kind::ALL {
                if ui
                    .selectable_label(interface == kind, interface_name(kind))
                    .clicked()
                {
                    interface = kind;
                }
            }
        });
        if interface != t.interface {
            t.interface = interface;
            t.custom = false;
        }
        if t.interface == Kind::None {
            ui.label("Keys");
            let edit = egui::TextEdit::singleline(&mut t.keys)
                .hint_text("O, P, SPACE")
                .desired_width(230.0);
            if ui.add(edit).on_hover_text(tip("keys")).changed() {
                t.custom = false;
            }
        } else {
            if ui.checkbox(&mut t.diagonals, "Diagonals").changed() {
                t.custom = false;
            }
            if ui
                .checkbox(&mut t.fire_moving, "Fire while moving")
                .changed()
            {
                t.custom = false;
            }
        }
    });
    let names: Vec<&str> = t
        .setup
        .env
        .inputs
        .actions
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    note(
        ui,
        format!(
            "{} actions{}: {}",
            names.len(),
            if t.custom {
                ", as the set-up file has them"
            } else {
                ""
            },
            names.join(", ")
        ),
    );
}

/// What each box does to the training, shown when the pointer rests on it.
/// Every box goes through `tip`, so one added without a word about it fails
/// the test rather than going unnoticed.
const TIPS: [(&str, &str); 22] = [
    (
        "score",
        "Where the game keeps its score, and how. Find a number below works it out. \
         Wrong, and there is nothing to learn from: the network is rewarded by this \
         alone unless lives or a reward a step are set.",
    ),
    (
        "point",
        "What one point of score is worth as reward. Rewards of about 1 are what the \
         learning is built for: a game scoring 50 a kill wants 0.02 here, or the \
         numbers it learns from run into the hundreds and swamp everything else.",
    ),
    (
        "lives",
        "Where the game keeps its lives, and how. It is how a game is known to be \
         over, and what makes dying cost something.",
    ),
    (
        "life",
        "What losing a life costs, against a point being worth what it is worth \
         above. Too little and the network throws lives away for a few points; too \
         much and it hides in a corner rather than playing.",
    ),
    (
        "over",
        "The number of lives at which the game is over and a new one starts. Nothing \
         after that point is learnt from, so a game that ends here rather than \
         playing on into a demonstration teaches more.",
    ),
    (
        "stop",
        "A game is stopped after this many steps however it is going, so one that \
         gets stuck cannot hold up the others. A game much longer than this is never \
         seen to its end.",
    ),
    (
        "step",
        "A reward for every step survived, whatever else happens. For games with \
         nothing else to go on, or to nudge the network towards staying alive. Small \
         beside a point's worth: 0.01 adds up over a long game.",
    ),
    (
        "frames",
        "How many frames are stacked into what the network is shown. One picture \
         cannot say which way anything is moving; a few can. What it sees below \
         shows whether the frames really differ.",
    ),
    (
        "hold",
        "How many frames each choice is held for. More frames means fewer, coarser \
         choices and a shorter way back from a reward to the move that earned it; \
         fewer means finer control and more to learn from.",
    ),
    (
        "wait",
        "Up to this many frames of doing nothing at the start of each game, a \
         different number each time, so games that start alike do not all go alike \
         and the network cannot learn one fixed opening.",
    ),
    (
        "games",
        "How many games are played at once. More games give steadier learning and \
         use the cores better, at more time and memory per update.",
    ),
    (
        "steps",
        "How many steps each game plays between updates. Games times steps is what \
         one update learns from: more is steadier and slower to arrive.",
    ),
    (
        "rate",
        "How far the network moves at each step of learning. Too high and it lurches \
         about and forgets what worked; too low and it takes far longer. 0.00025 is \
         where the Atari work settled.",
    ),
    (
        "discount",
        "How much a reward a step later counts against one now. 0.99 judges a choice \
         against the next hundred steps, which suits rewards that come long after \
         the move; lower suits a game that pays at once, and learns quicker.",
    ),
    (
        "trying",
        "A reward for staying undecided, so it goes on trying things. Too little and \
         it settles early on whatever worked first; too much and it never settles.",
    ),
    (
        "seed",
        "The number the randomness starts from. The same seed and the same set-up \
         play the same games, so a change can be told from luck.",
    ),
    (
        "lambda",
        "How far ahead the reward itself is trusted before the network's own guess \
         takes over. Lower leans on the guess, which is steadier but wronger early \
         on; 0.95 is the usual.",
    ),
    (
        "clip",
        "How far one update may move the chance of a choice. It is what keeps an \
         update from throwing the policy somewhere it cannot come back from.",
    ),
    (
        "passes",
        "How many times each update goes back over the steps it played, and how many \
         steps are learnt from at a time. More passes wring more out of the same \
         play, up to the point where the network starts chasing it.",
    ),
    (
        "value",
        "How much the network's guess at how well it is doing counts against \
         learning the moves themselves. The guess is what every choice is judged \
         against, so it matters, but too much of it crowds out the playing.",
    ),
    (
        "shown",
        "The number as the screen shows it, noughts and all — 001230 rather than \
         1230. The digits say how wide it is, which is what tells a score kept in \
         three bytes from the two bytes at its end.",
    ),
    (
        "keys",
        "The keys of the machine's own keyboard it may press, by the legends on \
         them, with commas between. Every key is one more thing to try, so name \
         only the ones the game plays with.",
    ),
];

/// What a box does, by the name it is known by here.
pub fn tip(name: &str) -> &'static str {
    TIPS.iter()
        .find(|(known, _)| *known == name)
        .map(|(_, text)| *text)
        .expect("every box has a word about what it does")
}

/// A number box of its own width, with what it does.
fn value_box(ui: &mut egui::Ui, drag: egui::DragValue<'_>, width: f32, name: &str) {
    let height = ui.spacing().interact_size.y;
    ui.add_sized(egui::vec2(width, height), drag)
        .on_hover_text(tip(name));
}

/// A box for a number written as the set-up file writes it.
fn address(ui: &mut egui::Ui, text: &mut String, hint: &str, name: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(230.0),
    )
    .on_hover_text(tip(name));
}

fn sight(t: &mut Training, ui: &mut egui::Ui) {
    theme::group_label(ui, "What it sees");
    let env = &mut t.setup.env;
    ui.horizontal_wrapped(|ui| {
        let s = &mut env.sight;
        let area_name = |a: Area| match a {
            Area::Display => "The display",
            Area::Television => "With a TV's border",
            Area::Overscan => "All the border",
        };
        theme::dropdown(ui, 160.0, area_name(s.area), |ui| {
            for a in [Area::Display, Area::Television, Area::Overscan] {
                if ui.selectable_label(s.area == a, area_name(a)).clicked() {
                    s.area = a;
                }
            }
        });
        let shrink_name = |n: usize| match n {
            1 => "Full size",
            2 => "Half size",
            _ => "Quarter size",
        };
        theme::dropdown(ui, 120.0, shrink_name(s.shrink), |ui| {
            for n in [1, 2, 4] {
                if ui.selectable_label(s.shrink == n, shrink_name(n)).clicked() {
                    s.shrink = n;
                }
            }
        });
        ui.checkbox(&mut s.colour, "Colour");
        ui.label("Frames");
        value_box(
            ui,
            egui::DragValue::new(&mut s.frames).range(1..=8),
            70.0,
            "frames",
        );
        note(
            ui,
            format!(
                "{}x{}{}",
                s.width(),
                s.height(),
                if s.colour { " in colour" } else { " in grey" }
            ),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Each choice is held for");
        value_box(
            ui,
            egui::DragValue::new(&mut env.frames_per_step).range(1..=50),
            70.0,
            "hold",
        );
        ui.label("frames; a game starts after up to");
        value_box(
            ui,
            egui::DragValue::new(&mut env.random_wait).range(0..=250),
            70.0,
            "wait",
        );
        ui.label("frames of nothing");
    });
}

fn reward(t: &mut Training, ui: &mut egui::Ui) {
    theme::group_label(ui, "The reward, read from memory");
    note(
        ui,
        "The judge reads these; the network never sees them. A number is written \
         byte ADDR, word ADDR, bcd ADDR BYTES or digits ADDR COUNT ZERO, in hex.",
    );
    let judge = &mut t.setup.env.judge;
    egui::Grid::new("training-reward")
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Score");
            address(ui, &mut t.score, "bcd 9C4E 3", "score");
            ui.label("a point");
            value_box(
                ui,
                egui::DragValue::new(&mut judge.score_scale)
                    .speed(0.01)
                    .range(0.0..=1000.0),
                110.0,
                "point",
            );
            ui.end_row();

            ui.label("Lives");
            address(ui, &mut t.lives, "byte 9C51", "lives");
            ui.label("a life");
            value_box(
                ui,
                egui::DragValue::new(&mut judge.life_penalty)
                    .speed(0.1)
                    .range(0.0..=1000.0),
                110.0,
                "life",
            );
            ui.end_row();

            let mut over = judge.over_at_lives.is_some();
            ui.checkbox(&mut over, "Over at").on_hover_text(tip("over"));
            let mut at = judge.over_at_lives.unwrap_or(0);
            ui.add_enabled_ui(over, |ui| {
                value_box(
                    ui,
                    egui::DragValue::new(&mut at).range(0..=255),
                    110.0,
                    "over",
                );
            });
            judge.over_at_lives = over.then_some(at);
            ui.label("lives left");
            ui.end_row();

            ui.label("Stop after");
            value_box(
                ui,
                egui::DragValue::new(&mut judge.max_steps).range(1..=1_000_000),
                110.0,
                "stop",
            );
            ui.label("a step");
            value_box(
                ui,
                egui::DragValue::new(&mut judge.per_step)
                    .speed(0.001)
                    .range(-10.0..=10.0),
                110.0,
                "step",
            );
            ui.end_row();
        });
}

fn learning(app: &mut App, ui: &mut egui::Ui) {
    theme::group_label(ui, "Learning");
    let filled: Vec<usize> = (0..10).filter(|n| app.quick[*n].is_some()).collect();
    let kept_start = app
        .notes_source()
        .as_deref()
        .map(worker::network_dir)
        .is_some_and(|d| d.join(worker::START).exists());
    let t = &mut app.training;
    let ppo = &mut t.setup.ppo;
    egui::Grid::new("training-learning")
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Games at once");
            value_box(
                ui,
                egui::DragValue::new(&mut ppo.games).range(1..=256),
                90.0,
                "games",
            );
            ui.label("steps each");
            value_box(
                ui,
                egui::DragValue::new(&mut ppo.steps).range(8..=4096),
                90.0,
                "steps",
            );
            ui.end_row();

            ui.label("Learning rate");
            value_box(
                ui,
                egui::DragValue::new(&mut ppo.learning_rate)
                    .speed(0.00001)
                    .range(0.000001..=0.1)
                    .max_decimals(6),
                90.0,
                "rate",
            );
            ui.label("discount").on_hover_text(tip("discount"));
            value_box(
                ui,
                egui::DragValue::new(&mut ppo.gamma)
                    .speed(0.001)
                    .range(0.5..=0.9999)
                    .max_decimals(4),
                90.0,
                "discount",
            );
            ui.end_row();

            ui.label("Trying things (entropy)")
                .on_hover_text(tip("trying"));
            value_box(
                ui,
                egui::DragValue::new(&mut ppo.entropy_weight)
                    .speed(0.001)
                    .range(0.0..=0.5)
                    .max_decimals(4),
                90.0,
                "trying",
            );
            ui.label("seed");
            value_box(ui, egui::DragValue::new(&mut ppo.seed), 90.0, "seed");
            ui.end_row();
        });
    egui::CollapsingHeader::new("More")
        .id_salt("training-more")
        .show(ui, |ui| {
            egui::Grid::new("training-more-grid")
                .num_columns(4)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Looking ahead (lambda)");
                    value_box(
                        ui,
                        egui::DragValue::new(&mut ppo.lambda)
                            .speed(0.01)
                            .range(0.0..=1.0),
                        90.0,
                        "lambda",
                    );
                    ui.label("clip");
                    value_box(
                        ui,
                        egui::DragValue::new(&mut ppo.clip)
                            .speed(0.01)
                            .range(0.01..=1.0),
                        90.0,
                        "clip",
                    );
                    ui.end_row();
                    ui.label("Passes each update");
                    value_box(
                        ui,
                        egui::DragValue::new(&mut ppo.epochs).range(1..=32),
                        90.0,
                        "passes",
                    );
                    ui.label("batch");
                    value_box(
                        ui,
                        egui::DragValue::new(&mut ppo.minibatch).range(8..=8192),
                        90.0,
                        "passes",
                    );
                    ui.end_row();
                    ui.label("Value weight (its guess at how well it is doing)");
                    value_box(
                        ui,
                        egui::DragValue::new(&mut ppo.value_weight)
                            .speed(0.01)
                            .range(0.0..=10.0),
                        90.0,
                        "value",
                    );
                    ui.end_row();
                });
        });
    ui.horizontal_wrapped(|ui| {
        ui.label("On the");
        let mut processor = t.processor;
        theme::dropdown(ui, 130.0, processor.name(), |ui| {
            for p in [Processor::Gpu, Processor::Cpu] {
                if ui.selectable_label(processor == p, p.name()).clicked() {
                    processor = p;
                }
            }
        });
        t.processor = processor;
        ui.label("starting every game from");
        let from_name = |f: From| match f {
            From::Now => "This machine".to_string(),
            From::Slot(n) => format!("Quick slot {n}"),
            From::Kept => "The kept start".to_string(),
        };
        let mut from = t.from;
        if matches!(from, From::Slot(n) if !filled.contains(&n))
            || (from == From::Kept && !kept_start)
        {
            from = From::Now;
        }
        theme::dropdown(ui, 130.0, from_name(from), |ui| {
            if ui
                .selectable_label(from == From::Now, from_name(From::Now))
                .clicked()
            {
                from = From::Now;
            }
            for n in &filled {
                let slot = From::Slot(*n);
                if ui.selectable_label(from == slot, from_name(slot)).clicked() {
                    from = slot;
                }
            }
            if kept_start
                && ui
                    .selectable_label(from == From::Kept, from_name(From::Kept))
                    .on_hover_text("Where the kept network's games started, kept beside it")
                    .clicked()
            {
                from = From::Kept;
            }
        });
        t.from = from;
    });
}

fn buttons(app: &mut App, ui: &mut egui::Ui, running: bool, problems: &[String]) {
    let source = app.notes_source();
    let keep_in = source.as_deref().map(worker::network_dir);
    let kept = keep_in
        .as_ref()
        .is_some_and(|d| d.join(format!("{}.bin", worker::NETWORK)).exists());
    ui.horizontal_wrapped(|ui| {
        if running {
            if ui.button("■ Stop").clicked() {
                if let Some(w) = app.training.worker.as_mut() {
                    w.stop();
                }
            }
            if ui
                .add_enabled(keep_in.is_some(), egui::Button::new("Keep now"))
                .on_hover_text("Keep the network at the end of this update")
                .clicked()
            {
                if let Some(w) = &app.training.worker {
                    w.keep_now();
                }
            }
        } else {
            let timing = app.training.is_timing();
            let start = ui
                .add_enabled(problems.is_empty() && !timing, egui::Button::new("▶ Start"))
                .on_hover_text("Start learning, with every game starting from the machine chosen");
            if start.clicked() {
                begin(app, keep_in.clone());
            }
            ui.add_enabled(
                kept,
                egui::Checkbox::new(&mut app.training.resume, "Go on from the kept network"),
            )
            .on_hover_text("Rather than a fresh network: it has to have been made for the same picture and actions");
            let label = if timing { "Timing…" } else { "Time it" };
            if ui
                .add_enabled(problems.is_empty() && !timing, egui::Button::new(label))
                .on_hover_text(
                    "Run two updates of this set-up and say how long the second took, \
                     so the wait is known before starting",
                )
                .clicked()
            {
                time_it(app, keep_in.clone());
            }
        }
        theme::divider(ui);
        if ui
            .add_enabled(source.is_some() && !running, egui::Button::new("Save set-up"))
            .on_hover_text("Write the set-up beside the tape, where it is read from next time")
            .clicked()
        {
            save_setup(app);
        }
        if ui
            .add_enabled(!running, egui::Button::new("Load set-up…"))
            .clicked()
        {
            load_setup(app);
        }
    });
    for p in problems {
        ui.label(egui::RichText::new(p).small().color(theme::RED));
    }
    // Only while the set-up is still the one that was timed: a figure for
    // another size of network says nothing about this one.
    let t = &app.training;
    if let Some((seconds, setup, processor)) = &t.timed {
        if *setup == t.setup && *processor == t.processor {
            note(
                ui,
                format!(
                    "An update of this set-up takes {seconds:.1} s on the {}, measured: the \
                     second of two, after one to warm up",
                    processor.name().to_lowercase()
                ),
            );
        }
    }
    match &keep_in {
        Some(dir) => note(ui, format!("The network is kept in {}", dir.display())),
        None => note(
            ui,
            "Load a tape or a snapshot to keep the network beside it",
        ),
    }
    if let Some((text, bad)) = &app.training.message {
        let colour = if *bad { theme::RED } else { theme::DIM };
        ui.label(egui::RichText::new(text).small().color(colour));
    }
}

/// The machine the kept network's games started from, put into a copy of
/// this one — a snapshot holds no ROM — with what the snapshot could not put
/// back. A snapshot of another model is refused with the reason.
pub fn kept_start(spec: &Spectrum, keep_in: Option<&Path>) -> Result<(Spectrum, String), String> {
    let path = keep_in
        .map(|d| d.join(worker::START))
        .ok_or("Nothing is kept: load the tape the network was trained on")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut machine = spec.clone();
    let note =
        crate::szx::load(&mut machine, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((machine, note))
}

/// The machine every game of a run starts as, and anything to say about it.
fn start_machine(app: &App, keep_in: Option<&Path>) -> Result<(Spectrum, String), String> {
    match app.training.from {
        From::Now => Ok((app.spec.clone(), String::new())),
        From::Slot(n) => app.quick[n]
            .as_ref()
            .map(|m| ((**m).clone(), String::new()))
            .ok_or_else(|| "That quick slot is empty".to_string()),
        From::Kept => kept_start(&app.spec, keep_in),
    }
}

/// Run two updates of the set-up as it stands, to see how long one takes.
fn time_it(app: &mut App, keep_in: Option<PathBuf>) {
    let mut machine = match start_machine(app, keep_in.as_deref()) {
        Ok((m, _)) => m,
        Err(e) => {
            app.training.message = Some((e, true));
            return;
        }
    };
    machine.bus.audio.detach();
    let t = &mut app.training;
    t.timed = None;
    let worker = Worker::start(Start {
        machine,
        setup: t.setup.clone(),
        processor: t.processor,
        resume: None,
        keep_in: None,
        updates: Some(2),
    });
    t.timing = Some((worker, t.setup.clone(), t.processor));
}

/// Start a run.
fn begin(app: &mut App, keep_in: Option<PathBuf>) {
    let machine = start_machine(app, keep_in.as_deref());
    let (mut machine, note) = match machine {
        Ok(m) => m,
        Err(e) => {
            app.training.message = Some((e, true));
            return;
        }
    };
    // The copy would otherwise share the sound queue, and be heard.
    machine.bus.audio.detach();
    if app.notes_source().is_some() {
        save_setup(app);
    }
    if !note.is_empty() {
        app.training.message = Some((format!("The kept start: {note}"), false));
    }
    let t = &mut app.training;
    let resume = keep_in
        .clone()
        .filter(|d| t.resume && d.join(format!("{}.bin", worker::NETWORK)).exists());
    t.worker = Some(Worker::start(Start {
        machine,
        setup: t.setup.clone(),
        processor: t.processor,
        resume,
        keep_in,
        updates: None,
    }));
    t.started = Some(Instant::now());
    t.preview = None;
}

fn save_setup(app: &mut App) {
    let Some(path) = app.notes_source().as_deref().map(Setup::sidecar) else {
        return;
    };
    let t = &mut app.training;
    t.message = Some(match std::fs::write(&path, t.setup.to_text()) {
        Ok(()) => (format!("Set-up written to {}", path.display()), false),
        Err(e) => (format!("{}: {e}", path.display()), true),
    });
}

fn load_setup(app: &mut App) {
    let mut dialog = rfd::FileDialog::new().add_filter("Training set-up", &["txt"]);
    if let Some(dir) = app.notes_source().as_deref().and_then(Path::parent) {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.pick_file() else {
        return;
    };
    let t = &mut app.training;
    t.message = Some(match t.read(&path) {
        Ok(()) => (format!("Set-up read from {}", path.display()), false),
        Err(e) => (e, true),
    });
}

fn progress(t: &Training, ui: &mut egui::Ui) {
    let Some(worker) = &t.worker else {
        return;
    };
    let shared = worker.shared();
    theme::group_label(ui, "How it is going");
    if let Some(e) = &shared.error {
        ui.label(egui::RichText::new(e).color(theme::RED));
    }
    chart(ui, &shared.history);
    match shared.history.last() {
        Some(p) => {
            let seconds = t.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
            ui.label(format!(
                "{} updates, {} steps, {} games finished, {:.0} steps a second, {:.1} s an update",
                p.updates,
                p.steps,
                p.games_finished,
                if seconds > 0.0 {
                    p.steps as f64 / seconds
                } else {
                    0.0
                },
                p.seconds
            ));
            ui.label(format!(
                "Reward a step {:.3}, a whole game {}; undecidedness {:.2}",
                p.step_reward,
                p.recent_game_reward
                    .map_or("—".to_string(), |r| format!("{r:.1}")),
                p.entropy
            ));
        }
        None if worker.is_running() => note(
            ui,
            "Playing: the first update comes once every game has played its steps",
        ),
        None => {}
    }
    if let Some(dir) = &shared.saved {
        note(ui, format!("Kept in {}", dir.display()));
    }
}

/// Updates the trend line averages over. One update's reward is mostly the
/// luck of that update's play; twenty is enough to see past it and few
/// enough that a real change shows within a few minutes.
pub const TREND: usize = 20;

/// Each point's average with the ones before it, up to `window` of them:
/// fewer at the start, where there are not yet that many to average.
pub fn trend(values: &[f32], window: usize) -> Vec<f32> {
    let window = window.max(1);
    let mut sum = 0f32;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            sum += v;
            if i >= window {
                sum -= values[i - window];
            }
            sum / (i + 1).min(window) as f32
        })
        .collect()
}

/// The reward a step, update by update, and its trend.
fn chart(ui: &mut egui::Ui, history: &[Progress]) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 110.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::LCD_BG);
    let font = egui::FontId::proportional(10.0);
    if history.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "The reward a step is drawn here, update by update",
            font,
            theme::DIM,
        );
        return;
    }
    let values: Vec<f32> = history.iter().map(|p| p.step_reward).collect();
    let lo = values
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min)
        .min(0.0);
    let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (hi - lo).max(1e-6);
    let inner = rect.shrink2(egui::vec2(8.0, 10.0));
    let line = |values: &[f32]| -> Vec<egui::Pos2> {
        values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                egui::pos2(
                    inner.left() + inner.width() * i as f32 / (values.len() - 1) as f32,
                    inner.bottom() - inner.height() * (v - lo) / span,
                )
            })
            .collect()
    };
    // Each update dimmed, since most of its ups and downs are luck; the
    // trend drawn over it is what says whether it is learning.
    painter.add(egui::Shape::line(
        line(&values),
        egui::Stroke::new(1.0, theme::LCD_FG.gamma_multiply(0.45)),
    ));
    painter.add(egui::Shape::line(
        line(&trend(&values, TREND)),
        egui::Stroke::new(2.5, theme::AMBER),
    ));
    painter.text(
        rect.right_top() + egui::vec2(-4.0, 2.0),
        egui::Align2::RIGHT_TOP,
        format!("average of the last {TREND}"),
        font.clone(),
        theme::AMBER,
    );
    painter.text(
        rect.right_top() + egui::vec2(-4.0, 14.0),
        egui::Align2::RIGHT_TOP,
        "each update",
        font.clone(),
        theme::LCD_FG.gamma_multiply(0.6),
    );
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("{hi:.3}"),
        font.clone(),
        theme::LCD_FG,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(4.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{lo:.3}"),
        font,
        theme::LCD_FG,
    );
}

/// One of the games as it is played, and what the network made of it.
fn watching(t: &mut Training, ui: &mut egui::Ui) {
    let Training {
        worker,
        preview,
        seen,
        setup,
        ..
    } = t;
    let Some(worker) = worker else {
        return;
    };
    let shared = worker.shared();
    let Some(p) = &shared.preview else {
        return;
    };
    theme::group_label(ui, "One of the games");
    let image = egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.picture);
    let tex = match preview {
        Some(tex) => {
            tex.set(image, egui::TextureOptions::NEAREST);
            tex
        }
        None => preview.insert(ui.ctx().load_texture(
            "training-preview",
            image,
            egui::TextureOptions::NEAREST,
        )),
    };
    ui.horizontal_top(|ui| {
        draw(ui, tex, p.width as f32, p.height as f32);
        bars(ui, &setup.env.inputs.actions, &p.probabilities, p.action);
    });
    frames(ui, seen, p);
}

/// Each action and how much the network wanted it, the one taken lit.
fn bars(ui: &mut egui::Ui, actions: &[Action], probabilities: &[f32], taken: usize) {
    ui.vertical(|ui| {
        for (i, (action, prob)) in actions.iter().zip(probabilities).enumerate() {
            let chosen = i == taken;
            let (bar, _) = ui.allocate_exact_size(egui::vec2(170.0, 14.0), egui::Sense::hover());
            let painter = ui.painter_at(bar);
            painter.rect_filled(bar, 2.0, theme::CASE_DARK);
            let filled = egui::Rect::from_min_size(
                bar.min,
                egui::vec2(bar.width() * prob.clamp(0.0, 1.0), bar.height()),
            );
            painter.rect_filled(
                filled,
                2.0,
                if chosen {
                    theme::AMBER
                } else {
                    theme::CASE_LIGHT
                },
            );
            painter.text(
                bar.left_center() + egui::vec2(4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{} {:.0}%", action.name, prob * 100.0),
                egui::FontId::proportional(10.0),
                if chosen { theme::ON_LIT } else { theme::INK },
            );
        }
    });
}

/// Put a texture on the screen at its own size.
fn draw(ui: &mut egui::Ui, tex: &egui::TextureHandle, w: f32, h: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().image(
        tex.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

/// One frame of the stack, as the network is shown it.
fn frame_image(bytes: &[u8], sight: &Sight) -> egui::ColorImage {
    let (w, h) = (sight.width(), sight.height());
    let mut rgba = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) * 4;
            if sight.channels() == 3 {
                for c in 0..3 {
                    rgba[at + c] = bytes[c * w * h + y * w + x];
                }
            } else {
                let grey = bytes[y * w + x];
                rgba[at..at + 3].fill(grey);
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

/// The frames the network is shown, oldest first, each with how much of it
/// moved since the one before. Four frames alike mean the stack says nothing
/// about movement — the game drew nothing new between the steps, so a longer
/// step or fewer frames would show it more.
fn frames(ui: &mut egui::Ui, kept: &mut Vec<egui::TextureHandle>, p: &Preview) {
    let sight = &p.sight;
    let len = sight.frame_len();
    if len == 0 || p.seen.len() < len {
        return;
    }
    let moved = changes(&p.seen, sight);
    theme::group_label(ui, "What it sees");
    let (w, h) = (sight.width() as f32, sight.height() as f32);
    ui.horizontal_wrapped(|ui| {
        for (i, frame) in p.seen.chunks_exact(len).enumerate() {
            let image = frame_image(frame, sight);
            match kept.get_mut(i) {
                Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                None => kept.push(ui.ctx().load_texture(
                    format!("training-seen-{i}"),
                    image,
                    egui::TextureOptions::NEAREST,
                )),
            }
            ui.vertical(|ui| {
                draw(ui, &kept[i], w, h);
                let back = p.seen.len() / len - 1 - i;
                let when = match back {
                    0 => "now".to_string(),
                    1 => "a step back".to_string(),
                    n => format!("{n} steps back"),
                };
                match i.checked_sub(1).and_then(|k| moved.get(k)) {
                    Some(0.0) => {
                        ui.label(
                            egui::RichText::new(format!("{when} — the same"))
                                .small()
                                .color(theme::AMBER),
                        );
                    }
                    Some(share) => note(ui, format!("{when} — {:.0}% moved", share * 100.0)),
                    None => note(ui, when),
                }
            });
        }
    });
    if moved.iter().all(|share| *share == 0.0) && !moved.is_empty() {
        ui.label(
            egui::RichText::new(
                "Every frame is the same: the stack is telling the network nothing about \
                 movement. Hold each choice for more frames, or watch a part of the screen \
                 that changes.",
            )
            .small()
            .color(theme::AMBER),
        );
    }
}

/// The kept network given the machine's controls, and taken back from it.
fn play(app: &mut App, ui: &mut egui::Ui) {
    theme::group_label(ui, "Letting it play");
    let kept = app
        .notes_source()
        .as_deref()
        .map(worker::network_dir)
        .filter(|d| d.join(format!("{}.bin", worker::NETWORK)).exists());
    ui.horizontal_wrapped(|ui| {
        if app.training.player.is_some() {
            if ui.button("Take the controls back").clicked() {
                take_back(app);
            }
        } else if ui
            .add_enabled(kept.is_some(), egui::Button::new("Let it play"))
            .on_hover_text(
                "Give the kept network the machine's controls: the keyboard and the stick \
                 on the desk are not read while it plays",
            )
            .clicked()
        {
            let dir = kept.clone().expect("the button waits for one");
            app.training.message = Some(match Player::load(&dir) {
                Ok(mut player) => {
                    player.greedy = app.training.greedy;
                    player.prepare(&mut app.spec);
                    app.training.player = Some(player);
                    app.running = true;
                    ("The network is playing".into(), false)
                }
                Err(e) => (e, true),
            });
        }
        let t = &mut app.training;
        if ui
            .checkbox(&mut t.greedy, "Its favourite every time")
            .on_hover_text(
                "Take the action it likes best rather than drawing one as it did while \
                 learning; it can get stuck doing one thing forever",
            )
            .changed()
        {
            if let Some(player) = t.player.as_mut() {
                player.greedy = t.greedy;
            }
        }
    });
    if kept.is_none() {
        note(
            ui,
            "Nothing is kept yet: a run keeps its network beside the tape",
        );
    }
    if let Some(player) = &app.training.player {
        if let Some((taken, probabilities)) = &player.last {
            bars(
                ui,
                &player.setup().env.inputs.actions,
                probabilities,
                *taken,
            );
        }
    }
}

/// Take the controls back from the network, letting go of whatever it held.
pub fn take_back(app: &mut App) {
    if let Some(player) = app.training.player.take() {
        player.release(&mut app.spec);
    }
}

/// Looking for where the game keeps a number — for the judge, never the
/// network — by saying what the number on the screen did.
fn find(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Find a number")
        .id_salt("training-find")
        .show(ui, |ui| {
            note(
                ui,
                "Start looking, play until the number on the screen changes, and say \
                 what it did. Say what it is now as the screen shows it, noughts and \
                 all, to tell how many bytes it takes.",
            );
            let spec = &app.spec;
            let t = &mut app.training;
            let peek = |a: u16| spec.bus.peek_raw(a);
            let shown = Test::is_shown(&t.search_is);
            let mut test = None;
            ui.horizontal_wrapped(|ui| {
                if ui.button("Start looking").clicked() {
                    t.search = Some(Search::new(&peek));
                }
                ui.add_enabled_ui(t.search.is_some(), |ui| {
                    for (label, what) in [
                        ("Went up", Test::Up),
                        ("Went down", Test::Down),
                        ("Changed", Test::Changed),
                        ("Did not change", Test::Same),
                    ] {
                        if ui.button(label).clicked() {
                            test = Some(what);
                        }
                    }
                    if ui
                        .add_enabled(shown.is_ok(), egui::Button::new("Is now"))
                        .clicked()
                    {
                        test = shown.clone().ok();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut t.search_is)
                            .hint_text("001230")
                            .desired_width(110.0),
                    )
                    .on_hover_text(tip("shown"));
                });
            });
            let Some(search) = t.search.as_mut() else {
                return;
            };
            if let Some(test) = test {
                search.narrow(test, &peek);
            }
            // Copied only once there are few enough to list: right after
            // Start looking there are hundreds of thousands.
            let count = search.candidates().len();
            note(
                ui,
                format!(
                    "{} way{} it could be kept left after {} round{}",
                    count,
                    if count == 1 { "" } else { "s" },
                    search.rounds,
                    if search.rounds == 1 { "" } else { "s" }
                ),
            );
            if count > 12 {
                if search.rounds > 0 {
                    note(
                        ui,
                        "Too many to list: say what it is now, noughts and all, or play on",
                    );
                }
                return;
            }
            let left: Vec<Candidate> = search.candidates().to_vec();
            let mut chosen = None;
            for c in &left {
                ui.horizontal(|ui| {
                    let reads = c
                        .number()
                        .map_or("?".to_string(), |n| n.read(&peek).to_string());
                    ui.label(
                        egui::RichText::new(format!("{}  reads {reads}", c.describe())).monospace(),
                    );
                    let usable = c.number().is_some();
                    let why = "Say what the number is now, as the screen shows it, to settle \
                               which byte stands for nought";
                    if ui
                        .add_enabled(usable, egui::Button::new("Use as score").small())
                        .on_disabled_hover_text(why)
                        .clicked()
                    {
                        chosen = c.number().map(|n| (n, true));
                    }
                    if ui
                        .add_enabled(usable, egui::Button::new("Use as lives").small())
                        .on_disabled_hover_text(why)
                        .clicked()
                    {
                        chosen = c.number().map(|n| (n, false));
                    }
                });
            }
            match chosen {
                Some((n, true)) => t.score = n.to_text(),
                Some((n, false)) => t.lives = n.to_text(),
                None => {}
            }
        });
}
