//! Looking for particular things in a program.

use zx_rustrum::detect::{
    joystick_input, keyboard_input, main_game_loop, main_game_loops, screen_clear, sprite_update,
    Sure,
};
use zx_rustrum::observe::{Observed, Observer, Site, Step};

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

/// One IN instruction and what it was seen reading, as the observer would have
/// recorded it.
fn reading(at: u16, ports: &[u16], reads_a_frame: u32, frames: u32) -> Site {
    let mut site = Site::default();
    site.at = at;
    site.ports = ports.to_vec();
    site.reads = reads_a_frame * frames;
    site.frames = frames;
    site
}

fn watched(sites: Vec<Site>) -> Observer {
    let mut observer = Observer::new();
    observer.port_sites = sites.into_iter().map(|site| (site.at, site)).collect();
    observer
}

/// The ULA puts the keyboard on port $FE with the row in the top half of the
/// address, so an instruction walking the eight half-rows is scanning the
/// keyboard whatever the code around it looks like.
#[test]
fn a_routine_that_walks_the_half_rows_is_reading_the_keyboard() {
    let observer = watched(vec![reading(
        0x8000,
        &[
            0xFEFE, 0xFDFE, 0xFBFE, 0xF7FE, 0xEFFE, 0xDFFE, 0xBFFE, 0x7FFE,
        ],
        8,
        200,
    )]);

    let found = keyboard_input(&observer);
    assert_eq!(found.len(), 1, "one instruction reads the keyboard");
    assert_eq!(found[0].address, 0x8000);
    assert_eq!(found[0].label, "read_keyboard");
    assert!(
        found[0].sure >= Sure::Likely,
        "eight half-rows is a keyboard scan, not a maybe: {} ({:.2})",
        found[0].sure.label(),
        found[0].score
    );
    assert!(
        found[0].because.contains("8 half-rows"),
        "it should say what the answer rests on: {:?}",
        found[0].because
    );
}

/// Port $FE carries the EAR bit as well as the keyboard, and a tape loader
/// polls one row of it thousands of times a frame. Calling that keyboard input
/// would be reading the port number and ignoring what was done with it.
#[test]
fn a_tape_loader_polling_the_ear_bit_is_not_keyboard_input() {
    let observer = watched(vec![reading(0x9000, &[0x7FFE], 40_000, 3)]);
    let found = keyboard_input(&observer);
    assert!(
        found.is_empty(),
        "an instruction reading one row 40,000 times a frame is not reading keys: {:?}",
        found.first().map(|finding| &finding.because)
    );
}

/// A Kempston has a port of its own and says so plainly.
#[test]
fn reading_port_1f_is_a_kempston_joystick() {
    let observer = watched(vec![reading(0x8100, &[0x001F], 1, 400)]);
    let found = joystick_input(&observer);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].address, 0x8100);
    assert_eq!(found[0].label, "read_joystick");
    assert!(
        found[0].sure >= Sure::Certain,
        "its own port leaves little to guess at: {} ({:.2})",
        found[0].sure.label(),
        found[0].score
    );
    assert!(
        found[0].because.contains("Kempston"),
        "and it should say which joystick: {:?}",
        found[0].because
    );
}

/// A Sinclair or Interface II joystick is wired to the number keys, so the
/// same instruction reads both. The most that can be said is that it might be
/// a joystick, and it is said in those words.
#[test]
fn the_sinclair_rows_are_only_a_possible_joystick() {
    let observer = watched(vec![
        reading(0x8200, &[0xF7FE, 0xEFFE], 2, 400),
        // The same rows among all eight is a keyboard scan, not a joystick.
        reading(
            0x8300,
            &[
                0xFEFE, 0xFDFE, 0xFBFE, 0xF7FE, 0xEFFE, 0xDFFE, 0xBFFE, 0x7FFE,
            ],
            8,
            400,
        ),
    ]);

    let found = joystick_input(&observer);
    let addresses: Vec<u16> = found.iter().map(|finding| finding.address).collect();
    assert_eq!(
        addresses,
        vec![0x8200],
        "only the instruction reading nothing but those two rows"
    );
    assert_eq!(
        found[0].sure,
        Sure::Possible,
        "and it is a possibility, not a finding: {:.2}",
        found[0].score
    );
    assert!(
        found[0].because.contains("number keys"),
        "with the ambiguity said out loud: {:?}",
        found[0].because
    );
}

