//! The tape monitor: what you hear from the recorder while a ZX81 loads.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use zx_spectrum_emulator::tape::{zx81_block, zx81_name, Tape};
use zx_spectrum_emulator::zx81::{Ram, Zx81};

fn machine_with_tape() -> (Zx81, Arc<Mutex<VecDeque<f32>>>) {
    let mut zx = Zx81::new(Ram::K16);
    let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    zx.bus.audio.attach(queue.clone(), 48_000.0);
    zx.bus.audio.enabled = true;
    zx.bus.audio.volume = 1.0;

    let block = zx81_block(&zx81_name("TEST"), &[0x5a; 200]);
    let mut tape = Tape::from_blocks("test".into(), vec![block]);
    tape.play(zx.bus.tstates);
    zx.bus.tape = Some(tape);
    (zx, queue)
}

fn samples(queue: &Arc<Mutex<VecDeque<f32>>>) -> Vec<f32> {
    let mut q = queue.lock().unwrap();
    q.drain(..).collect()
}

#[test]
fn a_playing_tape_makes_a_sound() {
    let (mut zx, queue) = machine_with_tape();
    for _ in 0..30 {
        zx.run(zx.frame_t());
        zx.bus.tape_tick();
    }
    let heard = samples(&queue);
    assert!(!heard.is_empty(), "no samples were produced at all");
    let loudest = heard.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(loudest > 0.01, "the tape is inaudible: peak {loudest}");
}

#[test]
fn a_stopped_tape_is_silent() {
    let (mut zx, queue) = machine_with_tape();
    zx.bus.tape.as_mut().unwrap().stop();
    for _ in 0..30 {
        zx.run(zx.frame_t());
        zx.bus.tape_tick();
    }
    let heard = samples(&queue);
    let loudest = heard.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(loudest < 0.01, "silence was expected, got peak {loudest}");
}

#[test]
fn the_sound_follows_the_pulses_rather_than_the_polling() {
    // Every edge is mixed in at the T-state it happened. Sampling the level
    // only when the CPU looks at the port would collapse a burst of pulses
    // into one transition, and a tape would sound like noise instead of a
    // tone. The ZX81's bits are bursts of four or nine pulses at about
    // 3.3 kHz, so the sound should swing back and forth many times over.
    let (mut zx, queue) = machine_with_tape();
    for _ in 0..10 {
        zx.run(zx.frame_t());
        zx.bus.tape_tick();
    }
    let heard = samples(&queue);
    let mut crossings = 0;
    for pair in heard.windows(2) {
        if (pair[0] > 0.0) != (pair[1] > 0.0) {
            crossings += 1;
        }
    }
    assert!(
        crossings > 50,
        "only {crossings} changes in {} samples: that is not a tape",
        heard.len()
    );
}

#[test]
fn the_monitor_is_quiet_when_no_tape_is_in_the_deck() {
    let mut zx = Zx81::new(Ram::K16);
    let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    zx.bus.audio.attach(queue.clone(), 48_000.0);
    zx.bus.audio.enabled = true;
    zx.bus.audio.volume = 1.0;
    for _ in 0..30 {
        zx.run(zx.frame_t());
        zx.bus.tape_tick();
    }
    let heard = samples(&queue);
    let loudest = heard.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(loudest < 0.01, "a machine with no tape hummed: {loudest}");
}
