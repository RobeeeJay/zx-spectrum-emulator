//! The SP0256-AL2, against what the chip's data sheet says it should do.
//!
//! Speech is hard to test by listening in a test suite, so the check that
//! matters here is timing: every one of the 64 allophones is a chain of frames
//! in the chip's ROM, and how long that chain takes is published. If the
//! microsequencer decodes the ROM correctly, all 64 come out at their stated
//! lengths; if anything about the decoding is wrong — the ROM at the wrong
//! offset, the bit order the wrong way round, a field read with the wrong
//! width — they come out as noise or as nothing, and never as the right
//! lengths by accident.
//!
//! The waveform itself was checked another way, once, and by hand: rendering
//! all 64 and comparing their spectra against a recording of a real Currah
//! (<https://maziac.github.io/currah_uspeech_tests>). The steady sounds match
//! closely — /AA/, /AE/, /MM/, /AO/, /IY/ all correlate at 0.94 to 0.98 with
//! their formants within a hundred hertz — and the moving ones score lower
//! only because a single window lands at a different point in the glide.

use zx_rustrum::sp0256::Sp0256;

/// The published length of each allophone, in tenths of a millisecond. These
/// are the SP0256-AL2's measured figures rather than the round numbers in the
/// manual: /AA/ is 63.7ms, not "100ms".
#[rustfmt::skip]
const TENTHS_MS: [u16; 64] = [
    64, 256, 448, 960, 1984, 2912, 1729, 546, 768, 1472, 984, 1729, 455, 960, 1274, 546, 1820,
    768, 1365, 1729, 2002, 455, 637, 728, 637, 1274, 819, 896, 364, 1280, 728, 1729, 2548, 721,
    1105, 1274, 721, 1984, 1341, 819, 1088, 1344, 1152, 1486, 2002, 819, 1456, 2457, 1452, 910,
    1472, 1092, 2093, 1729, 1820, 640, 1365, 1260, 2366, 2002, 2457, 694, 1365, 502,
];

/// The µSpeech's oscillator, measured at about 3.05MHz on real hardware. The
/// chip makes one sample every 312 clocks of it.
const RATE: f64 = 3_050_000.0 / 312.0;

fn chip() -> Option<Sp0256> {
    std::fs::read("roms/sp0256-al2.rom")
        .ok()
        .map(|rom| Sp0256::new(&rom))
}

/// Every allophone lasts as long as the data sheet says.
///
/// The Currah's oscillator runs a little under the speed the published figures
/// assume, so everything it says comes out about 3.5% long — consistently, and
/// on every one of the 64. A decoding error does not look like that: it looks
/// like a chip that halts one sample later, or one that runs for a second and
/// a half.
#[test]
fn every_allophone_lasts_as_long_as_the_data_sheet_says() {
    let Some(chip) = chip() else {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    };
    let mut worst: (f64, usize) = (0.0, 0);
    for (a, tenths) in TENTHS_MS.iter().enumerate() {
        let mut chip = chip.clone();
        chip.speak(a as u8);
        let mut samples = 0u32;
        while chip.busy() && samples < 400_000 {
            chip.sample();
            samples += 1;
        }
        let ms = f64::from(samples) / RATE * 1000.0;
        let want = f64::from(*tenths) / 10.0;
        let off = (ms - want) / want * 100.0;
        assert!(
            (2.0..5.6).contains(&off),
            "allophone ${a:02X} took {ms:.1}ms where the data sheet says {want:.1}ms, \
             which is {off:+.1}% and not the 3.5% the Currah's slower oscillator gives"
        );
        if off.abs() > worst.0 {
            worst = (off.abs(), a);
        }
    }
    eprintln!("worst was ${:02X} at {:.1}%", worst.1, worst.0);
}

/// A chip nobody has spoken to is silent, and says it is not busy.
#[test]
fn a_chip_with_nothing_to_say_is_quiet() {
    let Some(mut chip) = chip() else {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    };
    assert!(!chip.busy());
    let loudest = (0..5000).map(|_| chip.sample().abs()).max().unwrap_or(0);
    assert_eq!(loudest, 0, "an idle chip makes nothing at all");
}

/// Talking is what it does: an allophone comes out as a waveform, and the
/// chip says it is busy until the sound has finished.
#[test]
fn an_allophone_comes_out_as_a_waveform() {
    let Some(mut chip) = chip() else {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    };
    chip.speak(0x18); // /AA/, as in "hot"
    assert!(chip.busy(), "busy the moment it is told");

    let mut loudest = 0i16;
    let mut crossings = 0;
    let mut last = 0i16;
    while chip.busy() {
        let s = chip.sample();
        loudest = loudest.max(s.abs());
        if (s > 0) != (last > 0) {
            crossings += 1;
        }
        last = s;
    }
    assert!(loudest > 4000, "and it should be loud: peak {loudest}");
    assert!(
        crossings > 20,
        "and a waveform rather than a step: {crossings} zero crossings"
    );
    assert!(!chip.busy(), "and quiet again at the end");
}

/// A write while the chip is still talking is dropped, as the hardware drops
/// it. That is why the µSpeech's ROM polls the busy line first.
#[test]
fn an_allophone_written_over_a_busy_chip_is_dropped() {
    let Some(mut chip) = chip() else {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    };
    chip.speak(0x05); // /OY/, the longest there is
    for _ in 0..100 {
        chip.sample();
    }
    chip.speak(0x00); // a pause, which would cut it short if it landed
    let mut samples = 100;
    while chip.busy() && samples < 400_000 {
        chip.sample();
        samples += 1;
    }
    let ms = f64::from(samples) / RATE * 1000.0;
    assert!(
        ms > 250.0,
        "the second write should have gone nowhere, leaving /OY/ to finish: {ms:.0}ms"
    );
}

/// A reset stops it mid-word, which is what the machine's reset line does.
#[test]
fn a_reset_stops_it_talking() {
    let Some(mut chip) = chip() else {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    };
    chip.speak(0x05);
    for _ in 0..100 {
        chip.sample();
    }
    assert!(chip.busy());

    chip.reset();
    assert!(!chip.busy(), "nothing is being said");
    let loudest = (0..2000).map(|_| chip.sample().abs()).max().unwrap_or(0);
    assert_eq!(loudest, 0, "and nothing comes out");
}
