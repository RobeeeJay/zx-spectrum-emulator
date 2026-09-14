//! Training by proximal policy optimisation.
//!
//! An update plays every game in the pool for a number of steps, choosing each
//! action by sampling the network's preferences; works out from the rewards
//! and the network's own estimates how much better or worse each choice turned
//! out than expected (generalised advantage estimation); and then nudges the
//! network towards the choices that did better, a few passes over the steps in
//! shuffled batches, never moving any one choice's probability far in one
//! update — the clipping that gives PPO its name.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use burn::module::AutodiffModule;
use burn::optim::{AdamConfig, GradientsParams, Optimizer};
use burn::prelude::*;
use burn::record::{BinFileRecorder, FullPrecisionSettings, Recorder};
use burn::tensor::activation::{log_softmax, softmax};
use burn::tensor::backend::AutodiffBackend;

use super::env::{Config as EnvConfig, Pool};
use super::model::{batch, Net};
use super::sight::Sight;
use crate::machine::Spectrum;

/// How training goes.
#[derive(Clone, Debug, PartialEq)]
pub struct PpoConfig {
    /// Games played at once.
    pub games: usize,
    /// Steps each game plays between updates.
    pub steps: usize,
    pub learning_rate: f64,
    /// How much a reward later counts against one now.
    pub gamma: f32,
    /// How far the advantage looks ahead before trusting the value estimate.
    pub lambda: f32,
    /// How far one update may move the probability of a choice.
    pub clip: f32,
    /// Passes over each update's steps.
    pub epochs: usize,
    pub minibatch: usize,
    pub value_weight: f32,
    /// A reward for staying undecided, so the network keeps trying things.
    pub entropy_weight: f32,
    pub seed: u64,
}

impl Default for PpoConfig {
    fn default() -> Self {
        PpoConfig {
            games: 16,
            steps: 128,
            learning_rate: 2.5e-4,
            gamma: 0.99,
            lambda: 0.95,
            clip: 0.1,
            epochs: 4,
            minibatch: 256,
            value_weight: 0.5,
            entropy_weight: 0.01,
            seed: 1,
        }
    }
}

/// How it is going.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub updates: usize,
    pub steps: u64,
    pub games_finished: u64,
    /// The average whole-game reward over the last twenty games finished.
    pub recent_game_reward: Option<f32>,
    /// Reward per step over the last update's play.
    pub step_reward: f32,
    pub policy_loss: f32,
    pub value_loss: f32,
    pub entropy: f32,
}

/// A random number generator of the trainer's own, for sampling and shuffling.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

type Adam<B> = burn::optim::adaptor::OptimizerAdaptor<burn::optim::Adam, Net<B>, B>;

pub struct Trainer<B: AutodiffBackend> {
    model: Option<Net<B>>,
    optim: Adam<B>,
    pool: Pool,
    sight: Sight,
    actions: usize,
    config: PpoConfig,
    device: B::Device,
    rng: Rng,
    recent: VecDeque<f32>,
    progress: Progress,
    watch: Option<Watch>,
    stop: Option<Arc<AtomicBool>>,
}

/// Shown game 0's machine after every step, with the action taken and the
/// probabilities it was chosen from.
pub type Watch = Box<dyn FnMut(&Spectrum, usize, &[f32]) + Send>;

impl<B: AutodiffBackend> Trainer<B> {
    pub fn new(
        start: &Spectrum,
        env: EnvConfig,
        config: PpoConfig,
        device: B::Device,
    ) -> Result<Trainer<B>, String> {
        let sight = env.sight;
        let actions = env.inputs.actions.len();
        let model = Net::new(&sight, actions, &device)?;
        let pool = Pool::new(start, env, config.games, config.seed);
        Ok(Trainer {
            model: Some(model),
            optim: AdamConfig::new().init(),
            pool,
            sight,
            actions,
            rng: Rng(config.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1),
            config,
            device,
            recent: VecDeque::new(),
            progress: Progress::default(),
            watch: None,
            stop: None,
        })
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Something to be shown each step of game 0, for a window to draw.
    pub fn watch(&mut self, watch: Watch) {
        self.watch = Some(watch);
    }

    /// A flag that cuts an update short when it is set. At the default size
    /// an update on the processor takes most of a minute, which is too long
    /// for Stop to wait. An update cut short is not counted.
    pub fn stop_when(&mut self, flag: Arc<AtomicBool>) {
        self.stop = Some(flag);
    }

    fn stopped(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
    }

    fn model(&self) -> &Net<B> {
        self.model
            .as_ref()
            .expect("the model is only taken out to be stepped")
    }

    /// The action probabilities for a batch of observations, without
    /// gradients.
    fn preferences(&self, observations: &[&[u8]]) -> (Vec<f32>, Vec<f32>) {
        let model = self.model().valid();
        let x = batch::<B::InnerBackend>(observations, &self.sight, &self.device);
        let (logits, value) = model.forward(x);
        let probs = softmax(logits, 1)
            .into_data()
            .to_vec::<f32>()
            .expect("floats");
        let values = value.into_data().to_vec::<f32>().expect("floats");
        (probs, values)
    }

    fn sample(&mut self, probs: &[f32]) -> usize {
        let mut roll = self.rng.unit();
        for (i, p) in probs.iter().enumerate() {
            if roll < *p {
                return i;
            }
            roll -= p;
        }
        probs.len() - 1
    }

    /// The network's choice for one observation: the action it likes best.
    pub fn act(&self, observation: &[u8]) -> usize {
        let (probs, _) = self.preferences(&[observation]);
        probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i)
    }

