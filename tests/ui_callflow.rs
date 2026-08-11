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
    h.get_by_label("Find the loop");
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
    h.run_steps(3);
    h.get_by_label("Find the loop").click();
    h.run_steps(3);

    let state = h.state();
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
