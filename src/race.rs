//! Racing the beam: the frame replayed from its interrupt, a T-state at a time.
//!
//! What a program does within one frame is where the picture comes from, and
//! the only way to see it is to be at a particular moment of that frame. So a
//! copy of the machine is taken at the interrupt that starts the frame and run
//! forward to wherever the cursor is: what comes back is the machine as it
//! stood after every instruction that had been executed by then — the display
//! file part-written, the border set to whatever it was set to, the registers
//! wherever they had got to.
//!
//! It is a copy because the machine itself must not move. The user has stopped
//! it to look at it, and dragging the cursor down the picture would otherwise
//! run the program.
//!
//! Going down the screen costs nothing: the copy is already at the last
//! position, so it is run on from there. Going back up means starting again
//! from the snapshot, which is a frame of work at most — the machine does
//! fifty of those a second, so it is not worth being cleverer than that.

use crate::machine::Spectrum;

/// A frame being raced, and the machine it was taken from.
pub struct Race {
    /// The machine at the interrupt that begins the frame.
    from: Spectrum,
    /// That machine, run on to `at`.
    replay: Spectrum,
    /// How far into the frame the replay has been run, in T-states.
    at: u32,
    /// Where the real machine stood when the snapshot was taken, so that
    /// stepping it by hand takes a new one rather than showing a stale frame.
    taken_at: (u64, u32),
}

impl Race {
    /// Take the snapshot: a copy of `machine`, run on to the start of the next
    /// frame.
    ///
    /// The machine is stopped part-way through a frame and there is no way
    /// back to the start of the one it is in — so the frame that is raced is
    /// the next one, whole, from its interrupt.
    pub fn start(machine: &Spectrum) -> Race {
        let mut from = quiet_copy(machine);
        let frame = from.bus.frame;
        // Run to the frame boundary. Bounded in case the machine is wedged in
        // a way that never finishes one: a couple of frames' worth of
        // instructions is far more than any frame takes.
        let mut left = 200_000;
        while from.bus.frame == frame && left > 0 {
            from.step_instruction();
            left -= 1;
        }
        Race {
            replay: from.clone(),
            from,
            at: 0,
            taken_at: (machine.bus.frame, machine.bus.tstates),
        }
    }

    /// Is this snapshot still the one for `machine`? Stepping the machine, or
    /// letting it run, moves it on and makes the answer no.
    pub fn is_of(&self, machine: &Spectrum) -> bool {
        self.taken_at == (machine.bus.frame, machine.bus.tstates)
    }

    /// The machine as it stood `t` T-states into the frame.
    pub fn at(&mut self, t: u32) -> &Spectrum {
        if t < self.at {
            // Backwards: nothing can be un-executed, so start again.
            self.replay.clone_from(&self.from);
            self.at = 0;
        }
        let frame = self.from.bus.frame;
        while self.replay.bus.tstates < t && self.replay.bus.frame == frame {
            self.replay.step_instruction();
        }
        self.at = self.replay.bus.tstates;
        // The ULA has painted everything up to here, whether or not the
        // program has written to the screen since the last time it was asked.
        self.replay.bus.catch_up_painting();
        &self.replay
    }

    /// How far the replay has been run, in T-states into the frame.
    pub fn reached(&self) -> u32 {
        self.at
    }

    /// The frame being raced, counted from when the machine started.
    pub fn frame(&self) -> u64 {
        self.from.bus.frame
    }
}

/// A copy of a machine that cannot be heard or leave anything behind it.
///
/// The sound queue is shared with the device, so a copy holds the same one and
/// would play a frame of its own over the real machine's; and a copy that
/// carried on writing an RZX recording would write frames nobody played.
fn quiet_copy(machine: &Spectrum) -> Spectrum {
    let mut copy = machine.clone();
    copy.bus.audio.detach();
    copy.bus.capture = None;
    copy
}
