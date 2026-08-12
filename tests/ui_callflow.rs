//! The program's loop, drawn in the emulator.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn app() -> App {
    let roms = Roms {
        rom48: std::fs::read("roms/48.rom").ok(),
        rom128: std::fs::read("roms/128.rom").ok(),
        rom_plus3: None,
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

/// The window opens from the Windows row, and says plainly that it has nothing
/// to draw yet rather than drawing an empty box.
#[test]
fn the_call_flow_window_opens_and_says_when_it_has_nothing() {
    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app());
    h.run_steps(3);

    h.get_by_label("Call flow").click();
    h.run_steps(3);
    assert!(h.state().show_callflow, "the toggle did not open it");
    h.get_by_label("Main game loop");
}

/// Given a program that has actually been watched, it finds the loop and has
/// something to draw. Skips itself without a recording to watch.
#[test]
fn it_finds_the_loop_in_a_recording() {
    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let mut app = app();
    app.load_path(&path);
    if app.rzx.is_none() {
        return; // no ROMs on this machine
    }
    app.callflow.looking = true;
    app.spec.bus.observer.enabled = true;
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.max_speed = true;
    }
    for _ in 0..200 {
        app.advance(1.0 / 50.0);
    }
    app.show_callflow = true;

    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    // No clicking: detection runs by itself while the program is watched,
    // which is the behaviour being checked.
    h.run_steps(3);

    // The detector says where it thinks the loop is, and how sure it is,
    // above the call flow rather than only naming an address.
    let state = h.state();
    let finding = state
        .callflow
        .findings
        .first()
        .expect("the main game loop detector found nothing in a real game");
    assert_eq!(finding.what, "Main game loop");
    assert!(
        finding.because.contains("frames apart"),
        "a finding should say what it rests on: {:?}",
        finding.because
    );

    let turn = state
        .callflow
        .turn
        .as_ref()
        .expect("no loop was found in five thousand frames of a game");
    let calls = turn.steps.iter().filter(|step| step.enter).count();
    assert!(
        calls > 4,
        "a turn of a game's loop is more than {calls} calls"
    );
    assert!(
        state.callflow.summary.contains("frames a turn"),
        "it should say how long a turn takes: {:?}",
        state.callflow.summary
    );
}

/// Pressing the button is what starts the machine being watched: there is no
/// switch to find somewhere else first, and watching costs something so it
/// does not run until somebody asks for it.
#[test]
fn the_button_starts_the_watching() {
    let mut app = app();
    app.show_callflow = true;
    app.show_debugger = false;

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(
        !h.state().spec.bus.observer.enabled,
        "nothing should be watched before it is asked for"
    );

    h.get_by_label("Main game loop").click();
    h.run_steps(3);
    assert!(h.state().callflow.looking, "it should be looking now");
    assert!(
        h.state().spec.bus.observer.enabled,
        "and the machine should actually be watched, with no other window open"
    );

    h.get_by_label("Stop").click();
    h.run_steps(3);
    assert!(!h.state().spec.bus.observer.enabled, "and stop when told");
}

