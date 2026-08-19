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
    // A DJNZ of eight jumps back seven times. The figure is a total divided
    // by the number of calls, and the call still running when the frame's
    // budget ran out has been counted without its loop finishing, so six is
    // the honest answer here too.
    assert!(
        (6..=7).contains(&seen.longest_loop()),
        "a DJNZ of eight jumps back seven times, not {}",
        seen.longest_loop()
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

/// Where in the frame a routine runs is measured, so a routine timed against
/// the beam can be told from one doing its work in the border.
#[test]
fn when_in_the_frame_a_routine_runs_is_measured() {
    let program = vec![
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90), // CALL $9000
        (0x8003, 0x18),
        (0x8004, 0xFB), // JR back
        (0x9000, 0xC9), // RET
    ];
    let spec = watch(&program, 0x8000, 1);
    let seen = &spec.bus.observer.routines[&0x9000];

    assert!(
        seen.entered_at.high > 0,
        "nothing was noted about when it ran"
    );
    assert!(
        seen.entered_at.high > seen.entered_at.low,
        "a routine called all frame long should have been seen at more than \
         one point in it: {}..{}",
        seen.entered_at.low,
        seen.entered_at.high
    );
}

/// A routine that only ever writes a few addresses has them remembered, which
/// is what makes a variable findable.
#[test]
fn the_addresses_a_routine_keeps_are_remembered() {
    let program = vec![
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90),
        (0x8003, 0x18),
        (0x8004, 0xFB),
        // $9000: LD A,1 : LD ($C000),A : LD ($C001),A : RET
        (0x9000, 0x3E),
        (0x9001, 0x01),
        (0x9002, 0x32),
        (0x9003, 0x00),
        (0x9004, 0xC0),
        (0x9005, 0x32),
        (0x9006, 0x01),
        (0x9007, 0xC0),
        (0x9008, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 1);
    let seen = &spec.bus.observer.routines[&0x9000];

    assert_eq!(
        seen.hot,
        vec![0xC000, 0xC001],
        "it writes two addresses and nothing else"
    );
    assert_eq!(
        spec.bus.observer.users_of(0xC000),
        vec![0x9000],
        "and that is what writes to $C000"
    );
}

/// Which routine drew a given part of the screen is watched, not inferred:
/// the bus sees the write and remembers who was running.
#[test]
fn what_drew_each_part_of_the_screen_is_remembered() {
    let program = vec![
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90), // CALL $9000
        (0x8003, 0x76), // HALT
        // $9000: LD HL,$4000 : LD (HL),$FF : RET
        (0x9000, 0x21),
        (0x9001, 0x00),
        (0x9002, 0x40),
        (0x9003, 0x36),
        (0x9004, 0xFF),
        (0x9005, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 1);
    let observer = &spec.bus.observer;

    assert_eq!(
        observer.drew(0x4000),
        Some(0x9000),
        "the routine that wrote the top-left byte should be the one named"
    );
    assert_eq!(
        observer.drew(0x4001),
        None,
        "and nothing should be claimed about a byte nobody wrote"
    );
    assert_eq!(
        observer.drew(0x9000),
        None,
        "an address outside the screen is not part of the picture"
    );

    // The cell at the top left, which is what somebody pointing at the screen
    // would be asking about.
    let cell = observer.drew_cell(0, 0);
    assert_eq!(cell.first().map(|(entry, _)| *entry), Some(0x9000));
}

/// Totals say what a routine does; only a sequence says in what order. The
/// order is what somebody reading a game's main loop is asking about.
#[test]
fn the_order_of_calls_within_a_frame_is_recorded() {
    let program = vec![
        // $8000: CALL $9000 : CALL $9100 : JR $8000
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90),
        (0x8003, 0xCD),
        (0x8004, 0x00),
        (0x8005, 0x91),
        (0x8006, 0x18),
        (0x8007, 0xF8),
        // $9000: CALL $9200 : RET
        (0x9000, 0xCD),
        (0x9001, 0x00),
        (0x9002, 0x92),
        (0x9003, 0xC9),
        (0x9100, 0xC9),
        (0x9200, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 2);
    let observer = &spec.bus.observer;

    let frame = observer
        .last_whole_frame()
        .expect("a frame should have been watched from beginning to end");
    let steps: Vec<_> = observer
        .frame_steps(frame)
        .into_iter()
        .filter(|step| step.enter)
        .collect();
    assert!(steps.len() > 3, "only {} calls recorded", steps.len());

    // $9200 is called from inside $9000, and $9100 after both. Which of them
    // the frame happens to open with is not fixed: the interrupt falls where
    // it falls, part-way round the loop.
    let outer = steps
        .iter()
        .position(|step| step.entry == 0x9000)
        .expect("$9000 was not recorded");
    let inner = steps[outer..]
        .iter()
        .find(|step| step.entry == 0x9200)
        .expect("the call inside $9000 was not recorded");
    assert!(
        inner.depth > steps[outer].depth,
        "the call inside $9000 should be deeper than it: {} against {}",
        inner.depth,
        steps[outer].depth
    );
    let next = steps[outer..]
        .iter()
        .find(|step| step.entry == 0x9100)
        .expect("the call after $9000 was not recorded");
    assert!(
        next.t >= inner.t,
        "and it comes after the one nested inside the first"
    );

    // Time runs forwards within a frame.
    let times: Vec<u32> = observer.frame_steps(frame).iter().map(|s| s.t).collect();
    assert!(
        times.windows(2).all(|pair| pair[1] >= pair[0]),
        "the steps are not in order: {times:?}"
    );
}

