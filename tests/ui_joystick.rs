//! The Joystick window: the interface, the mapping, and what the mapping does
//! to the machine's own keyboard.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::joystick::{Kind, Way};
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::joystickwin::{self, Binding, Does, From, Pad, Pads};
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

/// The window offers every interface and says how a program reads each.
#[test]
fn the_window_offers_every_interface() {
    let mut app = test_app();
    app.show_joystick = true;
    let mut h = harness_for(app);
    h.run_steps(3);

    // The picker starts on None, which is what a machine has by default. It is
    // a button with an arrow after it, so the label is matched loosely.
    assert!(
        h.get_all_by_label_contains("None").next().is_some(),
        "nothing is plugged in to start with"
    );
    // And the five bound keys are listed.
    for way in Way::ALL {
        assert!(
            h.get_all_by_label_contains(way.name()).next().is_some(),
            "{} should have a line",
            way.name()
        );
    }
}

/// A key bound to the stick works the stick, and stops working the machine's
/// own keyboard: holding an arrow would otherwise steer and type at once.
#[test]
fn a_bound_key_works_the_stick_and_not_the_keyboard() {
    let mut app = test_app();
    app.spec.bus.joystick.kind = Kind::Kempston;
    let mut h = harness_for(app);

    // The arrow keys are bound by default. Hold one.
    h.key_down(egui::Key::ArrowLeft);
    h.run_steps(2);
    assert!(
        h.state().spec.bus.joystick.is_down(Way::Left),
        "the stick should be over"
    );
    // The ROM's own left cursor is CAPS SHIFT with 5, at row 3 bit 4: it must
    // not be down as well.
    let keys = h.state().spec.bus.keys;
    assert_eq!(keys[3] & (1 << 4), 1 << 4, "key 5 is not pressed as well");
    assert_eq!(keys[0] & 1, 1, "nor CAPS SHIFT");

    h.key_up(egui::Key::ArrowLeft);
    h.run_steps(2);
    assert!(
        !h.state().spec.bus.joystick.is_down(Way::Left),
        "and let go"
    );
}

/// With nothing plugged in, the keys go back to the machine.
#[test]
fn with_no_interface_the_stick_sees_nothing() {
    let mut app = test_app();
    app.spec.bus.joystick.kind = Kind::None;
    let mut h = harness_for(app);
    h.key_down(egui::Key::ArrowLeft);
    h.run_steps(2);
    assert!(
        !h.state().spec.bus.joystick.is_down(Way::Left),
        "an unplugged stick is not held"
    );
}