/// A finding is offered, not applied. Nothing is written against an address
/// until somebody says so — the emulator does not put its own guesses into
/// a person's notes behind their back.
#[test]
fn nothing_is_written_down_until_it_is_confirmed() {
    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let mut app = app();
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.show_callflow = true;
    app.callflow.looking = true;
    app.spec.bus.observer.enabled = true;

    // The notes go somewhere of this test's own. The real ones beside the
    // recording carry labels from earlier sessions, and a test that asserts
    // "nothing was written" would be reading somebody else's work.
    let scratch = std::env::temp_dir().join(format!("zxrs-callflow-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.path = scratch.join("manic.rzx");
    }
    app.reload_notes();

    if let Some(rzx) = app.rzx.as_mut() {
        rzx.max_speed = true;
    }
    for _ in 0..300 {
        app.advance(1.0 / 50.0);
    }

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    let findings = h.state().callflow.findings.clone();
    let found = findings
        .first()
        .expect("the loop should have been found")
        .address;
    assert!(
        h.state().notes.label(found).is_empty(),
        "it wrote a label without being asked"
    );

    // One button per loop found, not one for the window: a list of candidates
    // with a single button would label whichever the code picked.
    let buttons = h.get_all_by_label("Label").count();
    assert_eq!(
        buttons,
        findings.len(),
        "{} loops were found and {buttons} of them can be labelled",
        findings.len()
    );

    h.get_all_by_label("Label").next().unwrap().click();
    h.run_steps(3);

    assert_eq!(
        h.state().notes.label(found),
        "main_game_loop",
        "and after confirming, it should be written down"
    );
    assert!(
        h.state().notes.comment(found).contains("came back"),
        "with what the guess rests on as the comment, so the listing says why: {:?}",
        h.state().notes.comment(found)
    );

    // Each of the others writes its own name. Calling the second candidate the
    // main game loop would say something the detector did not.
    if let Some(other) = findings.get(1) {
        // The row just labelled says so instead of offering a button, so the
        // next button along is the second loop's.
        h.get_all_by_label("Label").next().unwrap().click();
        h.run_steps(3);
        assert_eq!(
            h.state().notes.label(other.address),
            format!("loop_{:04X}", other.address),
            "the second loop should be named after where it is"
        );
    }
    assert!(
        h.state().notes.label_is_auto(found),
        "as a guess, so a later one can replace it and nothing of the user's is lost"
    );
}

/// Clicking a loop's address takes you to it in the debugger and marks the
/// row. A listing scrolled to an address without marking it leaves the reader
/// counting lines to work out which of the twenty on show was meant.
#[test]
fn clicking_a_loop_marks_its_row_in_the_debugger() {
    use egui_kittest::kittest::Queryable;

    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let mut app = app();
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.show_callflow = true;
    app.show_debugger = false;
    app.callflow.looking = true;
    app.spec.bus.observer.enabled = true;
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.max_speed = true;
    }
    for _ in 0..300 {
        app.advance(1.0 / 50.0);
    }

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    let finding = h
        .state()
        .callflow
        .findings
        .first()
        .expect("the loop should have been found")
        .clone();
    assert_ne!(
        h.state().dbg.marked,
        Some(finding.address),
        "nothing is marked before it is asked for"
    );

    h.get_by_label(&format!("${:04X}", finding.address)).click();
    h.run_steps(3);

    assert!(h.state().show_debugger, "it should open the debugger");
    assert_eq!(
        h.state().dbg.view_addr,
        finding.address,
        "and show the listing there"
    );
    assert_eq!(
        h.state().dbg.marked,
        Some(finding.address),
        "with the row marked, so it can be picked out of the listing"
    );
    assert!(
        !h.state().dbg.follow_pc,
        "and stop following the PC, or the listing walks away from it"
    );
}

