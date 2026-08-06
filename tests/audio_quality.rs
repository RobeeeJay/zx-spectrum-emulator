//! Sound quality regressions: tape edges must reach the mixer at their own
//! T-states, the output must be DC-free, and gain changes must not click.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use zx_spectrum_emulator::audio::{Audio, SharedQueue};
use zx_spectrum_emulator::machine::Spectrum;
use zx_spectrum_emulator::tape::{Block, Tape};

fn queue() -> SharedQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

fn samples(q: &SharedQueue) -> Vec<f32> {
    q.lock().unwrap().iter().copied().collect()
}

/// Count how often the signal crosses zero, which for a square wave is twice
/// per cycle.
fn crossings(s: &[f32]) -> usize {
    s.windows(2)
        .filter(|w| (w[0] > 0.0) != (w[1] > 0.0))
        .count()
}

/// A machine playing a 1 kHz tone off tape, whose ROM never reads port $FE.
fn machine_with_tone_tape(rate: f64) -> (Spectrum, SharedQueue) {
    let mut spec = Spectrum::new();
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00); // NOPs: no I/O at all
    let q = queue();
    spec.bus.audio.attach(q.clone(), rate);
    spec.bus.audio.volume = 1.0;

    // 1750 T-states per half cycle is 1 kHz at 3.5 MHz.
    let tape = Tape::from_blocks(
        "tone".into(),
        vec![Block::PureTone {
            len: 1750,
            count: 20_000,
        }],
    );
    spec.bus.tape = Some(tape);
    let now = spec.bus.total_t();
    spec.bus.tape.as_mut().unwrap().play(now);
    (spec, q)
}

#[test]
fn tape_audio_keeps_its_shape_when_the_cpu_never_reads_the_port() {
    let rate = 48_000.0;
    let (mut spec, q) = machine_with_tone_tape(rate);

    // A sixth of a second of emulated time (short enough that the queue's
    // quarter-second cap does not drop anything, since nothing is draining
    // it here), with the tape ticked once per frame as the UI does. Nothing
    // reads port $FE, so the only way the tone can come out correctly is if
    // each tape edge is mixed at its own T-state.
    let frames = 8;
    for _ in 0..frames {
        spec.run(spec.bus.frame_t());
        spec.bus.tape_tick();
    }
    spec.bus.audio_sync();
    spec.bus.audio.flush();

    let s = samples(&q);
    let seconds = frames as f64 * spec.bus.frame_t() as f64 / 3_500_000.0;
    assert!(
        (s.len() as f64 - seconds * rate).abs() < rate * 0.05,
        "expected about {:.0} samples, got {}",
        seconds * rate,
        s.len()
    );

    // A 1 kHz square wave crosses zero 2000 times a second.
    let expected = 2.0 * 1000.0 * seconds;
    let got = crossings(&s) as f64;
    assert!(
        (got / expected - 1.0).abs() < 0.1,
        "tape tone came out at {:.0} crossings, expected about {expected:.0} \
         (a collapsed tape would give roughly one edge per frame)",
        got
    );
}

