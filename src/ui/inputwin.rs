//! The Input window: which interface the stick is plugged into, and what on
//! the desk works it — the stick's switches, keys of the machine's own
//! keyboard, and the buttons of the two mice.
//!
//! The machine has no joystick port, so an interface is a choice rather than a
//! fact — see `crate::joystick` for the four of them. What is here is that
//! choice, and the mapping from the keys of the desk to the five switches of a
//! stick, or to keys of the machine's own keyboard for the games that want a
//! key rather than a stick.
//!
//! A binding is a source and an action: the source is a key of the desk or a
//! control on a gamepad, and the action is one of the stick's five switches or
//! a key of the machine's own keyboard. Pads are read through `gilrs`, which
//! is the one crate in the lock file that is not there for the build's own
//! sake — see the note in CLAUDE.md.

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
    /// A button on one of the mice.
    Mouse(MouseButton),
}

/// The buttons of the two mice: a binding can hold one down as the host's
/// mouse button would, which is how a pad plays a game written for a mouse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    KempstonLeft,
    KempstonRight,
    AmxLeft,
    AmxMiddle,
    AmxRight,
}

impl MouseButton {
    pub const ALL: [MouseButton; 5] = [
        MouseButton::KempstonLeft,
        MouseButton::KempstonRight,
        MouseButton::AmxLeft,
        MouseButton::AmxMiddle,
        MouseButton::AmxRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MouseButton::KempstonLeft => "Kempston left",
            MouseButton::KempstonRight => "Kempston right",
            MouseButton::AmxLeft => "AMX left",
            MouseButton::AmxMiddle => "AMX middle",
            MouseButton::AmxRight => "AMX right",
        }
    }

    /// How it is written in the preferences.
    fn key(self) -> &'static str {
        match self {
            MouseButton::KempstonLeft => "kmouse.left",
            MouseButton::KempstonRight => "kmouse.right",
            MouseButton::AmxLeft => "amx.left",
            MouseButton::AmxMiddle => "amx.middle",
            MouseButton::AmxRight => "amx.right",
        }
    }

    /// The interface the button is on.
    pub fn on(self) -> crate::hardware::Peripheral {
        match self {
            MouseButton::KempstonLeft | MouseButton::KempstonRight => {
                crate::hardware::Peripheral::KempstonMouse
            }
            _ => crate::hardware::Peripheral::AmxMouse,
        }
    }
}

/// A control on a gamepad: a button, or a stick pushed one way.
///
/// Named in the crate's own terms, since it is the crate that reports them,
/// but written to the preferences as text of our own so that a file stays
/// readable if the crate renames anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pad {
    Button(gilrs::Button),
    /// An axis and which way along it: a stick has two directions per axis.
    Axis(gilrs::Axis, bool),
}

/// Where a binding comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum From {
    Key(egui::Key),
    Pad(Pad),
}

/// One line of the mapping: something to press, and what it works.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub from: From,
    pub does: Does,
}

/// How far a stick has to go before it counts as pushed. Sticks rest a little
/// off centre and a game cannot be played with a stick that is always over.
pub const DEADZONE: f32 = 0.5;

/// What the pads are doing this frame, which is what a binding is tested
/// against. Taken as a snapshot so the deciding can be tested without a pad.
#[derive(Clone, Default, Debug)]
pub struct Pads {
    pub buttons: Vec<gilrs::Button>,
    pub axes: Vec<(gilrs::Axis, f32)>,
    /// How many are plugged in, for the window to say.
    pub count: usize,
}

impl Pads {
    /// Whether a control is over. An axis counts once it is past the deadzone
    /// in the direction the binding asked for.
    pub fn holding(&self, pad: Pad) -> bool {
        match pad {
            Pad::Button(button) => self.buttons.contains(&button),
            Pad::Axis(axis, positive) => self.axes.iter().any(|(a, value)| {
                *a == axis
                    && if positive {
                        *value > DEADZONE
                    } else {
                        *value < -DEADZONE
                    }
            }),
        }
    }
}

