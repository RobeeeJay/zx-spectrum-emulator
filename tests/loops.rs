//! Finding a program's loops without assuming it has a frame.

use zx_rustrum::loops::{phases, turn};
use zx_rustrum::observe::Step;

const FRAME_T: u32 = 69888;

/// Build a stream of calls: `head` every `period` T-states, with `children`
/// underneath it each time.
fn looping(head: u16, children: &[u16], period: u64, times: usize, from: u64) -> Vec<Step> {
    let mut steps = Vec::new();
    for turn in 0..times {
        let at = from + turn as u64 * period;
        let step = |t: u64, entry: u16, depth: u8, enter: bool| Step {
            frame: (t / FRAME_T as u64) as u32,
            t: (t % FRAME_T as u64) as u32,
            entry,
            depth,
            enter,
        };
        steps.push(step(at, head, 1, true));
        for (n, child) in children.iter().enumerate() {
            let when = at + 100 + n as u64 * 50;
            steps.push(step(when, *child, 2, true));
            steps.push(step(when + 20, *child, 2, false));
        }
        steps.push(step(at + period - 10, head, 1, false));
    }
    steps
}

/// The loop is found from repetition alone, and its turn is not a frame: a
/// game may take several frames over one, and this one takes four.
#[test]
fn a_loop_is_found_without_counting_frames() {
    let steps = looping(0x8000, &[0x9000, 0x9100], FRAME_T as u64 * 4, 20, 0);
    let found = phases(&steps, FRAME_T, 8);

    assert_eq!(found.len(), 1, "one loop, not {}", found.len());
    let phase = &found[0];
    assert_eq!(phase.head, 0x8000, "the head of the loop");
    assert!(
        (phase.frames_per_turn(FRAME_T) - 4.0).abs() < 0.1,
        "a turn takes four frames, not {:.2}",
        phase.frames_per_turn(FRAME_T)
    );

    let turn = turn(&steps, phase, FRAME_T).expect("a turn of it");
    let called: Vec<u16> = turn
        .steps
        .iter()
        .filter(|s| s.enter)
        .map(|s| s.entry)
        .collect();
    assert!(
        called.contains(&0x9000) && called.contains(&0x9100),
        "a turn should carry what the loop does: {called:?}"
    );
}

/// A program does several different things over its life — a title screen, a
/// menu, the game — and they are told apart by the work changing, not by
/// anything counting frames.
#[test]
fn a_change_of_job_is_a_change_of_phase() {
    let mut steps = looping(0x8000, &[0x9000], FRAME_T as u64, 40, 0);
    let later = FRAME_T as u64 * 60;
    steps.extend(looping(
        0xA000,
        &[0xB000, 0xB100],
        FRAME_T as u64 * 2,
        40,
        later,
    ));

    let found = phases(&steps, FRAME_T, 8);
    assert!(found.len() >= 2, "only found {} phase(s)", found.len());
    let heads: Vec<u16> = found.iter().map(|p| p.head).collect();
    assert!(
        heads.contains(&0x8000) && heads.contains(&0xA000),
        "both loops should be found: {heads:04X?}"
    );
}

/// Something called in bursts is not a loop, however often it is called: what
/// makes a loop is coming back round at a steady interval.
#[test]
fn a_burst_of_calls_is_not_a_loop() {
    let mut steps = looping(0x8000, &[], FRAME_T as u64, 12, 0);
    // A routine called thirty times in quick succession, once.
    for n in 0..30u64 {
        let t = FRAME_T as u64 * 3 + n * 40;
        steps.push(Step {
            frame: (t / FRAME_T as u64) as u32,
            t: (t % FRAME_T as u64) as u32,
            entry: 0xC000,
            depth: 1,
            enter: true,
        });
    }
    steps.sort_by_key(|s| (s.frame, s.t));

    let found = phases(&steps, FRAME_T, 64);
    assert!(!found.is_empty());
    assert_eq!(
        found[0].head, 0x8000,
        "the steady one is the loop, not the one called in a burst"
    );
}
