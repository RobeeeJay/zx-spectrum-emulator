//! Where a routine ends, and where it only looks as though it does.

use zx_rustrum::flow::always_returns;
use zx_rustrum::machine::Spectrum;

/// A conditional return ends the call it is in. It does not end the routine:
/// the next call through may fall straight past it.
#[test]
fn a_conditional_return_is_not_the_end_of_a_routine() {
    assert!(always_returns([0xC9, 0x00]), "RET always returns");
    assert!(always_returns([0xED, 0x4D]), "and so does RETI");
    assert!(always_returns([0xED, 0x45]), "and RETN");
    for opcode in [0xC0u8, 0xC8, 0xD0, 0xD8, 0xE0, 0xE8, 0xF0, 0xF8] {
        assert!(
            !always_returns([opcode, 0x00]),
            "RET cc (${opcode:02X}) is an early way out, not an ending"
        );
    }
}

/// ```text
/// $8000  main:  CALL $8100      ; twice: once with A=0, once with A=1
/// ...
/// $8100  work:  OR   A
/// $8101         RET  Z          ; the early way out
/// $8102         LD   ($9000),A  ; only reached when A is not zero
/// $8105         RET
/// ```
///
/// The routine is six bytes. Watched with the flag set both ways, it used to
/// be recorded as ending at $8101 — its first test — because a taken `RET Z`
/// is a return, and a return was taken to be where the routine stops.
#[test]
fn a_routine_reaches_past_the_test_that_can_leave_it_early() {
    let mut spec = Spectrum::new();
    let program: &[(u16, &[u8])] = &[
        // main: A=0, call; A=1, call; jump back to the top.
        (0x8000, &[0x3E, 0x00]),       // LD A,0
        (0x8002, &[0xCD, 0x00, 0x81]), // CALL work
        (0x8005, &[0x3E, 0x01]),       // LD A,1
        (0x8007, &[0xCD, 0x00, 0x81]), // CALL work
        (0x800A, &[0xC3, 0x00, 0x80]), // JP main
        // work: the guarded routine.
        (0x8100, &[0xB7]),             // OR A
        (0x8101, &[0xC8]),             // RET Z
        (0x8102, &[0x32, 0x00, 0x90]), // LD ($9000),A
        (0x8105, &[0xC9]),             // RET
    ];
    for (at, bytes) in program {
        for (i, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + i as u16, *byte);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.bus.observer.enabled = true;
    spec.bus
        .observer
        .on_interrupt(0x8000, spec.cpu.sp, Default::default());
    for _ in 0..200 {
        spec.step_instruction();
    }

    let work = spec
        .bus
        .observer
        .routines
        .get(&0x8100)
        .expect("the routine was watched");

    // Both ways out are noticed…
    assert!(
        work.exits.contains(&0x8101) && work.exits.contains(&0x8105),
        "both ways out should be seen: {:?}",
        work.exits
    );
    // …and only the one that always ends it is where the routine stops.
    assert_eq!(
        work.after,
        vec![0x8106],
        "the routine ends after the plain RET at $8105, not after the RET Z \
         at $8101: {:?}",
        work.after
    );

    // Which is what the coloured blocks in the debugger are cut on.
    let blocks = zx_rustrum::blocks::work_out(&spec.bus.observer);
    let holding = blocks
        .iter()
        .find(|b| b.from <= 0x8100 && b.to >= 0x8100)
        .expect("a block for the routine");
    assert!(
        holding.to >= 0x8105,
        "the block should cover the whole routine, not stop at its test: \
         ${:04X}-${:04X}",
        holding.from,
        holding.to
    );
}

/// A jump that always jumps still ends the routine it is in: that is the tail
/// call a Z80 program writes instead of CALL followed by RET.
#[test]
fn an_unconditional_jump_still_ends_a_routine() {
    let mut spec = Spectrum::new();
    let program: &[(u16, &[u8])] = &[
        (0x8000, &[0xCD, 0x00, 0x81]), // CALL first
        (0x8003, &[0x18, 0xFB]),       // JR main
        (0x8100, &[0x00]),             // NOP
        (0x8101, &[0xC3, 0x00, 0x82]), // JP second — a tail call
        (0x8200, &[0xC9]),             // RET
    ];
    for (at, bytes) in program {
        for (i, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + i as u16, *byte);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.bus.observer.enabled = true;
    spec.bus
        .observer
        .on_interrupt(0x8000, spec.cpu.sp, Default::default());
    for _ in 0..200 {
        spec.step_instruction();
    }
    let first = spec
        .bus
        .observer
        .routines
        .get(&0x8100)
        .expect("the first routine was watched");
    assert_eq!(
        first.after,
        vec![0x8104],
        "a JP that always jumps ends the routine after it: {:?}",
        first.after
    );
}
