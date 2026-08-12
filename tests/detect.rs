//! Looking for particular things in a program.

use zx_rustrum::detect::{main_game_loop, main_game_loops, Sure};
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

/// A program does more than one thing over its life — a title screen, then the
/// game — and each has its own loop. Both are reported, in the order of how
/// good the case for them is, because picking one and throwing the other away
/// is deciding on the reader's behalf and being wrong silently.
#[test]
fn every_loop_the_program_went_round_is_listed() {
    // A title screen doing two things a frame, then the game doing four every
    // other frame. The addresses share nothing, so the phases part — and each
    // stretch is a whole number of the 64-entry windows the phase finder looks
    // through, since a window holding the end of one and the start of the
    // other overlaps both and reads as neither having changed.
    let mut steps = looping(0x8000, &[0x9000, 0x9100], FRAME_T as u64, 128);
    let started = 128 * FRAME_T as u64;
    let mut game = looping(
        0xA000,
        &[0xB000, 0xB100, 0xB200, 0xB300],
        FRAME_T as u64 * 2,
        128,
    );
    for step in &mut game {
        let when = step.frame as u64 * FRAME_T as u64 + step.t as u64 + started;
        step.frame = (when / FRAME_T as u64) as u32;
        step.t = (when % FRAME_T as u64) as u32;
    }
    steps.append(&mut game);

    let found = main_game_loops(&steps, FRAME_T);
    let addresses: Vec<u16> = found.iter().map(|finding| finding.address).collect();
    assert!(
        addresses.contains(&0x8000) && addresses.contains(&0xA000),
        "both loops should be listed, not only the better one: {addresses:02X?}"
    );

    // Ranked, and only the best one is called the main loop: the second is
    // named after where it is, since calling it the main game loop would say
    // something the evidence does not.
    for pair in found.windows(2) {
        assert!(
            pair[0].score >= pair[1].score,
            "the likeliest should come first: {:.2} before {:.2}",
            pair[0].score,
            pair[1].score
        );
    }
    assert_eq!(found[0].label, "main_game_loop", "the best candidate");
    assert_eq!(
        found[1].label,
        format!("loop_{:04X}", found[1].address),
        "and the rest are named after where they are"
    );
    assert_eq!(found[1].what, "Loop");
}

/// The same routine can head the loop in two stretches — a game that goes back
/// to its title screen and round again. One line for it, or the list turns
/// into a log of how often the program changed what it was doing.
#[test]
fn a_loop_seen_twice_is_listed_once() {
    let mut steps = looping(0x8000, &[0x9000, 0x9100], FRAME_T as u64, 128);
    let started = 128 * FRAME_T as u64;
    let mut other = looping(0xA000, &[0xB000, 0xB100], FRAME_T as u64, 128);
    let mut again = looping(0x8000, &[0x9000, 0x9100], FRAME_T as u64, 128);
    for (step, offset) in other
        .iter_mut()
        .map(|step| (step, started))
        .chain(again.iter_mut().map(|step| (step, started * 2)))
    {
        let when = step.frame as u64 * FRAME_T as u64 + step.t as u64 + offset;
        step.frame = (when / FRAME_T as u64) as u32;
        step.t = (when % FRAME_T as u64) as u32;
    }
    steps.append(&mut other);
    steps.append(&mut again);

    let found = main_game_loops(&steps, FRAME_T);
    let times = found
        .iter()
        .filter(|finding| finding.address == 0x8000)
        .count();
    assert_eq!(
        times, 1,
        "the loop at $8000 was gone round in two stretches and should be one \
         line, not {times}"
    );
}
