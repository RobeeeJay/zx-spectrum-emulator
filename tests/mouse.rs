//! The Kempston mouse: three ports, two counters that wrap, and the buttons.
//!
//! No ROM reads a mouse, so there is nothing on the machine to measure this
//! against. The port decoding and the bits are Fuse's `kempmouse.c`, written
//! out here so that changing them has to be deliberate.

use egui_kittest::Harness;
use zx_rustrum::hardware::Peripheral;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::mouse::KempstonMouse;
use zx_rustrum::ui::{App, Roms};
use zx_rustrum::z80::Bus;

/// $FADF is the buttons, $FBDF the X counter and $FFDF the Y — decoded only
/// on A0, A5, A8 and A10, so the partial decodes answer too.
#[test]
fn the_three_ports_answer_as_fuse_decodes_them() {
    let mouse = KempstonMouse {
        x: 0x12,
        y: 0x34,
        buttons: 0xFF,
    };
    assert_eq!(mouse.io_read(0xFADF), Some(0xFF), "buttons");
    assert_eq!(mouse.io_read(0xFBDF), Some(0x12), "X");
    assert_eq!(mouse.io_read(0xFFDF), Some(0x34), "Y");
    // Partly decoded: A0 set, A5 clear, then A8 and A10 pick which.
    assert_eq!(mouse.io_read(0x0001), Some(0xFF), "buttons, partly decoded");
    assert_eq!(mouse.io_read(0x0101), Some(0x12), "X, partly decoded");
    assert_eq!(mouse.io_read(0x0501), Some(0x34), "Y, partly decoded");
    // The keyboard's port has A0 clear, so it is nothing to do with it.
    assert_eq!(mouse.io_read(0xFEFE), None, "the ULA's port has A0 clear");
    // The Kempston joystick's $1F is inside the buttons' partial decode —
    // A0 set, A5 and A8 clear — as it is in Fuse. On the bus the joystick is
    // asked first, so a stick on the same machine still answers there.
    assert_eq!(
        mouse.io_read(0x001F),
        Some(0xFF),
        "$1F decodes as the buttons"
    );
}

/// The counters wrap. A program moves its pointer by the difference between
/// two reads, so a counter that stopped at 0 or 255 would pin the pointer to
/// an edge of the screen.
#[test]
fn the_counters_wrap_and_y_counts_up_the_screen() {
    let mut mouse = KempstonMouse::default();
    mouse.move_by(-3, 0);
    assert_eq!(mouse.x, 253, "left of nought wraps to the top");
    mouse.move_by(5, 0);
    assert_eq!(mouse.x, 2);
    // The host's Y runs down the screen; the mouse's runs up.
    mouse.move_by(0, 10);
    assert_eq!(mouse.y, 246, "down the screen counts Y down");
    mouse.move_by(0, -20);
    assert_eq!(mouse.y, 10);
}

/// The buttons are active low, left on bit 1 and right on bit 0, the rest
/// held high.
#[test]
fn a_held_button_clears_its_bit() {
    let mut mouse = KempstonMouse::default();
    assert_eq!(mouse.buttons, 0xFF, "nothing held");
    mouse.set_buttons(true, false);
    assert_eq!(mouse.buttons, 0xFD, "left is bit 1");
    mouse.set_buttons(false, true);
    assert_eq!(mouse.buttons, 0xFE, "right is bit 0");
    mouse.set_buttons(true, true);
    assert_eq!(mouse.buttons, 0xFC, "both");
}

/// Not fitted, the ports are not the mouse's: a program that probes for a
/// mouse must find none rather than a mouse that never moves.
#[test]
fn the_machine_only_has_a_mouse_when_one_is_fitted() {
    let mut spec = Spectrum::new();
    spec.bus.mouse.x = 0x5A;
    let before = spec.bus.io_read(0xFBDF);
    assert_ne!(
        before, 0x5A,
        "no mouse fitted, and X read back as {before:02X}"
    );

    spec.bus.hardware.fit(Peripheral::KempstonMouse, true);
    assert_eq!(spec.bus.io_read(0xFBDF), 0x5A, "fitted, X is the counter");
    spec.bus.mouse.set_buttons(true, false);
    assert_eq!(spec.bus.io_read(0xFADF), 0xFD, "and the buttons are there");
}

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
    app
}

/// The host's mouse over the picture is the Kempston mouse: moving it moves
/// the counters by the machine's pixels, not the host's, and its button is
/// the mouse's button rather than a click into the debugger.
#[test]
fn the_host_mouse_over_the_screen_moves_the_counters() {
    let mut app = test_app();
    app.spec.bus.hardware.fit(Peripheral::KempstonMouse, true);
    app.running = true;
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    let scale = h.state().scale;
    let centre = egui::pos2(750.0, 600.0);
    h.event(egui::Event::PointerMoved(centre));
    h.run_steps(2);
    let start = h.state().spec.bus.mouse;

    // Forty machine pixels right and twenty down, in host points.
    h.event(egui::Event::PointerMoved(
        centre + egui::vec2(40.0 * scale, 20.0 * scale),
    ));
    h.run_steps(2);
    let moved = h.state().spec.bus.mouse;
    assert_eq!(
        moved.x.wrapping_sub(start.x),
        40,
        "X by forty machine pixels at scale {scale}: {start:?} to {moved:?}"
    );
    assert_eq!(
        start.y.wrapping_sub(moved.y),
        20,
        "Y down by twenty, which counts it down: {start:?} to {moved:?}"
    );

    let at = centre + egui::vec2(40.0 * scale, 20.0 * scale);
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(h.state().spec.bus.mouse.buttons, 0xFD, "left held");
    h.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(h.state().spec.bus.mouse.buttons, 0xFF, "and let go");
}