    /// The action probabilities for one observation, for the window to show
    /// and for tests to compare.
    pub fn probabilities(&self, observation: &[u8]) -> Vec<f32> {
        self.preferences(&[observation]).0
    }

    /// Play, and learn from it, once.
    pub fn update(&mut self) -> &Progress {
        let (games, steps) = (self.pool.len(), self.config.steps.max(1));
        let a = self.actions;
        let mut observations: Vec<Vec<u8>> = Vec::with_capacity(games * steps);
        let mut chosen = Vec::with_capacity(games * steps);
        let mut old_logp = Vec::with_capacity(games * steps);
        let mut values = Vec::with_capacity(games * steps);
        let mut rewards = Vec::with_capacity(games * steps);
        let mut dones = Vec::with_capacity(games * steps);

        let mut current = self.pool.observations();
        for _ in 0..steps {
            if self.stopped() {
                return &self.progress;
            }
            let refs: Vec<&[u8]> = current.iter().map(|o| o.as_slice()).collect();
            let (probs, value) = self.preferences(&refs);
            let picks: Vec<usize> = (0..games)
                .map(|g| self.sample(&probs[g * a..(g + 1) * a]))
                .collect();
            for (g, pick) in picks.iter().enumerate() {
                old_logp.push(probs[g * a + pick].max(1e-8).ln());
            }
            values.extend_from_slice(&value);
            let results = self.pool.step(&picks);
            if let Some(watch) = self.watch.as_mut() {
                watch(self.pool.game(0).machine(), picks[0], &probs[..a]);
            }
            for step in &results {
                rewards.push(step.reward);
                dones.push(step.done);
                if let Some(total) = step.game_reward {
                    self.progress.games_finished += 1;
                    self.recent.push_back(total);
                    if self.recent.len() > 20 {
                        self.recent.pop_front();
                    }
                }
            }
            chosen.extend_from_slice(&picks);
            observations.append(&mut current);
            current = results.into_iter().map(|s| s.observation).collect();
        }

        // How much better each choice did than the network expected.
        let refs: Vec<&[u8]> = current.iter().map(|o| o.as_slice()).collect();
        let (_, last_values) = self.preferences(&refs);
        let mut advantages = vec![0f32; games * steps];
        for g in 0..games {
            let mut running = 0f32;
            for t in (0..steps).rev() {
                let i = t * games + g;
                let next = if t + 1 == steps {
                    last_values[g]
                } else {
                    values[(t + 1) * games + g]
                };
                let going_on = if dones[i] { 0.0 } else { 1.0 };
                let delta = rewards[i] + self.config.gamma * next * going_on - values[i];
                running = delta + self.config.gamma * self.config.lambda * going_on * running;
                advantages[i] = running;
            }
        }
        let returns: Vec<f32> = advantages.iter().zip(&values).map(|(a, v)| a + v).collect();

        // Learn from it, a few passes over shuffled batches.
        let total = games * steps;
        let mut order: Vec<usize> = (0..total).collect();
        let (mut pl, mut vl, mut en, mut batches) = (0f32, 0f32, 0f32, 0f32);
        for _ in 0..self.config.epochs.max(1) {
            for i in (1..total).rev() {
                let j = (self.rng.next() % (i as u64 + 1)) as usize;
                order.swap(i, j);
            }
            for part in order.chunks(self.config.minibatch.max(1)) {
                if self.stopped() {
                    return &self.progress;
                }
                let m = part.len();
                let refs: Vec<&[u8]> = part.iter().map(|i| observations[*i].as_slice()).collect();
                let x = batch::<B>(&refs, &self.sight, &self.device);
                let adv: Vec<f32> = part.iter().map(|i| advantages[*i]).collect();
                let mean = adv.iter().sum::<f32>() / m as f32;
                let spread =
                    (adv.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / m as f32).sqrt();
                let adv: Vec<f32> = adv.iter().map(|v| (v - mean) / (spread + 1e-8)).collect();
                let tensor =
                    |v: Vec<f32>| Tensor::<B, 1>::from_data(TensorData::new(v, [m]), &self.device);
                let adv = tensor(adv);
                let ret = tensor(part.iter().map(|i| returns[*i]).collect());
                let old = tensor(part.iter().map(|i| old_logp[*i]).collect());
                let picks = Tensor::<B, 2, Int>::from_data(
                    TensorData::new(
                        part.iter().map(|i| chosen[*i] as i64).collect::<Vec<_>>(),
                        [m, 1],
                    ),
                    &self.device,
                );

                let model = self.model.take().expect("the model is here");
                let (logits, value) = model.forward(x);
                let logp_all = log_softmax(logits, 1);
                let logp = logp_all.clone().gather(1, picks).reshape([m]);
                let ratio = (logp - old).exp();
                let clip = self.config.clip;
                let kept = ratio.clone() * adv.clone();
                let clipped = ratio.clamp(1.0 - clip, 1.0 + clip) * adv;
                let policy_loss = kept.min_pair(clipped).mean().neg();
                let value_loss = (value.reshape([m]) - ret).powf_scalar(2.0).mean();
                let entropy = (logp_all.clone().exp() * logp_all).sum_dim(1).mean().neg();
                let loss = policy_loss.clone()
                    + value_loss.clone().mul_scalar(self.config.value_weight)
                    - entropy.clone().mul_scalar(self.config.entropy_weight);
                pl += policy_loss.into_scalar().to_f32();
                vl += value_loss.into_scalar().to_f32();
                en += entropy.into_scalar().to_f32();
                batches += 1.0;
                let grads = GradientsParams::from_grads(loss.backward(), &model);
                self.model = Some(self.optim.step(self.config.learning_rate, model, grads));
            }
        }

        let p = &mut self.progress;
        p.updates += 1;
        p.steps += total as u64;
        p.step_reward = rewards.iter().sum::<f32>() / total as f32;
        p.policy_loss = pl / batches;
        p.value_loss = vl / batches;
        p.entropy = en / batches;
        p.recent_game_reward = (!self.recent.is_empty())
            .then(|| self.recent.iter().sum::<f32>() / self.recent.len() as f32);
        &self.progress
    }

