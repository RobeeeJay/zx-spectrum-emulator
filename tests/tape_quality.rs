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

/// The intervals between edges over a window of the tape.
fn intervals_from(tape: &mut Tape, from: u64, to: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut level = tape.level_at(0);
    let mut last = 0u64;
    let mut t = 0u64;
    while t < to {
        let now = tape.level_at(t);
        if now != level {
            if last > 0 && t >= from {
                out.push(t - last);
            }
            last = t;
            level = now;
        }
        t += 4;
    }
    out
}

/// The motor wanders, and does it slowly: a tape wavers over seconds, not
/// milliseconds. Pulses a moment apart are much the same length; pulses
/// seconds apart are not.
#[test]
fn a_wavering_motor_wanders_slowly() {
    let wobbly = Quality {
        speed: true,
        speed_wobble: 0.1,
        ..Quality::default()
    };
    let second = CPU_HZ as u64;

    let mean_at = |at: u64| -> f64 {
        let mut tape = tone_tape(wobbly);
        let gaps = intervals_from(&mut tape, at, at + second / 20);
        gaps.iter().sum::<u64>() as f64 / gaps.len().max(1) as f64
    };
    let means: Vec<f64> = (0..7).map(|s| mean_at(s * second)).collect();
    let low = means.iter().cloned().fold(f64::MAX, f64::min);
    let high = means.iter().cloned().fold(0.0, f64::max);
    assert!(
        high - low > 100.0,
        "the motor should wander over seconds: {means:?}"
    );

    let mut tape = tone_tape(wobbly);
    let gaps = intervals_from(&mut tape, 0, second / 20);
    let jumpy = gaps.windows(2).filter(|w| w[0].abs_diff(w[1]) > 8).count();
    assert!(
        jumpy * 20 < gaps.len(),
        "and waver rather than shake: {jumpy} jumps in {} pulses",
        gaps.len()
    );
}

/// A head out of square rolls the signal off rather than cutting it. The edges
/// creep first — short pulses squeezed, long ones stretched, none lost — and
/// only when the swing stops reaching the reader's threshold do they start
/// going missing.
#[test]
fn a_head_out_of_square_rolls_off_rather_than_cutting() {
    let mixed = |quality: Quality| -> Vec<u64> {
        let mut tape = Tape::from_blocks(
            "t".into(),
            vec![Block::PureData {
                data: vec![0b1010_1010; 400],
                zero: 400,
                one: 800,
                used_bits: 8,
                pause_ms: 0,
            }],
        );
        tape.quality = quality;
        tape.play(0);
        intervals(&mut tape, CPU_HZ as u64 / 4)
    };
    let mean_of = |gaps: &[u64], short: bool| -> f64 {
        let picked: Vec<u64> = gaps
            .iter()
            .copied()
            .filter(|g| (*g < 600) == short)
            .collect();
        picked.iter().sum::<u64>() as f64 / picked.len().max(1) as f64
    };

    let square = mixed(Quality::default());
    assert_eq!(
        mean_of(&square, true),
        400.0,
        "a square head changes nothing"
    );
    assert_eq!(mean_of(&square, false), 800.0);

    let mut squeezed = Vec::new();
    for offset in [0.25f32, 0.35, 0.45] {
        let gaps = mixed(Quality {
            alignment: true,
            alignment_offset: offset,
            ..Quality::default()
        });
        let short_count = |g: &[u64]| g.iter().filter(|g| **g < 600).count();
        assert_eq!(
            short_count(&gaps),
            short_count(&square),
            "no quick pulse should be lost yet at this corner"
        );
        let (short, long) = (mean_of(&gaps, true), mean_of(&gaps, false));
        assert!(
            short < 400.0 && long > 800.0,
            "short pulses should be squeezed and long ones stretched: \
             {short:.0} and {long:.0}"
        );
        squeezed.push(short);
    }
    assert!(
        squeezed[0] > squeezed[2],
        "and further out should squeeze them further: {squeezed:?}"
    );

    let gone = mixed(Quality {
        alignment: true,
        alignment_offset: 0.6,
        ..Quality::default()
    });
    assert!(
        gone.len() * 3 < square.len() * 2,
        "by this far out the quick pulses should be going missing: {} edges \
         against {}",
        gone.len(),
        square.len()
    );
}

/// The corner comes down as the head goes further out, and wanders — slowly,
/// as a head creeps rather than shakes.
#[test]
fn the_corner_comes_down_and_wanders() {
    let square = Quality {
        alignment: true,
        ..Quality::default()
    };
    let bit_out = Quality {
        alignment_offset: 0.3,
        ..square
    };
    let far_out = Quality {
        alignment_offset: 0.9,
        ..square
    };
    assert!(
        square.cutoff(0) > bit_out.cutoff(0) && bit_out.cutoff(0) > far_out.cutoff(0),
        "further out should mean a lower corner: {:.0}, {:.0}, {:.0} Hz",
        square.cutoff(0),
        bit_out.cutoff(0),
        far_out.cutoff(0)
    );

    let wandering = Quality {
        alignment_wobble: 0.3,
        ..bit_out
    };
    // Over half a minute, which is the sort of time it takes.
    let corners: Vec<f32> = (0..30)
        .map(|i| wandering.cutoff(i * CPU_HZ as u64))
        .collect();
    let low = corners.iter().cloned().fold(f32::MAX, f32::min);
    let high = corners.iter().cloned().fold(0.0, f32::max);
    assert!(
        high > low * 1.4,
        "the corner should wander up and down: {low:.0} to {high:.0} Hz"
    );
    assert_eq!(
        bit_out.cutoff(0),
        bit_out.cutoff(123_456),
        "and stand still when it is not asked to wander"
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

/// The scope is shown the signal, not the reader's idea of it.
///
/// With the head square the trace is a square wave — corners and nothing in
/// between. With it out of square the trace is the charging curve, which is
/// the whole point of being able to see it: the squares round off, the swing
/// gets smaller as the corner comes down, and when the swing stops reaching
/// the reader's threshold that is visible too.
#[test]
fn the_scope_is_given_the_shape_of_the_signal() {
    let block = || Block::PureData {
        data: vec![0b1100_1100; 40],
        zero: 400,
        one: 800,
        used_bits: 8,
        pause_ms: 0,
    };

    let mut square = Tape::from_blocks("t".into(), vec![block()]);
    square.play(0);
    square.level_at(60_000);
    assert!(
        square.trace.iter().all(|(_, y)| y.abs() > 0.99),
        "a square head puts out squares: {:?}",
        square.trace.iter().take(8).collect::<Vec<_>>()
    );

    let mut rolled_off = Tape::from_blocks("t".into(), vec![block()]);
    rolled_off.quality = Quality {
        alignment: true,
        alignment_offset: 0.3,
        ..Quality::default()
    };
    rolled_off.play(0);
    rolled_off.level_at(60_000);
    let middling = rolled_off
        .trace
        .iter()
        .filter(|(_, y)| y.abs() < 0.9)
        .count();
    assert!(
        middling * 3 > rolled_off.trace.len(),
        "a rolled-off signal spends its time between the rails, not on them: \
         {middling} of {} samples",
        rolled_off.trace.len()
    );
    assert!(
        rolled_off.trace.len() > square.trace.len() * 2,
        "and is drawn with enough points to show the curve: {} against {}",
        rolled_off.trace.len(),
        square.trace.len()
    );
}