#[test]
fn the_output_has_no_dc_offset() {
    let q = queue();
    let mut audio = Audio::new(3_500_000.0);
    audio.attach(q.clone(), 48_000.0);
    audio.volume = 1.0;

    // Hold the beeper high: a real machine's AC coupling means a constant
    // level is inaudible, so the output must settle back to zero.
    audio.beeper = 0.55;
    audio.advance_to(700_000); // 0.2 s, several DC-blocker time constants
    audio.flush();

    let s = samples(&q);
    assert!(s.len() > 9_000, "got {} samples", s.len());
    let tail = &s[s.len() - 1000..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(
        mean.abs() < 0.01,
        "a constant level should decay to silence, mean is {mean}"
    );
    assert!(
        s[..100].iter().any(|v| v.abs() > 0.05),
        "the initial step should still be audible"
    );
}

#[test]
fn muting_ramps_instead_of_clicking() {
    let q = queue();
    let mut audio = Audio::new(3_500_000.0);
    audio.attach(q.clone(), 48_000.0);
    audio.volume = 1.0;

    // A square wave, so there is something to cut off.
    let mut t = 0u64;
    let mut level = false;
    for i in 0..400 {
        audio.advance_to(t);
        level = !level;
        audio.beeper = if level { 0.55 } else { 0.0 };
        if i == 200 {
            audio.enabled = false;
        }
        t += 1750;
    }
    audio.flush();

    let s = samples(&q);
    let biggest_step = s
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    // The waveform's own edges are ~0.55 tall; a mute click would be a jump
    // on top of that, so anything at or below one edge means no click.
    assert!(
        biggest_step <= 0.6,
        "largest sample-to-sample jump was {biggest_step}, which is a click"
    );
    let tail = &s[s.len() - 200..];
    assert!(
        tail.iter().all(|v| v.abs() < 0.02),
        "should be silent after muting"
    );
}

#[test]
fn pacing_speeds_up_when_starved_and_slows_when_backed_up() {
    let q = queue();
    let mut audio = Audio::new(3_500_000.0);
    audio.attach(q.clone(), 48_000.0);

    // Empty queue: run a little faster to catch up.
    assert!(audio.pace(0.06) > 1.0);

    // Overfull queue: ease off.
    {
        let mut g = q.lock().unwrap();
        for _ in 0..10_000 {
            g.push_back(0.0);
        }
    }
    assert!(audio.pace(0.06) < 1.0);

    // And never wildly either way.
    assert!((0.9..=1.1).contains(&audio.pace(0.06)));
}

#[test]
fn the_mixer_clock_never_runs_backwards() {
    let q = queue();
    let mut audio = Audio::new(3_500_000.0);
    audio.attach(q.clone(), 48_000.0);
    audio.beeper = 0.5;

    audio.advance_to(100_000);
    let after_first = audio.produced;
    // An out-of-order timestamp must not replay that stretch of time.
    audio.advance_to(50_000);
    audio.advance_to(100_000);
    assert_eq!(
        audio.produced, after_first,
        "time was replayed after an out-of-order update"
    );
}

#[test]
fn a_steady_ay_tone_comes_out_at_the_right_pitch_and_without_jumps() {
    use zx_spectrum_emulator::machine::Model;

    let rate = 48_000.0;
    let q = queue();
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.bus.audio.attach(q.clone(), rate);
    spec.bus.audio.volume = 1.0;

    // Channel A, tone only, full volume, period 111: about 998 Hz.
    for (reg, value) in [(7u8, 0b0011_1110u8), (0, 111), (1, 0), (8, 15)] {
        spec.bus.audio.ay.selected = reg;
        spec.bus.audio.ay.write(value);
    }

    // An eighth of a second, driven a frame at a time like the real loop.
    let frames = 6;
    for _ in 0..frames {
        spec.run(spec.bus.frame_t());
    }
    spec.bus.audio_sync();
    spec.bus.audio.flush();

    let s = samples(&q);
    let seconds = frames as f64 * spec.bus.frame_t() as f64 / Model::Spectrum128.cpu_hz();
    let expected_hz = Model::Spectrum128.cpu_hz() / 2.0 / (16.0 * 111.0);
    let expected = 2.0 * expected_hz * seconds;
    let got = crossings(&s) as f64;
    assert!(
        (got / expected - 1.0).abs() < 0.1,
        "AY tone came out at {got:.0} crossings, expected about {expected:.0}"
    );

    // No sample-to-sample jump bigger than the waveform's own amplitude:
    // anything larger is a discontinuity, which is what crackle sounds like.
    let amplitude = s.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
    let biggest_step = s
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(amplitude > 0.05, "the tone is inaudible ({amplitude})");
    assert!(
        biggest_step <= amplitude * 2.1,
        "largest jump {biggest_step} against amplitude {amplitude}"
    );
}
