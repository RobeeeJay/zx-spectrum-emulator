//! Drives the real user interface through egui's test harness, so the machine
//! selector is covered as a UI interaction rather than only as the state it
//! calls into.
//!
//! Note on the debug windows: this harness embeds viewports instead of opening
//! real OS windows, so with them enabled they are drawn *inside* the main
//! window and can sit on top of its menu bar, which synthetic clicks then hit
//! instead of the widget underneath. That is an artefact of embedding, not of
//! the emulator, so these tests keep them closed and exercise the widgets
//! directly.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_spectrum_emulator::machine::{Model, Spectrum};
use zx_spectrum_emulator::ui::{App, Roms};

/// An app with stand-in ROMs for every machine, so switching always has an
/// image to load.
fn test_app() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: Some(vec![0x00; 0x8000]),
        rom_plus3: Some(vec![0x00; 0x10000]),
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
    Harness::new_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

fn harness<'a>() -> Harness<'a, App> {
    harness_for(test_app())
}

fn collect_labels(node: &egui_kittest::Node<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(l) = node.accesskit_node().label() {
        out.push(l.to_string());
    }
    for c in node.children() {
        out.extend(collect_labels(&c));
    }
    out
}

/// Labels of the entries in the Machine menu, and the model each selects.
const MENU_ENTRIES: [(&str, Model); 4] = [
    ("ZX Spectrum 48K", Model::Spectrum48),
    ("ZX Spectrum 128K (AY sound)", Model::Spectrum128),
    ("ZX Spectrum +2A", Model::Plus2A),
    ("ZX Spectrum +3 (no disk drive)", Model::Plus3),
];

/// Labels of the always-visible selector buttons.
const ROW_ENTRIES: [(&str, Model); 4] = [
    ("48K", Model::Spectrum48),
    ("128K", Model::Spectrum128),
    ("+2A", Model::Plus2A),
    ("+3", Model::Plus3),
];

#[test]
fn the_machine_menu_switches_model() {
    let mut h = harness();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum48);

    h.get_by_label("Machine").click();
    h.run_steps(3);
    h.get_by_label("ZX Spectrum 128K (AY sound)").click();
    h.run_steps(3);

    assert_eq!(
        h.state().spec.bus.model,
        Model::Spectrum128,
        "status says: {}",
        h.state().status
    );
}

#[test]
fn every_machine_menu_entry_works() {
    for (label, model) in MENU_ENTRIES {
        let mut h = harness();
        h.run_steps(3);
        if model == Model::Spectrum48 {
            // Start somewhere else so switching back is a real change.
            h.state_mut().switch_model(Model::Spectrum128);
            h.run_steps(3);
        }
        h.get_by_label("Machine").click();
        h.run_steps(3);
        h.get_by_label(label).click();
        h.run_steps(3);
        assert_eq!(
            h.state().spec.bus.model,
            model,
            "clicking {label:?} should select {}; status says: {}",
            model.name(),
            h.state().status
        );
    }
}

#[test]
fn every_machine_row_button_works() {
    for (label, model) in ROW_ENTRIES {
        let mut h = harness();
        h.run_steps(3);
        if model == Model::Spectrum48 {
            h.state_mut().switch_model(Model::Spectrum128);
            h.run_steps(3);
        }
        h.get_by_label(label).click();
        h.run_steps(3);
        assert_eq!(
            h.state().spec.bus.model,
            model,
            "clicking the {label} button; status says: {}",
            h.state().status
        );
    }
}

#[test]
fn switching_resumes_the_new_machine() {
    let mut h = harness();
    h.run_steps(3);
    assert!(!h.state().running, "this test starts paused");
    h.get_by_label("128K").click();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum128);
    assert!(
        h.state().running,
        "a switch should leave the new machine running, not frozen"
    );
}

#[test]
fn a_missing_rom_reports_an_error_instead_of_failing_silently() {
    let mut app = test_app();
    app.roms.rom128 = None;
    let mut h = harness_for(app);
    h.run_steps(3);

    h.get_by_label("128K (no ROM)").click();
    h.run_steps(3);

    assert_eq!(
        h.state().spec.bus.model,
        Model::Spectrum48,
        "must not switch without a ROM"
    );
    assert!(h.state().status_is_error, "the failure must be flagged");
    assert!(
        h.state().status.contains("128.rom"),
        "the message should name the file: {}",
        h.state().status
    );
}

#[test]
fn the_machine_row_marks_missing_roms() {
    let mut app = test_app();
    app.roms.rom_plus3 = None;
    let mut h = harness_for(app);
    h.run_steps(3);
    let labels = collect_labels(&h.root());
    assert!(
        labels.iter().any(|l| l == "+3 (no ROM)"),
        "missing ROMs should be labelled: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "128K"),
        "present ROMs should not be: {labels:?}"
    );
}

