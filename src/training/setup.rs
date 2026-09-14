//! Everything that decides a training run, as a file.
//!
//! Kept beside the tape as `<name>.zxrs-train.txt`, and beside every saved
//! network, which cannot be loaded without knowing the picture and the
//! actions it was made for. One `key = value` a line, addresses in hex as
//! the notes file writes them:
//!
//! ```text
//! inputs = kempston; nothing; left; right; up; down; fire
//! score = bcd 9C4E 3
//! lives = byte 9C51
//! ```
//!
//! Unlike the notes, a line that cannot be read is an error that names it
//! rather than something skipped: a mistyped score address quietly left out
//! would train for hours on nothing.

use std::path::{Path, PathBuf};

use super::env::Config;
use super::inputs::InputSet;
use super::judge::{Judge, Number};
use super::ppo::PpoConfig;
use super::sight::{Area, Sight};
use crate::joystick::Kind;

/// A game to be played and how to learn it.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub env: Config,
    pub ppo: PpoConfig,
}

impl Default for Setup {
    /// A Kempston joystick with fire while moving, which is how most games
    /// are played; nothing to reward until a score is found.
    fn default() -> Self {
        Setup {
            env: Config {
                inputs: InputSet::joystick(Kind::Kempston, false, true),
                sight: Sight::default(),
                judge: Judge::default(),
                frames_per_step: 4,
                random_wait: 30,
            },
            ppo: PpoConfig::default(),
        }
    }
}

fn hex(at: u16) -> String {
    format!("{at:04X}")
}

fn parse_hex(text: &str) -> Result<u16, String> {
    u16::from_str_radix(text.trim_start_matches('$'), 16)
        .map_err(|_| format!("{text:?} is not an address in hex"))
}

impl Number {
    /// `byte 9000`, `word 9000`, `bcd 9000 3` or `digits 9000 6 30`.
    pub fn to_text(&self) -> String {
        match *self {
            Number::Byte(at) => format!("byte {}", hex(at)),
            Number::Word(at) => format!("word {}", hex(at)),
            Number::Bcd { at, bytes } => format!("bcd {} {bytes}", hex(at)),
            Number::Digits { at, count, zero } => {
                format!("digits {} {count} {zero:02X}", hex(at))
            }
        }
    }

    pub fn from_text(text: &str) -> Result<Number, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let count = |i: usize| -> Result<u8, String> {
            words
                .get(i)
                .ok_or_else(|| format!("{text:?} is missing a count"))?
                .parse()
                .map_err(|_| format!("{text:?}: {:?} is not a count", words[i]))
        };
        let at = parse_hex(
            words
                .get(1)
                .ok_or_else(|| format!("{text:?} has no address"))?,
        )?;
        match words[0] {
            "byte" => Ok(Number::Byte(at)),
            "word" => Ok(Number::Word(at)),
            "bcd" => Ok(Number::Bcd {
                at,
                bytes: count(2)?,
            }),
            "digits" => Ok(Number::Digits {
                at,
                count: count(2)?,
                zero: words
                    .get(3)
                    .map_or(Ok(0), |z| u8::from_str_radix(z, 16))
                    .map_err(|_| format!("{text:?}: the zero is a byte in hex"))?,
            }),
            other => Err(format!(
                "{other:?} is not a kind of number: byte, word, bcd or digits"
            )),
        }
    }
}

impl Area {
    pub fn key(&self) -> &'static str {
        match self {
            Area::Display => "display",
            Area::Television => "television",
            Area::Overscan => "overscan",
        }
    }

    pub fn from_key(key: &str) -> Option<Area> {
        [Area::Display, Area::Television, Area::Overscan]
            .into_iter()
            .find(|a| a.key() == key)
    }
}

fn yes_no(on: bool) -> &'static str {
    if on {
        "yes"
    } else {
        "no"
    }
}

fn or_none<T>(value: Option<T>, text: impl Fn(T) -> String) -> String {
    value.map_or_else(|| "none".to_string(), text)
}

impl Setup {
    pub const EXTENSION: &'static str = "zxrs-train.txt";

    /// Where the set-up for a tape lives: beside it, under its own name.
    pub fn sidecar(source: &Path) -> PathBuf {
        source.with_extension(Self::EXTENSION)
    }

