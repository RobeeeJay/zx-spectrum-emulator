//! Stepping back over instructions that were stepped by hand.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms, REWIND};

fn machine(program: &[(u16, &[u8])]) -> Spectrum {
    let mut spec = Spectrum::new();
    for (at, bytes) in program {
        for (offset, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + offset as u16, *byte);
        }
    }
    spec.cpu.pc = program[0].0;
    spec.cpu.sp = 0xFF00;
    spec
}

fn app(spec: Spectrum) -> App {
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

/// An instruction is undone by putting back what it changed: the registers it
/// touched and the bytes it wrote. A copy of the machine each step would be
/// sixty-four kilobytes of RAM, the tape, the audio and everything the
/// observer has watched, twenty times over.
#[test]
fn stepping_back_puts_the_registers_and_the_memory_back() {
    // LD A,$55 : LD ($9000),A : INC A : LD ($9001),A
    let mut app = app(machine(&[(
        0x8000,
        &[0x3E, 0x55, 0x32, 0x00, 0x90, 0x3C, 0x32, 0x01, 0x90],
    )]));

    for _ in 0..4 {
        app.step_machine();
    }
    assert_eq!(app.cpu().pc, 0x8009, "four instructions in");
    assert_eq!(app.peek(0x9000), 0x55);
    assert_eq!(app.peek(0x9001), 0x56);
    assert_eq!(app.cpu().a, 0x56);

    // Back over the last write.
    app.step_back();
    assert_eq!(app.cpu().pc, 0x8006, "the PC should be back at the LD");
    assert_eq!(
        app.peek(0x9001),
        0x00,
        "and the byte it wrote should be what was there before"
    );
    assert_eq!(app.peek(0x9000), 0x55, "without disturbing the other one");

    // Back over the INC: the register goes with it.
    app.step_back();
    assert_eq!(app.cpu().a, 0x55, "A should be what it was before the INC");

    // And all the way to where it started.
    app.step_back();
    app.step_back();
    assert_eq!(app.cpu().pc, 0x8000);
    assert_eq!(app.peek(0x9000), 0x00, "nothing it wrote should remain");
    assert!(!app.can_step_back(), "and there is nothing left to undo");
}

/// Only what was stepped by hand can be stepped back over: a running machine
/// writes millions of bytes a second, and keeping them would cost the running
/// machine something to buy what nobody is going to use.
#[test]
fn only_hand_stepped_instructions_can_be_undone() {
    let mut app = app(machine(&[(0x8000, &[0x3C, 0x18, 0xFD])]));
    assert!(!app.can_step_back(), "nothing has been stepped");

    app.advance(1.0 / 50.0);
    assert!(
        !app.can_step_back(),
        "a frame of running should not have filled the rewind"
    );

    app.step_machine();
    assert!(app.can_step_back(), "and a hand step should");
}

/// Twenty of them, and the oldest goes when the twenty-first arrives: enough
/// to see how the machine got where it is without keeping a recording.
#[test]
fn the_rewind_keeps_the_last_twenty() {
    // NOPs, so every step is the same and only the count is being tested.
    let mut app = app(machine(&[(0x8000, &[0x00; 64])]));

    for _ in 0..REWIND + 12 {
        app.step_machine();
    }
    assert_eq!(app.rewind.len(), REWIND, "no more than twenty are kept");

    let pc = app.cpu().pc;
    for _ in 0..REWIND {
        app.step_back();
    }
    assert_eq!(
        app.cpu().pc,
        pc - REWIND as u16,
        "twenty NOPs back from where it was"
    );
    assert!(
        !app.can_step_back(),
        "and the twelve before those are gone, not wrong"
    );
}

/// Running the machine throws away what could have been stepped back over.
///
/// A step over a CALL is thousands of instructions, and what was kept from
/// before it describes a machine that no longer exists. Stepping back into
/// that would put the registers somewhere plausible and leave the memory
/// wrong, which is worse than not offering it at all.
#[test]
fn running_over_a_call_gives_up_the_rewind() {
    // LD A,$01 : CALL $9000 : NOP, with a subroutine that writes and returns.
    let mut app = app(machine(&[
        (0x8000, &[0x3E, 0x01, 0xCD, 0x00, 0x90, 0x00]),
        (0x9000, &[0x3E, 0x99, 0x32, 0x00, 0xA0, 0xC9]),
    ]));

    app.step_machine();
    assert!(app.can_step_back(), "the LD was stepped by hand");

    app.step_over();
    assert_eq!(app.cpu().pc, 0x8005, "it should be past the call");
    assert_eq!(app.peek(0xA000), 0x99, "which ran the subroutine");
    assert!(
        !app.can_step_back(),
        "and what was kept from before the call cannot undo what it did"
    );
}
