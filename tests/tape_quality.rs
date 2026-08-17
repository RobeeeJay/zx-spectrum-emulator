//! A deck that is not quite right: a wavering motor and a head out of square.

use zx_rustrum::machine::CPU_HZ;
use zx_rustrum::tape::{Block, Quality, Tape};

/// A tape of one long tone, whose pulses are all meant to be the same length.
fn tone_tape(quality: Quality) -> Tape {
    let mut tape = Tape::from_blocks(
        "t".into(),
        vec![Block::PureTone {
            len: 2168,
            count: 4000,
        }],
    );
    tape.quality = quality;
    tape.play(0);
    tape
}

/// Every interval between edges over a stretch of the tone.
fn intervals(tape: &mut Tape, until: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut level = tape.level_at(0);
    let mut last = 0u64;
    for t in (0..until).step_by(4) {
        let now = tape.level_at(t);
        if now != level {
            if last > 0 {
                out.push(t - last);
            }
            last = t;
            level = now;
        }
    }
    out
}

/// With the deck behaving, a tone is a tone: every pulse the same length.
#[test]
fn a_steady_deck_puts_out_a_steady_tone() {
    let mut tape = tone_tape(Quality::default());
    let gaps = intervals(&mut tape, CPU_HZ as u64);
    assert!(gaps.len() > 100, "only {} pulses came out", gaps.len());
    let odd = gaps.iter().filter(|g| **g != 2168).count();
    assert_eq!(odd, 0, "every pulse should be 2168 T-states: {gaps:?}");
}

/// A wavering motor stretches and squeezes the tone, and does it slowly: the
/// pulse either side of one is nearly the same length, and the pulse a second
/// away is not.
#[test]
fn a_wavering_motor_stretches_the_tone() {
    let mut tape = tone_tape(Quality {
        speed: true,
        speed_wobble: 0.1,
        ..Quality::default()
    });
    let gaps = intervals(&mut tape, CPU_HZ as u64);

    let shortest = *gaps.iter().min().expect("some pulses");
    let longest = *gaps.iter().max().expect("some pulses");
    assert!(
        shortest < 2100 && longest > 2240,
        "ten per cent either way of 2168 should show: {shortest} to {longest}"
    );
    // Neighbouring pulses stay close: this is a motor, not noise.
    let jumps = gaps.windows(2).filter(|w| w[0].abs_diff(w[1]) > 60).count();
    assert!(
        jumps * 20 < gaps.len(),
        "the speed should wander rather than jump: {jumps} jumps in {}",
        gaps.len()
    );
}

/// A head out of square reads one edge early and the other late, so the mark
/// and the space stop being the same length — while the pair of them still
/// adds up to about what it should.
#[test]
fn a_head_out_of_square_skews_the_mark_and_the_space() {
    let mut tape = tone_tape(Quality {
        alignment: true,
        alignment_offset: 0.2,
        ..Quality::default()
    });
    let gaps = intervals(&mut tape, CPU_HZ as u64 / 4);

    let odd: Vec<u64> = gaps.iter().step_by(2).copied().collect();
    let even: Vec<u64> = gaps.iter().skip(1).step_by(2).copied().collect();
    let mean = |v: &[u64]| v.iter().sum::<u64>() as f64 / v.len() as f64;
    let (a, b) = (mean(&odd), mean(&even));
    assert!(
        (a - b).abs() > 600.0,
        "one should be a fifth long and the other a fifth short: {a:.0} and {b:.0}"
    );
    assert!(
        ((a + b) - 2.0 * 2168.0).abs() < 60.0,
        "and the pair should still add up to two pulses: {a:.0} + {b:.0}"
    );
}

/// The wobbles come from the clock rather than from a dice, so the same tape
/// played twice sounds the same both times.
#[test]
fn a_bad_deck_is_bad_in_the_same_way_every_time() {
    let quality = Quality {
        speed: true,
        speed_wobble: 0.08,
        alignment: true,
        alignment_offset: 0.1,
        alignment_wobble: 0.05,
    };
    let first = intervals(&mut tone_tape(quality), CPU_HZ as u64 / 8);
    let second = intervals(&mut tone_tape(quality), CPU_HZ as u64 / 8);
    assert_eq!(
        first, second,
        "the same tape should play the same way twice"
    );
}
