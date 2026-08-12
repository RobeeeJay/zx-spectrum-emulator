//! Looking for particular things in a program.

use zx_rustrum::detect::{main_game_loop, Sure};
use zx_rustrum::observe::Step;

const FRAME_T: u32 = 69888;

/// A loop that goes round every `period` T-states, calling `children` each time.
fn looping(head: u16, children: &[u16], period: u64, turns: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    let at = |t: u64, entry: u16, depth: u8, enter: bool| Step {
        frame: (t / FRAME_T as u64) as u32,
        t: (t % FRAME_T as u64) as u32,
        entry,
        depth,
        enter,
    };
    for turn in 0..turns {
        let start = turn as u64 * period;
        steps.push(at(start, head, 1, true));
        for (n, child) in children.iter().enumerate() {
            let when = start + 200 + n as u64 * 100;
            steps.push(at(when, *child, 2, true));
            steps.push(at(when + 50, *child, 2, false));
        }
        steps.push(at(start + period - 20, head, 1, false));
    }
    steps
}

/// A loop that comes back steadily, does several different things each turn,
/// and has done so many times, is the main game loop — and the detector says
/// how sure it is rather than only naming an address.
#[test]
fn a_steady_varied_loop_is_the_main_game_loop() {
    let steps = looping(
        0x8000,
        &[0x9000, 0x9100, 0x9200, 0x9300],
        FRAME_T as u64 * 2,
        60,
    );
    let found = main_game_loop(&steps, FRAME_T).expect("it should be found");

    assert_eq!(found.address, 0x8000);
    assert!(
        found.sure >= Sure::Likely,
        "a steady loop of sixty turns should not be a maybe: {} ({:.2})",
        found.sure.label(),
        found.score
    );
    assert!(
        found.because.contains("60") && found.because.contains("2.00"),
        "it should say what the answer rests on: {:?}",
        found.because
    );
}

/// A program that has barely run has not shown anything to find. Naming the
/// least bad candidate would put an address in front of somebody who would go
/// and look at it.
#[test]
fn too_little_watched_finds_nothing() {
    let steps = looping(0x8000, &[0x9000], FRAME_T as u64, 2);
    assert!(
        main_game_loop(&steps, FRAME_T).is_none(),
        "two turns is not a habit"
    );
}

/// An irregular loop is less certain than a steady one, and the number says so.
#[test]
fn an_irregular_loop_is_less_sure_than_a_steady_one() {
    let steady = looping(0x8000, &[0x9000, 0x9100, 0x9200], FRAME_T as u64, 60);
    let mut ragged = steady.clone();
    // Push every other turn later by a varying amount.
    for (n, step) in ragged.iter_mut().enumerate() {
        if n % 4 == 0 {
            step.t = (step.t + (n as u32 * 977) % 40000).min(FRAME_T - 1);
        }
    }

    let steady = main_game_loop(&steady, FRAME_T).expect("the steady one");
    let ragged = main_game_loop(&ragged, FRAME_T).expect("the ragged one");
    assert!(
        steady.score > ragged.score,
        "steady {:.2} should beat ragged {:.2}",
        steady.score,
        ragged.score
    );
}

/// Against a real game, the detector should find the loop the tracer finds and
/// say something sensible about it. Skips itself without a recording.
#[test]
fn it_finds_manic_miners_loop() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let roms = Roms {
        rom48: std::fs::read("roms/48.rom").ok(),
        ..Default::default()
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.spec.bus.observer.enabled = true;
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.max_speed = true;
    }
    for _ in 0..300 {
        app.advance(1.0 / 50.0);
    }

    let steps: Vec<Step> = app.spec.bus.observer.steps().copied().collect();
    let found = main_game_loop(&steps, app.spec.bus.frame_t())
        .expect("a game that has run for thousands of frames has a loop");

    // Manic Miner's turn takes four frames and a bit, not one, and the
    // detector must not have assumed otherwise.
    assert!(
        found.because.contains("frames apart"),
        "it should say how far apart the turns are: {:?}",
        found.because
    );
    assert!(
        found.sure >= Sure::Likely,
        "thousands of turns of a real game should not read as a maybe: {} ({:.2}) — {}",
        found.sure.label(),
        found.score,
        found.because
    );
}
