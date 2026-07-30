//! RAM access map controls.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_spectrum_emulator::machine::Spectrum;
use zx_spectrum_emulator::ui::{App, Roms};

fn harness<'a>() -> Harness<'a, App> {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = true;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

#[test]
fn reads_writes_and_executes_can_each_be_toggled() {
    let mut h = harness();
    h.run_steps(3);
    assert!(h.state().ram.show_read);
    assert!(h.state().ram.show_write);
    assert!(h.state().ram.show_exec);

    h.get_by_label("Read").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_read, "Read should have turned off");
    assert!(h.state().ram.show_write, "and left the others alone");
    assert!(h.state().ram.show_exec);

    h.get_by_label("Write").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_write);

    h.get_by_label("Execute").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_exec);

    // And back on again.
    h.get_by_label("Read").click();
    h.run_steps(3);
    assert!(h.state().ram.show_read);
}

#[test]
fn each_channel_has_its_own_fade_control() {
    let mut h = harness();
    h.run_steps(3);
    // A slider contributes more than one node (the drag value and its label),
    // so count matches rather than expecting exactly one.
    for l in ["read fade", "write fade", "exec fade"] {
        assert!(
            h.query_all_by_label(l).count() > 0,
            "expected a {l} slider in the RAM map window"
        );
    }
}
