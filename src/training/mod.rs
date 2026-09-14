//! Teaching a neural network to play a game, inside the emulator.
//!
//! The one rule is that the network sees the screen and nothing else: not the
//! memory, not the registers, not what the CPU is doing — the picture a
//! player would see, and a reward. What it may press is a set of actions
//! chosen per game (`inputs`), made of a joystick interface's directions and
//! fire, or particular keys.
//!
//! The reward has to come from somewhere, and it is the judge (`judge`) that
//! reads the score, the lives and whether the game is over out of memory —
//! as the Arcade Learning Environment does for Atari games. The judge's
//! reading goes no further than the number it hands back; the network never
//! sees an address.
//!
//! `env` is one machine being played, reset to a saved start and stepped an
//! action at a time, and a pool of them stepped in parallel. `model` is the
//! network, built with `burn`, and `ppo` trains it.

pub mod env;
pub mod inputs;
pub mod judge;
pub mod model;
pub mod ppo;
pub mod sight;