/// What a routine causes to happen is as much a fact about it as what its own
/// instructions do. A routine whose job is to call the drawing routine wrote
/// nothing itself and looked, in the measurements, like one that thinks rather
/// than draws.
#[test]
fn a_caller_is_credited_with_what_its_callees_wrote() {
    let program = vec![
        (0x8000, 0xCD),
        (0x8001, 0x00),
        (0x8002, 0x90), // CALL $9000
        (0x8003, 0x76), // HALT
        // $9000 calls $9100 and writes nothing itself.
        (0x9000, 0xCD),
        (0x9001, 0x00),
        (0x9002, 0x91),
        (0x9003, 0xC9),
        // $9100: LD HL,$4000 : LD (HL),$FF : RET
        (0x9100, 0x21),
        (0x9101, 0x00),
        (0x9102, 0x40),
        (0x9103, 0x36),
        (0x9104, 0xFF),
        (0x9105, 0xC9),
    ];
    let spec = watch(&program, 0x8000, 1);
    let routines = &spec.bus.observer.routines;

    let caller = &routines[&0x9000];
    assert_eq!(
        caller.writes.screen, 0,
        "it writes nothing itself, and that is still true"
    );
    assert!(
        caller.inclusive.screen > 0,
        "but everything it causes should be counted against it too"
    );
    let drawer = &routines[&0x9100];
    assert_eq!(
        drawer.writes.screen, drawer.inclusive.screen,
        "a routine with no callees has the same figure either way"
    );
}

/// When in the frame a routine ran is recorded in the frame's own T-states.
///
/// A 48K frame is 69,888 of them and a `u16` stops at 65,535, so anything
/// entered in the last four and a half thousand — the bottom two character
/// rows and the border under them — was recorded as having happened at 65,535.
/// Every routine that ran down there looked as though it ran at the very end
/// of the frame, which is where AutoDoc reads the beam from.
#[test]
fn when_in_the_frame_a_routine_ran_is_not_cut_off_at_65535() {
    let mut spec = Spectrum::new();
    for (at, bytes) in [
        (0x8000u16, &[0xCD, 0x00, 0x90, 0x18, 0xFB][..]),
        (0x9000, &[0x00, 0xC9][..]),
    ] {
        for (offset, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + offset as u16, *byte);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xFF00;
    spec.bus.observer.enabled = true;

    // Near the bottom of the picture, past where a u16 gives out.
    spec.bus.tstates = 68_000;
    spec.step_instruction();
    spec.step_instruction();

    let seen = spec
        .bus
        .observer
        .routines
        .get(&0x9000)
        .expect("it was called");
    assert!(
        seen.entered_at.low > 65_535,
        "it was called at T {}, and the frame is {} long",
        seen.entered_at.low,
        spec.bus.frame_t()
    );
}

/// How much of a frame a program spends waiting for the interrupt.
///
/// A game that waits on HALT does no work while it waits, and the count of M1
/// cycles spent there is the difference between a program that is short of
/// time and one that is idling. Without it, a frame that runs 13,000
/// instructions looks busy whether 8,000 of them were the CPU sitting still or
/// not.
#[test]
fn time_spent_halted_is_counted() {
    let mut spec = Spectrum::new();
    // DI so nothing wakes it, then HALT for good.
    spec.bus.poke(0x8000, 0xF3);
    spec.bus.poke(0x8001, 0x76);
    spec.cpu.pc = 0x8000;

    let before = spec.cpu.halted_fetches;
    for _ in 0..500 {
        spec.step_instruction();
    }
    let halted = spec.cpu.halted_fetches - before;
    assert!(
        halted > 400,
        "it halted almost immediately and should have spent the rest of those \
         steps there, not {halted}"
    );
    assert!(spec.cpu.halted, "and it should still be halted");

    // A program that is working counts none of it.
    let mut spec = Spectrum::new();
    for at in 0x8000..0x8100u16 {
        spec.bus.poke(at, 0x00);
    }
    spec.cpu.pc = 0x8000;
    let before = spec.cpu.halted_fetches;
    for _ in 0..50 {
        spec.step_instruction();
    }
    assert_eq!(
        spec.cpu.halted_fetches - before,
        0,
        "a program running NOPs is not waiting for anything"
    );
}
