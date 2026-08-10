//! Call profiler: attributes emulated time to the functions the CPU is in.
//!
//! Calls and returns are spotted from what the CPU did rather than by decoding
//! opcodes: a call is any instruction that leaves SP two lower with the address
//! of the following instruction on top of the stack, which catches `CALL`,
//! `RST` and interrupt acceptance alike. A return is any instruction that
//! leaves SP two higher having jumped to the word it took off the stack.
//!
//! From that, each function gets:
//! * **inclusive** time — everything between entry and return, callees included
//! * **self** time — inclusive minus the time spent inside its callees

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    SelfTime,
    Inclusive,
}

impl Metric {
    pub fn label(&self) -> &'static str {
        match self {
            Metric::SelfTime => "self time",
            Metric::Inclusive => "inclusive",
        }
    }
}

/// What one function accumulated during a run.
#[derive(Clone, Copy, Debug, Default)]
pub struct FuncStats {
    pub entry: u16,
    /// T-states in this function, excluding its callees.
    pub self_t: u64,
    /// T-states between entry and return, callees included.
    pub incl_t: u64,
    pub calls: u64,
    /// Deepest nesting seen, which spots recursion.
    pub max_depth: u32,
}

impl FuncStats {
    pub fn time(&self, metric: Metric) -> u64 {
        match metric {
            Metric::SelfTime => self.self_t,
            Metric::Inclusive => self.incl_t,
        }
    }
}

/// One profiling run.
#[derive(Clone)]
pub struct ProfileRun {
    pub started_at: SystemTime,
    /// Local date and time the run started, ready to display.
    pub started_label: String,
    /// Wall-clock length, once the run has been stopped.
    pub wall: Option<Duration>,
    /// Emulated T-states covered by the run.
    pub emulated_t: u64,
    /// Instructions executed during the run.
    pub instructions: u64,
    /// T-states that were not inside any tracked call.
    pub outside_t: u64,
    /// Calls that were still on the stack when the run was stopped.
    pub unfinished: usize,
    pub funcs: HashMap<u16, FuncStats>,
    pub cpu_hz: f64,
}

impl ProfileRun {
    /// Functions ordered by the chosen metric, most time first.
    pub fn ranked(&self, metric: Metric) -> Vec<FuncStats> {
        let mut v: Vec<FuncStats> = self.funcs.values().copied().collect();
        v.sort_by(|a, b| {
            b.time(metric)
                .cmp(&a.time(metric))
                .then(a.entry.cmp(&b.entry))
        });
        v
    }

    /// Total of the chosen metric across every function, for percentages.
    pub fn total(&self, metric: Metric) -> u64 {
        match metric {
            // Self time plus the time outside any call is the whole run.
            Metric::SelfTime => self.funcs.values().map(|f| f.self_t).sum::<u64>() + self.outside_t,
            Metric::Inclusive => self.emulated_t,
        }
    }

    pub fn seconds(&self, t: u64) -> f64 {
        t as f64 / self.cpu_hz
    }
}

/// A call that has been entered but not yet returned from.
#[derive(Clone, Copy, Debug)]
struct Frame {
    entry: u16,
    /// SP just after the return address was pushed, used to resynchronise if
    /// the program plays games with the stack.
    sp: u16,
    start_t: u64,
    /// Time already attributed to callees of this frame.
    child_t: u64,
}

pub struct Profiler {
    pub running: bool,
    pub runs: Vec<ProfileRun>,
    pub selected: Option<usize>,
    pub metric: Metric,
    /// Give up tracking deeper than this, so a runaway stack cannot grow
    /// memory without bound.
    pub max_depth: usize,

