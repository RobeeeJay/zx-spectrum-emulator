//! The Joystick window: which interface the stick is plugged into, and what on
//! the desk works it.
//!
//! The machine has no joystick port, so an interface is a choice rather than a
//! fact — see `crate::joystick` for the four of them. What is here is that
//! choice, and the mapping from the keys of the desk to the five switches of a
//! stick, or to keys of the machine's own keyboard for the games that want a
//! key rather than a stick.
//!
//! A gamepad would be the obvious thing to map instead, and cannot be: reading
//! one needs a crate that is not in the lock file, and nothing may be added to
//! it. The mapping is written as a source and an action for that reason — a
//! pad's buttons would be more sources, and nothing else here would change.

use eframe::egui;

use crate::joystick::{Kind, Way};
use crate::ui::theme;
use crate::ui::App;

/// What a binding does when its key goes down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Does {
    /// One of the stick's five switches.
    Way(Way),
    /// A key of the machine's own keyboard, by where it is in the matrix.
    Key(usize, u8),
}

/// One line of the mapping: a key of the desk, and what it works.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub from: egui::Key,
    pub does: Does,
}

/// What a machine with nothing set up gets: the arrow keys and the space bar,
/// which is what a game expects a stick to be.
pub fn defaults() -> Vec<Binding> {
    vec![
        Binding {
            from: egui::Key::ArrowLeft,
            does: Does::Way(Way::Left),
        },
        Binding {
            from: egui::Key::ArrowRight,
            does: Does::Way(Way::Right),
        },
        Binding {
            from: egui::Key::ArrowUp,
            does: Does::Way(Way::Up),
        },
        Binding {
            from: egui::Key::ArrowDown,
            does: Does::Way(Way::Down),
        },
        Binding {
            from: egui::Key::Space,
            does: Does::Way(Way::Fire),
        },
    ]
}

/// What a binding does, as a line of text.
pub fn describes(does: Does) -> String {
    match does {
        Does::Way(way) => way.name().to_string(),
        Does::Key(row, bit) => crate::keyboard::SPECTRUM
            .iter()
            .find(|k| k.press == [(row, bit)])
            .map(|k| format!("the {} key", k.main))
            .unwrap_or_else(|| format!("key at row {row} bit {bit}")),
    }
}

