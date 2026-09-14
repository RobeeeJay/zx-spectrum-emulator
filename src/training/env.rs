//! One game being played, and a pool of them played at once.
//!
//! A game starts from a saved machine and is put back to it when it ends —
//! a copy of the whole machine takes about 45µs, so this is nearly free —
//! with a few frames of doing nothing first, a different few each time, so
//! that games which start identically do not all go identically. Every step
//! puts an action on the machine, runs some frames, and hands back what the
//! network may see, the judge's reward, and whether the game is over.

use std::collections::VecDeque;
use std::sync::Arc;

use super::inputs::InputSet;
use super::judge::{Judge, Tally};
use super::sight::{look, Sight};
use crate::machine::{Spectrum, FRAME_T};

/// Everything that decides how a game is played.
#[derive(Clone, Debug)]
pub struct Config {
    pub inputs: InputSet,
    pub sight: Sight,
    pub judge: Judge,
    /// Frames an action is held for before the next is chosen.
    pub frames_per_step: u32,
    /// Up to this many frames of nothing at the start of each game.
    pub random_wait: u32,
}

/// What a step hands back.
#[derive(Clone, Debug)]
pub struct Step {
    /// The frames stacked, most recent last — of the next game, if this one
    /// has just ended.
    pub observation: Vec<u8>,
    pub reward: f32,
    pub done: bool,
    /// The whole game's reward, when it has just ended.
    pub game_reward: Option<f32>,
}

/// A small random number generator of its own, so that games are the same
/// every time for the same seed and nothing is needed from outside.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// One game.
pub struct Env {
    start: Arc<Spectrum>,
    machine: Spectrum,
    config: Arc<Config>,
    tally: Tally,
    frames: VecDeque<Vec<u8>>,
    rng: Rng,
    game_reward: f32,
}

/// A machine fit to be played without a window: silent, and with its stick
/// plugged into the interface the input set is for.
pub fn prepare(start: &Spectrum, config: &Config) -> Spectrum {
    let mut machine = start.clone();
    machine.bus.audio.detach();
    if config.inputs.interface != crate::joystick::Kind::None {
        machine.bus.set_joystick(config.inputs.interface);
    }
    machine
}

impl Env {
    pub fn new(start: Arc<Spectrum>, config: Arc<Config>, seed: u64) -> Env {
        let machine = prepare(&start, &config);
        let mut env = Env {
            start,
            machine,
            config,
            tally: Tally::default(),
            frames: VecDeque::new(),
            rng: Rng(seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            game_reward: 0.0,
        };
        env.reset();
        env
    }

    /// Back to the start, and the first observation of a new game.
    pub fn reset(&mut self) -> Vec<u8> {
        self.machine = prepare(&self.start, &self.config);
        let wait = if self.config.random_wait > 0 {
            self.rng.next() % (self.config.random_wait as u64 + 1)
        } else {
            0
        };
        self.config.inputs.apply(&mut self.machine, 0);
        for _ in 0..wait {
            self.machine.run(FRAME_T);
        }
        let peek = |a: u16| self.machine.bus.peek_raw(a);
        self.tally = self.config.judge.start(&peek);
        self.game_reward = 0.0;
        let first = look(&self.machine, &self.config.sight);
        self.frames = std::iter::repeat_n(first, self.config.sight.frames.max(1)).collect();
        self.observation()
    }

    /// The frames on hand, stacked.
    pub fn observation(&self) -> Vec<u8> {
        self.frames.iter().flatten().copied().collect()
    }

    /// Take an action. A game that ends is reset, and the observation handed
    /// back is the new game's first.
    pub fn step(&mut self, action: usize) -> Step {
        self.config.inputs.apply(&mut self.machine, action);
        for _ in 0..self.config.frames_per_step.max(1) {
            self.machine.run(FRAME_T);
        }
        let seen = look(&self.machine, &self.config.sight);
        self.frames.pop_front();
        self.frames.push_back(seen);
        let peek = |a: u16| self.machine.bus.peek_raw(a);
        let (reward, done) = self.config.judge.judge(&mut self.tally, &peek);
        self.game_reward += reward;
        if done {
            let whole = self.game_reward;
            let observation = self.reset();
            return Step {
                observation,
                reward,
                done,
                game_reward: Some(whole),
            };
        }
        Step {
            observation: self.observation(),
            reward,
            done,
            game_reward: None,
        }
    }

    /// The machine as it stands, for a window to show.
    pub fn machine(&self) -> &Spectrum {
        &self.machine
    }
}

/// Many games at once, stepped across the processor's cores.
pub struct Pool {
    envs: Vec<Env>,
}

impl Pool {
    pub fn new(start: &Spectrum, config: Config, games: usize, seed: u64) -> Pool {
        let start = Arc::new(start.clone());
        let config = Arc::new(config);
        let envs = (0..games.max(1))
            .map(|i| Env::new(start.clone(), config.clone(), seed.wrapping_add(i as u64)))
            .collect();
        Pool { envs }
    }

    pub fn len(&self) -> usize {
        self.envs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.envs.is_empty()
    }

    pub fn observations(&self) -> Vec<Vec<u8>> {
        self.envs.iter().map(Env::observation).collect()
    }

    /// One action for each game, taken on as many threads as there are cores.
    pub fn step(&mut self, actions: &[usize]) -> Vec<Step> {
        assert_eq!(actions.len(), self.envs.len(), "an action for every game");
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(self.envs.len());
        let chunk = self.envs.len().div_ceil(threads);
        let mut steps: Vec<Option<Step>> = (0..self.envs.len()).map(|_| None).collect();
        std::thread::scope(|scope| {
            for ((envs, acts), out) in self
                .envs
                .chunks_mut(chunk)
                .zip(actions.chunks(chunk))
                .zip(steps.chunks_mut(chunk))
            {
                scope.spawn(move || {
                    for ((env, action), slot) in envs.iter_mut().zip(acts).zip(out) {
                        *slot = Some(env.step(*action));
                    }
                });
            }
        });
        steps
            .into_iter()
            .map(|s| s.expect("every game stepped"))
            .collect()
    }

    pub fn game(&self, i: usize) -> &Env {
        &self.envs[i]
    }
}