/// What a machine with nothing set up gets: the arrow keys and the space bar,
/// which is what a game expects a stick to be.
pub fn defaults() -> Vec<Binding> {
    let key = |key, does| Binding {
        from: From::Key(key),
        does,
    };
    let pad = |pad, does| Binding {
        from: From::Pad(pad),
        does,
    };
    vec![
        key(egui::Key::ArrowLeft, Does::Way(Way::Left)),
        key(egui::Key::ArrowRight, Does::Way(Way::Right)),
        key(egui::Key::ArrowUp, Does::Way(Way::Up)),
        key(egui::Key::ArrowDown, Does::Way(Way::Down)),
        key(egui::Key::Space, Does::Way(Way::Fire)),
        // And a pad, on the controls a pad has for this: the left stick and
        // the d-pad both steer, and the bottom face button fires.
        pad(Pad::Button(gilrs::Button::DPadLeft), Does::Way(Way::Left)),
        pad(Pad::Button(gilrs::Button::DPadRight), Does::Way(Way::Right)),
        pad(Pad::Button(gilrs::Button::DPadUp), Does::Way(Way::Up)),
        pad(Pad::Button(gilrs::Button::DPadDown), Does::Way(Way::Down)),
        pad(
            Pad::Axis(gilrs::Axis::LeftStickX, false),
            Does::Way(Way::Left),
        ),
        pad(
            Pad::Axis(gilrs::Axis::LeftStickX, true),
            Does::Way(Way::Right),
        ),
        pad(Pad::Axis(gilrs::Axis::LeftStickY, true), Does::Way(Way::Up)),
        pad(
            Pad::Axis(gilrs::Axis::LeftStickY, false),
            Does::Way(Way::Down),
        ),
        pad(Pad::Button(gilrs::Button::South), Does::Way(Way::Fire)),
    ]
}

/// The pad controls that can be named, and what they are called in a
/// preferences file and in the window.
const BUTTONS: [(gilrs::Button, &str); 15] = [
    (gilrs::Button::South, "A"),
    (gilrs::Button::East, "B"),
    (gilrs::Button::North, "Y"),
    (gilrs::Button::West, "X"),
    (gilrs::Button::LeftTrigger, "L1"),
    (gilrs::Button::LeftTrigger2, "L2"),
    (gilrs::Button::RightTrigger, "R1"),
    (gilrs::Button::RightTrigger2, "R2"),
    (gilrs::Button::Select, "Select"),
    (gilrs::Button::Start, "Start"),
    (gilrs::Button::Mode, "Mode"),
    (gilrs::Button::LeftThumb, "L3"),
    (gilrs::Button::RightThumb, "R3"),
    (gilrs::Button::DPadUp, "DPadUp"),
    (gilrs::Button::DPadDown, "DPadDown"),
];

const MORE_BUTTONS: [(gilrs::Button, &str); 2] = [
    (gilrs::Button::DPadLeft, "DPadLeft"),
    (gilrs::Button::DPadRight, "DPadRight"),
];

const AXES: [(gilrs::Axis, &str); 4] = [
    (gilrs::Axis::LeftStickX, "LeftX"),
    (gilrs::Axis::LeftStickY, "LeftY"),
    (gilrs::Axis::RightStickX, "RightX"),
    (gilrs::Axis::RightStickY, "RightY"),
];

fn button_name(button: gilrs::Button) -> Option<&'static str> {
    BUTTONS
        .iter()
        .chain(MORE_BUTTONS.iter())
        .find(|(b, _)| *b == button)
        .map(|(_, name)| *name)
}

fn axis_name(axis: gilrs::Axis) -> Option<&'static str> {
    AXES.iter().find(|(a, _)| *a == axis).map(|(_, name)| *name)
}

/// What a pad control is called, which is also how it is written down.
pub fn pad_name(pad: Pad) -> String {
    match pad {
        Pad::Button(button) => button_name(button)
            .map(|n| format!("pad {n}"))
            .unwrap_or_else(|| format!("pad {button:?}")),
        Pad::Axis(axis, positive) => {
            let name = axis_name(axis).unwrap_or("axis");
            format!("pad {name}{}", if positive { "+" } else { "-" })
        }
    }
}

fn pad_from_name(name: &str) -> Option<Pad> {
    let name = name.trim();
    if let Some((axis, way)) = name
        .strip_suffix('+')
        .map(|a| (a, true))
        .or_else(|| name.strip_suffix('-').map(|a| (a, false)))
    {
        if let Some((axis, _)) = AXES.iter().find(|(_, n)| n.eq_ignore_ascii_case(axis)) {
            return Some(Pad::Axis(*axis, way));
        }
    }
    BUTTONS
        .iter()
        .chain(MORE_BUTTONS.iter())
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .map(|(b, _)| Pad::Button(*b))
}

