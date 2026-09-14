//! A kept network playing the machine in front of the user.
//!
//! It plays the way it was trained: every step's worth of frames it looks at
//! the picture — the frames stacked as they were in training — chooses, and
//! holds what it chose until the next step. The choice is drawn from its
//! preferences as it was while learning, unless it is told to take the one it
//! likes best: a network that learnt with a little chance in its play can
//! get stuck doing its favourite thing forever when the chance is taken away.

use std::collections::VecDeque;
use std::path::Path;

use burn::module::Module;
use burn::record::{BinFileRecorder, FullPrecisionSettings};
use burn::tensor::activation::softmax;

use super::model::{batch, Net};
use super::setup::Setup;
use super::sight::look;
use super::worker::{NETWORK, SETUP};
use crate::machine::Spectrum;

/// A single choice at a time is quick enough on the processor, and leaves
/// the graphics card to training.
type Cpu = burn::backend::NdArray;

pub struct Player {
    net: Net<Cpu>,
    device: burn_ndarray::NdArrayDevice,
    setup: Setup,
    frames: VecDeque<Vec<u8>>,
    /// The machine's frame count at which to choose again.
    next_at: Option<u64>,
    rng: u64,
    /// Take the action it likes best rather than drawing one.
    pub greedy: bool,
    /// The last choice, and the preferences it was made from.
    pub last: Option<(usize, Vec<f32>)>,
}

impl Player {
    /// A network kept by training, and the set-up kept beside it.
    pub fn load(dir: &Path) -> Result<Player, String> {
        let setup_path = dir.join(SETUP);
        let text = std::fs::read_to_string(&setup_path)
            .map_err(|e| format!("{}: {e}", setup_path.display()))?;
        let setup =
            Setup::from_text(&text).map_err(|e| format!("{}: {e}", setup_path.display()))?;
        let device = Default::default();
        let net = Net::<Cpu>::new(&setup.env.sight, setup.env.inputs.actions.len(), &device)?
            .load_file(
                dir.join(NETWORK),
                &BinFileRecorder::<FullPrecisionSettings>::new(),
                &device,
            )
            .map_err(|e| format!("{}: {e}", dir.join(NETWORK).display()))?;
        Ok(Player {
            net,
            device,
            setup,
            frames: VecDeque::new(),
            next_at: None,
            rng: 0x9E37_79B9_7F4A_7C15,
            greedy: false,
            last: None,
        })
    }

    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    /// Plug the machine's stick into the interface the network learnt on.
    pub fn prepare(&mut self, spec: &mut Spectrum) {
        let interface = self.setup.env.inputs.interface;
        if interface != crate::joystick::Kind::None {
            spec.bus.set_joystick(interface);
        }
        self.frames.clear();
        self.next_at = None;
    }

    /// Let go of everything the network was holding.
    pub fn release(&self, spec: &mut Spectrum) {
        self.setup.env.inputs.release(spec);
    }

    pub fn probabilities(&self, observation: &[u8]) -> Vec<f32> {
        let x = batch::<Cpu>(&[observation], &self.setup.env.sight, &self.device);
        let (logits, _) = self.net.forward(x);
        softmax(logits, 1)
            .into_data()
            .to_vec::<f32>()
            .expect("floats")
    }

    /// Choose, if a step's worth of frames has gone by since the last choice,
    /// and put the choice on the machine. Returns what was chosen.
    pub fn play(&mut self, spec: &mut Spectrum) -> Option<usize> {
        let frame = spec.bus.frame;
        if self.next_at.is_some_and(|at| frame < at) {
            return None;
        }
        let sight = &self.setup.env.sight;
        let seen = look(spec, sight);
        if self.frames.is_empty() {
            self.frames = std::iter::repeat_n(seen, sight.frames.max(1)).collect();
        } else {
            self.frames.pop_front();
            self.frames.push_back(seen);
        }
        let observation: Vec<u8> = self.frames.iter().flatten().copied().collect();
        let probs = self.probabilities(&observation);
        let action = if self.greedy {
            probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(i, _)| i)
        } else {
            self.draw(&probs)
        };
        self.setup.env.inputs.apply(spec, action);
        self.next_at = Some(frame + self.setup.env.frames_per_step.max(1) as u64);
        self.last = Some((action, probs));
        Some(action)
    }

    fn draw(&mut self, probs: &[f32]) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let mut roll = (self.rng >> 40) as f32 / (1u64 << 24) as f32;
        for (i, p) in probs.iter().enumerate() {
            if roll < *p {
                return i;
            }
            roll -= p;
        }
        probs.len() - 1
    }
}
