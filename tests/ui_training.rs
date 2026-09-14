//! The Training window: opened from its toggle, refusing to start a set-up
//! that cannot be used, and starting and stopping a run that keeps its
//! network and its set-up beside the tape.

use std::time::{Duration, Instant};

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::training::judge::Number;
use zx_rustrum::training::setup::Setup;
use zx_rustrum::training::worker::{network_dir, Processor, SETUP};
use zx_rustrum::ui::{App, Roms};

fn test_app() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: Some(vec![0x00; 0x8000]),
        rom_plus3: Some(vec![0x00; 0x10000]),
        rom_zx81: Some(vec![0x00; 0x2000]),
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1500.0, 1400.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// A machine that sits in a loop — enough to be played, with a score at
/// $9000 that never moves — and a set-up small enough to train in moments.
fn small_run(app: &mut App) {
    app.spec.bus.poke(0x8000, 0x18); // JR $
    app.spec.bus.poke(0x8001, 0xFE);
    app.spec.cpu.pc = 0x8000;
    let mut setup = Setup::default();
    setup.env.sight.shrink = 4;
    setup.env.sight.frames = 1;
    setup.env.frames_per_step = 1;
    setup.env.random_wait = 0;
    setup.env.judge.score = Some(Number::Byte(0x9000));
    setup.ppo.games = 2;
    setup.ppo.steps = 8;
    setup.ppo.minibatch = 8;
    setup.ppo.epochs = 1;
    app.training.adopt(setup);
    app.training.processor = Processor::Cpu;
}

/// The window is opened from the row of window toggles, and a Spectrum has
/// one to open.
#[test]
fn the_training_window_opens_from_its_toggle() {
    let mut h = harness_for(test_app());
    h.run_steps(2);
    h.get_by_label("Training").click();
    h.run_steps(3);
    assert!(h.state().show_training, "the toggle opened it");
    assert!(
        h.query_by_label("▶ Start").is_some(),
        "and it has a Start button"
    );
}

/// A score address that cannot be read is said under the buttons, and Start
/// cannot be pressed: a run with a reward it cannot read would learn nothing.
#[test]
fn a_set_up_that_cannot_be_used_says_why_and_will_not_start() {
    let mut app = test_app();
    small_run(&mut app);
    app.training.score = "byte 9G00".into();
    app.show_training = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("\"9G00\" is not an address")
            .is_some(),
        "the reason is shown"
    );
    assert!(
        h.get_by_label("▶ Start").accesskit_node().is_disabled(),
        "and Start waits for it to be put right"
    );

    h.state_mut().training.score = "byte 9000".into();
    h.run_steps(2);
    assert!(
        !h.get_by_label("▶ Start").accesskit_node().is_disabled(),
        "a readable address lets it start"
    );
}

/// Start runs the learning on a thread of its own, the window shows it
/// going, and Stop stops it — keeping the network, and the set-up it was
/// trained with, beside the tape.
#[test]
fn a_run_is_started_and_stopped_from_the_window() {
    let dir = std::env::temp_dir().join(format!("zxrs-ui-training-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tape = dir.join("game.tap");
    std::fs::write(&tape, []).unwrap();

    let mut app = test_app();
    small_run(&mut app);
    app.tape_path = Some(tape.clone());
    app.show_training = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    h.get_by_label("▶ Start").click();
    h.run_steps(2);
    assert!(h.state().training.is_running(), "Start started it");

    let started = Instant::now();
    while h.query_by_label_contains("updates,").is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "no update was shown"
        );
        std::thread::sleep(Duration::from_millis(50));
        h.run_steps(1);
    }
    h.get_by_label("■ Stop").click();
    h.run_steps(2);
    assert!(!h.state().training.is_running(), "Stop stopped it");
    assert!(
        h.query_by_label("▶ Start").is_some(),
        "and it can be started again"
    );

    let kept = network_dir(&tape);
    assert!(
        kept.join("network.bin").exists(),
        "the network is kept beside the tape, in {}",
        kept.display()
    );
    let with_it = Setup::from_text(&std::fs::read_to_string(kept.join(SETUP)).unwrap());
    assert_eq!(with_it.as_ref(), Ok(&h.state().training.setup));
    let beside = Setup::from_text(&std::fs::read_to_string(Setup::sidecar(&tape)).unwrap());
    assert_eq!(
        beside.as_ref(),
        Ok(&h.state().training.setup),
        "and the set-up is written beside the tape"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
