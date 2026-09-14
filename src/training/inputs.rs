//! What the network is allowed to press: a list of actions, each a
//! combination of a joystick's switches and keys of the keyboard.

use crate::joystick::{Kind, Way};
use crate::machine::Spectrum;

/// One switch the network can hold down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// One of the stick's five, on whichever interface the set is for.
    Stick(Way),
    /// A key of the matrix, as (row, bit).
    Key(usize, u8),
}

/// One thing the network can choose to do: everything in it held together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub controls: Vec<Control>,
}

/// Everything the network may choose from, and the interface a stick in it
/// is plugged into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSet {
    pub interface: Kind,
    pub actions: Vec<Action>,
}

fn key_named(name: &str) -> Option<(usize, u8)> {
    crate::keyboard::SPECTRUM
        .iter()
        .find(|k| k.main.eq_ignore_ascii_case(name) && k.press.len() == 1)
        .map(|k| k.press[0])
}

/// What an action is called: its controls joined by `+`, a stick switch by
/// its direction and a key by its legend — `left+fire`, `Q` — or `nothing`.
/// The builders and the text form both name actions this way, so a set read
/// back from its text is the set that was written.
fn name_of(controls: &[Control]) -> String {
    if controls.is_empty() {
        return "nothing".into();
    }
    controls
        .iter()
        .map(|c| match *c {
            Control::Stick(w) => w.name().to_string(),
            Control::Key(r, b) => key_name((r, b)).to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn key_name(at: (usize, u8)) -> &'static str {
    crate::keyboard::SPECTRUM
        .iter()
        .find(|k| k.press == [at])
        .map_or("?", |k| k.main)
}

impl InputSet {
    /// A joystick on `interface`: standing still, the four directions and
    /// fire; the diagonals if asked for; and each of those with fire held, if
    /// asked for — which is how most joystick games are played.
    pub fn joystick(interface: Kind, diagonals: bool, fire_while_moving: bool) -> InputSet {
        use Way::*;
        let mut moves: Vec<Vec<Way>> = vec![vec![Left], vec![Right], vec![Up], vec![Down]];
        if diagonals {
            moves.extend([
                vec![Up, Left],
                vec![Up, Right],
                vec![Down, Left],
                vec![Down, Right],
            ]);
        }
        let mut actions = vec![Action {
            name: "nothing".into(),
            controls: vec![],
        }];
        let named = |ways: &[Way]| ways.iter().map(|w| w.name()).collect::<Vec<_>>().join("+");
        for ways in &moves {
            actions.push(Action {
                name: named(ways),
                controls: ways.iter().map(|w| Control::Stick(*w)).collect(),
            });
        }
        actions.push(Action {
            name: "fire".into(),
            controls: vec![Control::Stick(Fire)],
        });
        if fire_while_moving {
            for ways in &moves {
                let mut with = ways.clone();
                with.push(Fire);
                actions.push(Action {
                    name: named(&with),
                    controls: with.iter().map(|w| Control::Stick(*w)).collect(),
                });
            }
        }
        InputSet { interface, actions }
    }

    /// Keys by the legend on them — "Q", "SPACE", "SYMBOL SHIFT" — one action
    /// each, and doing nothing.
    pub fn keys(names: &[&str]) -> Result<InputSet, String> {
        let mut actions = vec![Action {
            name: "nothing".into(),
            controls: vec![],
        }];
        for name in names {
            let at = key_named(name).ok_or_else(|| format!("no key called {name:?}"))?;
            actions.push(Action {
                name: key_name(at).to_string(),
                controls: vec![Control::Key(at.0, at.1)],
            });
        }
        Ok(InputSet {
            interface: Kind::None,
            actions,
        })
    }

    /// Put an action on the machine: what it holds is down, and everything
    /// else the set could press is let go, so the last action does not linger.
    pub fn apply(&self, spec: &mut Spectrum, action: usize) {
        let bus = &mut spec.bus;
        for way in Way::ALL {
            bus.joystick.set(way, false);
        }
        bus.keys = [0xFF; 8];
        if let Some(action) = self.actions.get(action) {
            for control in &action.controls {
                match *control {
                    Control::Stick(way) => bus.joystick.set(way, true),
                    Control::Key(row, bit) => bus.keys[row] &= !(1 << bit),
                }
            }
        }
    }

    /// Saved as `interface; action; action`, an action being its controls
    /// joined by `+`: `kempston; nothing; left; left+fire; key:SPACE`.
    pub fn to_text(&self) -> String {
        let mut parts = vec![self.interface.key().to_string()];
        for a in &self.actions {
            if a.controls.is_empty() {
                parts.push("nothing".into());
                continue;
            }
            let controls: Vec<String> = a
                .controls
                .iter()
                .map(|c| match *c {
                    Control::Stick(w) => w.name().to_string(),
                    Control::Key(r, b) => format!("key:{}", key_name((r, b))),
                })
                .collect();
            parts.push(controls.join("+"));
        }
        parts.join("; ")
    }

    pub fn from_text(text: &str) -> Result<InputSet, String> {
        let mut parts = text.split(';').map(str::trim);
        let interface = parts
            .next()
            .and_then(Kind::from_key)
            .ok_or("the input set starts with an interface: none, kempston, …")?;
        let mut actions = Vec::new();
        for part in parts.filter(|p| !p.is_empty()) {
            let mut controls = Vec::new();
            if part != "nothing" {
                for piece in part.split('+') {
                    let control = if let Some(key) = piece.strip_prefix("key:") {
                        let at = key_named(key).ok_or_else(|| format!("no key called {key:?}"))?;
                        Control::Key(at.0, at.1)
                    } else {
                        let way = Way::ALL
                            .into_iter()
                            .find(|w| w.name() == piece)
                            .ok_or_else(|| format!("{piece:?} is not a direction, fire or key:"))?;
                        Control::Stick(way)
                    };
                    controls.push(control);
                }
            }
            actions.push(Action {
                name: name_of(&controls),
                controls,
            });
        }
        if actions.is_empty() {
            return Err("an input set needs at least one action".into());
        }
        Ok(InputSet { interface, actions })
    }
}
