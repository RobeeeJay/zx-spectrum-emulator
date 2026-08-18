//! What each routine is measured doing: its size, its writes and its reads.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::callflow::measured;

/// A machine running a program that calls a routine which reads a table and
/// writes it to the screen.
///
/// ```text
/// $8000  main:  CALL $8100
/// $8003         JR   main
/// $8100  copy:  LD   HL,$9000
/// $8103         LD   DE,$4000
/// $8106         LD   BC,$0010
/// $8109         LDIR
/// $810B         RET
/// ```
fn machine() -> Spectrum {
    let mut spec = Spectrum::new();
    let program: &[(u16, &[u8])] = &[
        (0x8000, &[0xCD, 0x00, 0x81]),
        (0x8003, &[0x18, 0xFB]),
        (0x8100, &[0x21, 0x00, 0x90]),
        (0x8103, &[0x11, 0x00, 0x40]),
        (0x8106, &[0x01, 0x10, 0x00]),
        (0x8109, &[0xED, 0xB0]),
        (0x810B, &[0xC9]),
    ];
    for (at, bytes) in program {
        for (i, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + i as u16, *byte);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.bus.observer.enabled = true;
    // The interrupt handler is how the observer is told a routine has been
    // entered from nowhere, which is what a main loop is.
    spec.bus
        .observer
        .on_interrupt(0x8000, spec.cpu.sp, Default::default());
    spec
}

/// The copying routine reads sixteen bytes and writes sixteen, per call, and
/// is measured as the code it actually covers.
#[test]
fn a_routine_is_measured_by_what_it_did() {
    let mut spec = machine();
    // Two turns of the loop, so the per-call figures have something to divide.
    for _ in 0..400 {
        spec.step_instruction();
    }

    let copy = measured(spec.bus.observer.routines.get(&0x8100));
    assert!(
        (12..=24).contains(&copy.size),
        "the routine is twelve bytes of code: measured {}",
        copy.size
    );
    // Sixteen each way per call. Not exactly sixteen: the figures are totals
    // divided by calls, and the last call is part way through when they are
    // read, so a turn's worth is spread over one call more than has finished.
    assert!(
        (14..=17).contains(&copy.wrote),
        "sixteen bytes copied into the screen per call: {}",
        copy.wrote
    );
    assert!(
        (14..=18).contains(&copy.read),
        "and sixteen read out of the table to do it: {}",
        copy.read
    );

    // And what main is credited with is what it caused: it has no writes of
    // its own, and saying so would be the wrong thing to say about it.
    let main = spec
        .bus
        .observer
        .routines
        .get(&0x8000)
        .expect("main was watched");
    assert_eq!(
        main.writes.screen, 0,
        "main draws nothing itself — its own writes are the stack pushes a \
         CALL makes"
    );
    assert!(
        main.inclusive.screen > 0 && main.inclusive_reads.total() > 0,
        "but it is credited with the drawing the routine it calls does"
    );
}

/// The three figures are what the row shows, and the wording spells out what
/// they are.
#[test]
fn the_row_says_which_number_is_which() {
    let mut spec = machine();
    for _ in 0..400 {
        spec.step_instruction();
    }
    let copy = measured(spec.bus.observer.routines.get(&0x8100));
    let short = copy.short();
    assert!(
        short.contains('B') && short.contains('w') && short.contains('r'),
        "the row carries all three: {short}"
    );
    assert_eq!(
        short,
        format!("{}B  {}w  {}r", copy.size, copy.wrote, copy.read),
        "in that order: how big it is, what it wrote, what it read"
    );
    let long = copy.long();
    assert!(
        long.contains("bytes of code") && long.contains("writes") && long.contains("reads"),
        "and the hover says which is which: {long}"
    );
}

/// A routine nobody has watched has nothing to say about itself, rather than
/// numbers that look measured.
#[test]
fn an_unwatched_routine_measures_nothing() {
    let nothing = measured(None);
    assert_eq!(
        (nothing.size, nothing.wrote, nothing.read),
        (0, 0, 0),
        "nothing observed, nothing claimed"
    );
}