/// Manic Miner reads the keyboard every frame, and the detector should find it
/// in a recording of the game being played rather than only in a fixture.
#[test]
fn it_finds_manic_miners_keyboard_routine() {
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

    let found = keyboard_input(&app.spec.bus.observer);
    assert!(
        !found.is_empty(),
        "a game being played reads the keyboard somewhere"
    );
    assert!(
        found[0].because.contains("port $FE"),
        "and it should say what it rests on: {:?}",
        found[0].because
    );
}

/// A game reads the keyboard from several places — a different IN for each
/// half-row it cares about — and every one of them is worth a line. Answering
/// with the one routine they happen to sit in says less than stopping on the
/// port would have told you anyway.
#[test]
fn every_instruction_that_reads_the_keyboard_is_listed() {
    let observer = watched(vec![
        reading(0x87F4, &[0xFEFE], 1, 2995),
        reading(0x87F9, &[0x7FFE], 1, 2995),
        reading(0x8803, &[0xFDFE], 1, 2995),
        reading(0x8822, &[0xBFFE], 1, 2995),
    ]);

    let found = keyboard_input(&observer);
    let addresses: Vec<u16> = found.iter().map(|finding| finding.address).collect();
    assert_eq!(
        addresses,
        vec![0x87F4, 0x87F9, 0x8803, 0x8822],
        "all four reads should be listed, not the one thing they have in common"
    );
}

/// Code the machine was already running when watching started was never seen
/// to be called, so it has no routine to be credited to — and a game's own key
/// handling is exactly that. Dropping those accesses left the detector with
/// only what the ROM did.
#[test]
fn a_read_outside_any_known_routine_is_still_found() {
    use zx_rustrum::machine::Spectrum;

    let mut spec = Spectrum::new();
    // LD BC,$FEFE : IN A,(C) : JR back — the row in B and the ULA's port in
    // C, which is how the keyboard is read, in a loop called by nobody: the
    // machine is simply running here.
    for (offset, byte) in [0x01u8, 0xFE, 0xFE, 0xED, 0x78, 0x18, 0xFA]
        .iter()
        .enumerate()
    {
        spec.bus.poke(0x8000 + offset as u16, *byte);
    }
    spec.cpu.pc = 0x8000;
    spec.bus.observer.enabled = true;
    for _ in 0..40 {
        spec.step_instruction();
    }

    assert!(
        spec.bus.observer.routines.is_empty(),
        "nothing was called, so there are no routines to hang this on"
    );
    let found = keyboard_input(&spec.bus.observer);
    assert!(
        !found.is_empty(),
        "the IN at $8003 reads the keyboard and should be found regardless"
    );
    assert_eq!(found[0].address, 0x8003, "at the instruction doing it");
    assert_eq!(
        found[0].entry, None,
        "with no routine to name, so the label goes where the reading is"
    );
}

/// A routine as the observer would have recorded it, for the detectors that
/// work from what a routine did rather than from one instruction.
fn wrote(entry: u16, screen: u32, attrs: u32, calls: u32, frames: u32) -> (u16, Observed) {
    let mut seen = Observed::default();
    seen.entry = entry;
    seen.calls = calls;
    seen.frames = frames;
    seen.writes.screen = screen * calls;
    seen.writes.attrs = attrs * calls;
    (entry, seen)
}

fn routines(seen: Vec<(u16, Observed)>) -> Observer {
    let mut observer = Observer::new();
    observer.routines = seen.into_iter().collect();
    observer
}