/// Saved as `key:action`, so the file can be read and edited.
pub fn to_text(bindings: &[Binding]) -> String {
    bindings
        .iter()
        .map(|b| {
            let does = match b.does {
                Does::Way(way) => way.name().to_string(),
                Does::Key(row, bit) => format!("key{row}.{bit}"),
            };
            format!("{}:{does}", key_name(b.from))
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn from_text(text: &str) -> Vec<Binding> {
    let mut out = Vec::new();
    for piece in text.split(',').filter(|p| !p.trim().is_empty()) {
        let Some((from, does)) = piece.split_once(':') else {
            continue;
        };
        let Some(from) = key_from_name(from.trim()) else {
            continue;
        };
        let does = match does.trim() {
            "left" => Does::Way(Way::Left),
            "right" => Does::Way(Way::Right),
            "up" => Does::Way(Way::Up),
            "down" => Does::Way(Way::Down),
            "fire" => Does::Way(Way::Fire),
            other => match other.strip_prefix("key").and_then(|r| r.split_once('.')) {
                Some((row, bit)) => match (row.parse::<usize>(), bit.parse::<u8>()) {
                    (Ok(row), Ok(bit)) if row < 8 && bit < 5 => Does::Key(row, bit),
                    _ => continue,
                },
                None => continue,
            },
        };
        out.push(Binding { from, does });
    }
    out
}

/// egui's own name for a key, which is what goes in the preferences.
fn key_name(key: egui::Key) -> &'static str {
    key.name()
}

fn key_from_name(name: &str) -> Option<egui::Key> {
    egui::Key::ALL
        .iter()
        .copied()
        .find(|k| k.name().eq_ignore_ascii_case(name))
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Interface");
        let mut kind = app.spec.bus.joystick.kind;
        theme::dropdown(ui, 96.0, kind.name(), |ui| {
            for option in Kind::ALL {
                if ui
                    .selectable_label(kind == option, option.name())
                    .on_hover_text(option.how())
                    .clicked()
                {
                    kind = option;
                    ui.close();
                }
            }
        });
        if kind != app.spec.bus.joystick.kind {
            // Whatever was over is let go: a direction held on an interface
            // nobody is reading any more would be held for ever.
            app.spec.bus.joystick.release();
            app.spec.bus.joystick.kind = kind;
        }
    });
    ui.label(
        egui::RichText::new(app.spec.bus.joystick.kind.how())
            .small()
            .color(theme::DIM),
    );
    ui.add_space(6.0);

    // What the stick is doing now, so a mapping can be checked without a game.
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Now");
        for way in Way::ALL {
            let down = app.spec.bus.joystick.is_down(way);
            ui.label(egui::RichText::new(way.name()).small().color(if down {
                theme::AMBER
            } else {
                theme::DIM
            }));
        }
        if app.spec.bus.joystick.kind == Kind::None {
            ui.label(
                egui::RichText::new("— nothing is plugged in, so the machine sees none of it")
                    .small()
                    .color(theme::DIM),
            );
        }
    });
    ui.add_space(6.0);

    theme::group_label(ui, "Keys");
    ui.label(
        egui::RichText::new(
            "A key here works the stick instead of the machine's own keyboard. Press Set and \
             then the key you want.",
        )
        .small()
        .color(theme::DIM),
    );
    ui.add_space(4.0);

    let mut remove = None;
    let mut rebind = None;
    for (i, binding) in app.joystick_map.clone().iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            let waiting = app.joystick_binding == Some(i);
            if theme::selectable(ui, waiting, if waiting { "press a key…" } else { "Set" })
                .clicked()
            {
                rebind = Some(if waiting { usize::MAX } else { i });
            }
            ui.label(egui::RichText::new(format!("{:<12}", key_name(binding.from))).monospace());
            ui.label(
                egui::RichText::new(describes(binding.does))
                    .small()
                    .color(theme::INK),
            );
            if theme::selectable(ui, false, "×")
                .on_hover_text("Take this one out")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = rebind {
        app.joystick_binding = if i == usize::MAX { None } else { Some(i) };
    }
    if let Some(i) = remove {
        app.joystick_map.remove(i);
        app.joystick_binding = None;
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Add");
        for way in Way::ALL {
            if theme::selectable(ui, false, way.name()).clicked() {
                app.joystick_map.push(Binding {
                    from: egui::Key::Num0,
                    does: Does::Way(way),
                });
                app.joystick_binding = Some(app.joystick_map.len() - 1);
            }
        }
        if theme::selectable(ui, false, "a machine key").clicked() {
            // Somewhere to start: the space bar, which is the key most games
            // want when they do not want a stick.
            app.joystick_map.push(Binding {
                from: egui::Key::Num0,
                does: Does::Key(7, 0),
            });
            app.joystick_binding = Some(app.joystick_map.len() - 1);
        }
    });
    if let Some(i) = app.joystick_binding {
        if let Some(binding) = app.joystick_map.get(i) {
            if let Does::Key(..) = binding.does {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    theme::group_label(ui, "Which key");
                    for key in crate::keyboard::SPECTRUM
                        .iter()
                        .filter(|k| k.press.len() == 1)
                    {
                        if theme::selectable(ui, false, key.main).clicked() {
                            let (row, bit) = key.press[0];
                            app.joystick_map[i].does = Does::Key(row, bit);
                        }
                    }
                });
            }
        }
    }

    if app.joystick_map.is_empty() {
        ui.add_space(6.0);
        if theme::selectable(ui, false, "Put the arrow keys back").clicked() {
            app.joystick_map = defaults();
        }
    }
}
