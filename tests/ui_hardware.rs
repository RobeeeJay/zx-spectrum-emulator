//! The Hardware window in sections, and the joystick interfaces in it kept in
//! step with the Input window.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::hardware::{Peripheral, Section};
use zx_rustrum::joystick::Kind;
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

/// Every interface is in one section, and each section has something in it:
/// a heading over nothing, or an interface under no heading, would be a
/// mistake in the list rather than a choice.
#[test]
fn every_interface_is_in_a_section() {
    for section in Section::ALL {
        let members: Vec<_> = Peripheral::ALL
            .into_iter()
            .filter(|p| p.section() == section)
            .collect();
        assert!(!members.is_empty(), "{} has nothing in it", section.name());
    }
    assert_eq!(Peripheral::KempstonJoystick.section(), Section::Joysticks);
    assert_eq!(Peripheral::DkTronicsJoystick.section(), Section::Joysticks);
    assert_eq!(Peripheral::Interface1.section(), Section::Drives);
    assert_eq!(Peripheral::Uspeech.section(), Section::Audio);
}

/// The window shows the sections as headings, in the order they are listed.
#[test]
fn the_window_is_in_sections() {
    let mut app = test_app();
    app.show_hardware = true;
    let mut h = Harness::builder()
        .with_size([1500.0, 2400.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    let mut tops = Vec::new();
    for section in Section::ALL {
        // Headings are set in capitals, as on the machine's front panel.
        let heading = section.name().to_uppercase();
        let node = h
            .query_all_by_value(&heading)
            .next()
            .unwrap_or_else(|| panic!("a {} heading", section.name()));
        tops.push((
            section.name(),
            node.accesskit_node()
                .bounding_box()
                .map(|b| b.y0)
                .unwrap_or(0.0),
        ));
    }
    assert!(
        tops.windows(2).all(|w| w[0].1 < w[1].1),
        "the headings run down the window in order: {tops:?}"
    );
    assert!(
        h.query_by_label("Kempston Joystick Interface").is_some()
            && h.query_by_label("DK'Tronics Joystick Interface").is_some(),
        "the two joystick interfaces are listed"
    );
}

/// One stick, so one interface: fitting a joystick interface in the Hardware
/// window plugs the stick into it, a choice in the Input window fits the
/// interface it needs, and taking the interface off unplugs the stick.
#[test]
fn the_hardware_and_input_windows_agree_about_the_stick() {
    let mut app = test_app();
    app.fit(Peripheral::KempstonJoystick, true);
    assert_eq!(app.spec.bus.joystick.kind, Kind::Kempston);

    app.fit(Peripheral::DkTronicsJoystick, true);
    assert_eq!(
        app.spec.bus.joystick.kind,
        Kind::DkTronicsKempston,
        "its Kempston socket"
    );
    assert!(
        !app.spec.bus.hardware.fitted(Peripheral::KempstonJoystick),
        "and the Kempston interface came off"
    );

    // The Input window moves the stick to the other socket: still DK'Tronics.
    app.spec.bus.set_joystick(Kind::DkTronicsKeys);
    assert!(app.spec.bus.hardware.fitted(Peripheral::DkTronicsJoystick));

    // Interface 2 is wired to keys and is not in the Hardware window: choosing
    // it takes the DK'Tronics off.
    app.spec.bus.set_joystick(Kind::Sinclair1);
    assert!(!app.spec.bus.hardware.fitted(Peripheral::DkTronicsJoystick));

    app.spec.bus.set_joystick(Kind::Kempston);
    assert!(app.spec.bus.hardware.fitted(Peripheral::KempstonJoystick));
    app.fit(Peripheral::KempstonJoystick, false);
    assert_eq!(app.spec.bus.joystick.kind, Kind::None, "unplugged with it");
}

/// A preferences file from before the joystick interfaces were in the
/// Hardware window says only which kind the stick was: loading it fits the
/// interface that kind needs.
#[test]
fn an_old_preferences_file_fits_the_interface_its_stick_needs() {
    let mut app = test_app();
    app.prefs = zx_rustrum::prefs::Prefs::parse("joystick = \"kempston\"\n");
    app.apply_prefs();
    assert_eq!(app.spec.bus.joystick.kind, Kind::Kempston);
    assert!(app.spec.bus.hardware.fitted(Peripheral::KempstonJoystick));
}

/// Teaching the programmable through the app: with its slider at 2, holding
/// the arrow bound to up and pressing 8 on the desk teaches it 8, and with the
/// slider back at 1 the same arrow presses 8 on the machine. What it learnt is
/// remembered between launches.
#[test]
fn the_programmable_is_taught_from_the_desk_and_remembered() {
    let mut app = test_app();
    app.fit(Peripheral::DkTronicsProgrammable, true);
    assert_eq!(app.spec.bus.joystick.kind, Kind::DkTronicsProgrammable);
    app.spec.bus.joystick.programming = true;
    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.key_down(egui::Key::ArrowUp);
    h.key_down(egui::Key::Num8);
    h.run_steps(2);
    h.key_up(egui::Key::Num8);
    h.key_up(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(
        h.state()
            .spec
            .bus
            .joystick
            .taught(zx_rustrum::joystick::Way::Up),
        Some((4, 2)),
        "up was taught 8"
    );

    h.state_mut().spec.bus.joystick.programming = false;
    h.key_down(egui::Key::ArrowUp);
    h.run_steps(2);
    use zx_rustrum::z80::Bus;
    let row = h.state_mut().spec.bus.io_read(0xEFFE);
    assert_eq!(
        row & (1 << 2),
        0,
        "the arrow presses 8 on the machine: {row:02X}"
    );
    h.key_up(egui::Key::ArrowUp);
    h.run_steps(1);

    h.state_mut().save_window_state();
    let saved = h.state().prefs.to_text();
    assert!(saved.contains("joystick_program = \"up:4.2\""), "{saved}");
    let mut next = test_app();
    next.prefs = zx_rustrum::prefs::Prefs::parse(&saved);
    next.apply_prefs();
    assert_eq!(
        next.spec.bus.joystick.taught(zx_rustrum::joystick::Way::Up),
        Some((4, 2))
    );
    assert!(next
        .spec
        .bus
        .hardware
        .fitted(Peripheral::DkTronicsProgrammable));
}