    /// Keep the network in a file, to go on from or to play with later.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        self.model()
            .clone()
            .save_file(path, &BinFileRecorder::<FullPrecisionSettings>::new())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Put a network kept in a file back. It has to have been made for the
    /// same picture and the same actions.
    pub fn load(&mut self, path: &std::path::Path) -> Result<(), String> {
        let model = self.model.take().expect("the model is here");
        match model.clone().load_file(
            path,
            &BinFileRecorder::<FullPrecisionSettings>::new(),
            &self.device,
        ) {
            Ok(loaded) => {
                self.model = Some(loaded);
                Ok(())
            }
            Err(e) => {
                self.model = Some(model);
                Err(format!("{}: {e}", path.display()))
            }
        }
    }

    /// Keep what the optimiser has worked out about each parameter — Adam's
    /// running averages — so a run that goes on carries on, rather than
    /// taking its first steps as if nothing had been learnt.
    pub fn save_optimiser(&self, path: &std::path::Path) -> Result<(), String> {
        BinFileRecorder::<FullPrecisionSettings>::new()
            .record(self.optim.to_record(), path.to_path_buf())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Put it back. The averages are kept against each parameter's id, which
    /// a network loaded from a file takes from the file, so they meet up
    /// with the network they were worked out for.
    pub fn load_optimiser(&mut self, path: &std::path::Path) -> Result<(), String> {
        let record = BinFileRecorder::<FullPrecisionSettings>::new()
            .load(path.to_path_buf(), &self.device)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        self.optim = self.optim.clone().load_record(record);
        Ok(())
    }
}
