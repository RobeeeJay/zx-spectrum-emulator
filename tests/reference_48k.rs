//! The 48K reference's own numbers, checked one at a time.
//!
//! <https://worldofspectrum.org/faq/reference/48kreference.htm>. Everything
//! here is quoted from it: a frame of 69888 T-states, a line of 224, the
//! contention table T-state by T-state, and the four I/O patterns.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::z80::Bus;

fn at(t: u32) -> Spectrum {
    let mut spec = Spectrum::new();
    spec.bus.tstates = t;
    spec
}

/// "a frame is (64+192+56)×224=69888 T states long, which means that the
/// '50 Hz' interrupt is actually a 3.5MHz/69888=50.08 Hz interrupt."
#[test]
fn a_frame_is_64_plus_192_plus_56_lines_of_224() {
    let model = Model::Spectrum48;
    assert_eq!(
        model.t_per_line(),
        224,
        "each line takes exactly 224 T states"
    );
    assert_eq!(
        (64 + 192 + 56) * model.t_per_line(),
        69888,
        "the reference's own arithmetic"
    );
    assert_eq!(FRAME_T, 69888);
    assert_eq!(model.cpu_hz(), 3_500_000.0);
    let interrupts = model.cpu_hz() / FRAME_T as f64;
    assert!(
        (interrupts - 50.08).abs() < 0.005,
        "the '50 Hz' interrupt is 50.08 Hz, not {interrupts}"
    );
}

/// "After an interrupt occurs, 64 line times (14336 T states...) pass before
/// the first byte of the screen (16384) is displayed."
///
/// The emulator counts the display from 14335 rather than 14336, because that
/// is where the reference's own contention table starts: an access beginning
/// at 14335 is the first one the ULA delays. The two are the same statement
/// one T-state apart — where the pixels land was settled against photographs
/// of a real machine, not against either number.
#[test]
fn the_first_screen_byte_is_64_lines_after_the_interrupt() {
    let model = Model::Spectrum48;
    assert_eq!(64 * model.t_per_line(), 14336);
    assert_eq!(
        Spectrum::new().bus.first_pixel_t(),
        14335,
        "one T-state before the byte appears, which is where the delays start"
    );
}

/// The contention table, copied out of the reference:
///
/// ```text
/// Cycle #    Delay
///  14335       6      14343       6
///  14336       5      14344       5
///  14337       4      14345       4
///  14338       3      14346       3
///  14339       2      14347       2
///  14340       1      14348       1
///  14341   No delay   14349   No delay
///  14342   No delay   14350   No delay
/// ```
#[test]
fn the_contention_table_matches_the_reference() {
    let table = [
        (14335, 6),
        (14336, 5),
        (14337, 4),
        (14338, 3),
        (14339, 2),
        (14340, 1),
        (14341, 0),
        (14342, 0),
        (14343, 6),
        (14344, 5),
        (14345, 4),
        (14346, 3),
        (14347, 2),
        (14348, 1),
        (14349, 0),
        (14350, 0),
    ];
    for (cycle, delay) in table {
        let mut spec = at(cycle);
        spec.bus.read(0x4000);
        assert_eq!(
            spec.bus.tstates - cycle,
            delay + 3,
            "an access at {cycle} should be delayed by {delay} and then take \
             its three T-states"
        );
    }
}

/// "This is valid for all 192 lines of screen data. While the ULA is updating
/// the border the delay does not happen at any time."
#[test]
fn the_pattern_holds_for_192_lines_and_stops_at_the_border() {
    let first = Spectrum::new().bus.first_pixel_t();
    let per_line = Model::Spectrum48.t_per_line();
    for line in [0u32, 1, 95, 191] {
        let mut spec = at(first + line * per_line);
        spec.bus.read(0x4000);
        assert_eq!(
            spec.bus.tstates - (first + line * per_line),
            6 + 3,
            "line {line} should be contended like every other"
        );
    }
    // 128 T-states of screen, then 96 of border and retrace.
    for along in [128u32, 150, 223] {
        let start = first + along;
        let mut spec = at(start);
        spec.bus.read(0x4000);
        assert_eq!(
            spec.bus.tstates - start,
            3,
            "T-state {along} of the line is border, where nothing is delayed"
        );
    }
    // And the 56 lines after the display.
    let after = first + 192 * per_line;
    let mut spec = at(after);
    spec.bus.read(0x4000);
    assert_eq!(
        spec.bus.tstates - after,
        3,
        "below the display there is no ULA"
    );
}

