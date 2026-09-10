//! The printer window, and the toggle that opens it.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::hardware::Peripheral;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::printer::Paper;
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
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// The window is there to toggle only while there is a printer to show: a
/// Printer button with nothing behind it would open an empty window.
#[test]
fn the_printer_toggle_comes_with_the_printer() {
    let mut h = harness_for(test_app());
    h.run_steps(2);
    assert!(
        h.query_by_label("Printer").is_none(),
        "no printer, no toggle"
    );

    h.state_mut().fit(Peripheral::ZxPrinter, true);
    h.run_steps(2);
    assert!(
        h.query_by_label("Printer").is_some(),
        "a toggle once one is fitted"
    );

    h.state_mut().fit(Peripheral::ZxPrinter, false);
    h.run_steps(2);
    assert!(
        h.query_by_label("Printer").is_none(),
        "and gone when it comes off"
    );
    assert!(!h.state().show_printer, "taking it off closes the window");
}

/// The two printers share a port, so there is room for one: fitting the
/// Alphacom takes the ZX Printer off, and it comes loaded with thermal paper.
#[test]
fn an_alphacom_takes_the_zx_printers_place_on_thermal_paper() {
    let mut app = test_app();
    app.fit(Peripheral::ZxPrinter, true);
    assert_eq!(
        app.spec.bus.printer.as_ref().unwrap().paper,
        Paper::Metallised
    );
    app.fit(Peripheral::Alphacom32, true);
    assert!(
        !app.spec.bus.hardware.fitted(Peripheral::ZxPrinter),
        "the ZX Printer came off"
    );
    assert_eq!(app.spec.bus.printer.as_ref().unwrap().paper, Paper::Thermal);
}

/// The switch at the top changes the paper, and Save PNG… and Tear off wait
/// until something has been printed.
#[test]
fn the_paper_switch_and_the_buttons_in_the_window() {
    let mut app = test_app();
    app.fit(Peripheral::ZxPrinter, true);
    app.show_printer = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.get_by_label("Save PNG…").accesskit_node().is_disabled(),
        "nothing printed, nothing to save"
    );
    assert!(h.get_by_label("Tear off").accesskit_node().is_disabled());

    h.get_by_label("Thermal paper").click();
    h.run_steps(2);
    assert_eq!(
        h.state().spec.bus.printer.as_ref().unwrap().paper,
        Paper::Thermal,
        "the switch put thermal paper in"
    );
    h.get_by_label("Thermal paper").click();
    h.run_steps(2);
    assert_eq!(
        h.state().spec.bus.printer.as_ref().unwrap().paper,
        Paper::Metallised,
        "and back"
    );

    h.state_mut()
        .spec
        .bus
        .printer
        .as_mut()
        .unwrap()
        .lines
        .extend([[0xAA; 32]; 40]);
    h.run_steps(2);
    assert!(
        !h.get_by_label("Save PNG…").accesskit_node().is_disabled(),
        "now there is"
    );
    h.get_by_label("Tear off").click();
    h.run_steps(2);
    assert!(
        h.state()
            .spec
            .bus
            .printer
            .as_ref()
            .unwrap()
            .lines
            .is_empty(),
        "torn off, the paper starts again"
    );
}