    stack: Vec<Frame>,
    start_t: u64,
    start_instant: Option<Instant>,
    last_t: u64,
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler {
    pub fn new() -> Self {
        Profiler {
            running: false,
            runs: Vec::new(),
            selected: None,
            metric: Metric::SelfTime,
            max_depth: 256,
            stack: Vec::new(),
            start_t: 0,
            start_instant: None,
            last_t: 0,
        }
    }

    pub fn current(&self) -> Option<&ProfileRun> {
        if self.running {
            self.runs.last()
        } else {
            None
        }
    }

    /// Begin a run at emulated time `now`.
    pub fn start(&mut self, now: u64, cpu_hz: f64) {
        if self.running {
            return;
        }
        let started_at = SystemTime::now();
        self.runs.push(ProfileRun {
            started_at,
            started_label: format_local(started_at),
            wall: None,
            emulated_t: 0,
            instructions: 0,
            outside_t: 0,
            unfinished: 0,
            funcs: HashMap::new(),
            cpu_hz,
        });
        self.selected = Some(self.runs.len() - 1);
        self.stack.clear();
        self.start_t = now;
        self.last_t = now;
        self.start_instant = Some(Instant::now());
        self.running = true;
    }

    /// End the run, attributing the time of anything still on the stack.
    pub fn stop(&mut self, now: u64) {
        if !self.running {
            return;
        }
        let wall = self.start_instant.take().map(|i| i.elapsed());
        let unfinished = self.stack.len();
        // Unwind: a function that has not returned still spent the time.
        while let Some(frame) = self.stack.pop() {
            self.close_frame(frame, now);
        }
        if let Some(run) = self.runs.last_mut() {
            run.wall = wall;
            run.emulated_t = now.saturating_sub(self.start_t);
            run.unfinished = unfinished;
        }
        self.running = false;
    }

    pub fn clear(&mut self) {
        self.runs.clear();
        self.selected = None;
        self.stack.clear();
        self.running = false;
    }

    /// Depth of the call stack right now, for the UI.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Entry point of the function the CPU is currently in, if any.
    pub fn innermost(&self) -> Option<u16> {
        self.stack.last().map(|f| f.entry)
    }

    fn stats(&mut self, entry: u16) -> &mut FuncStats {
        let run = self.runs.last_mut().expect("a run is in progress");
        run.funcs.entry(entry).or_insert(FuncStats {
            entry,
            ..Default::default()
        })
    }

    /// Record a finished frame and give its time back to its caller.
    fn close_frame(&mut self, frame: Frame, end_t: u64) {
        let incl = end_t.saturating_sub(frame.start_t);
        let self_t = incl.saturating_sub(frame.child_t);
        let depth = self.stack.len() as u32 + 1;
        {
            let s = self.stats(frame.entry);
            s.incl_t += incl;
            s.self_t += self_t;
            s.max_depth = s.max_depth.max(depth);
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.child_t += incl;
        }
    }

    /// Record entry to a function directly, used for interrupts: the CPU
    /// pushes a return address and jumps without executing an instruction, so
    /// there is nothing for [`Self::on_instruction`] to look at.
    pub fn on_call(&mut self, entry: u16, sp_after: u16, now: u64) {
        if !self.running || self.stack.len() >= self.max_depth {
            return;
        }
        self.stack.push(Frame {
            entry,
            sp: sp_after,
            start_t: now,
            child_t: 0,
        });
        self.stats(entry).calls += 1;
    }

    /// Offer one executed instruction to the profiler.
    ///
    /// `peek` reads memory without side effects, used to see what was pushed.
    // The arguments are the CPU state either side of one instruction; bundling
    // them into a struct would only move the same fields somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub fn on_instruction(
        &mut self,
        pc_before: u16,
        sp_before: u16,
        pc_after: u16,
        sp_after: u16,
        now: u64,
        elapsed: u64,
        peek: impl Fn(u16) -> u16,
    ) {
        if !self.running {
            return;
        }
        if let Some(run) = self.runs.last_mut() {
            run.instructions += 1;
            // Kept up to date while recording, so the window can show live
            // percentages rather than waiting for the run to be stopped.
            run.emulated_t = now.saturating_sub(self.start_t);
        }
        if self.stack.is_empty() {
            if let Some(run) = self.runs.last_mut() {
                run.outside_t += elapsed;
            }
        }
        self.last_t = now;

        // Calls and returns are spotted from what the CPU did; the observer
        // works the same way, so the two share one classifier.
        match crate::flow::classify(pc_before, sp_before, pc_after, sp_after, peek) {
            crate::flow::Flow::Call { entry, sp } => {
                if self.stack.len() < self.max_depth {
                    self.stack.push(Frame {
                        entry,
                        sp,
                        start_t: now,
                        child_t: 0,
                    });
                    self.stats(entry).calls += 1;
                }
            }
            crate::flow::Flow::Return { sp_before, .. } => {
                // Unwind any frames the program abandoned (a routine that
                // dropped its own return address, say) so the stack stays in
                // step with the machine's.
                while let Some(&frame) = self.stack.last() {
                    if frame.sp > sp_before {
                        // Returning past a frame we are not tracking; leave
                        // the outer frames alone.
                        break;
                    }
                    self.stack.pop();
                    self.close_frame(frame, now);
                    if frame.sp == sp_before {
                        break;
                    }
                }
            }
            crate::flow::Flow::Straight => {}
        }
    }
}

/// Local date and time as `YYYY-MM-DD HH:MM:SS`.
fn format_local(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format a duration of emulated or wall-clock time compactly.
pub fn format_duration(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{seconds:.2} s")
    } else {
        let mins = (seconds / 60.0).floor();
        format!("{mins:.0} m {:.1} s", seconds - mins * 60.0)
    }
}
