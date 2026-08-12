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

use std::collections::BTreeMap;

use crate::loops::{self, Phase};
use crate::observe::{Observer, Site, Step};

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
    /// What it would be called if it were written down. The best loop is the
    /// main one; the others are named after where they are, because calling a
    /// second candidate the main loop would be saying something the evidence
    /// does not.
    pub label: String,
    pub address: u16,
    pub sure: Sure,
    /// The score behind the word, 0 to 1.
    pub score: f64,
    /// Where the label belongs, when the finding itself is in the middle of a
    /// routine. An IN instruction is worth a comment where it is; a name
    /// belongs at the entry point, since that is what a label is for.
    pub entry: Option<u16>,
    /// What the answer rests on, in one line. A finding without this is an
    /// assertion, and the point of the exercise is not to make those.
    pub because: String,
}

/// Every loop the program was found going round, best candidate first.
///
/// A loop is a routine the program keeps coming back to at the top of the call
/// tree, going round at a steady interval, calling several different things
/// each time. Those three properties are what is scored: nothing here assumes
/// a frame, since a game may take several over one turn.
///
/// A program has more than one — a title screen, a menu, the game itself, and
/// often an inner loop that is doing most of the work — so they are all
/// returned, with what each rests on, and it is left to the reader to say which
/// is which. Deciding on their behalf means being wrong silently.
pub fn main_game_loops(steps: &[Step], frame_t: u32) -> Vec<Finding> {
    let mut best: BTreeMap<u16, Finding> = BTreeMap::new();
    for phase in loops::phases(steps, frame_t, 64) {
        // Every routine the phase kept coming back to, not only the one it
        // came back to most regularly. A program runs several loops over its
        // life and often more than one at a time — a title screen waiting for
        // a key while it animates, a game loop with a slower one around it —
        // and the reader is the one who should say which is which.
        for finding in in_phase(steps, &phase, frame_t) {
            // The same routine can head a loop in two phases: a game that goes
            // back to its title screen and round again. One line for it,
            // showing the stretch that makes the better case.
            let keep = best
                .get(&finding.address)
                .is_none_or(|already| finding.score > already.score);
            if keep {
                best.insert(finding.address, finding);
            }
        }
    }

    let mut found: Vec<Finding> = best.into_values().collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(MOST);
    for (rank, finding) in found.iter_mut().enumerate() {
        if rank > 0 {
            finding.what = "Loop";
            finding.label = format!("loop_{:04X}", finding.address);
        }
    }
    found
}

/// The best candidate, or nothing.
pub fn main_game_loop(steps: &[Step], frame_t: u32) -> Option<Finding> {
    main_game_loops(steps, frame_t).into_iter().next()
}

/// How many loops are worth listing. Past a handful the list stops being
/// something a reader looks down and starts being a log.
const MOST: usize = 8;

/// A candidate has to look enough like a loop to be worth a line. Below this
/// it is a routine that happened to be called a few times.
const WORTH_SAYING: f64 = 0.35;

/// Every loop in one phase, scored.
fn in_phase(steps: &[Step], phase: &Phase, frame_t: u32) -> Vec<Finding> {
    let when = |step: &Step| step.frame as u64 * frame_t as u64 + step.t as u64;
    let within: Vec<(usize, &Step)> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| when(step) >= phase.from && when(step) <= phase.to)
        .collect();

    // The top of the program as it actually ran, which need not be depth one.
    let Some(top) = within
        .iter()
        .filter(|(_, step)| step.enter)
        .map(|(_, step)| step.depth)
        .min()
    else {
        return Vec::new();
    };

    let mut entries: BTreeMap<u16, Vec<(usize, u64)>> = BTreeMap::new();
    for (index, step) in &within {
        if step.enter && step.depth == top {
            entries
                .entry(step.entry)
                .or_default()
                .push((*index, when(step)));
        }
    }

    let span = phase.to.saturating_sub(phase.from).max(1) as f64;
    let mut found = Vec::new();
    for (entry, seen) in entries {
        if seen.len() < 3 {
            continue;
        }
        let times: Vec<u64> = seen.iter().map(|(_, at)| *at).collect();

        // How steady the turns are: the interval between one and the next
        // should be much the same every time.
        let steadiness = steadiness_of(&times);

        // Weighed against how much of the phase it was going round for. Thirty
        // calls in a burst are perfectly steady and are not a loop.
        let coverage = ((times[times.len() - 1] - times[0]) as f64 / span).min(1.0);

        // A loop of any interest does several different things each turn. One
        // that calls nothing is a routine being called repeatedly.
        let variety = (calls_in_a_turn(steps, &seen).min(8) as f64) / 8.0;

        // And it should have gone round enough times to be a habit rather than
        // a coincidence.
        let repetition = ((times.len() as f64) / 50.0).min(1.0);

        let score = 0.5 * steadiness * coverage + 0.3 * variety + 0.2 * repetition;
        if score < WORTH_SAYING {
            continue;
        }
        let mut gaps: Vec<u64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
        gaps.sort_unstable();
        let period = gaps[gaps.len() / 2] as f64 / frame_t as f64;
        found.push(Finding {
            what: "Main game loop",
            label: "main_game_loop".to_string(),
            address: entry,
            entry: Some(entry),
            sure: Sure::from_score(score),
            score,
            because: format!(
                "came back {} times, {:.2} frames apart, calling {} different routines",
                times.len(),
                period,
                calls_in_a_turn(steps, &seen)
            ),
        });
    }
    found
}

