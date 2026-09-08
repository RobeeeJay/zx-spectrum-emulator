//! The three sound switches: the beeper, the sound chips and the add-ons.
//!
//! One switch for the lot was the wrong shape for this machine. A 128K game
//! plays its music on the AY and clicks the beeper for its sound effects; a
//! SpecDrum is a third thing again. These are what says each switch silences
//! its own and leaves the others alone.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use zx_rustrum::audio::{Audio, SharedQueue};

fn queue() -> SharedQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

/// A mixer with something on every input: the beeper up, a note on each of the
/// two sound chips, and the converter at the top of its range.
fn mixer(rate: f64) -> (Audio, SharedQueue) {
    let mut audio = Audio::new(3_500_000.0);
    let q = queue();
    audio.attach(q.clone(), rate);
    audio.volume = 1.0;
    audio.ay_present = true;
    audio.extra_ay = Some(Default::default());
    (audio, q)
}

/// How loud a burst was: the distance between its highest and lowest sample
/// over the end of it.
///
/// Measuring the peak from the start of a burst measures the DC blocker
/// unwinding from the burst before — its pole is at 0.9995, so a level that
/// has just been cut takes a couple of thousand samples to fall away, and a
/// mixer that had gone properly silent read as a quarter of full scale.
fn spread(q: &SharedQueue) -> f32 {
    let samples: Vec<f32> = q.lock().unwrap().iter().copied().collect();
    q.lock().unwrap().clear();
    let tail = &samples[samples.len().saturating_sub(2000)..];
    let high = tail.iter().fold(f32::MIN, |a, s| a.max(*s));
    let low = tail.iter().fold(f32::MAX, |a, s| a.min(*s));
    high - low
}

/// Run the mixer for a stretch with the beeper clicking, and say how loud it
/// was. A square wave rather than a held level, because the DC blocker eats a
/// straight line whatever the switches say.
fn loudness(audio: &mut Audio, q: &SharedQueue, from: u64) -> f32 {
    let mut t = from;
    let mut level = false;
    for _ in 0..400 {
        audio.advance_to(t);
        level = !level;
        audio.beeper = if level { 0.5 } else { 0.0 };
        t += 1750;
    }
    audio.flush();
    spread(q)
}

/// Each switch silences its own part and leaves the others where they were.
#[test]
fn each_switch_silences_its_own_part_of_the_sound() {
    let (mut audio, q) = mixer(48_000.0);

    // A tone on the machine's AY: channel A's period, then its volume.
    audio.ay.selected = 0;
    audio.ay.write(0xFD);
    audio.ay.selected = 8;
    audio.ay.write(0x0F);

    let everything = loudness(&mut audio, &q, 0);
    assert!(
        everything > 0.05,
        "something should be audible: {everything}"
    );

    // The beeper alone, with the chips and the boxes off.
    audio.ay_on = false;
    audio.hardware_on = false;
    let beeper_only = loudness(&mut audio, &q, 10_000_000);
    assert!(beeper_only > 0.05, "the beeper is still on: {beeper_only}");

    // And with the beeper off as well there is nothing left.
    audio.beeper_on = false;
    let nothing = loudness(&mut audio, &q, 20_000_000);
    assert!(
        nothing < 0.01,
        "every switch off is silence: {nothing}, where the beeper alone was \
         {beeper_only}"
    );

    // The AY on its own, with the beeper still muted, is heard again.
    audio.ay_on = true;
    let ay_only = loudness(&mut audio, &q, 30_000_000);
    assert!(
        ay_only > 0.01,
        "the sound chip is not muted by the beeper's switch: {ay_only}"
    );
}

/// The SpecDrum goes with the add-ons, not with the beeper or the chips.
///
/// It is the one people want on its own — a drum machine over a game's own
/// music — so it matters which switch it answers to.
#[test]
fn the_specdrums_converter_answers_to_the_hardware_switch() {
    let (mut audio, q) = mixer(48_000.0);
    audio.beeper_on = false;
    audio.ay_on = false;

    // A converter held at the top of its range is a step, and the DC blocker
    // eats a held level, so alternate it: that is what a drum sample is.
    let hear = |audio: &mut Audio, from: u64| {
        let mut t = from;
        for i in 0..400 {
            audio.advance_to(t);
            audio.dac = if i % 2 == 0 { 0.5 } else { -0.5 };
            t += 1750;
        }
        audio.flush();
        spread(&q)
    };

    let on = hear(&mut audio, 0);
    assert!(on > 0.05, "the converter should be heard: {on}");

    audio.hardware_on = false;
    let off = hear(&mut audio, 10_000_000);
    assert!(
        off < 0.01,
        "and silenced by its own switch: {off}, where it had been {on}"
    );
}

/// The tape's hiss comes out of the machine's own speaker, so it goes with the
/// beeper — and it is drawn whether or not it is wanted, so muting does not
/// change the noise that comes back.
#[test]
fn the_tapes_hiss_follows_the_beeper() {
    let (mut audio, q) = mixer(48_000.0);
    audio.ay_on = false;
    audio.hardware_on = false;
    audio.tape_hiss = 0.3;

    let hiss = |audio: &mut Audio, from: u64| {
        let mut t = from;
        for _ in 0..400 {
            audio.advance_to(t);
            t += 1750;
        }
        audio.flush();
        spread(&q)
    };

    let on = hiss(&mut audio, 0);
    assert!(on > 0.01, "a tape hisses: {on}");

    audio.beeper_on = false;
    let off = hiss(&mut audio, 10_000_000);
    assert!(off < 0.01, "and the beeper's switch silences it: {off}");
}
