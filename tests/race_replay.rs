//! Racing the beam by replaying the frame from its interrupt.

use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::race::Race;

/// A machine running a program that draws and rubs out within every frame.
///
/// It waits for the interrupt, writes $FF to a byte of the display file, waits
/// again and puts it back — so what is at that address depends on how far into
/// the frame you look, which is the whole question racing the beam asks.
fn drawing_machine() -> Spectrum {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    // A ROM of RETs: the interrupt at $0038 returns at once, so the frame
    // belongs to the program rather than to somebody else's handler.
    spec.bus.rom.iter_mut().for_each(|b| *b = 0xC9);

    let program: &[u8] = &[
        0xFB, // EI
        0x76, // HALT — the frame starts here
        0x06, 0x40, // LD B,$40
        0x10, 0xFE, // DJNZ -2      — a while into the frame
        0x3E, 0xFF, // LD A,$FF
        0x32, 0x00, 0x41, // LD ($4100),A
        0x06, 0xC0, // LD B,$C0
        0x10, 0xFE, // DJNZ -2      — a good while longer
        0xAF, // XOR A
        0x32, 0x00, 0x41, // LD ($4100),A
        0x18, 0xEC, // JR back to the EI
    ];
    for (offset, byte) in program.iter().enumerate() {
        spec.bus.poke(0x8000 + offset as u16, *byte);
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.im = 1;
    // Part-way through a frame, as a machine stopped by hand would be.
    for _ in 0..500 {
        spec.step_instruction();
    }
    spec
}

/// The same snapshot the race takes: the machine at the next interrupt.
fn at_next_interrupt(spec: &Spectrum) -> Spectrum {
    let mut copy = spec.clone();
    let frame = copy.bus.frame;
    while copy.bus.frame == frame {
        copy.step_instruction();
    }
    copy
}

fn run_to(spec: &mut Spectrum, t: u32) {
    let frame = spec.bus.frame;
    while spec.bus.tstates < t && spec.bus.frame == frame {
        spec.step_instruction();
    }
}

/// What the cursor shows is the machine as it was when the beam got there:
/// every instruction up to that T-state executed and none after it. Nothing
/// less will do — reading the display file as it stands shows writes the ULA
/// has not put out, and reading what it painted shows nothing of the registers
/// or the border at that moment.
#[test]
fn the_replay_is_the_machine_run_to_that_moment() {
    let spec = drawing_machine();
    let mut race = Race::start(&spec);

    for t in [1_000u32, 8_000, 30_000, 60_000] {
        let mut reference = at_next_interrupt(&spec);
        run_to(&mut reference, t);
        let raced = race.at(t);
        assert_eq!(
            raced.cpu.pc, reference.cpu.pc,
            "at T {t} the replay is at ${:04X} and the machine was at ${:04X}",
            raced.cpu.pc, reference.cpu.pc
        );
        assert_eq!(
            raced.bus.tstates, reference.bus.tstates,
            "at T {t} the replay had run {} T-states and the machine {}",
            raced.bus.tstates, reference.bus.tstates
        );
        assert!(
            raced.bus.ram == reference.bus.ram,
            "at T {t} the memory differs between the replay and the machine"
        );
    }
}

/// The picture is different at different points of the frame, which is the
/// point of dragging the cursor down it.
#[test]
fn what_is_drawn_depends_on_where_in_the_frame_you_look() {
    let spec = drawing_machine();
    let mut race = Race::start(&spec);

    // Before the program has written anything this frame.
    assert_eq!(
        race.at(200).bus.video(0x0100),
        0x00,
        "nothing has been drawn this early in the frame"
    );
    // After the write and before the rubbing out.
    assert_eq!(
        race.at(1_400).bus.video(0x0100),
        0xFF,
        "the program has drawn it by here"
    );
    // And after it has been put back.
    assert_eq!(
        race.at(6_000).bus.video(0x0100),
        0x00,
        "and rubbed it out again by here"
    );
}

/// Dragging the cursor back up cannot un-execute anything, so the frame is
/// started again from the snapshot. What comes back must be the same as if it
/// had never been run past that point.
#[test]
fn going_back_up_the_screen_starts_the_frame_again() {
    let spec = drawing_machine();

    let mut straight = Race::start(&spec);
    let expected = straight.at(3_000).clone();

    let mut wandering = Race::start(&spec);
    wandering.at(60_000);
    let back = wandering.at(3_000);

    assert_eq!(
        back.cpu.pc, expected.cpu.pc,
        "after going down and back up the replay is at ${:04X}, not ${:04X}",
        back.cpu.pc, expected.cpu.pc
    );
    assert!(
        back.bus.ram == expected.bus.ram,
        "the memory differs after going down the screen and back up"
    );
}

/// The machine being looked at is stopped, and looking at it must not start
/// it: the frame is replayed on a copy.
#[test]
fn the_machine_being_looked_at_does_not_move() {
    let spec = drawing_machine();
    let (pc, frame, t) = (spec.cpu.pc, spec.bus.frame, spec.bus.tstates);

    let mut race = Race::start(&spec);
    race.at(20_000);
    race.at(60_000);
    race.at(1_000);

    assert_eq!(spec.cpu.pc, pc, "the machine executed something");
    assert_eq!(
        (spec.bus.frame, spec.bus.tstates),
        (frame, t),
        "the machine's clock moved on"
    );
    assert!(
        race.frame() > frame,
        "the frame raced should be the one after the machine's, not its own \
         half-finished one"
    );
}

/// A copy of the machine holds the same sound queue as the machine it came
/// from, and would play a frame of its own over the top of it every time the
/// cursor moved.
#[test]
fn the_replay_is_silent() {
    use std::sync::{Arc, Mutex};

    let mut spec = drawing_machine();
    let queue: zx_rustrum::audio::SharedQueue = Arc::new(Mutex::new(Default::default()));
    spec.bus.audio.attach(queue.clone(), 44_100.0);

    let mut race = Race::start(&spec);
    race.at(60_000);

    assert_eq!(
        queue.lock().unwrap().len(),
        0,
        "the replay put samples into the machine's sound queue"
    );
}