/// How many different routines one turn of this loop calls.
///
/// The second turn rather than the first: the first time round usually does
/// setting up that the rest do not. Looking at one turn rather than all of
/// them keeps this to a few hundred steps however long the program has been
/// watched.
fn calls_in_a_turn(steps: &[Step], seen: &[(usize, u64)]) -> usize {
    let Some(window) = seen.windows(2).nth(1) else {
        return 0;
    };
    let (from, to) = (window[0].0, window[1].0);
    steps[from + 1..to]
        .iter()
        .filter(|step| step.enter)
        .map(|step| step.entry)
        .collect::<std::collections::BTreeSet<u16>>()
        .len()
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

/// The keyboard's eight half-rows, as they appear in the top half of the port
/// address. Reading `$FEFE` is the caps-shift to V row; a routine that reads
/// several of these is walking the keyboard.
const HALF_ROWS: [u8; 8] = [0xFE, 0xFD, 0xFB, 0xF7, 0xEF, 0xDF, 0xBF, 0x7F];

/// The two half-rows a Sinclair-style joystick sits on: keys 1-5 and 6-0. An
/// Interface II joystick *is* those keys, so reading them is the same
/// instruction either way and only the reader knows which was meant.
const SINCLAIR_ROWS: [u8; 2] = [0xF7, 0xEF];

/// Which instructions read the keyboard.
///
/// The ULA puts the keyboard on port $FE, with the row to read in the top half
/// of the address, so a keyboard scan shows up as reads of `$xxFE` with
/// several different `xx`. That same port carries the EAR bit, which a tape
/// loader polls thousands of times a call — so how many rows an instruction
/// reads, and how hard it hammers the port, are what tell them apart.
///
/// One line per IN instruction, not per routine. A routine can read the
/// keyboard from three places, and answering with the routine says less than
/// stopping on the port would have told you anyway.
pub fn keyboard_input(observer: &Observer) -> Vec<Finding> {
    let mut found = Vec::new();
    for site in observer.port_sites.values() {
        if site.reads == 0 {
            continue;
        }
        let rows: Vec<u8> = site
            .ports
            .iter()
            .filter(|port| port.to_le_bytes()[0] == 0xFE)
            .map(|port| port.to_le_bytes()[1])
            .filter(|high| HALF_ROWS.contains(high) || *high == 0x00)
            .collect();
        if rows.is_empty() {
            continue;
        }

        // Walking several rows is a keyboard scan; one row is an instruction
        // waiting on one key, which is still the keyboard.
        let breadth = (rows.len().min(8) as f64) / 8.0;
        let per_frame = site.reads as f64 / site.frames.max(1) as f64;

        // A tape loader reads one row thousands of times a frame for the EAR
        // bit. Only counted against an instruction that reads one row or two:
        // anything walking three or more of them is scanning the keyboard
        // however often it does so, and penalising that hid the routine a game
        // sits in while it waits for a key.
        let polling = if rows.len() <= 2 {
            (per_frame / 512.0).min(1.0)
        } else {
            0.0
        };
        let score = (0.45 + 0.5 * breadth - 0.6 * polling).clamp(0.0, 1.0);
        if score < WORTH_SAYING {
            continue;
        }
        found.push(Finding {
            what: "Keyboard input",
            label: "read_keyboard".to_string(),
            address: site.at,
            entry: site.routine,
            sure: Sure::from_score(score),
            score,
            because: format!(
                "reads port $FE on {} half-row{}, {:.0} times a frame{}",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" },
                per_frame,
                in_routine(site),
            ),
        });
    }
    rank(found, "read_keyboard")
}

/// Which instructions read a joystick.
///
/// A Kempston has a port of its own and says so plainly; a Fuller likewise. A
/// Sinclair or an Interface II is wired to the keyboard, so the most that can
/// be said of an instruction reading those two half-rows is that it might be a
/// joystick being read — and it is said in those words rather than dressed up.
pub fn joystick_input(observer: &Observer) -> Vec<Finding> {
    let mut found = Vec::new();
    for site in observer.port_sites.values() {
        if site.reads == 0 {
            continue;
        }
        let Some((score, kind)) = joystick_port(site) else {
            continue;
        };
        let per_frame = site.reads as f64 / site.frames.max(1) as f64;
        found.push(Finding {
            what: "Joystick input",
            label: "read_joystick".to_string(),
            address: site.at,
            entry: site.routine,
            sure: Sure::from_score(score),
            score,
            because: format!("{kind}, {:.0} times a frame{}", per_frame, in_routine(site)),
        });
    }
    rank(found, "read_joystick")
}

/// Which routine an instruction is in, when anything was seen to call one.
fn in_routine(site: &Site) -> String {
    match site.routine {
        Some(entry) => format!(", in the routine at ${entry:04X}"),
        None => String::new(),
    }
}

/// What an instruction's reads say about which joystick it is reading, if any.
fn joystick_port(site: &Site) -> Option<(f64, String)> {
    let low: Vec<u8> = site
        .ports
        .iter()
        .map(|port| port.to_le_bytes()[0])
        .collect();
    // A Kempston is read with `IN A,($1F)`, which puts A in the top half of
    // the address, so only the bottom half can be relied on.
    if low.contains(&0x1F) {
        return Some((0.9, "reads the Kempston port $1F".to_string()));
    }
    if low.contains(&0x7F) && !low.contains(&0xFE) {
        return Some((0.7, "reads the Fuller port $7F".to_string()));
    }

    // Wired to the keyboard: the two half-rows an Interface II sits on, and
    // nothing else. An instruction that reads those two and the other six is
    // scanning the keyboard, not reading a joystick.
    let rows: Vec<u8> = site
        .ports
        .iter()
        .filter(|port| port.to_le_bytes()[0] == 0xFE)
        .map(|port| port.to_le_bytes()[1])
        .collect();
    if !rows.is_empty() && rows.iter().all(|row| SINCLAIR_ROWS.contains(row)) {
        return Some((
            0.45,
            format!(
                "reads the {} half-row{} a Sinclair or Interface II joystick sits on, \
                 which are also the number keys",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" }
            ),
        ));
    }
    None
}

/// Best first, and only the best one gets the plain name: a second candidate
/// called `read_keyboard` would be saying the two are the same routine.
fn rank(mut found: Vec<Finding>, name: &str) -> Vec<Finding> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(MOST);
    for (rank, finding) in found.iter_mut().enumerate() {
        if rank > 0 {
            finding.label = format!("{name}_{:04X}", finding.address);
        }
    }
    found
}

/// What is being looked for. A detector answers one question, and the reader
/// wants to know which was asked before weighing the answer, so the question
/// is chosen rather than everything being run at once and the results piled
/// together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Question {
    #[default]
    MainGameLoop,
    Keyboard,
    Joystick,
}

impl Question {
    /// What the button says, and what the window says it is looking for.
    pub fn label(&self) -> &'static str {
        match self {
            Question::MainGameLoop => "Main game loop",
            Question::Keyboard => "Keyboard input",
            Question::Joystick => "Joystick input",
        }
    }

    pub fn all() -> [Question; 3] {
        [
            Question::MainGameLoop,
            Question::Keyboard,
            Question::Joystick,
        ]
    }
}

/// Ask one question of what has been watched.
pub fn ask(question: Question, steps: &[Step], observer: &Observer, frame_t: u32) -> Vec<Finding> {
    match question {
        Question::MainGameLoop => main_game_loops(steps, frame_t),
        Question::Keyboard => keyboard_input(observer),
        Question::Joystick => joystick_input(observer),
    }
}