/// "programs which run in the contended memory (from 0x4000 to 0x7fff)":
/// nothing above that is delayed, whatever the ULA is doing.
#[test]
fn only_the_lower_ram_is_contended() {
    let first = Spectrum::new().bus.first_pixel_t();
    for addr in [0x0000u16, 0x3fff, 0x8000, 0xbfff, 0xc000, 0xffff] {
        let mut spec = at(first);
        spec.bus.read(addr);
        assert_eq!(
            spec.bus.tstates - first,
            3,
            "${addr:04X} is outside $4000-$7FFF and should never be delayed"
        );
    }
    for addr in [0x4000u16, 0x5b00, 0x7fff] {
        let mut spec = at(first);
        spec.bus.read(addr);
        assert_eq!(
            spec.bus.tstates - first,
            6 + 3,
            "${addr:04X} is in the contended range"
        );
    }
}

/// The four I/O patterns, from the reference's table:
///
/// ```text
/// High byte in 40-7F? | Low bit | Contention pattern
///          No         |  Reset  | N:1, C:3
///          No         |   Set   | N:4
///         Yes         |  Reset  | C:1, C:3
///         Yes         |   Set   | C:1, C:1, C:1, C:1
/// ```
///
/// Worked through by hand from the delay table above, starting at 14335:
///
/// - `$00FE` (N:1, C:3): 1, then a stall of 5 from 14336, then 3 — nine.
/// - `$00FF` (N:4): four, whatever the ULA is doing.
/// - `$40FE` (C:1, C:3): a stall of 6, 1, no stall at 14342, then 3 — ten.
/// - `$40FF` (C:1 ×4): stalls of 6, 0, 6, 0 with a T-state after each —
///   sixteen.
#[test]
fn the_io_contention_patterns_match_the_reference() {
    for (port, expected, pattern) in [
        (0x00FEu16, 9u32, "N:1, C:3"),
        (0x00FF, 4, "N:4"),
        (0x40FE, 10, "C:1, C:3"),
        (0x40FF, 16, "C:1, C:1, C:1, C:1"),
    ] {
        let mut spec = at(14335);
        spec.bus.io_read(port);
        assert_eq!(
            spec.bus.tstates - 14335,
            expected,
            "IN from ${port:04X} is {pattern}, which from 14335 takes \
             {expected} T-states"
        );
    }
}

/// An M1 fetch is contended once, at its start: "only the first cycle will be
/// affected and only if PC lies within the contended memory range".
#[test]
fn an_opcode_fetch_is_contended_once() {
    let mut spec = at(14335);
    spec.bus.fetch_op(0x4000);
    assert_eq!(
        spec.bus.tstates - 14335,
        6 + 4,
        "one delay, then the four T-states of the M1 cycle"
    );

    let mut spec = at(14335);
    spec.bus.fetch_op(0x8000);
    assert_eq!(
        spec.bus.tstates - 14335,
        4,
        "an M1 outside the contended range is never delayed"
    );
}

/// The EAR bit is sampled where the ULA puts it on the bus, not where the
/// instruction ends.
///
/// An `IN A,($FE)` on a contended port is stalled by the ULA and then takes
/// its four T-states; the byte is on the bus at the IORQ cycle, which is three
/// of those T-states before the instruction is over. Reading the tape at the
/// end instead samples it late — and late by a varying amount, since the stall
/// depends on where the beam is, which is exactly the kind of jitter an edge
/// loader measures against.
#[test]
fn the_ear_bit_is_sampled_at_the_iorq_cycle() {
    use zx_rustrum::tape::{Block, Tape};
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    let mut tape = Tape::from_blocks(
        "t".into(),
        vec![Block::PureTone {
            len: 2168,
            count: 200,
        }],
    );
    tape.play(0);
    spec.bus.tape = Some(tape);

    // In the display area, where the ULA stalls the read: `LD A,$7F` then
    // `IN A,($FE)`, the loop every loader is built round.
    spec.bus.tstates = 15_000;
    spec.cpu.pc = 0x8000;
    spec.bus.poke(0x8000, 0x3E);
    spec.bus.poke(0x8001, 0x7F);
    spec.bus.poke(0x8002, 0xDB);
    spec.bus.poke(0x8003, 0xFE);
    spec.step_instruction();
    spec.step_instruction();

    let ended = spec.bus.total_t();
    let sampled = spec.bus.tape.as_ref().expect("a tape").clock;
    assert!(
        ended > sampled,
        "the tape should have been read before the instruction ended: read at \
         {sampled}, ended at {ended}"
    );
    assert!(
        ended - sampled >= 3,
        "and three T-states before it at least, which is what follows the \
         IORQ cycle: {} T",
        ended - sampled
    );
}
