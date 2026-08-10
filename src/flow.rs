//! Telling a call from a return by what the CPU did, rather than by decoding
//! the instruction.
//!
//! A call is any instruction that leaves SP two lower with the address of the
//! following instruction on top of the stack. That catches `CALL`, `RST` and
//! interrupt acceptance alike, and misses a program that pushes a return
//! address and jumps — which is the point: what matters is where the machine
//! went, not which opcode took it there.
//!
//! A return is any instruction that leaves SP two higher having jumped to the
//! word it took off the stack.

/// What one instruction did to the call stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// Went into a routine at `entry`, with SP now at `sp`.
    Call { entry: u16, sp: u16 },
    /// Came back to `to`, from the frame whose SP was `sp_before`.
    Return { to: u16, sp_before: u16 },
    /// Carried on.
    Straight,
}

/// Classify one executed instruction. `peek` reads the word at an address.
pub fn classify(
    pc_before: u16,
    sp_before: u16,
    pc_after: u16,
    sp_after: u16,
    peek: impl Fn(u16) -> u16,
) -> Flow {
    if sp_after == sp_before.wrapping_sub(2) {
        let pushed = peek(sp_after);
        // The pushed address has to be the instruction just after the one that
        // ran: a program pushing data looks nothing like that.
        let plausible_return = pushed > pc_before && pushed.wrapping_sub(pc_before) <= 4;
        if plausible_return && pc_after != pushed {
            return Flow::Call {
                entry: pc_after,
                sp: sp_after,
            };
        }
    }
    if sp_after == sp_before.wrapping_add(2) && peek(sp_before) == pc_after {
        return Flow::Return {
            to: pc_after,
            sp_before,
        };
    }
    Flow::Straight
}
