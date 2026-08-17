//! Loading a tape by answering the ROM instead of playing it.
//!
//! A tape normally loads at 1,500 baud whatever the emulator does about it:
//! the deck makes pulses, the ROM's loader counts them, and four minutes of
//! tape takes four minutes of machine time however fast the machine is run.
//! Running the CPU flat out shortens the wait; it cannot remove it.
//!
//! This removes it. When the machine calls the ROM's LD-BYTES with a standard
//! block under the head, the block is handed over whole — copied into memory,
//! the registers left as the routine would have left them — and the return is
//! taken there and then. A tape loads in the time it takes to memcpy it.
//!
//! It only works where the ROM is doing the loading. A game with a loader of
//! its own is reading the port itself and counting its own pulses, and nothing
//! here can help it: those still load at whatever speed the machine is being
//! run at, which is what Max Speed is for.

use crate::machine::Spectrum;
use crate::tape::Block;

/// Where the 48K ROM's LD-BYTES starts.
pub const LD_BYTES: u16 = 0x0556;

/// The first bytes of it: `INC D / EX AF,AF' / DEC D / DI / LD A,$0F /
/// OUT ($FE),A`.
///
/// Checked rather than assumed, because $0556 is only the loader while the
/// right ROM is paged in — a 128K is running its own ROM until a game pages
/// the other one back — and because a program is free to put anything it
/// likes at that address in RAM.
const LD_BYTES_SIGNATURE: [u8; 8] = [0x14, 0x08, 0x15, 0xF3, 0x3E, 0x0F, 0xD3, 0xFE];

/// The edge-sampling loop that nearly every loader is built round, from the
/// [loading routine cores](https://sinclair.wiki.zxnet.co.uk/wiki/Loading_routine_%22cores%22):
///
/// ```text
/// LD-SAMPLE  INC B          04
///            RET Z          C8
///            LD A,$7F       3E 7F
///            IN A,($FE)     DB FE
///            RRA            1F
///            XOR C          A9
///            AND $20        E6 20
///            JR Z,LD-SAMPLE 28 xx
/// ```
///
/// The ROM has a `RET NC` in the middle of it to abort on BREAK and Speedlock
/// does not, which is the only difference between the two; the jump back is
/// the last byte and its displacement depends on where the loop starts, so it
/// is not part of what is matched.
const SAMPLER: [u8; 11] = [
    0x04, 0xC8, 0x3E, 0x7F, 0xDB, 0xFE, 0x1F, 0xA9, 0xE6, 0x20, 0x28,
];

/// What one turn of that loop costs when nothing is in its way: `INC B` 4,
/// `RET Z` 5, `LD A,$7F` 7, `IN A,($FE)` 11, `RRA` 4, `XOR C` 4, `AND $20` 7,
/// and `JR Z` 12 when it is taken.
///
/// Only when nothing is in its way. The port it reads is $7FFE, whose high
/// byte is in the contended range, so the ULA stalls the read by however much
/// it feels like: measured over a load of Head over Heels, 54 T-states most of
/// the time but 56, 58 and 60 often enough to matter. That is why the wait
/// cannot simply be divided out and skipped — see the note in
/// `docs/tape-loading.md`.
#[allow(dead_code)]
const SAMPLER_T: u64 = 54;

/// Is the machine sitting in a loader's sampling loop?
///
/// Which is to say: is a loader of the game's own reading the tape? The ROM's
/// blocks can be handed over whole, and these cannot — the bytes on the tape
/// are not the bytes that reach memory, since Speedlock decrypts each one as
/// it goes with a key it rewrites into its own code — but knowing that a
/// loader is at work is what says the machine should be let run flat out.
pub fn at_sampler(spec: &Spectrum) -> bool {
    use crate::z80::Bus;
    SAMPLER
        .iter()
        .enumerate()
        .all(|(offset, byte)| spec.bus.peek(spec.cpu.pc.wrapping_add(offset as u16)) == *byte)
}

/// What came of trying to hand a block over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loaded {
    /// A block was handed over and the routine returned.
    Block { bytes: usize, wanted: usize },
    /// The block under the head is not the one the program asked for — a
    /// header where it wants data, or the other way about. The ROM answers
    /// that with "no", and so does this.
    WrongKind,
    /// Nothing could be done: no tape, no standard block under the head, or
    /// the ROM that owns the address is not the one paged in.
    NotOurs,
}