    pub fn to_text(&self) -> String {
        let (env, ppo, judge) = (&self.env, &self.ppo, &self.env.judge);
        let lines = [
            ("inputs", env.inputs.to_text()),
            ("area", env.sight.area.key().into()),
            ("shrink", env.sight.shrink.to_string()),
            ("colour", yes_no(env.sight.colour).into()),
            ("frames", env.sight.frames.to_string()),
            ("frames_per_step", env.frames_per_step.to_string()),
            ("random_wait", env.random_wait.to_string()),
            ("score", or_none(judge.score, |n| n.to_text())),
            ("score_scale", judge.score_scale.to_string()),
            ("lives", or_none(judge.lives, |n| n.to_text())),
            ("life_penalty", judge.life_penalty.to_string()),
            (
                "over_at_lives",
                or_none(judge.over_at_lives, |n| n.to_string()),
            ),
            (
                "over_when",
                or_none(judge.over_when, |(at, v)| format!("{} {v:02X}", hex(at))),
            ),
            ("per_step", judge.per_step.to_string()),
            ("max_steps", judge.max_steps.to_string()),
            ("games", ppo.games.to_string()),
            ("steps", ppo.steps.to_string()),
            ("learning_rate", ppo.learning_rate.to_string()),
            ("gamma", ppo.gamma.to_string()),
            ("lambda", ppo.lambda.to_string()),
            ("clip", ppo.clip.to_string()),
            ("epochs", ppo.epochs.to_string()),
            ("minibatch", ppo.minibatch.to_string()),
            ("value_weight", ppo.value_weight.to_string()),
            ("entropy_weight", ppo.entropy_weight.to_string()),
            ("seed", ppo.seed.to_string()),
        ];
        let mut out = String::from(
            "# ZX-Rustrum training set-up. One key = value a line; addresses in hex.\n",
        );
        for (key, value) in lines {
            out.push_str(&format!("{key} = {value}\n"));
        }
        out
    }

    /// Read a set-up back. A key left out keeps its default, so a file
    /// written by hand need only say what differs.
    pub fn from_text(text: &str) -> Result<Setup, String> {
        let mut setup = Setup::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fail = |why: String| format!("line {}: {why}", n + 1);
            let (key, value) = line
                .split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or_else(|| fail(format!("{line:?} is not key = value")))?;
            setup.set(key, value).map_err(fail)?;
        }
        Ok(setup)
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        fn num<T: std::str::FromStr>(key: &str, value: &str) -> Result<T, String> {
            value
                .parse()
                .map_err(|_| format!("{key}: {value:?} is not a number"))
        }
        fn optional<T>(
            value: &str,
            read: impl Fn(&str) -> Result<T, String>,
        ) -> Result<Option<T>, String> {
            if value == "none" {
                Ok(None)
            } else {
                read(value).map(Some)
            }
        }
        let (env, ppo) = (&mut self.env, &mut self.ppo);
        let judge = &mut env.judge;
        match key {
            "inputs" => env.inputs = InputSet::from_text(value)?,
            "area" => {
                env.sight.area = Area::from_key(value).ok_or_else(|| {
                    format!("area: {value:?} is not display, television or overscan")
                })?
            }
            "shrink" => {
                env.sight.shrink = num(key, value)?;
                if ![1, 2, 4].contains(&env.sight.shrink) {
                    return Err(format!("shrink: {value} is not 1, 2 or 4"));
                }
            }
            "colour" => {
                env.sight.colour = match value {
                    "yes" => true,
                    "no" => false,
                    _ => return Err(format!("colour: {value:?} is not yes or no")),
                }
            }
            "frames" => env.sight.frames = num(key, value)?,
            "frames_per_step" => env.frames_per_step = num(key, value)?,
            "random_wait" => env.random_wait = num(key, value)?,
            "score" => judge.score = optional(value, Number::from_text)?,
            "score_scale" => judge.score_scale = num(key, value)?,
            "lives" => judge.lives = optional(value, Number::from_text)?,
            "life_penalty" => judge.life_penalty = num(key, value)?,
            "over_at_lives" => judge.over_at_lives = optional(value, |v| num(key, v))?,
            "over_when" => {
                judge.over_when = optional(value, |v| {
                    let (at, byte) = v
                        .split_once(char::is_whitespace)
                        .ok_or_else(|| format!("over_when: {v:?} is an address and a byte"))?;
                    let byte = u8::from_str_radix(byte.trim(), 16)
                        .map_err(|_| format!("over_when: {byte:?} is not a byte in hex"))?;
                    Ok((parse_hex(at)?, byte))
                })?
            }
            "per_step" => judge.per_step = num(key, value)?,
            "max_steps" => judge.max_steps = num(key, value)?,
            "games" => ppo.games = num(key, value)?,
            "steps" => ppo.steps = num(key, value)?,
            "learning_rate" => ppo.learning_rate = num(key, value)?,
            "gamma" => ppo.gamma = num(key, value)?,
            "lambda" => ppo.lambda = num(key, value)?,
            "clip" => ppo.clip = num(key, value)?,
            "epochs" => ppo.epochs = num(key, value)?,
            "minibatch" => ppo.minibatch = num(key, value)?,
            "value_weight" => ppo.value_weight = num(key, value)?,
            "entropy_weight" => ppo.entropy_weight = num(key, value)?,
            "seed" => ppo.seed = num(key, value)?,
            other => return Err(format!("{other:?} is not something a set-up has")),
        }
        Ok(())
    }
}
