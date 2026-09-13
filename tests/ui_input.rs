//! The Input window: the interface, the mapping, and what the mapping does to
//! the stick, the machine's own keyboard and the buttons of the two mice.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::hardware::Peripheral;
use zx_rustrum::joystick::{Kind, Way};
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::inputwin::{self, Binding, Does, From, MouseButton, Pad, Pads};
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
    app.show_input = true;
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
    let text = inputwin::to_text(&map);
    assert!(text.contains("up"), "{text}");
    assert!(text.contains("key6.0"), "{text}");
    assert_eq!(inputwin::from_text(&text), map, "and back again");

    // A line that means nothing is dropped rather than throwing the rest away,
    // the same way a badly edited note is. egui calls the arrow keys "Up" and
    // the rest, which is what goes in the file.
    let salvaged = inputwin::from_text("Up:up,nonsense,Tab:key9.9");
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
    let text = inputwin::to_text(&map);
    assert!(text.contains("pad.A:fire"), "{text}");
    assert!(text.contains("pad.LeftX-:left"), "{text}");
    assert!(text.contains("pad.Start:key6.0"), "{text}");
    assert_eq!(inputwin::from_text(&text), map, "and back again");

    // A control this does not know is dropped rather than taking the rest of
    // the mapping with it.
    assert_eq!(inputwin::from_text("pad.Nonsuch:fire").len(), 0);
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

    assert!(inputwin::holding(
        From::Pad(Pad::Button(gilrs::Button::South)),
        &nothing,
        &pads
    ));
    assert!(!inputwin::holding(
        From::Pad(Pad::Button(gilrs::Button::East)),
        &nothing,
        &pads
    ));
    assert!(
        inputwin::holding(
            From::Pad(Pad::Axis(gilrs::Axis::LeftStickX, false)),
            &nothing,
            &pads
        ),
        "the stick is well over to the left"
    );
    assert!(
        !inputwin::holding(
            From::Pad(Pad::Axis(gilrs::Axis::LeftStickX, true)),
            &nothing,
            &pads
        ),
        "and that is not the same as being over to the right"
    );
    assert!(
        !inputwin::holding(
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
    assert!(inputwin::holding(From::Key(egui::Key::Z), &z_down, &pads));
    assert!(!inputwin::holding(From::Key(egui::Key::X), &z_down, &pads));
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

/// The window is called Input now, since it works more than a joystick, and
/// the toolbar says so.
#[test]
fn the_toolbar_opens_an_input_window() {
    let mut h = harness_for(test_app());
    h.run_steps(2);
    assert!(h.query_by_label("Input").is_some(), "an Input toggle");
    assert!(
        h.query_by_label("Joystick").is_none(),
        "and no Joystick one"
    );
}

/// A key bound to a mouse button holds it down, on either mouse, and is taken
/// away from the machine's own keyboard like any other bound key.
#[test]
fn a_key_can_hold_a_mouse_button() {
    let mut app = test_app();
    app.fit(Peripheral::KempstonMouse, true);
    app.joystick_map = vec![Binding {
        from: From::Key(egui::Key::Z),
        does: Does::Mouse(MouseButton::KempstonRight),
    }];
    let mut h = harness_for(app);
    h.key_down(egui::Key::Z);
    h.run_steps(2);
    assert_eq!(
        h.state().spec.bus.mouse.buttons & 0x01,
        0,
        "the Kempston's right button, bit 0, is down: {:02X}",
        h.state().spec.bus.mouse.buttons
    );
    assert_eq!(
        h.state().spec.bus.keys[0] & (1 << 1),
        1 << 1,
        "and Z is not typed as well"
    );
    h.key_up(egui::Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().spec.bus.mouse.buttons, 0xFF, "and let go");

    let mut app = test_app();
    app.fit(Peripheral::AmxMouse, true);
    app.joystick_map = vec![
        Binding {
            from: From::Key(egui::Key::Tab),
            does: Does::Mouse(MouseButton::AmxLeft),
        },
        Binding {
            from: From::Key(egui::Key::Q),
            does: Does::Mouse(MouseButton::AmxMiddle),
        },
    ];
    let mut h = harness_for(app);
    h.key_down(egui::Key::Tab);
    h.key_down(egui::Key::Q);
    h.run_steps(2);
    let buttons = h.state().spec.bus.amx.as_ref().unwrap().buttons;
    assert_eq!(
        buttons, 0x3F,
        "the AMX's left and middle, bits 7 and 6: {buttons:02X}"
    );
}

/// The mouse buttons are written down and read back with the rest.
#[test]
fn a_mouse_binding_is_remembered_as_text() {
    let map: Vec<Binding> = MouseButton::ALL
        .iter()
        .map(|button| Binding {
            from: From::Pad(Pad::Button(gilrs::Button::South)),
            does: Does::Mouse(*button),
        })
        .collect();
    let text = inputwin::to_text(&map);
    assert!(text.contains("pad.A:kmouse.left"), "{text}");
    assert!(text.contains("pad.A:amx.middle"), "{text}");
    assert_eq!(inputwin::from_text(&text), map, "and back again");
}

/// Space is the stick's fire by default, but only while a stick is plugged in
/// to be fired. With none — which is how a machine starts — it has to be the
/// Spectrum's own space, or it is a key that does nothing at all.
#[test]
fn space_types_a_space_with_no_stick_plugged_in() {
    let mut app = test_app();
    app.spec.bus.joystick.kind = Kind::None;
    let mut h = harness_for(app);
    h.key_down(egui::Key::Space);
    h.run_steps(2);
    assert_eq!(
        h.state().spec.bus.keys[7] & 1,
        0,
        "SPACE, row 7 bit 0, is down: {:02X}",
        h.state().spec.bus.keys[7]
    );
    h.key_up(egui::Key::Space);
    h.run_steps(2);

    // Plug a stick in and the same key fires it instead.
    h.state_mut().spec.bus.joystick.kind = Kind::Kempston;
    h.key_down(egui::Key::Space);
    h.run_steps(2);
    assert!(h.state().spec.bus.joystick.is_down(Way::Fire), "fire");
    assert_eq!(h.state().spec.bus.keys[7] & 1, 1, "and no space typed");
}

/// A key bound to a mouse button whose mouse is not fitted keeps typing: the
/// binding waits for its mouse rather than swallowing the key.
#[test]
fn a_mouse_binding_without_its_mouse_leaves_the_key_alone() {
    let mut app = test_app();
    app.joystick_map = vec![Binding {
        from: From::Key(egui::Key::Z),
        does: Does::Mouse(MouseButton::AmxLeft),
    }];
    let mut h = harness_for(app);
    h.key_down(egui::Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().spec.bus.keys[0] & (1 << 1), 0, "Z is typed");
}

/// A key held down in one of the other windows reaches the machine as well:
/// whichever window was clicked last, the machine is what is being typed at.
/// The windows gather what they hold while they draw, and the next frame's
/// keyboard reads it.
#[test]
fn a_key_held_in_another_window_reaches_the_machine() {
    let mut h = harness_for(test_app());
    h.run_steps(1);
    let mut held = zx_rustrum::ui::HeldKeys::default();
    held.keys.insert(egui::Key::A);
    held.shift = true;
    h.state_mut().window_keys_next = held;
    h.run_steps(1);
    let keys = h.state().spec.bus.keys;
    assert_eq!(keys[1] & 1, 0, "A, row 1 bit 0, is down: {keys:02X?}");
    assert_eq!(keys[0] & 1, 0, "and CAPS SHIFT with it");
    h.run_steps(1);
    assert_eq!(
        h.state().spec.bus.keys[1] & 1,
        1,
        "let go once the window stops holding it"
    );
}