/// What a binding is worked by, as a line of text.
pub fn from_name(from: From) -> String {
    match from {
        From::Key(key) => key_name(key).to_string(),
        From::Pad(pad) => pad_name(pad),
    }
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
        Does::Mouse(button) => format!("the {} mouse button", button.name()),
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
                Does::Mouse(button) => button.key().to_string(),
            };
            let from = match b.from {
                From::Key(key) => key_name(key).to_string(),
                From::Pad(pad) => format!("pad.{}", pad_name(pad).trim_start_matches("pad ")),
            };
            format!("{from}:{does}")
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
        let from = match from.trim().strip_prefix("pad.") {
            Some(pad) => match pad_from_name(pad) {
                Some(pad) => From::Pad(pad),
                None => continue,
            },
            None => match key_from_name(from.trim()) {
                Some(key) => From::Key(key),
                None => continue,
            },
        };
        let does = match does.trim() {
            "left" => Does::Way(Way::Left),
            "right" => Does::Way(Way::Right),
            "up" => Does::Way(Way::Up),
            "down" => Does::Way(Way::Down),
            "fire" => Does::Way(Way::Fire),
            other if MouseButton::ALL.iter().any(|b| b.key() == other) => Does::Mouse(
                *MouseButton::ALL
                    .iter()
                    .find(|b| b.key() == other)
                    .expect("checked"),
            ),
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

/// Whether a binding is being worked, given what the desk and the pads are
/// doing. Pure, so the deciding can be tested without a pad plugged in.
pub fn holding(from: From, key_down: &dyn Fn(egui::Key) -> bool, pads: &Pads) -> bool {
    match from {
        From::Key(key) => key_down(key),
        From::Pad(pad) => pads.holding(pad),
    }
}

/// What the pads are doing now: every button down and every axis off centre,
/// across all of them. Two pads work the one stick, which is what somebody
/// with a pad in each hand expects.
pub fn read_pads(gilrs: &mut gilrs::Gilrs) -> Pads {
    // The events are pumped rather than read: gilrs keeps the state itself and
    // will not update it until they are taken.
    while gilrs.next_event().is_some() {}

    let mut pads = Pads::default();
    for (_, pad) in gilrs.gamepads() {
        pads.count += 1;
        for (button, _) in BUTTONS.iter().chain(MORE_BUTTONS.iter()) {
            if pad.is_pressed(*button) && !pads.buttons.contains(button) {
                pads.buttons.push(*button);
            }
        }
        for (axis, _) in AXES.iter() {
            let value = pad.value(*axis);
            if value.abs() > DEADZONE {
                pads.axes.push((*axis, value));
            }
        }
    }
    pads
}

/// The first thing on a pad that has just been pressed, for a line that is
/// waiting to be bound.
pub fn pad_pressed(gilrs: &mut gilrs::Gilrs) -> Option<Pad> {
    let mut found = None;
    while let Some(event) = gilrs.next_event() {
        match event.event {
            gilrs::EventType::ButtonPressed(button, _) if found.is_none() => {
                if button_name(button).is_some() {
                    found = Some(Pad::Button(button));
                }
            }
            gilrs::EventType::AxisChanged(axis, value, _)
                if found.is_none() && axis_name(axis).is_some() && value.abs() > DEADZONE =>
            {
                found = Some(Pad::Axis(axis, value > 0.0));
            }
            _ => {}
        }
    }
    found
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
            app.spec.bus.set_joystick(kind);
        }
    });
    ui.label(
        egui::RichText::new(app.spec.bus.joystick.kind.how())
            .small()
            .color(theme::DIM),
    );
    if app.spec.bus.joystick.kind == Kind::DkTronicsProgrammable {
        programmable(app, ui);
    }
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
    // And the mouse buttons, for whichever mouse is fitted.
    let kempston = app.spec.bus.mouse.buttons;
    let amx = app.spec.bus.amx.as_ref().map(|a| a.buttons);
    let fitted: Vec<MouseButton> = MouseButton::ALL
        .into_iter()
        .filter(|b| app.spec.bus.hardware.fitted(b.on()))
        .collect();
    if !fitted.is_empty() {
        ui.horizontal_wrapped(|ui| {
            theme::group_label(ui, "Mouse");
            for button in fitted {
                let down = match button {
                    MouseButton::KempstonLeft => kempston & 0x02 == 0,
                    MouseButton::KempstonRight => kempston & 0x01 == 0,
                    MouseButton::AmxLeft => amx.is_some_and(|b| b & 0x80 == 0),
                    MouseButton::AmxMiddle => amx.is_some_and(|b| b & 0x40 == 0),
                    MouseButton::AmxRight => amx.is_some_and(|b| b & 0x20 == 0),
                };
                ui.label(egui::RichText::new(button.name()).small().color(if down {
                    theme::AMBER
                } else {
                    theme::DIM
                }));
            }
        });
    }
    ui.add_space(6.0);

    theme::group_label(ui, "Keys and pads");
    let pads = app.pads.count;
    ui.label(
        egui::RichText::new(format!(
            "A key here works the stick, a key of the machine or a mouse button instead of \
             the machine's own keyboard. Press Set, then the key or the pad control you \
             want. {}",
            match pads {
                0 => "No gamepad is plugged in.".to_string(),
                1 => "One gamepad is plugged in.".to_string(),
                n => format!("{n} gamepads are plugged in."),
            }
        ))
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
            ui.label(egui::RichText::new(format!("{:<14}", from_name(binding.from))).monospace());
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
                    from: From::Key(egui::Key::Num0),
                    does: Does::Way(way),
                });
                app.joystick_binding = Some(app.joystick_map.len() - 1);
            }
        }
        if theme::selectable(ui, false, "a machine key").clicked() {
            // Somewhere to start: the space bar, which is the key most games
            // want when they do not want a stick.
            app.joystick_map.push(Binding {
                from: From::Key(egui::Key::Num0),
                does: Does::Key(7, 0),
            });
            app.joystick_binding = Some(app.joystick_map.len() - 1);
        }
    });
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Add mouse");
        for button in MouseButton::ALL {
            let fitted = app.spec.bus.hardware.fitted(button.on());
            let response = theme::selectable(ui, false, button.name()).on_hover_text(if fitted {
                "Hold this mouse button down with a key or a pad control"
            } else {
                "Its mouse is not fitted, so the binding waits until it is: fit it in the \
                 Hardware window"
            });
            if response.clicked() {
                app.joystick_map.push(Binding {
                    from: From::Key(egui::Key::Num0),
                    does: Does::Mouse(button),
                });
                app.joystick_binding = Some(app.joystick_map.len() - 1);
            }
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

/// The name on a key of the matrix, for saying what the programmable has been
/// taught.
fn key_named(key: Option<(usize, u8)>) -> &'static str {
    match key {
        None => "—",
        Some(at) => crate::keyboard::SPECTRUM
            .iter()
            .find(|k| k.press == [at])
            .map_or("?", |k| k.main),
    }
}

