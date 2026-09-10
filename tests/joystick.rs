//! The joystick interfaces: four different answers to the same five switches.
//!
//! The masks and the key mappings are checked against Fuse's `joystick.c`,
//! which is what every other emulator agrees with. Nothing about a joystick
//! can be measured off a ROM — no ROM reads one — so a reference is the best
//! there is, and the numbers are written out here so a change to them is
//! deliberate.

use zx_rustrum::joystick::{Joystick, Kind, Way};
use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::z80::Bus;

/// Kempston is a port with a bit per direction, set while the stick is over.
#[test]
fn kempston_sets_a_bit_for_each_direction() {
    let mut stick = Joystick::new();
    stick.kind = Kind::Kempston;
    assert_eq!(stick.io_read(0x001F), Some(0x00), "nothing over: no bits");

    for (way, bit) in [
        (Way::Right, 0x01),
        (Way::Left, 0x02),
        (Way::Down, 0x04),
        (Way::Up, 0x08),
        (Way::Fire, 0x10),
    ] {
        stick.release();
        stick.set(way, true);
        assert_eq!(
            stick.io_read(0x001F),
            Some(bit),
            "{} should be bit ${bit:02X}",
            way.name()
        );
    }

    // Two at once, which is a diagonal.
    stick.release();
    stick.set(Way::Up, true);
    stick.set(Way::Right, true);
    assert_eq!(stick.io_read(0x001F), Some(0x09));

    // And it is deaf to everything else.
    assert_eq!(stick.io_read(0x00FE), None, "not the keyboard's port");
    assert_eq!(stick.io_read(0x7FFD), None, "nor the paging one");
}

/// The Fuller's port is the same idea the other way up: idle is $FF.
#[test]
fn the_fuller_reads_the_other_way_up() {
    let mut stick = Joystick::new();
    stick.kind = Kind::Fuller;
    assert_eq!(stick.io_read(0x007F), Some(0xFF), "idle is all ones");

    stick.set(Way::Up, true);
    assert_eq!(stick.io_read(0x007F), Some(0xFE), "up clears bit 0");
    stick.release();
    stick.set(Way::Fire, true);
    assert_eq!(stick.io_read(0x007F), Some(0x7F), "and fire clears bit 7");
    assert_eq!(stick.io_read(0x001F), None, "Kempston's port is not its");
}

/// Sinclair and Cursor are wired to keys, so they pull the matrix down and a
/// game reading the keyboard cannot tell.
#[test]
fn the_keyboard_interfaces_pull_the_keys_they_are_wired_to() {
    // Sinclair 1 is 6 7 8 9 0, which is the half-row at $EFFE.
    let mut stick = Joystick::new();
    stick.kind = Kind::Sinclair1;
    stick.set(Way::Left, true); // key 6, row 4 bit 4
    assert_eq!(stick.matrix()[4], 0xEF);
    assert_eq!(stick.io_read(0x001F), None, "it answers no port at all");

    // Sinclair 2 is 1 2 3 4 5, the other half-row.
    let mut stick = Joystick::new();
    stick.kind = Kind::Sinclair2;
    stick.set(Way::Fire, true); // key 5, row 3 bit 4
    assert_eq!(stick.matrix()[3], 0xEF);

    // Cursor's fire is 0 and its left is 5, which is why it fights the ROM's
    // own cursor keys.
    let mut stick = Joystick::new();
    stick.kind = Kind::Cursor;
    stick.set(Way::Left, true);
    assert_eq!(stick.matrix()[3], 0xEF, "5");
    stick.set(Way::Fire, true);
    assert_eq!(stick.matrix()[4], 0xFE, "0");
}

/// Through the machine: a Kempston read reaches the port, and a Sinclair one
/// reaches the keyboard.
#[test]
fn the_machine_reads_the_stick_the_way_a_game_would() {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.joystick.kind = Kind::Kempston;
    spec.bus.joystick.set(Way::Fire, true);
    assert_eq!(spec.bus.io_read(0x001F), 0x10, "IN 31 is the stick");

    // The same machine with a Sinclair interface answers on the keyboard
    // instead, and its port read goes back to the floating bus.
    spec.bus.joystick = Joystick::new();
    spec.bus.joystick.kind = Kind::Sinclair1;
    spec.bus.joystick.set(Way::Fire, true); // key 0
    let keys = spec.bus.io_read(0xEFFE);
    assert_eq!(keys & 0x01, 0, "key 0 is down at $EFFE: ${keys:02X}");
    assert_eq!(
        spec.bus.io_read(0xF7FE) & 0x1F,
        0x1F,
        "and the other half-rows are not"
    );
}

/// A direction held over a reset would be held for ever.
#[test]
fn a_reset_lets_go_of_the_stick() {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.joystick.kind = Kind::Kempston;
    spec.bus.joystick.set(Way::Up, true);
    assert!(spec.bus.joystick.anything_down());

    spec.reset();
    assert!(!spec.bus.joystick.anything_down(), "let go");
    assert_eq!(
        spec.bus.joystick.kind,
        Kind::Kempston,
        "but the interface is still plugged in"
    );
}

/// The interface is remembered by a name that does not change when its label
/// does, the way the peripherals are.
#[test]
fn every_interface_has_a_name_and_a_key() {
    for kind in Kind::ALL {
        assert!(!kind.name().is_empty());
        assert_eq!(Kind::from_key(kind.key()), Some(kind));
        assert!(
            kind.how().len() > 20,
            "{} should say how a program reads it",
            kind.name()
        );
    }
    assert_eq!(Kind::from_key("kempston"), Some(Kind::Kempston));
    assert_eq!(Kind::from_key("nonesuch"), None);
}