#[test]
fn selecting_the_current_machine_says_so() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label("48K").click();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum48);
    assert!(
        h.state().status.contains("Already running"),
        "status says: {}",
        h.state().status
    );
}

#[test]
fn the_toolbar_toggles_the_debug_windows() {
    let mut h = harness();
    h.run_steps(3);
    assert!(!h.state().show_tape);
    h.get_by_label("Tape").click();
    h.run_steps(3);
    assert!(h.state().show_tape, "the Tape toggle should open the window");
}

#[test]
fn the_toolbar_reset_keeps_the_model() {
    let mut h = harness();
    h.run_steps(3);
    h.get_by_label("128K").click();
    h.run_steps(3);
    h.get_by_label("Reset").click();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum128);
    assert!(
        h.state().status.contains("Reset"),
        "status says: {}",
        h.state().status
    );
}

// ---------------------------------------------------------------------------
// loading ROM images
// ---------------------------------------------------------------------------

/// Write a ROM image of `len` bytes whose first byte is `marker`.
fn temp_rom(name: &str, len: usize, marker: u8) -> std::path::PathBuf {
    let mut data = vec![0u8; len];
    data[0] = marker;
    let path = std::env::temp_dir().join(format!("zx-test-{name}.rom"));
    std::fs::write(&path, &data).unwrap();
    path
}

#[test]
fn a_32k_rom_selects_the_128k() {
    let path = temp_rom("128", 0x8000, 0xaa);
    let mut h = harness();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum48);

    h.state_mut().load_path(&path);
    h.run_steps(3);

    let app = h.state();
    assert_eq!(
        app.spec.bus.model,
        Model::Spectrum128,
        "status says: {}",
        app.status
    );
    assert_eq!(app.spec.bus.rom.len(), 0x8000, "the whole image must fit");
    assert_eq!(app.spec.bus.rom[0], 0xaa, "and actually be loaded");
    assert!(!app.status_is_error, "status says: {}", app.status);
    assert!(
        app.status.contains("128K"),
        "the status should name the machine: {}",
        app.status
    );
}

#[test]
fn a_64k_rom_selects_a_plus3_and_a_16k_rom_a_48k() {
    let mut h = harness();
    h.run_steps(3);

    h.state_mut().load_path(&temp_rom("plus3", 0x10000, 0xbb));
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Plus3);
    assert_eq!(h.state().spec.bus.rom.len(), 0x10000);
    assert_eq!(h.state().spec.bus.rom[0], 0xbb);

    h.state_mut().load_path(&temp_rom("48", 0x4000, 0xcc));
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum48);
    assert_eq!(h.state().spec.bus.rom.len(), 0x4000);
    assert_eq!(h.state().spec.bus.rom[0], 0xcc);
}

#[test]
fn a_loaded_rom_is_remembered_for_later_switches() {
    let mut app = test_app();
    app.roms.rom128 = None;
    let mut h = harness_for(app);
    h.run_steps(3);

    h.state_mut().load_path(&temp_rom("128-remember", 0x8000, 0xd5));
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum128);

    // Away and back again: the image loaded from disk should still be used.
    h.get_by_label("48K").click();
    h.run_steps(3);
    assert_eq!(h.state().spec.bus.model, Model::Spectrum48);
    h.get_by_label("128K").click();
    h.run_steps(3);
    assert_eq!(
        h.state().spec.bus.model,
        Model::Spectrum128,
        "status says: {}",
        h.state().status
    );
    assert_eq!(h.state().spec.bus.rom[0], 0xd5, "the loaded ROM was kept");
}

#[test]
fn an_odd_sized_rom_is_reported_rather_than_silently_truncated() {
    let path = temp_rom("odd", 0x5000, 0xee);
    let mut h = harness();
    h.run_steps(3);
    h.state_mut().load_path(&path);
    h.run_steps(3);
    assert!(h.state().status_is_error, "status says: {}", h.state().status);
    assert_eq!(
        h.state().spec.bus.model,
        Model::Spectrum48,
        "an unrecognised size should not change machine"
    );
}

#[test]
fn the_overscan_toggle_changes_how_much_border_is_drawn() {
    use zx_spectrum_emulator::screen::View;

    let mut h = harness();
    h.run_steps(3);
    assert!(h.state().overscan, "the full border is shown by default");
    assert_eq!(h.state().view(), View::OVERSCAN);

    h.get_by_label("Overscan").click();
    h.run_steps(3);

    assert!(!h.state().overscan);
    let view = h.state().view();
    assert_eq!(view, View::CROPPED);
    assert!(
        view.width() < View::OVERSCAN.width() && view.height() < View::OVERSCAN.height(),
        "cropping should show a smaller area: {view:?}"
    );

    h.get_by_label("Overscan").click();
    h.run_steps(3);
    assert_eq!(h.state().view(), View::OVERSCAN, "and back again");
}
