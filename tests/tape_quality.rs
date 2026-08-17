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

/// A tape of one tone at `len` T-states a pulse, and how many edges come out
/// of a stretch of it.
fn edges_at(len: u16, quality: Quality, until: u64) -> usize {
    let mut tape = Tape::from_blocks("t".into(), vec![Block::PureTone { len, count: 20_000 }]);
    tape.quality = quality;
    tape.play(0);
    intervals(&mut tape, until).len()
}

/// A head out of square is a low-pass filter, and what it takes first is the
/// quick loaders: at a corner between the two, a tone at ROM speed comes
/// through and one at turbo speed does not.
#[test]
fn a_head_out_of_square_swallows_the_quick_pulses_first() {
    // Far enough out that the corner sits near 2 kHz. A pilot pulse of 2168
    // T-states is half a cycle of 807 Hz and comes through; a turbo bit of
    // 400 is half a cycle of 4,375 Hz and does not.
    let out = Quality {
        alignment: true,
        alignment_offset: 0.7,
        ..Quality::default()
    };
    let corner = out.cutoff(0);
    assert!(
        (1500.0..2500.0).contains(&corner),
        "the corner should sit between the two speeds, not {corner:.0} Hz"
    );

    let tenth = zx_rustrum::machine::CPU_HZ as u64 / 10;
    let rom_speed = edges_at(2168, out, tenth);
    let turbo = edges_at(400, out, tenth);
    let turbo_square = edges_at(400, Quality::default(), tenth);

    assert!(
        rom_speed > 130,
        "a ROM-speed tone should still come through: {rom_speed} edges"
    );
    assert!(
        turbo * 4 < turbo_square,
        "and a turbo one should not: {turbo} edges against {turbo_square} with \
         the head square"
    );
}

/// The corner comes down as the head goes further out, and wanders when it is
/// asked to.
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
    let over_a_second: Vec<f32> = (0..20)
        .map(|i| wandering.cutoff(i * zx_rustrum::machine::CPU_HZ as u64 / 20))
        .collect();
    let low = over_a_second.iter().cloned().fold(f32::MAX, f32::min);
    let high = over_a_second.iter().cloned().fold(0.0, f32::max);
    assert!(
        high > low * 1.4,
        "the corner should wander up and down: {low:.0} to {high:.0} Hz"
    );
    assert_eq!(
        bit_out.cutoff(0),
        bit_out.cutoff(12345),
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