/// A clear writes one value over the whole display file. What tells it from a
/// routine painting a screenful of graphics is not how much it writes — a blit
/// writes as much, as fast — but that every byte is the same one.
#[test]
fn a_routine_that_fills_the_display_file_with_one_value_is_a_clear() {
    let (entry, mut seen) = wrote(0x8000, 6144, 0, 40, 40);
    seen.filled_with = Some(0x00);
    seen.wrote_between = Some((0x4000, 0x57FF));
    let observer = routines(vec![(entry, seen)]);

    let found = screen_clear(&observer);
    assert_eq!(found.len(), 1, "one routine clears the screen");
    assert_eq!(found[0].address, 0x8000);
    assert_eq!(found[0].label, "clear_screen");
    assert!(
        found[0].sure >= Sure::Likely,
        "a whole display file of one value is not a maybe: {} ({:.2})",
        found[0].sure.label(),
        found[0].score
    );
    assert!(
        found[0].because.contains("$00"),
        "and it should say what it filled it with: {:?}",
        found[0].because
    );
}

/// A screen blit writes as much of the display file as a clear does, and is
/// not one. Nothing but the values distinguishes them, so a routine writing
/// several is not offered at all rather than offered with a low number.
#[test]
fn a_screen_blit_is_not_a_clear() {
    let (entry, mut seen) = wrote(0x8100, 6144, 0, 40, 40);
    seen.filled_with = None;
    seen.mixed_values = true;
    seen.wrote_between = Some((0x4000, 0x57FF));

    assert!(
        screen_clear(&routines(vec![(entry, seen)])).is_empty(),
        "writing a screenful of different bytes is drawing, not clearing"
    );
}

/// A main loop is credited with everything it does over minutes of play, which
/// is hundreds of screenfuls. Asking only what fraction of a screen it covered
/// called Manic Miner's main loop a screen clear.
#[test]
fn a_main_loop_is_not_a_screen_clear() {
    let (entry, mut seen) = wrote(0x9028, 1_953_494, 276_172, 5, 5);
    seen.filled_with = Some(0x00);
    seen.wrote_between = Some((0x4000, 0x9CFD));

    assert!(
        screen_clear(&routines(vec![(entry, seen)])).is_empty(),
        "three hundred screenfuls in one call is not a screen being cleared"
    );
}

/// A sprite goes on the screen a few dozen bytes at a time and the next one
/// goes somewhere else: little in any one call, over the whole screen across
/// many of them, again and again.
#[test]
fn a_routine_that_draws_a_little_all_over_the_screen_is_drawing_sprites() {
    let (entry, mut seen) = wrote(0x8FF4, 12, 0, 9000, 3000);
    seen.filled_with = None;
    seen.mixed_values = true;
    seen.wrote_between = Some((0x4000, 0x57FF));

    let found = sprite_update(&routines(vec![(entry, seen)]));
    assert_eq!(found.len(), 1, "one routine draws the sprites");
    assert_eq!(found[0].address, 0x8FF4);
    assert_eq!(found[0].label, "draw_sprite");
    assert!(
        found[0].because.contains("3.0 times a frame"),
        "it should say how often, since that is half the case: {:?}",
        found[0].because
    );

    // A routine that fills the same few bytes with one value is not drawing.
    let (entry, mut seen) = wrote(0x92CB, 8, 0, 9000, 3000);
    seen.filled_with = Some(0xFF);
    seen.wrote_between = Some((0x5000, 0x577F));
    assert!(
        sprite_update(&routines(vec![(entry, seen)])).is_empty(),
        "writing one value over and over is filling, not drawing a shape"
    );
}

/// The sprite detector against a recording of a real game rather than a
/// fixture. Manic Miner draws Willy, the guardians and the conveyor a dozen
/// bytes at a time, three times a frame, out of shapes held above the code.
#[test]
fn it_finds_manic_miners_sprite_routine() {
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
    for _ in 0..600 {
        app.advance(1.0 / 50.0);
    }

    let found = sprite_update(&app.spec.bus.observer);
    assert!(
        !found.is_empty(),
        "a game with things moving about draws them somewhere"
    );
    assert!(
        found[0].because.contains("times a frame"),
        "and it should say how often: {:?}",
        found[0].because
    );

    // Nothing clears the whole screen in this stretch: the recording starts
    // part-way through a level and never leaves it. Saying so is the answer;
    // offering the nearest routine would put an address in front of somebody
    // who would go and look at it.
    let clears = screen_clear(&app.spec.bus.observer);
    assert!(
        clears.is_empty(),
        "nothing clears the screen here, and it should not invent one: {:?}",
        clears.first().map(|finding| &finding.because)
    );
}
