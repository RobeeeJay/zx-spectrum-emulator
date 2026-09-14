//! Training on a thread of its own, so the emulator goes on answering.
//!
//! The window starts a worker with a copy of the machine to start every game
//! from and the set-up, and from then on only reads what the worker has
//! written down: every update's progress, a picture of one of the games as it
//! is played, and anything that went wrong. Stop is a flag the trainer looks
//! at between steps, so it is answered in a step or a batch rather than at
//! the end of an update that may take most of a minute.
//!
//! A network is kept as a directory — `network.bin` and the `setup.txt` it
//! was trained with — since it cannot be rebuilt without knowing the picture
//! and the actions it was made for.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use burn::tensor::backend::AutodiffBackend;

use super::ppo::{Progress, Trainer};
use super::setup::Setup;
use crate::machine::Spectrum;
use crate::screen::View;

/// What does the arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Processor {
    /// The graphics card, through wgpu: about ten times the processor's
    /// speed at the default size, measured on an M5 Pro.
    #[default]
    Gpu,
    Cpu,
}

impl Processor {
    pub fn name(&self) -> &'static str {
        match self {
            Processor::Gpu => "Graphics card",
            Processor::Cpu => "Processor",
        }
    }
}

/// One of the games as it is being played.
#[derive(Clone, Debug)]
pub struct Preview {
    /// RGBA, `View::CROPPED`: the picture as a television showed it.
    pub picture: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub action: usize,
    pub probabilities: Vec<f32>,
}

/// What the worker has written down for the window.
#[derive(Default)]
pub struct Shared {
    /// Every update's progress, in order.
    pub history: Vec<Progress>,
    pub preview: Option<Preview>,
    pub error: Option<String>,
    /// Where the network was last kept.
    pub saved: Option<PathBuf>,
}

/// What a run starts from.
pub struct Start {
    /// Every game begins as this machine.
    pub machine: Spectrum,
    pub setup: Setup,
    pub processor: Processor,
    /// A network kept earlier, to go on training rather than start afresh.
    pub resume: Option<PathBuf>,
    /// Where to keep the network: every few updates and when stopped.
    pub keep_in: Option<PathBuf>,
    /// Stop by itself after this many updates, which is how Time it
    /// measures one.
    pub updates: Option<usize>,
}

const PREVIEW_EVERY: Duration = Duration::from_millis(40);
/// Updates between keeping the network, so a crash or a closed window loses
/// a few minutes rather than the evening.
const KEEP_EVERY: usize = 10;

pub const NETWORK: &str = "network";
pub const SETUP: &str = "setup.txt";
/// The machine every game of the run started from, so the next run can
/// start there too.
pub const START: &str = "start.szx";
/// Adam's running averages, so going on carries on.
pub const OPTIMISER: &str = "optimiser";

/// Where a tape's network is kept: `games/manic.zxrs-net/`.
pub fn network_dir(source: &Path) -> PathBuf {
    source.with_extension("zxrs-net")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Worker {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    keep_now: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(start: Start) -> Worker {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let keep_now = Arc::new(AtomicBool::new(false));
        let (s, f, k) = (shared.clone(), stop.clone(), keep_now.clone());
        let thread =
            std::thread::Builder::new()
                .name("training".into())
                .spawn(move || {
                    // A machine with no graphics card wgpu can use panics on the
                    // way in; that is said in the window, not by the emulator
                    // falling over.
                    let outcome =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            match start.processor {
                                Processor::Gpu => {
                                    run::<burn::backend::Autodiff<burn::backend::Wgpu>>(
                                        Default::default(),
                                        start,
                                        &s,
                                        &f,
                                        &k,
                                    )
                                }
                                Processor::Cpu => run::<
                                    burn::backend::Autodiff<burn::backend::NdArray>,
                                >(
                                    Default::default(), start, &s, &f, &k
                                ),
                            }
                        }));
                    let error = match outcome {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(e),
                        Err(panic) => Some(
                            panic
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_else(|| "training stopped with an error".into()),
                        ),
                    };
                    if error.is_some() {
                        lock(&s).error = error;
                    }
                })
                .expect("a thread");
        Worker {
            shared,
            stop,
            keep_now,
            thread: Some(thread),
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// What the worker has written down so far.
    pub fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Keep the network at the end of the update under way.
    pub fn keep_now(&self) {
        self.keep_now.store(true, Ordering::Relaxed);
    }

    /// Stop, and wait for the network to be kept.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Keep a network, the set-up it needs and the machine its games started
/// from in a directory of their own.
pub fn keep<B: AutodiffBackend>(
    trainer: &Trainer<B>,
    setup: &Setup,
    start: &Spectrum,
    dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    trainer.save(&dir.join(NETWORK))?;
    trainer.save_optimiser(&dir.join(OPTIMISER))?;
    let write = |name: &str, bytes: &[u8]| {
        std::fs::write(dir.join(name), bytes)
            .map_err(|e| format!("{}: {e}", dir.join(name).display()))
    };
    write(SETUP, setup.to_text().as_bytes())?;
    write(START, &crate::szx::save(start))
}

fn run<B: AutodiffBackend>(
    device: B::Device,
    start: Start,
    shared: &Arc<Mutex<Shared>>,
    stop: &Arc<AtomicBool>,
    keep_now: &AtomicBool,
) -> Result<(), String> {
    let Start {
        machine,
        setup,
        resume,
        keep_in,
        updates: limit,
        ..
    } = start;
    let mut trainer = Trainer::<B>::new(&machine, setup.env.clone(), setup.ppo.clone(), device)?;
    if let Some(dir) = &resume {
        trainer.load(&dir.join(NETWORK))?;
        // A network kept before the optimiser was is still worth going on
        // from, with the averages worked out again.
        if dir.join(format!("{OPTIMISER}.bin")).exists() {
            trainer.load_optimiser(&dir.join(OPTIMISER))?;
        }
    }
    let seen = shared.clone();
    let mut last: Option<Instant> = None;
    trainer.watch(Box::new(move |machine, action, probabilities| {
        if last.is_some_and(|t| t.elapsed() < PREVIEW_EVERY) {
            return;
        }
        last = Some(Instant::now());
        let view = View::CROPPED;
        let mut picture = vec![0u8; view.buffer_len()];
        let flash_on = (machine.bus.frame / 16) % 2 == 1;
        crate::screen::render(&machine.bus, view, &mut picture, flash_on);
        lock(&seen).preview = Some(Preview {
            picture,
            width: view.width(),
            height: view.height(),
            action,
            probabilities: probabilities.to_vec(),
        });
    }));
    trainer.stop_when(stop.clone());

    let keep_it = |trainer: &Trainer<B>| -> Result<(), String> {
        if let Some(dir) = &keep_in {
            keep(trainer, &setup, &machine, dir)?;
            lock(shared).saved = Some(dir.clone());
        }
        Ok(())
    };
    while !stop.load(Ordering::Relaxed) {
        let before = trainer.progress().updates;
        let progress = trainer.update().clone();
        if progress.updates == before {
            break;
        }
        lock(shared).history.push(progress.clone());
        if keep_now.swap(false, Ordering::Relaxed) || progress.updates % KEEP_EVERY == 0 {
            keep_it(&trainer)?;
        }
        if limit.is_some_and(|n| progress.updates >= n) {
            break;
        }
    }
    if trainer.progress().updates > 0 {
        keep_it(&trainer)?;
    }
    Ok(())
}
