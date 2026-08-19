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

/// Whether an opcode is a jump that always jumps.
///
/// `JP nn`, and the indirect `JP (HL)`, `JP (IX)` and `JP (IY)` a dispatch
/// table goes through. A conditional `JP cc,nn` is not one of these: it is an
/// early way out of a routine taken when a flag says so, and the routine
/// carries on underneath it. Treating those as endings would cut every guarded
/// routine into pieces at its first test.
///
/// `JR` is left out on purpose. Its reach is a hundred and twenty-odd bytes
/// either way, which is inside the routine it is in almost every time, and
/// calling each one an ending would divide loops rather than routines.
pub fn always_jumps(opcode: [u8; 2]) -> bool {
    match opcode[0] {
        // JP nn, JP (HL)
        0xC3 | 0xE9 => true,
        // JP (IX), JP (IY)
        0xDD | 0xFD => opcode[1] == 0xE9,
        _ => false,
    }
}

/// Whether an opcode is a return that always returns.
///
/// `RET`, `RETI` and `RETN`. A conditional `RET cc` is not one: it is an early
/// way out taken when a flag says so, and the routine carries on underneath
/// it. The distinction matters where a routine's *extent* is being worked out
/// rather than where one call ended — a taken `RET Z` ends that call, and the
/// next call through the same routine may fall straight past it. Treating it
/// as the end drew the routine as far as its first test and no further.
pub fn always_returns(opcode: [u8; 2]) -> bool {
    match opcode[0] {
        0xC9 => true,
        // RETI and RETN, which are ED-prefixed.
        0xED => matches!(
            opcode[1],
            0x4D | 0x45 | 0x55 | 0x5D | 0x65 | 0x6D | 0x75 | 0x7D
        ),
        _ => false,
    }
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
