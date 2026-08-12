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

/// AutoDoc can be switched on from the call flow window, and it watches even
/// with the debugger shut.
///
/// Turning it on used to take effect in the debugger's own frame, so switching
/// it on from here with the debugger closed set a flag and watched nothing.
#[test]
fn autodoc_can_be_switched_on_from_the_call_flow_window() {
    let mut app = app();
    app.show_callflow = true;
    app.show_debugger = false;
    app.dbg.autodoc = false;

    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(
        !h.state().spec.bus.observer.enabled,
        "nothing should be watched before it is asked for"
    );

    h.get_by_label("AutoDoc").click();
    h.run_steps(3);

    assert!(h.state().dbg.autodoc, "the toggle did not take");
    assert!(
        h.state().spec.bus.observer.enabled,
        "the switch is on but nothing is being watched, which is the bug this \
         window used to have with the debugger closed"
    );

    // And off again.
    h.get_by_label("AutoDoc").click();
    h.run_steps(3);
    assert!(!h.state().spec.bus.observer.enabled, "it should stop too");
}
