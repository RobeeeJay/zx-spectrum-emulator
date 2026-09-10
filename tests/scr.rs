//! `.scr`: the screen on its own.

use zx_rustrum::machine::{Model, Spectrum};

/// A screen written out and read back is the same screen, and it is the one
/// the ULA is showing rather than a fixed bank: a 128K can put its screen in
/// bank 7.
#[test]
fn a_screen_written_out_comes_back_the_same() {
    let mut spec = Spectrum::new();
    // Something with structure in it, so a wrong order shows up rather than
    // hiding in a field of the same byte.
    for i in 0..zx_rustrum::scr::LEN {
        spec.bus.ram[5 * 0x4000 + i] = (i % 251) as u8;
    }
    let out = zx_rustrum::scr::save(&spec);
    assert_eq!(out.len(), 6912, "6,144 of pixels and 768 of attributes");

    let mut other = Spectrum::new();
    zx_rustrum::scr::load(&mut other, &out).expect("a screen");
    assert_eq!(
        &other.bus.ram[5 * 0x4000..5 * 0x4000 + zx_rustrum::scr::LEN],
        &out[..],
        "byte for byte"
    );
}

/// The screen goes where the machine is showing from, not always to bank 5.
#[test]
fn the_screen_follows_the_bank_the_ula_is_showing() {
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::with_model(Model::Spectrum128);
    // $7FFD bit 3 puts the screen in bank 7.
    spec.bus.io_write(0x7FFD, 0x08);
    assert_eq!(spec.bus.screen_bank(), 7);

    let screen: Vec<u8> = (0..zx_rustrum::scr::LEN).map(|i| (i % 97) as u8).collect();
    zx_rustrum::scr::load(&mut spec, &screen).expect("a screen");
    assert_eq!(
        &spec.bus.ram[7 * 0x4000..7 * 0x4000 + 16],
        &screen[..16],
        "into bank 7, which is the one being shown"
    );
    assert_eq!(spec.bus.ram[5 * 0x4000], 0, "and bank 5 is left alone");
    assert_eq!(
        zx_rustrum::scr::save(&spec),
        screen,
        "and read back from it"
    );
}

/// A file that is not 6,912 bytes is not a screen, and the error says what one
/// is: there is no header to check, so the size is the whole of the test.
#[test]
fn something_that_is_not_a_screen_says_what_one_is() {
    let mut spec = Spectrum::new();
    let err = zx_rustrum::scr::load(&mut spec, &[0; 100]).expect_err("not a screen");
    assert!(err.contains("6,144"), "{err}");
    assert!(!zx_rustrum::scr::is_scr(&[0; 100]));
    assert!(zx_rustrum::scr::is_scr(&[0; 6912]));
}

/// Loading one puts it on the screen at once, without the machine running.
///
/// The picture is what the ULA painted rather than what the display file
/// holds, so a screen poked into a stopped machine would not be seen at all —
/// and a stopped machine is where somebody looking at a .scr usually is.
#[test]
fn a_loaded_screen_is_on_the_picture_before_the_machine_runs() {
    let mut spec = Spectrum::new();
    let screen: Vec<u8> = (0..zx_rustrum::scr::LEN).map(|i| (i % 89) as u8).collect();
    zx_rustrum::scr::load(&mut spec, &screen).expect("a screen");
    assert_eq!(
        &spec.bus.painted[..32],
        &screen[..32],
        "the painted frame has it"
    );
    assert_eq!(
        &spec.bus.screen_prev[..32],
        &screen[..32],
        "and so has the one the window draws"
    );
}
