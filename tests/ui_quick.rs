//! The Quick area: ten quicksaves in memory, Load and Save, and the function
//! keys.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
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
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// A quicksave put back is the machine as it was: registers, memory and all.
#[test]
fn a_quicksave_puts_the_machine_back_as_it_was() {
    let mut app = test_app();
    app.spec.cpu.pc = 0x8000;
    app.spec.bus.poke(0x9000, 0x42);
    app.quick_save(3);
    assert_eq!(app.quick_slot, 3, "saving selects the slot it saved to");

    // Somewhere else entirely.
    app.spec.cpu.pc = 0x1234;
    app.spec.bus.poke(0x9000, 0x99);

    app.quick_load();
    assert_eq!(app.spec.cpu.pc, 0x8000, "PC came back");
    assert_eq!(app.spec.bus.peek_raw(0x9000), 0x42, "and so did memory");
    assert!(app.status.contains("slot 3"), "{}", app.status);

    // Loading does not use the quicksave up: it is still there to go back to.
    app.spec.cpu.pc = 0x1111;
    app.quick_load();
    assert_eq!(app.spec.cpu.pc, 0x8000, "and can be gone back to again");
}

/// An empty slot is said to be empty, and the machine is left alone.
#[test]
fn loading_an_empty_slot_leaves_the_machine_alone() {
    let mut app = test_app();
    app.spec.cpu.pc = 0x4321;
    app.quick_slot = 7;
    app.quick_load();
    assert_eq!(app.spec.cpu.pc, 0x4321, "nothing happened to the machine");
    assert!(app.status.contains("empty"), "{}", app.status);
}

/// The function keys save to their own slots and select them: F1 to F9 are
/// slots 1 to 9, and F10 is slot 0.
#[test]
fn the_function_keys_save_to_their_own_slots() {
    let mut app = test_app();
    app.spec.cpu.pc = 0x6000;
    let mut h = harness_for(app);

    h.key_press(egui::Key::F3);
    h.run_steps(2);
    assert!(h.state().quick[3].is_some(), "F3 saves to slot 3");
    assert_eq!(h.state().quick_slot, 3, "and selects it");

    h.key_press(egui::Key::F10);
    h.run_steps(2);
    assert!(h.state().quick[0].is_some(), "F10 saves to slot 0");
    assert_eq!(h.state().quick_slot, 0, "and selects it");
    assert!(h.state().quick[5].is_none(), "and nothing else is touched");
}

/// The Quick area is on the main window: Load, Save and the slot.
#[test]
fn the_quick_area_is_on_the_main_window() {
    let mut app = test_app();
    app.quick_slot = 4;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(h.query_by_label("Load").is_some(), "a Load button");
    assert!(h.query_by_label("Save").is_some(), "a Save button");
    assert!(
        h.query_by_label("4  \u{25be}").is_some(),
        "and the slot, as a button that drops down"
    );

    // Save from the window, into the slot it shows.
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(h.state().quick[4].is_some(), "Save filled slot 4");
}

/// A quicksave is cut off from the sound card while it is kept, and the
/// machine it is restored into takes the sound card over.
///
/// A copy that shared the sound queue would play into it: a kept one would be
/// silent only by never being run, and a restored one without the queue would
/// be a machine nobody could hear.
#[test]
fn a_restored_machine_keeps_the_sound_card() {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    let mut app = test_app();
    let queue: zx_rustrum::audio::SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
    app.spec.bus.audio.attach(queue, 48_000.0);
    assert!(app.spec.bus.audio.attached());

    app.quick_save(2);
    assert!(
        !app.quick[2].as_ref().unwrap().bus.audio.attached(),
        "the kept copy holds no queue"
    );
    assert!(
        app.spec.bus.audio.attached(),
        "and the live machine still does"
    );

    app.quick_load();
    assert!(
        app.spec.bus.audio.attached(),
        "and after loading, the restored machine is the one being heard"
    );
}
