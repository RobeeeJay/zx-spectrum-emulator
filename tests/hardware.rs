//! What is plugged into the back of the machine.

use zx_rustrum::hardware::{Emulated, Hardware, Peripheral};
use zx_rustrum::machine::Spectrum;

/// Nothing is plugged in until it is plugged in.
#[test]
fn a_machine_starts_with_nothing_on_the_back() {
    let hardware = Hardware::default();
    for what in Peripheral::ALL {
        assert!(
            !hardware.fitted(what),
            "{} should not be fitted",
            what.name()
        );
    }
    assert_eq!(hardware.if1_drives, 1, "one microdrive, when there is one");
}

/// Fitting and unfitting, and the names they are saved under.
///
/// The key is what goes in the preferences file and the name is what goes on
/// the screen; they are separate so the label can be reworded without somebody
/// losing what they had plugged in.
#[test]
fn peripherals_are_fitted_by_name_and_saved_by_key() {
    let mut hardware = Hardware::default();
    hardware.fit(Peripheral::SpecDrum, true);
    hardware.fit(Peripheral::Fuller, true);
    assert!(hardware.fitted(Peripheral::SpecDrum));
    assert_eq!(hardware.all_fitted().len(), 2);

    hardware.fit(Peripheral::SpecDrum, false);
    assert!(!hardware.fitted(Peripheral::SpecDrum));
    assert_eq!(hardware.all_fitted(), &[Peripheral::Fuller]);

    // Fitting twice is fitting once: there is one socket.
    hardware.fit(Peripheral::Fuller, true);
    assert_eq!(hardware.all_fitted().len(), 1);

    for what in Peripheral::ALL {
        assert_eq!(
            Peripheral::from_key(what.key()),
            Some(what),
            "{} should come back from its key",
            what.name()
        );
    }
}

/// Each one says how far it is emulated, and a switch with nothing behind it
/// says what is missing.
///
/// A switch that turns on nothing is worse than no switch, because it looks
/// like the thing is working.
#[test]
fn every_peripheral_says_how_far_it_is_emulated() {
    for what in Peripheral::ALL {
        assert!(
            !what.what().is_empty(),
            "{} should say what it is",
            what.name()
        );
        match what.emulated() {
            Emulated::Yes => {}
            Emulated::NeedsRom(rom) => assert!(
                rom.contains(".rom"),
                "{} should name the ROM it wants: {rom}",
                what.name()
            ),
            Emulated::No(why) => assert!(
                why.len() > 30,
                "{} should say why it does nothing yet: {why}",
                what.name()
            ),
        }
    }
    assert_eq!(Peripheral::Fuller.emulated(), Emulated::Yes);
    assert_eq!(Peripheral::SpecDrum.emulated(), Emulated::Yes);
    assert!(matches!(
        Peripheral::Interface1.emulated(),
        Emulated::NeedsRom(_)
    ));
    assert!(matches!(Peripheral::Uspeech.emulated(), Emulated::No(_)));
}

/// The SpecDrum is an eight-bit converter on a port: a byte written to $DF is
/// a sample, and nothing happens at all when the box is not plugged in.
#[test]
fn the_specdrum_turns_bytes_written_to_it_into_sound() {
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    assert_eq!(spec.bus.audio.dac, 0.0);

    // Not fitted: the port belongs to nobody and nothing comes of it.
    spec.bus.io_write(0x00DF, 0xFF);
    assert_eq!(spec.bus.audio.dac, 0.0, "no box, no sound");

    spec.bus.hardware.fit(Peripheral::SpecDrum, true);
    spec.bus.io_write(0x00DF, 0xFF);
    assert!(spec.bus.audio.dac > 0.0, "a byte at the top of the range");
    spec.bus.io_write(0x00DF, 0x00);
    assert!(spec.bus.audio.dac < 0.0, "and one at the bottom");
    spec.bus.io_write(0x00DF, 0x80);
    assert!(
        spec.bus.audio.dac.abs() < 0.01,
        "the converter idles at half scale, so silence is silence: {}",
        spec.bus.audio.dac
    );
}

/// The Fuller Audio Box is a sound chip of its own: a 48K with one fitted has
/// one, and a 128K with one has two.
#[test]
fn the_fuller_box_gives_a_48k_a_sound_chip() {
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    spec.bus.audio.extra_ay = Some(Default::default());
    spec.bus.hardware.fit(Peripheral::Fuller, true);

    // Register select at $3F, data at $5F: channel A's period, then volume.
    spec.bus.io_write(0x003F, 0x00);
    spec.bus.io_write(0x005F, 0xFD);
    spec.bus.io_write(0x003F, 0x08);
    spec.bus.io_write(0x005F, 0x0F);

    let ay = spec.bus.audio.extra_ay.as_ref().expect("a sound chip");
    assert_eq!(ay.regs[0], 0xFD, "the period went where it was addressed");
    assert_eq!(ay.regs[8], 0x0F, "and so did the volume");

    // The machine's own chip is untouched: they are two chips, not one.
    assert_eq!(spec.bus.audio.ay.regs[0], 0);
}