/// A binding can be a key of the machine's own keyboard rather than a
/// direction: some games want a key and no stick will do.
#[test]
fn a_binding_can_press_a_key_of_the_machine() {
    let mut app = test_app();
    app.joystick_map = vec![Binding {
        from: From::Key(egui::Key::Tab),
        does: Does::Key(6, 0), // ENTER
    }];
    let mut h = harness_for(app);
    h.key_down(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(
        h.state().spec.bus.keys[6] & 1,
        0,
        "ENTER should be down on the machine"
    );
}

/// The mapping goes to the preferences as text that can be read and edited,
/// and comes back the same.
#[test]
fn the_mapping_is_remembered_as_text() {
    let map = vec![
        Binding {
            from: From::Key(egui::Key::ArrowUp),
            does: Does::Way(Way::Up),
        },
        Binding {
            from: From::Key(egui::Key::Tab),
            does: Does::Key(6, 0),
        },
    ];
    let text = joystickwin::to_text(&map);
    assert!(text.contains("up"), "{text}");
    assert!(text.contains("key6.0"), "{text}");
    assert_eq!(joystickwin::from_text(&text), map, "and back again");

    // A line that means nothing is dropped rather than throwing the rest away,
    // the same way a badly edited note is. egui calls the arrow keys "Up" and
    // the rest, which is what goes in the file.
    let salvaged = joystickwin::from_text("Up:up,nonsense,Tab:key9.9");
    assert_eq!(salvaged.len(), 1, "{salvaged:?}");
}

/// A pad control is written down the same way a key is, and comes back the
/// same: a mapping is a file somebody can read.
#[test]
fn a_pad_control_is_written_down_and_read_back() {
    let map = vec![
        Binding {
            from: From::Pad(Pad::Button(gilrs::Button::South)),
            does: Does::Way(zx_rustrum::joystick::Way::Fire),
        },
        Binding {
            from: From::Pad(Pad::Axis(gilrs::Axis::LeftStickX, false)),
            does: Does::Way(zx_rustrum::joystick::Way::Left),
        },
        Binding {
            from: From::Pad(Pad::Button(gilrs::Button::Start)),
            does: Does::Key(6, 0),
        },
    ];
    let text = joystickwin::to_text(&map);
    assert!(text.contains("pad.A:fire"), "{text}");
    assert!(text.contains("pad.LeftX-:left"), "{text}");
    assert!(text.contains("pad.Start:key6.0"), "{text}");
    assert_eq!(joystickwin::from_text(&text), map, "and back again");

    // A control this does not know is dropped rather than taking the rest of
    // the mapping with it.
    assert_eq!(joystickwin::from_text("pad.Nonsuch:fire").len(), 0);
}

/// Whether a binding is being worked is decided against a snapshot of the
/// pads, so it can be tested without one plugged in — which is the only way
/// to test it at all in a suite that runs on a machine with no pad.
#[test]
fn a_pad_binding_is_on_when_the_control_is_over() {
    let pads = Pads {
        buttons: vec![gilrs::Button::South],
        // A stick a little off centre is not a stick that has been pushed.
        axes: vec![
            (gilrs::Axis::LeftStickX, -0.9),
            (gilrs::Axis::LeftStickY, 0.2),
        ],
        count: 1,
    };
    let nothing = |_key| false;

    assert!(joystickwin::holding(
        From::Pad(Pad::Button(gilrs::Button::South)),
        &nothing,
        &pads
    ));
    assert!(!joystickwin::holding(
        From::Pad(Pad::Button(gilrs::Button::East)),
        &nothing,
        &pads
    ));
    assert!(
        joystickwin::holding(
            From::Pad(Pad::Axis(gilrs::Axis::LeftStickX, false)),
            &nothing,
            &pads
        ),
        "the stick is well over to the left"
    );
    assert!(
        !joystickwin::holding(
            From::Pad(Pad::Axis(gilrs::Axis::LeftStickX, true)),
            &nothing,
            &pads
        ),
        "and that is not the same as being over to the right"
    );
    assert!(
        !joystickwin::holding(
            From::Pad(Pad::Axis(gilrs::Axis::LeftStickY, true)),
            &nothing,
            &pads
        ),
        "a stick resting a little off centre is not pushed: {} is inside the deadzone",
        0.2
    );

    // And a key binding is decided by the keys, with the pads saying nothing
    // about it either way.
    let z_down = |key| key == egui::Key::Z;
    assert!(joystickwin::holding(
        From::Key(egui::Key::Z),
        &z_down,
        &pads
    ));
    assert!(!joystickwin::holding(
        From::Key(egui::Key::X),
        &z_down,
        &pads
    ));
}

/// What is plugged in and what works it are remembered between launches.
#[test]
fn the_interface_and_its_keys_are_remembered() {
    let mut app = test_app();
    app.spec.bus.joystick.kind = Kind::Sinclair2;
    app.joystick_map = vec![Binding {
        from: From::Key(egui::Key::Z),
        does: Does::Way(Way::Fire),
    }];
    app.save_window_state();
    let saved = app.prefs.to_text();
    assert!(saved.contains("joystick = \"sinclair2\""), "{saved}");
    assert!(saved.contains("Z:fire"), "{saved}");

    let mut next = test_app();
    next.prefs = zx_rustrum::prefs::Prefs::parse(&saved);
    next.apply_prefs();
    assert_eq!(next.spec.bus.joystick.kind, Kind::Sinclair2);
    assert_eq!(next.joystick_map.len(), 1);
    assert_eq!(next.joystick_map[0].from, From::Key(egui::Key::Z));
}