/// Answer the ROM's loader, if that is what the machine is calling.
///
/// Returns what happened, having left the machine as the routine would have:
/// the return address popped, the registers set, and the deck moved on to the
/// next block.
pub fn load_block(spec: &mut Spectrum) -> Loaded {
    if spec.cpu.pc != LD_BYTES || !rom_loader_is_here(spec) {
        return Loaded::NotOurs;
    }
    let Some(tape) = spec.bus.tape.as_ref() else {
        return Loaded::NotOurs;
    };
    // Only from a deck that is running. A stopped tape is a stopped tape,
    // however quickly the emulator is willing to read one: `LOAD ""` on a
    // machine with the tape paused waits for somebody to press Play, and
    // handing it blocks anyway ran the whole tape through the moment it was
    // asked for.
    if !tape.playing {
        return Loaded::NotOurs;
    }

    // What the caller asked for: the flag it expects in A, the address in IX,
    // the length in DE, and carry telling load from verify. The routine keeps
    // them in the alternate set, but it has not run yet, so they are here.
    let wanted_flag = spec.cpu.a;
    let verifying = spec.cpu.f & 0x01 == 0;
    let mut at = spec.cpu.ix;
    let wanted = ((spec.cpu.d as u16) << 8) | spec.cpu.e as u16;

    // The block the program asked for, which is the next one along with the
    // right flag byte: a program looking for its data steps over the headers
    // in between, and so does this. The search stops at anything that is not
    // a plain block of bytes — a turbo block or a stretch of pulses belongs
    // to a loader of the game's own, and the ROM would never get past it.
    let mut index = tape.block;
    let found = loop {
        match block_bytes(tape.blocks.get(index)) {
            Some(data) if data.len() >= 2 && data[0] == wanted_flag => break Some(data.to_vec()),
            Some(_) => index += 1,
            None => break None,
        }
    };
    let Some(data) = found else {
        return Loaded::NotOurs;
    };

    // The bytes the ROM would have read: the flag, then as many as were asked
    // for, then one more as the parity byte. A block longer than the program
    // wanted is ordinary — the ROM stops listening and the rest of the block
    // goes past unread — so what decides success is whether there were enough
    // of them and whether the parity agrees.
    let body = &data[1..];
    let count = (wanted as usize).min(body.len());
    let mut parity = wanted_flag;
    for byte in &body[..count] {
        parity ^= byte;
        if !verifying {
            spec.bus.poke(at, *byte);
        }
        at = at.wrapping_add(1);
    }
    let enough = count == wanted as usize;
    if let Some(checksum) = body.get(count) {
        parity ^= checksum;
    }
    // A block too short for what was asked is the "R Tape loading error" every
    // mistyped POKE ends in.
    let ok = enough && body.len() > count && parity == 0;
    seek_past(spec, index);
    finish(spec, ok, at, wanted - count as u16);
    Loaded::Block {
        bytes: count,
        wanted: wanted as usize,
    }
}

/// Is the ROM that owns $0556 the one paged in?
fn rom_loader_is_here(spec: &Spectrum) -> bool {
    use crate::z80::Bus;
    LD_BYTES_SIGNATURE
        .iter()
        .enumerate()
        .all(|(offset, byte)| spec.bus.peek(LD_BYTES + offset as u16) == *byte)
}

/// The bytes of a block the ROM could have read: a flag, the data, and a
/// checksum. A turbo block holds its bytes the same way — only its pulses are
/// quicker — but the ROM is not the one reading those, so they are left alone.
fn block_bytes(block: Option<&Block>) -> Option<&[u8]> {
    match block? {
        Block::Standard { data, .. } => Some(data),
        _ => None,
    }
}

/// Move the deck to just past the block handed over.
fn seek_past(spec: &mut Spectrum, block: usize) {
    let now = spec.bus.total_t();
    if let Some(tape) = spec.bus.tape.as_mut() {
        tape.seek(block + 1);
        // Playing from the end rewinds — which, having just handed over the
        // last block of the tape, started the whole thing loading again.
        if tape.playing && !tape.finished() {
            tape.play(now);
        } else {
            tape.stop();
        }
    }
}

/// Leave the machine where LD-BYTES would have left it, and take the return.
fn finish(spec: &mut Spectrum, ok: bool, at: u16, left: u16) {
    spec.cpu.ix = at;
    spec.cpu.d = (left >> 8) as u8;
    spec.cpu.e = left as u8;
    // Carry says whether it worked, which is the only flag the callers read.
    // The loader ends with the parity byte in H, zero when it agreed.
    spec.cpu.f = if ok {
        spec.cpu.f | 0x01
    } else {
        spec.cpu.f & !0x01
    };
    spec.cpu.h = 0;
    // Interrupts are disabled on the way in and put back on the way out.
    spec.cpu.iff1 = true;
    spec.cpu.iff2 = true;

    let low = spec.bus.mem(spec.cpu.sp) as u16;
    let high = spec.bus.mem(spec.cpu.sp.wrapping_add(1)) as u16;
    spec.cpu.sp = spec.cpu.sp.wrapping_add(2);
    spec.cpu.pc = (high << 8) | low;
}
