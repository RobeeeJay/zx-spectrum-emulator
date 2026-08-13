//! How fast the tape runs, and when the hubs turn.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::tape::Tape;
use zx_rustrum::ui::{App, Roms, MAX_SPEED};

/// A tape of two blocks: a header and four kilobytes of data, so there is a
/// stretch of loading to measure and a pause at the end of a block to reach.
fn built_tape() -> Option<Tape> {
    let mut tap: Vec<u8> = Vec::new();
    let mut header = vec![0x00, 0x00];
    header.extend_from_slice(b"TEST      ");
    header.extend_from_slice(&[0x00, 0x10, 0x00, 0x80, 0x00, 0x10]);
    let mut data = vec![0xFFu8];
    data.extend((0..4096u32).map(|i| (i % 251) as u8));
    for block in [header, data] {
        let mut whole = block.clone();
        whole.push(block.iter().fold(0u8, |sum, byte| sum ^ byte));
        tap.extend_from_slice(&(whole.len() as u16).to_le_bytes());
        tap.extend_from_slice(&whole);
    }
    Tape::from_bytes("test.tap", &tap).ok()
}

fn app_with_tape() -> Option<App> {
    let tape = built_tape()?;

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.set_tape(Some(tape));
    Some(app)
}

/// Hurrying a tape along runs the machine as fast as it will go. Eight times
/// was slower than the emulator's own Max, which made "hurry up" the slow way
/// to load a tape.
#[test]
fn hurrying_the_tape_runs_at_the_emulators_top_speed() {
    let Some(mut app) = app_with_tape() else {
        return;
    };
    *app.tape_boost_mut() = true;
    app.running = true;
    if let Some(tape) = app.tape_mut() {
        tape.playing = true;
    }
    assert!(app.tape_is_loading(), "the tape should be loading");

    // A host frame at a time, which is how the emulator is really driven: the
    // work per call is capped so that one long call cannot lock the window up,
    // and asking for a tenth of a second in one go measures the cap instead of
    // the boost.
    // One host frame's worth, which is how the emulator is really driven. Not
    // a whole second of them: the tape reaches the pause at the end of its
    // block part-way through, where the hurry-up stops by design, and the
    // average over that measures the pause rather than the loading.
    let run = |app: &mut App| {
        assert!(app.tape_is_loading(), "the tape should still be loading");
        let before = app.spec.bus.total_t();
        app.advance(1.0 / 60.0);
        app.spec.bus.total_t() - before
    };
    let plain = {
        let mut app = app_with_tape().expect("a tape");
        *app.tape_boost_mut() = false;
        app.running = true;
        if let Some(tape) = app.tape_mut() {
            tape.playing = true;
        }
        run(&mut app)
    };
    let boosted = run(&mut app);

    let ratio = boosted as f32 / plain.max(1) as f32;
    assert!(
        ratio > MAX_SPEED / 2.0,
        "boosted ran {ratio:.1} times as much work as unboosted, and Max is {MAX_SPEED}"
    );
}

/// The hurry-up is for the loading, not for the silence after it. The pause at
/// the end of a block is where the program does something worth watching — and
/// where the tape stops, if that block was the one that stops it — so the
/// speed comes back to normal as the pause starts rather than after it.
#[test]
fn the_speed_comes_back_at_the_pause_after_a_block() {
    let Some(mut app) = app_with_tape() else {
        return;
    };
    *app.tape_boost_mut() = true;
    app.running = true;
    if let Some(tape) = app.tape_mut() {
        tape.playing = true;
    }

    // Run until the block ends and the tape sits in its pause.
    let mut reached = false;
    for _ in 0..4000 {
        app.advance(1.0 / 60.0);
        if app.tape_ref().is_some_and(|tape| tape.in_block_pause()) {
            reached = true;
            break;
        }
    }
    assert!(reached, "the tape should have reached the end of its block");

    assert!(
        app.tape_is_playing(),
        "the tape is still running: the pause is part of the block"
    );
    assert!(
        !app.tape_is_loading(),
        "but nothing is being loaded in the silence, so the hurry-up stops here"
    );
}

/// A paused machine passes no T-states, so the tape stands still. Hubs that
/// keep turning over a stopped tape say the opposite of what has happened.
#[test]
fn the_hubs_stand_still_while_the_machine_is_paused() {
    use egui_kittest::Harness;

    let Some(mut app) = app_with_tape() else {
        return;
    };
    app.show_tape = true;
    app.running = true;
    if let Some(tape) = app.tape_mut() {
        tape.playing = true;
    }

    let mut harness = Harness::builder()
        .with_size([1200.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(4);

    // Running: the hubs wind on.
    let before = harness.state().tape.left_spin;
    for _ in 0..8 {
        harness.state_mut().advance(1.0 / 60.0);
        harness.run_steps(2);
    }
    let running = harness.state().tape.left_spin;
    assert!(
        running > before,
        "the hubs should turn while the tape runs: {before} to {running}"
    );

    // Paused: they stop, though the tape is still threaded up and "playing".
    harness.state_mut().running = false;
    for _ in 0..8 {
        harness.run_steps(2);
    }
    assert!(
        harness.state().tape_is_playing(),
        "the deck is still in play, which is what made this look wrong"
    );
    assert_eq!(
        harness.state().tape.left_spin,
        running,
        "but the hubs should not have moved while the machine was paused"
    );
}
