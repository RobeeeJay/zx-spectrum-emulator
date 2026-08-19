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

/// The hurry-up is for the loading, not for the silence the program is meant
/// to be watched through. The last pause of a tape is where the loader hands
/// over, so the speed comes back as that pause starts rather than after it.
#[test]
fn the_speed_comes_back_at_the_pause_the_tape_ends_on() {
    let Some(mut app) = app_with_tape() else {
        return;
    };
    *app.tape_boost_mut() = true;
    app.running = true;
    if let Some(tape) = app.tape_mut() {
        tape.playing = true;
    }

    // Run until the tape reaches the silence it ends on.
    let mut reached = false;
    for _ in 0..8000 {
        app.advance(1.0 / 60.0);
        if app
            .tape_ref()
            .is_some_and(|tape| tape.pause_ends_the_tape())
        {
            reached = true;
            break;
        }
    }
    assert!(reached, "the tape should have reached its last pause");

    assert!(
        app.tape_is_playing(),
        "the tape is still running: the pause is part of the block"
    );
    assert!(
        !app.tape_is_loading(),
        "but nothing is being loaded in the silence it ends on, so the \
         hurry-up stops here"
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

/// The hurry-up only lets go for a pause that ends the tape.
///
/// The silence between two blocks of a multi-load is the loader getting ready
/// for the next one and nobody is watching it. Coming back to normal speed
/// through every one of them makes a hurried tape barely quicker than an
/// unhurried one; the silence worth watching is the last one, or one before a
/// block that stops the tape.
#[test]
fn only_the_pause_that_ends_the_tape_slows_down() {
    use zx_rustrum::tape::Block;

    let block = |bytes: usize| Block::Standard {
        pause_ms: 1000,
        data: vec![0xFF; bytes],
    };

    // Two blocks of data, so the first pause is a gap in the middle.
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = true;
    *app.tape_boost_mut() = true;
    app.set_tape(Some(Tape::from_blocks(
        "two.tap".into(),
        vec![block(600), block(600)],
    )));
    if let Some(tape) = app.tape_mut() {
        tape.playing = true;
    }

    let mut middle = false;
    let mut last = false;
    for _ in 0..8000 {
        app.advance(1.0 / 60.0);
        let Some(tape) = app.tape_ref() else { break };
        if tape.in_block_pause() {
            if tape.pause_ends_the_tape() {
                last = true;
                assert!(
                    !app.tape_is_loading(),
                    "the pause at the end of the tape is where the program \
                     takes over, and is worth watching at normal speed"
                );
            } else {
                middle = true;
                assert!(
                    app.tape_is_loading(),
                    "a gap between two blocks is the loader getting ready, and \
                     should stay hurried"
                );
            }
        }
        if !tape.playing {
            break;
        }
    }
    assert!(middle, "the tape should have had a gap between its blocks");
    assert!(last, "and a pause at the end of it");
}

/// A pause before a block that stops the tape is the same thing as the last
/// one: the loader is about to hand over.
#[test]
fn a_pause_before_a_stop_block_slows_down_too() {
    use zx_rustrum::tape::Block;

    let mut tape = Tape::from_blocks(
        "stop.tap".into(),
        vec![
            Block::Standard {
                pause_ms: 1000,
                data: vec![0xFF; 400],
            },
            Block::Pause(0),
            Block::Standard {
                pause_ms: 1000,
                data: vec![0xFF; 400],
            },
        ],
    );
    tape.playing = true;

    let mut reached = false;
    let mut now = 0u64;
    for _ in 0..400_000 {
        now += 200;
        tape.level_at(now);
        if tape.in_block_pause() {
            reached = true;
            assert!(
                tape.pause_ends_the_tape(),
                "the block after this pause stops the tape, so the pause ends it"
            );
            break;
        }
        if !tape.playing {
            break;
        }
    }
    assert!(reached, "the tape should have reached its first pause");
}

/// A tape that stops itself says so.
///
/// A multi-load carries a block that tells the deck to stop: the program takes
/// over there and asks for the next part when it wants it. Nothing said so,
/// and a deck that stops half way through a tape looks exactly like a load
/// that has gone wrong — which is what Gauntlet III looks like, since it stops
/// half way through its first side.
#[test]
fn the_deck_says_when_the_tape_stopped_it() {
    use zx_rustrum::tape::{Block, Tape};

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = true;
    // A short block, then the block that stops the tape, then more.
    app.spec.bus.tape = Some(Tape::from_blocks(
        "t".into(),
        vec![
            Block::Standard {
                pause_ms: 1,
                data: vec![0xFF, 1, 2, 3],
            },
            Block::Pause(0),
            Block::Standard {
                pause_ms: 1,
                data: vec![0xFF, 4, 5, 6],
            },
            Block::Pause(0),
            Block::Standard {
                pause_ms: 1,
                data: vec![0xFF, 7, 8, 9],
            },
        ],
    ));
    let now = app.spec.bus.total_t();
    app.spec.bus.tape.as_mut().unwrap().play(now);

    for _ in 0..600 {
        app.advance(1.0 / 50.0);
        if !app.spec.bus.tape_playing() {
            break;
        }
    }
    app.advance(1.0 / 50.0);

    assert!(
        !app.spec.bus.tape_playing(),
        "the block should have stopped the deck"
    );
    assert!(
        app.status.contains("stop"),
        "and the window should say why it stopped, not just stop: {:?}",
        app.status
    );

    // And it is said again the next time, rather than once a session: the
    // status line is written over with something else, the tape started, and
    // the next stop has to speak up for itself.
    app.set_status("something else".to_string(), false);
    let now = app.spec.bus.total_t();
    app.spec.bus.tape.as_mut().unwrap().play(now);
    for _ in 0..600 {
        app.advance(1.0 / 50.0);
        if !app.spec.bus.tape_playing() {
            break;
        }
    }
    app.advance(1.0 / 50.0);
    assert!(
        app.status.contains("stop"),
        "the second stop should say so as well: {:?}",
        app.status
    );
}
