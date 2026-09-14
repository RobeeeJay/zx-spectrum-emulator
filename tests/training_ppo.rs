//! Training a network, on the processor with burn's ndarray backend, so it
//! runs anywhere the suite does.
//!
//! The game: fire scores a point, and while fire is held a solid bar lies
//! across the top of the screen; holding fire scores nothing more, and the
//! bar goes when fire is let go. The best play alternates — fire, then
//! anything else — for half a point a step. Random play gets about a seventh.
//!
//! What makes this a test of seeing: play that ignores the screen can only
//! press fire with some probability p, earning p(1 - p), which is at most a
//! quarter of a point a step. Beating that needs the bar.
//!
//! The first version of this game had only an 8-pixel mark drawn in a new
//! place for each point, and the network learnt to press fire half the time —
//! 0.234 a step, just under the quarter — and never used the screen: a mark
//! that small, averaged down fourfold and never in the same place, was not
//! something it found.
//!
//! The bar alone was not enough at first either: with the default discount of
//! 0.99 the network reached only 0.23 in 61 updates. Probing the fire
//! probability for a screen with the bar and one without showed it did learn to
//! tell them apart — 0.08 against 0.88 — but not until about the 140th update.
//! Each advantage at 0.99 carries the next twenty-odd steps of other random
//! choices, which drowns a consequence that here lasts a single step. At 0.9 it
//! reaches half a point a step by about the 25th update, on two seeds. Raising
//! the learning rate did nothing, and turning the value loss down tenfold only
//! a little. The default stays at 0.99, which is right for a game whose rewards
//! come long after the move that earned them.

use zx_rustrum::joystick::Kind;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::training::env::Config;
use zx_rustrum::training::inputs::InputSet;
use zx_rustrum::training::judge::{Judge, Number};
use zx_rustrum::training::ppo::{PpoConfig, Trainer};
use zx_rustrum::training::sight::{Area, Sight};

type B = burn::backend::Autodiff<burn::backend::NdArray>;

fn tiny_game() -> Spectrum {
    let mut spec = Spectrum::new();
    let code: &[(u16, &[u8])] = &[
        (0x8000, &[0xF3]),             // DI
        (0x8001, &[0xDB, 0x1F]),       // loop: IN A,($1F)
        (0x8003, &[0xE6, 0x10]),       // AND $10
        (0x8005, &[0x28, 0xFA]),       // JR Z,loop
        (0x8007, &[0x21, 0x00, 0x90]), // LD HL,$9000
        (0x800A, &[0x34]),             // INC (HL)
        (0x800B, &[0x3E, 0xFF]),       // LD A,$FF
        (0x800D, &[0xCD, 0x20, 0x80]), // CALL bar
        (0x8010, &[0xDB, 0x1F]),       // wait: IN A,($1F)
        (0x8012, &[0xE6, 0x10]),       // AND $10
        (0x8014, &[0x20, 0xFA]),       // JR NZ,wait
        (0x8016, &[0xAF]),             // XOR A
        (0x8017, &[0xCD, 0x20, 0x80]), // CALL bar
        (0x801A, &[0x18, 0xE5]),       // JR loop
        // bar: the top character row, all eight pixel rows, filled with A.
        (0x8020, &[0x21, 0x00, 0x40]), // LD HL,$4000
        (0x8023, &[0x0E, 0x08]),       // LD C,8
        (0x8025, &[0x06, 0x20]),       // row: LD B,32
        (0x8027, &[0xE5]),             // PUSH HL
        (0x8028, &[0x77]),             // cell: LD (HL),A
        (0x8029, &[0x23]),             // INC HL
        (0x802A, &[0x10, 0xFC]),       // DJNZ cell
        (0x802C, &[0xE1]),             // POP HL
        (0x802D, &[0x24]),             // INC H
        (0x802E, &[0x0D]),             // DEC C
        (0x802F, &[0x20, 0xF4]),       // JR NZ,row
        (0x8031, &[0xC9]),             // RET
    ];
    for (at, bytes) in code {
        for (i, b) in bytes.iter().enumerate() {
            spec.bus.poke(at + i as u16, *b);
        }
    }
    for addr in 0x4000..0x5B00u16 {
        spec.bus.poke(addr, if addr >= 0x5800 { 0x38 } else { 0 });
    }
    spec.bus.poke(0x9000, 0);
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0x7F00;
    spec
}

fn env_config() -> Config {
    Config {
        inputs: InputSet::joystick(Kind::Kempston, false, false),
        sight: Sight {
            area: Area::Display,
            shrink: 4,
            colour: false,
            frames: 2,
        },
        judge: Judge {
            score: Some(Number::Byte(0x9000)),
            max_steps: 32,
            ..Judge::default()
        },
        frames_per_step: 2,
        random_wait: 3,
    }
}

fn ppo_config() -> PpoConfig {
    PpoConfig {
        games: 8,
        steps: 32,
        learning_rate: 1e-3,
        gamma: 0.9,
        epochs: 4,
        minibatch: 64,
        ..PpoConfig::default()
    }
}

/// Trained from nothing but the screen and the judge's reward, the network
/// learns to alternate: 0.45 a step, which play blind to the screen cannot
/// reach, and at least twice where it began.
#[test]
fn a_network_learns_the_tiny_game_from_the_screen() {
    let mut trainer =
        Trainer::<B>::new(&tiny_game(), env_config(), ppo_config(), Default::default()).unwrap();
    let first = trainer.update().step_reward;
    let mut last = first;
    for _ in 0..60 {
        last = trainer.update().step_reward;
        if last >= 0.45 {
            break;
        }
    }
    let p = trainer.progress();
    eprintln!(
        "started at {first:.3} a step, reached {last:.3} after {} updates, {} steps",
        p.updates, p.steps
    );
    assert!(
        last >= 0.45 && last >= 2.0 * first,
        "from {first:.3} to {last:.3} a step: the network did not learn to alternate"
    );
}

/// A network kept in a file and put back chooses exactly as it did.
#[test]
fn a_saved_network_comes_back_the_same() {
    let mut trainer =
        Trainer::<B>::new(&tiny_game(), env_config(), ppo_config(), Default::default()).unwrap();
    trainer.update();
    let observation = trainer.pool().game(0).observation();
    let before = trainer.probabilities(&observation);

    let dir = std::env::temp_dir().join(format!("zxrs-net-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("net");
    trainer.save(&path).unwrap();

    let mut fresh =
        Trainer::<B>::new(&tiny_game(), env_config(), ppo_config(), Default::default()).unwrap();
    assert_ne!(
        fresh.probabilities(&observation),
        before,
        "a fresh network differs"
    );
    fresh.load(&path).unwrap();
    assert_eq!(
        fresh.probabilities(&observation),
        before,
        "and the loaded one does not"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A picture too small for the network is refused with the reason.
#[test]
fn a_picture_too_small_is_refused() {
    let mut env = env_config();
    env.sight.shrink = 8;
    let result = Trainer::<B>::new(&tiny_game(), env, ppo_config(), Default::default());
    let err = result.err().expect("refused");
    assert!(err.contains("too small"), "{err}");
}
