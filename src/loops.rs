//! Finding a program's loops by watching it, without assuming it has any.
//!
//! A game does not necessarily do its work once a frame. It may take three
//! frames over a turn, or draw into a back buffer and show it when it is
//! ready, or not be tied to the frame at all. So nothing here counts frames:
//! the only evidence used is the order routines were entered in, and the only
//! thing looked for is repetition.
//!
//! A loop is a routine that keeps being entered at the shallowest depth the
//! program is using, with the same sort of work between one entry and the
//! next. A program has several over its life — a title screen, a menu, the
//! game itself — and they are found by noticing that the set of routines being
//! run has changed.

use std::collections::{BTreeMap, BTreeSet};

use crate::observe::Step;

/// A stretch of time in which the program was doing one kind of thing.
#[derive(Clone, Debug)]
pub struct Phase {
    /// When it started and ended, in the T-states the observer counted.
    pub from: u64,
    pub to: u64,
    /// The routine that keeps coming round: the head of the loop.
    pub head: u16,
    /// How many times it came round.
    pub iterations: usize,
    /// How long an iteration took, in T-states: the middle value, which says
    /// more than the average about a loop that occasionally waits.
    pub period: u64,
    /// Everything seen in this phase, commonest first.
    pub routines: Vec<u16>,
}

impl Phase {
    /// How long a turn of this loop takes, as a fraction of a frame. A game
    /// that takes three frames over a turn is a fact about the game, not
    /// something to be assumed either way.
    pub fn frames_per_turn(&self, frame_t: u32) -> f64 {
        self.period as f64 / frame_t as f64
    }
}

/// One turn of a loop: everything the program did between one entry of the
/// head and the next.
#[derive(Clone, Debug)]
pub struct Turn {
    pub head: u16,
    pub from: u64,
    pub to: u64,
    pub steps: Vec<Step>,
}

/// Global time for a step, in T-states since watching started.
fn when(step: &Step, frame_t: u32) -> u64 {
    step.frame as u64 * frame_t as u64 + step.t as u64
}

/// Break the recording into phases, and find the loop in each.
///
/// `window` is how many top-level entries to look at when deciding whether the
/// program has moved on to doing something else.
pub fn phases(steps: &[Step], frame_t: u32, window: usize) -> Vec<Phase> {
    let top = shallowest(steps);
    let heads: Vec<&Step> = steps
        .iter()
        .filter(|step| step.enter && step.depth == top)
        .collect();
    if heads.len() < 4 {
        return Vec::new();
    }

    // Where the program changed what it was doing: the set of routines being
    // entered stops overlapping the set from a moment ago.
    let mut boundaries = vec![0usize];
    let mut previous: BTreeSet<u16> = BTreeSet::new();
    for start in (0..heads.len()).step_by(window.max(1)) {
        let end = (start + window).min(heads.len());
        let current: BTreeSet<u16> = heads[start..end].iter().map(|s| s.entry).collect();
        if !previous.is_empty() {
            let shared = current.intersection(&previous).count();
            let both = current.union(&previous).count();
            // Less than a third in common is a different job, not a quiet
            // patch of the same one.
            if both > 0 && shared * 3 < both {
                boundaries.push(start);
            }
        }
        previous = current;
    }
    boundaries.push(heads.len());
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|pair| describe(&heads[pair[0]..pair[1]], frame_t))
        .collect()
}

/// The shallowest depth anything was entered at: the top of the program as it
/// actually ran, which need not be depth one.
fn shallowest(steps: &[Step]) -> u8 {
    steps
        .iter()
        .filter(|step| step.enter)
        .map(|step| step.depth)
        .min()
        .unwrap_or(1)
}

/// What one stretch of top-level entries amounts to.
fn describe(heads: &[&Step], frame_t: u32) -> Option<Phase> {
    if heads.len() < 3 {
        return None;
    }
    let mut counts: BTreeMap<u16, usize> = BTreeMap::new();
    for step in heads {
        *counts.entry(step.entry).or_insert(0) += 1;
    }

    // The head of the loop is the routine that comes round most regularly, not
    // simply the most often: a routine called five times in a row and then not
    // again is not what the program keeps returning to.
    let head = counts
        .iter()
        .filter(|(_, count)| **count >= 3)
        .max_by(|a, b| {
            regularity(heads, *a.0, frame_t).total_cmp(&regularity(heads, *b.0, frame_t))
        })
        .map(|(entry, _)| *entry)?;

    let times: Vec<u64> = heads
        .iter()
        .filter(|step| step.entry == head)
        .map(|step| when(step, frame_t))
        .collect();
    let mut gaps: Vec<u64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    gaps.sort_unstable();
    let period = gaps.get(gaps.len() / 2).copied().unwrap_or(0);

    let mut routines: Vec<(u16, usize)> = counts.into_iter().collect();
    routines.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    Some(Phase {
        from: when(heads[0], frame_t),
        to: when(heads[heads.len() - 1], frame_t),
        head,
        iterations: times.len(),
        period,
        routines: routines.into_iter().map(|(entry, _)| entry).collect(),
    })
}

/// How much a routine looks like the head of a loop: more is better.
///
/// Two things make one. It comes round at about the same interval every time,
/// and it keeps doing so for as long as the phase lasts. Steadiness alone is
/// not enough — a routine called thirty times in a burst, forty T-states
/// apart, is perfectly steady and is not a loop — so it is weighed against how
/// much of the phase its calls actually span.
fn regularity(heads: &[&Step], entry: u16, frame_t: u32) -> f64 {
    let times: Vec<u64> = heads
        .iter()
        .filter(|step| step.entry == entry)
        .map(|step| when(step, frame_t))
        .collect();
    if times.len() < 3 {
        return 0.0;
    }
    let gaps: Vec<f64> = times
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64)
        .collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = gaps.iter().map(|gap| (gap - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
    let steadiness = 1.0 / (1.0 + variance.sqrt() / mean);

    // How much of the phase it was going round for.
    let (first, last) = (
        when(heads[0], frame_t),
        when(heads[heads.len() - 1], frame_t),
    );
    let phase_span = last.saturating_sub(first).max(1) as f64;
    let own_span = (times[times.len() - 1] - times[0]) as f64;
    let coverage = (own_span / phase_span).min(1.0);

    steadiness * coverage
}

/// One turn of a phase's loop: everything between one entry of the head and
/// the next, at every depth.
pub fn turn(steps: &[Step], phase: &Phase, frame_t: u32) -> Option<Turn> {
    let mut entries = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            step.enter
                && step.entry == phase.head
                && when(step, frame_t) >= phase.from
                && when(step, frame_t) <= phase.to
        })
        .map(|(index, _)| index);

    // The second turn rather than the first: the first time round a loop
    // usually does setting up that the rest do not.
    entries.next();
    let start = entries.next()?;
    let end = entries.next()?;

    Some(Turn {
        head: phase.head,
        from: when(&steps[start], frame_t),
        to: when(&steps[end], frame_t),
        steps: steps[start..end].to_vec(),
    })
}
