//! Measuring what a routine does while it does it.

use zx_rustrum::machine::{Spectrum, FRAME_T};

/// Assemble at $8000, run it, and hand back what was measured.
fn watch(program: &[(u16, u8)], entry: u16, frames: u32) -> Spectrum {
    let mut spec = Spectrum::new();
    for (addr, byte) in program {
        spec.bus.poke(*addr, *byte);
    }
    spec.cpu.pc = entry;
    spec.cpu.sp = 0xFF00;
    spec.bus.observer.enabled = true;
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
    spec
}

/// A routine is credited with what it wrote, not with what its instructions
/// might have written.
#[test]
fn a_routine_is_credited_with_what_it_wrote() {
    // $8000: CALL $9000 : JR $8000
    // $9000: LD HL,$4000 : LD (HL),$FF : INC HL : ... : RET
    let mut program = vec![
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90),
        (0x8003, 0x18),
        (0x8004, 0xFB), // JR back to $8000
        (0x9000, 0x21),
        (0x9001, 0x00),
        (0x9002, 0x40), // LD HL,$4000
        (0x9003, 0x06),
        (0x9004, 0x08), // LD B,8
        // loop at $9005: LD (HL),$FF : INC HL : DJNZ
        (0x9005, 0x36),
        (0x9006, 0xFF),
        (0x9007, 0x23),
        (0x9008, 0x10),
        (0x9009, 0xFB),
        (0x900A, 0xC9), // RET
    ];
    program.push((0x900B, 0x00));

    let spec = watch(&program, 0x8000, 1);
    let seen = spec
        .bus
        .observer
        .routines
        .get(&0x9000)
        .expect("the routine that was called was not seen at all");

    assert!(seen.calls > 10, "only {} calls", seen.calls);
    // Eight bytes a call, give or take the call that was still running when
    // the frame's budget ran out.
    let expected = seen.calls * 8;
    assert!(
        seen.writes.screen <= expected && seen.writes.screen + 8 >= expected,
        "eight bytes a call is what it writes; got {} over {} calls",
        seen.writes.screen,
        seen.calls
    );
    assert_eq!(seen.writes.attrs, 0, "it never touches the attributes");
    assert_eq!(
        seen.longest_loop(),
        7,
        "a DJNZ of eight jumps back seven times"
    );
    // The routine leaves HL past the end of what it wrote, so the second call
    // arrives with a different HL from the first: measured, not assumed.
    assert!(
        !seen.entry_hl.constant(),
        "HL came in as ${:04X} every time, which cannot be right after the \
         routine has advanced it",
        seen.entry_hl.low
    );
}

/// The call graph is who called whom, taken from what the CPU did.
#[test]
fn the_call_graph_records_who_called_whom() {
    let program = vec![
        // $8000: CALL $9000 : JR $8000
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90),
        (0x8003, 0x18),
        (0x8004, 0xFB),
        // $9000: CALL $9100 : RET
        (0x9000, 0xCD),
        (0x9001, 0x00),
        (0x9002, 0x91),
        (0x9003, 0xC9),
        // $9100: RET
        (0x9100, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 1);
    let observer = &spec.bus.observer;

    let edge = observer
        .edges
        .get(&(0x9000, 0x9100))
        .expect("the inner call was not recorded");
    assert!(edge.calls > 10, "only {} of them", edge.calls);
    assert!(
        observer.routines.contains_key(&0x9100),
        "the inner routine was not seen"
    );
    assert_eq!(
        observer.routines[&0x9100].max_depth, 2,
        "it is two deep, being called from a routine that was itself called"
    );
}

/// Registers on the way in say what a routine is handed: one that is always
/// the same is a constant, one that varies is an argument.
#[test]
fn registers_on_the_way_in_are_remembered() {
    let program = vec![
        // $8000: LD HL,$1234 : CALL $9000 : LD HL,$4321 : CALL $9000 : JR $8000
        (0x8000, 0x21),
        (0x8001, 0x34),
        (0x8002, 0x12),
        (0x8003, 0xCD),
        (0x8004, 0x00),
        (0x8005, 0x90),
        (0x8006, 0x21),
        (0x8007, 0x21),
        (0x8008, 0x43),
        (0x8009, 0xCD),
        (0x800A, 0x00),
        (0x800B, 0x90),
        (0x800C, 0x18),
        (0x800D, 0xF2),
        (0x9000, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 1);
    let seen = &spec.bus.observer.routines[&0x9000];

    assert_eq!(seen.entry_hl.low, 0x1234);
    assert_eq!(seen.entry_hl.high, 0x4321);
    assert!(!seen.entry_hl.constant(), "HL varies, so it is an argument");
}

/// What is executed is code; what is only read is data.
#[test]
fn what_is_read_but_never_run_is_data() {
    let program = vec![
        // $8000: LD HL,$C000 : LD A,(HL) : JR $8000
        (0x8000, 0x21),
        (0x8001, 0x00),
        (0x8002, 0xC0),
        (0x8003, 0x7E),
        (0x8004, 0x18),
        (0x8005, 0xFA),
    ];
    let spec = watch(&program, 0x8000, 1);
    let observer = &spec.bus.observer;

    assert!(observer.was_executed(0x8000), "the code should be code");
    assert!(
        !observer.is_data(0x8000),
        "and code that is also read is still code"
    );
    assert!(
        observer.is_data(0xC000),
        "the byte it read and never ran is data"
    );
}

/// Watching costs something, so nothing is recorded until it is switched on.
#[test]
fn nothing_is_watched_until_it_is_switched_on() {
    let mut spec = Spectrum::new();
    spec.bus.poke(0x8000, 0xCD);
    spec.bus.poke(0x8001, 0x00);
    spec.bus.poke(0x8002, 0x90);
    spec.bus.poke(0x9000, 0xC9);
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xFF00;
    spec.run(FRAME_T);

    assert!(spec.bus.observer.routines.is_empty());
    assert_eq!(spec.bus.observer.frames, 0);
}
