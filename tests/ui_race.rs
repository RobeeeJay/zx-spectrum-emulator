//! Racing the beam belongs to a stopped machine.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn test_app() -> App {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    let mut h = Harness::builder()
        .with_size([1500.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    h
}

/// The frame is replayed from its interrupt on a copy of the machine, which
/// only means anything while the machine is holding still. A running machine
/// is somewhere else by the time the cursor has been read, so the button is
/// not offered.
#[test]
fn the_button_does_nothing_while_the_machine_runs() {
    let mut h = harness_for(test_app());
    assert!(
        !h.get_by_label("Race the beam")
            .accesskit_node()
            .is_disabled(),
        "a stopped machine should offer it"
    );

    h.state_mut().running = true;
    h.run_steps(2);
    assert!(
        h.get_by_label("Race the beam")
            .accesskit_node()
            .is_disabled(),
        "the button was still offered while the machine was running"
    );
}

/// And starting the machine puts it away: it is a way of looking at a frame
/// that has been stopped in, not a display mode.
#[test]
fn running_the_machine_switches_racing_off() {
    let mut app = test_app();
    app.race_the_beam = true;
    let mut h = harness_for(app);
    assert!(h.state().race_the_beam, "it should be on to begin with");

    h.state_mut().running = true;
    h.run_steps(2);

    assert!(
        !h.state().race_the_beam,
        "the machine was started and racing stayed on"
    );
    assert!(
        h.state().race.is_none(),
        "the snapshot of the frame was kept after the machine started"
    );
}

/// Hovering the picture of a stopped machine replays the frame: what is drawn
/// comes from the copy being run, not from the machine sitting still.
#[test]
fn hovering_a_stopped_machine_replays_its_next_frame() {
    let mut app = test_app();
    app.race_the_beam = true;
    let before = (app.spec.bus.frame, app.spec.bus.tstates);
    let mut h = harness_for(app);

    // The picture fills the window under the toolbar; half way down it is
    // over the display.
    h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(400.0, 400.0)));
    h.run_steps(3);

    assert!(
        h.state().race.is_some(),
        "the cursor was over the picture and no frame was replayed"
    );
    assert_eq!(
        (h.state().spec.bus.frame, h.state().spec.bus.tstates),
        before,
        "the machine moved while its frame was being replayed"
    );
}
