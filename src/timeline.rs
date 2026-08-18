//! When each routine ran, against the frame it ran in.
//!
//! The thread says what was called and in what order, and the flame says what
//! it cost. Neither says *when*, and on this machine when is the whole
//! question: the ULA is drawing the picture while the program runs, so a
//! routine that writes to the display file above the beam is seen this frame
//! and one that writes below it is seen next frame. A turn of the loop laid
//! out against the frame's T-states says which is which.
//!
//! The observer records the T-state of every entry and exit. This pairs them
//! up into bars and gives each routine a lane to be drawn on.

use crate::observe::Step;

/// One stretch of one routine running.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub entry: u16,
    /// Which row it is drawn on: one per routine, in the order they first run.
    pub lane: usize,
    /// How deep in the call stack, so a caller and its callee are told apart
    /// where they overlap.
    pub depth: u8,
    /// When it started and stopped, in T-states since watching began.
    pub from: u64,
    pub to: u64,
}

impl Bar {
    pub fn length(&self) -> u64 {
        self.to.saturating_sub(self.from)
    }
}

/// A turn of the loop, laid out in time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timeline {
    /// The routines that ran, in the order they first ran: the lanes.
    pub lanes: Vec<u16>,
    pub bars: Vec<Bar>,
    pub from: u64,
    pub to: u64,
}

impl Timeline {
    pub fn span(&self) -> u64 {
        self.to.saturating_sub(self.from).max(1)
    }
}

/// Global time for a step, in T-states since watching started.
fn when(step: &Step, frame_t: u32) -> u64 {
    step.frame as u64 * frame_t as u64 + step.t as u64
}

/// Pair the entries and exits up into bars.
///
/// A routine still running when the turn ends gets a bar to the end of it:
/// that is what the picture should show — something that was still going —
/// rather than the bar being dropped for want of a matching exit.
pub fn lay_out(steps: &[Step], frame_t: u32) -> Timeline {
    let Some(first) = steps.first() else {
        return Timeline::default();
    };
    let from = when(first, frame_t);
    let to = steps.last().map(|s| when(s, frame_t)).unwrap_or(from);

    let mut lanes: Vec<u16> = Vec::new();
    let mut open: Vec<(u16, u8, u64)> = Vec::new();
    let mut bars: Vec<Bar> = Vec::new();
    let lane_of = |lanes: &mut Vec<u16>, entry: u16| -> usize {
        match lanes.iter().position(|at| *at == entry) {
            Some(i) => i,
            None => {
                lanes.push(entry);
                lanes.len() - 1
            }
        }
    };

    for step in steps {
        let at = when(step, frame_t);
        if step.enter {
            open.push((step.entry, step.depth, at));
            continue;
        }
        // The innermost open call of that routine, which is what this exit
        // belongs to: a routine can be inside itself.
        if let Some(i) = open.iter().rposition(|(entry, _, _)| *entry == step.entry) {
            let (entry, depth, started) = open.remove(i);
            let lane = lane_of(&mut lanes, entry);
            bars.push(Bar {
                entry,
                lane,
                depth,
                from: started,
                to: at.max(started),
            });
        }
    }
    // Whatever was still running when the turn ended.
    for (entry, depth, started) in open {
        let lane = lane_of(&mut lanes, entry);
        bars.push(Bar {
            entry,
            lane,
            depth,
            from: started,
            to: to.max(started),
        });
    }

    bars.sort_by_key(|bar| (bar.from, bar.depth, bar.entry));
    Timeline {
        lanes,
        bars,
        from,
        to,
    }
}

/// Where the display is being drawn within a frame, in T-states: the stretch
/// during which what a routine writes to the display file may already be too
/// late to be seen this time round.
pub fn display_window(first_pixel: u32, lines: u32, per_line: u32) -> (u32, u32) {
    (first_pixel, first_pixel + lines * per_line)
}