/// The DK'Tronics Programmable: its slider, and what it has been taught.
fn programmable(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let programming = app.spec.bus.joystick.programming;
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Slider");
        if theme::selectable(ui, !programming, "1 play").clicked() {
            app.spec.bus.joystick.programming = false;
        }
        if theme::selectable(ui, programming, "2 program")
            .on_hover_text(
                "Hold the stick one way and press the key it should be, then let go of both. \
                 Back to 1 to play.",
            )
            .clicked()
        {
            app.spec.bus.joystick.programming = true;
        }
    });
    if programming {
        ui.label(
            egui::RichText::new(
                "Teaching: hold a direction and press the key it should press. The stick \
                 presses nothing until the slider is back at 1.",
            )
            .small()
            .color(theme::AMBER),
        );
    }
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Taught");
        for way in Way::ALL {
            let label = format!(
                "{} {}",
                way.name(),
                key_named(app.spec.bus.joystick.taught(way))
            );
            let picking = app.dkprog_picking == Some(way);
            if theme::selectable(ui, picking, &label)
                .on_hover_text("Choose the key for this direction from a list instead")
                .clicked()
            {
                app.dkprog_picking = if picking { None } else { Some(way) };
            }
        }
        if theme::selectable(ui, false, "Forget all").clicked() {
            for way in Way::ALL {
                app.spec.bus.joystick.teach(way, None);
            }
        }
    });
    if let Some(way) = app.dkprog_picking {
        ui.horizontal_wrapped(|ui| {
            theme::group_label(ui, way.name());
            for key in crate::keyboard::SPECTRUM
                .iter()
                .filter(|k| k.press.len() == 1)
            {
                if theme::selectable(ui, false, key.main).clicked() {
                    app.spec.bus.joystick.teach(way, Some(key.press[0]));
                    app.dkprog_picking = None;
                }
            }
        });
    }
}