/// Each question has a button of its own, and pressing one asks that question
/// of the machine — the list underneath is the answer to what was chosen, not
/// a pile of everything the emulator can think of.
#[test]
fn each_question_has_its_own_button() {
    use egui_kittest::kittest::Queryable;
    use zx_rustrum::detect::Question;

    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let mut app = app();
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.show_callflow = true;

    // Notes of this test's own: the real ones beside the recording are the
    // user's work.
    let scratch = std::env::temp_dir().join(format!("zxrs-questions-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.path = scratch.join("manic.rzx");
        rzx.max_speed = true;
    }
    app.reload_notes();

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    for question in Question::all() {
        h.get_by_label(question.label()).click();
        h.run_steps(2);
        assert_eq!(
            h.state().callflow.asking,
            question,
            "pressing {} should ask that question",
            question.label()
        );
        assert!(
            h.state().callflow.looking,
            "and start watching, so there is no switch to find first"
        );

        // Let the game run under the question being asked.
        h.state_mut().spec.bus.observer.enabled = true;
        for _ in 0..300 {
            h.state_mut().advance(1.0 / 50.0);
        }
        h.state_mut().callflow.looked_at = None;
        h.run_steps(3);

        // Whatever is in the list answers the question that was asked, not the
        // one asked before it.
        let findings = h.state().callflow.findings.clone();
        for finding in &findings {
            // The loops after the best one are called "Loop" rather than the
            // main game loop, which is still an answer to the question asked.
            let answers = finding.what == question.label()
                || (question == Question::MainGameLoop && finding.what == "Loop");
            assert!(
                answers,
                "asked for the {}, and the list holds {:?}",
                question.label(),
                finding.what
            );
        }
        // Manic Miner has a loop, reads the keys and draws its sprites. It is
        // played on the keyboard, and the recording starts part-way through a
        // level and never leaves it, so nothing is claimed about the joystick
        // or about clearing the screen — the answer to those is nothing.
        let must_find = matches!(
            question,
            Question::MainGameLoop | Question::Keyboard | Question::SpriteUpdate
        );
        if must_find {
            assert!(
                !findings.is_empty(),
                "the {} should have been found in a recording of the game",
                question.label()
            );
        }
    }
}

/// The keyboard detector, through the window: press the button, let the game
/// run, and the routines that read port $FE are offered with their own name.
#[test]
fn the_keyboard_question_finds_and_labels_a_routine() {
    use egui_kittest::kittest::Queryable;

    let path = std::path::PathBuf::from("recordings/manic.rzx");
    if !path.exists() {
        return;
    }
    let mut app = app();
    app.load_path(&path);
    if app.rzx.is_none() {
        return;
    }
    app.show_callflow = true;
    let scratch = std::env::temp_dir().join(format!("zxrs-keys-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    if let Some(rzx) = app.rzx.as_mut() {
        rzx.path = scratch.join("manic.rzx");
        rzx.max_speed = true;
    }
    app.reload_notes();

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    h.get_by_label("Keyboard input").click();
    h.run_steps(2);

    h.state_mut().spec.bus.observer.enabled = true;
    for _ in 0..300 {
        h.state_mut().advance(1.0 / 50.0);
    }
    h.state_mut().callflow.looked_at = None;
    h.run_steps(3);

    let found = h
        .state()
        .callflow
        .findings
        .first()
        .expect("the game reads the keyboard while it is being played")
        .clone();
    assert!(h.state().notes.label(found.address).is_empty());

    h.get_all_by_label("Label").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(
        h.state().notes.label(found.address),
        "read_keyboard",
        "the keyboard routine is named for what it does, not for the loop"
    );
    assert!(
        h.state().notes.label_is_auto(found.address),
        "as a guess, so nothing of the user's is overwritten by it"
    );
}

/// Being sent to a listing in a window that is behind the one you are looking
/// at is being sent nowhere, so the debugger asks for the focus as well as
/// opening. The window clears the request as it draws, which is why this asks
/// the App rather than clicking through a harness.
#[test]
fn being_sent_to_the_debugger_asks_for_the_front() {
    let mut app = app();
    app.show_debugger = false;
    app.dbg.raise = false;

    app.show_in_debugger(0x8000);

    assert!(app.show_debugger, "the window should be open");
    assert!(app.dbg.raise, "and asking to be brought to the front");
    assert_eq!(app.dbg.marked, Some(0x8000), "with the row marked");
}

/// The call flow window carries the same Run/Pause button as the main window
/// and the debugger. Watching a program and then stopping it to read what was
/// found is the whole of the job, and reaching for another window to stop it
/// loses your place in this one.
#[test]
fn the_call_flow_window_can_run_and_pause_the_machine() {
    use egui_kittest::kittest::Queryable;

    let mut app = app();
    app.show_callflow = true;
    app.show_debugger = false;
    app.running = true;

    let mut h = egui_kittest::Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    // While it is running the button offers to pause, as it does elsewhere.
    // The last one on the page: the main window carries the same button, and
    // this test is about the call flow window's own.
    let pause = h.get_all_by_label("⏸ Pause").count();
    assert_eq!(pause, 2, "the main window's button and this window's");
    h.get_all_by_label("⏸ Pause").last().unwrap().click();
    h.run_steps(2);
    assert!(!h.state().running, "pressing Pause should stop the machine");

    h.get_all_by_label("▶ Run").last().unwrap().click();
    h.run_steps(2);
    assert!(h.state().running, "and Run should start it again");
}
