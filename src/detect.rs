//! Looking for particular things in a program, one heuristic at a time.
//!
//! Each detector answers one question — where is the main game loop, where is
//! the sprite plotter, where is the score kept — and says how sure it is and
//! why. They are kept apart from the general analysis in [`crate::autodoc`]
//! because they are answering a named question rather than describing whatever
//! is in front of them, and because a reader wants to know which question was
//! asked before weighing the answer.
//!
//! A detector that cannot find its thing says so. Returning the least bad
//! candidate with a low number attached is worse than returning nothing: it
//! puts an address in front of somebody who will go and look at it.

use crate::loops;
use crate::observe::Step;

/// How sure a detector is, in the words the rest of the emulator uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sure {
    Possible,
    Likely,
    Certain,
}

impl Sure {
    pub fn label(&self) -> &'static str {
        match self {
            Sure::Certain => "certain",
            Sure::Likely => "likely",
            Sure::Possible => "possible",
        }
    }

    /// From a score between 0 and 1.
    fn from_score(score: f64) -> Sure {
        if score >= 0.8 {
            Sure::Certain
        } else if score >= 0.5 {
            Sure::Likely
        } else {
            Sure::Possible
        }
    }
}

/// One thing a detector found.
#[derive(Clone, Debug)]
pub struct Finding {
    /// What was being looked for, for the reader.
    pub what: &'static str,
    pub address: u16,
    pub sure: Sure,
    /// The score behind the word, 0 to 1.
    pub score: f64,
    /// What the answer rests on, in one line. A finding without this is an
    /// assertion, and the point of the exercise is not to make those.
    pub because: String,
}

/// Where the main game loop is.
///
/// The main loop is the routine the program keeps coming back to at the top of
/// the call tree, going round at a steady interval, calling several different
/// things each time. Those three properties are what is scored: nothing here
/// assumes a frame, since a game may take several over one turn.
pub fn main_game_loop(steps: &[Step], frame_t: u32) -> Option<Finding> {
    let phases = loops::phases(steps, frame_t, 64);
    let phase = phases.iter().max_by_key(|phase| phase.iterations)?;
    if phase.iterations < 3 {
        return None;
    }

    // How steady the turns are: the interval between one and the next should
    // be much the same every time.
    let times: Vec<u64> = steps
        .iter()
        .filter(|step| step.enter && step.entry == phase.head)
        .map(|step| step.frame as u64 * frame_t as u64 + step.t as u64)
        .collect();
    let steadiness = steadiness_of(&times);

    // A main loop does several different things each turn. One that calls
    // nothing, or the same thing over and over, is an inner loop.
    let variety = (phase.routines.len().min(8) as f64) / 8.0;

    // And it should have gone round enough times to be a habit rather than a
    // coincidence.
    let repetition = ((phase.iterations as f64) / 50.0).min(1.0);

    let score = 0.5 * steadiness + 0.3 * variety + 0.2 * repetition;
    let turns = phase.frames_per_turn(frame_t);
    Some(Finding {
        what: "Main game loop",
        address: phase.head,
        sure: Sure::from_score(score),
        score,
        because: format!(
            "came back {} times, {:.2} frames apart, calling {} different routines",
            phase.iterations,
            turns,
            phase.routines.len()
        ),
    })
}

/// How regular a series of times is, as a number between 0 and 1.
fn steadiness_of(times: &[u64]) -> f64 {
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
    1.0 / (1.0 + variance.sqrt() / mean)
}

/// Every detector there is, run over what has been watched.
///
/// One so far. The shape is here for the others: each answers its own question
/// and says how sure it is, and the window lists whatever they find.
pub fn everything(steps: &[Step], frame_t: u32) -> Vec<Finding> {
    [main_game_loop(steps, frame_t)]
        .into_iter()
        .flatten()
        .collect()
}
