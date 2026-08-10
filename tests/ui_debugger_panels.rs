//! The stack beside the registers, the picture beside that, and stopping on
//! what a program does rather than on where it is.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::demo_rom::DEMO_ROM;
use zx_rustrum::machine::{Breaks, Event, Model, Spectrum, Stop, FRAME_T};
use zx_rustrum::ui::{App, Roms};

fn app() -> App {
    let mut spec = Spectrum::new();
    spec.load_rom(&DEMO_ROM);
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_debugger = true;
    app.running = false;
    app
}

fn harness<'a>(app: App) -> Harness<'a, App> {
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(4);
    h
}

/// A write to the display file stops the machine, so "what draws this?" is a
/// question that can be answered without knowing where to put a breakpoint.
#[test]
fn a_write_to_the_screen_can_stop_the_machine() {
    let mut spec = Spectrum::new();
    spec.load_rom(&DEMO_ROM);
    spec.bus.breaks.screen = true;

    let mut stopped = None;
    for _ in 0..80 {
        if let Stop::Watched(event, _) = spec.run(FRAME_T) {
            stopped = Some(event);
            break;
        }
    }

    match stopped {
        Some(Event::Screen(addr)) => assert!(
            (0x4000..0x5B00).contains(&addr),
            "${addr:04X} is not in the display file"
        ),
        other => panic!("the demo ROM draws, but the machine stopped for {other:?}"),
    }
}

/// The frame interrupt is where a game's per-frame work starts, and it is not
/// at an address the program chose, so it is worth a watch of its own.
#[test]
fn the_frame_interrupt_can_stop_the_machine() {
    // Not the demo ROM: it starts with DI and never enables interrupts again,
    // so there would be nothing to catch. An empty machine runs NOPs with
    // interrupts on, which is the state a program waiting for the frame
    // interrupt leaves the CPU in.
    let mut spec = Spectrum::new();
    spec.bus.breaks.interrupt = true;
    spec.cpu.iff1 = true;
    spec.cpu.iff2 = true;
    spec.cpu.im = 1;

    let mut stopped = None;
    for _ in 0..8 {
        if let Stop::Watched(event, _) = spec.run(FRAME_T) {
            stopped = Some(event);
            break;
        }
    }
    assert_eq!(stopped, Some(Event::Interrupt), "no interrupt was caught");
}

/// The beeper is one bit of one port, and the sound chip is two more ports.
/// Both are watched where they are written rather than by disassembling.
#[test]
fn the_beeper_and_the_sound_chip_can_stop_the_machine() {
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    spec.bus.breaks.beeper = true;
    spec.bus.io_write(0x00FE, 0x10);
    assert_eq!(
        spec.bus.break_hit,
        Some(Event::Beeper),
        "flipping the speaker bit went unnoticed"
    );

    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.bus.breaks.ay = true;
    spec.bus.io_write(0xFFFD, 0x07);
    assert_eq!(spec.bus.break_hit, Some(Event::Ay), "AY select went unseen");
}

/// Nothing is watched unless it has been asked for: a watch that is off costs
/// the machine nothing and stops nothing.
#[test]
fn nothing_stops_the_machine_unless_it_is_asked_for() {
    let mut spec = Spectrum::new();
    spec.load_rom(&DEMO_ROM);
    assert_eq!(spec.bus.breaks, Breaks::default());

    for _ in 0..20 {
        match spec.run(FRAME_T) {
            Stop::Watched(event, _) => panic!("stopped for {event:?} with every watch off"),
            _ => continue,
        }
    }
    assert!(!spec.bus.breaks.any());
}

/// Stopping on one of them stops the machine, says why, and brings the
/// debugger forward, the same as reaching a breakpoint does.
#[test]
fn a_watched_stop_says_what_it_was_and_shows_the_debugger() {
    // An empty machine, for the reason above: this one has to reach an
    // interrupt to have anything to report.
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = true;
    app.show_debugger = false;
    app.spec.bus.breaks.interrupt = true;
    app.spec.cpu.iff1 = true;
    app.spec.cpu.iff2 = true;
    app.spec.cpu.im = 1;

    for _ in 0..8 {
        app.advance(1.0 / 60.0);
        if matches!(app.last_stop, Some(Stop::Watched(..))) {
            break;
        }
    }

    assert!(
        matches!(app.last_stop, Some(Stop::Watched(Event::Interrupt, _))),
        "never stopped: {:?}",
        app.last_stop
    );
    assert!(!app.running, "a watched stop should stop the machine");
    assert!(app.show_debugger && app.dbg.raise, "and show the debugger");
    assert!(
        app.status.starts_with("Took the frame interrupt (PC $"),
        "the status should say where it stopped, not {:?}",
        app.status
    );
}

/// The stack is on show beside the registers, from the stack pointer up.
#[test]
fn the_stack_is_shown_from_the_stack_pointer_up() {
    let mut app = app();
    app.spec.cpu.sp = 0x8000;
    app.spec.bus.poke(0x8000, 0x34);
    app.spec.bus.poke(0x8001, 0x12);

    let h = harness(app);
    let text = every_string(&h);
    assert!(text.iter().any(|t| t == "Stack"), "no stack view: {text:?}");
    assert!(
        text.iter()
            .any(|t| t.contains("8000") && t.contains("1234")),
        "the word at the top of the stack is not on show: {text:?}"
    );
}

/// Clicking a sixteen-bit register takes the dump to what it points at, which
/// is what a register mostly is while stepping through code.
#[test]
fn clicking_a_register_moves_the_memory_dump() {
    let mut app = app();
    app.spec.cpu.set_hl(0x7654);
    app.dbg.mem_addr = 0x0000;

    let mut h = harness(app);
    h.get_by_label_contains("HL  7654").click();
    h.run_steps(2);

    assert_eq!(h.state().dbg.mem_addr, 0x7654, "HL did not move the dump");
    assert_eq!(h.state().dbg.mem_text, "7654", "nor its address box");
}

/// And so does clicking an entry on the stack: a return address is a place to
/// go and look at.
#[test]
fn clicking_the_stack_moves_the_memory_dump() {
    let mut app = app();
    app.spec.cpu.sp = 0x8000;
    app.spec.bus.poke(0x8000, 0xCD);
    app.spec.bus.poke(0x8001, 0xAB);
    app.dbg.mem_addr = 0x0000;

    let mut h = harness(app);
    h.get_by_label_contains("8000  ABCD").click();
    h.run_steps(2);

    assert_eq!(
        h.state().dbg.mem_addr,
        0xABCD,
        "the stack entry did nothing"
    );
}

/// The four watches are offered as one row of single words, under a heading
/// that says what they are.
#[test]
fn the_break_toggles_are_one_word_each_under_one_heading() {
    let h = harness(app());
    let text = every_string(&h);
    // The heading is set in the case the other group headings use.
    for word in ["BREAK", "Screen", "Beeper", "Interrupt"] {
        assert!(text.iter().any(|t| t == word), "no {word} toggle: {text:?}");
    }
}

fn every_string(h: &Harness<'_, App>) -> Vec<String> {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        for text in [node.accesskit_node().label(), node.accesskit_node().value()]
            .into_iter()
            .flatten()
        {
            out.push(text.to_string());
        }
        for child in node.children() {
            walk(&child, out);
        }
    }
    let mut found = Vec::new();
    walk(&h.root(), &mut found);
    found
}

/// An interrupt watch stops with the handler still to run, not after its first
/// instruction. The interrupt is accepted between instructions, so this one
/// really can be caught at the moment it fires.
///
/// The handler has to do something visible for the test to mean anything: an
/// empty machine reads $FF everywhere, which is RST $38, so it sits in a loop
/// where before and after the first instruction look exactly the same.
#[test]
fn an_interrupt_stops_before_the_handler_runs() {
    let mut rom = vec![0u8; 0x4000];
    rom[0x0000] = 0xFB; // EI
    rom[0x0001] = 0x18; // JR -2: wait here for the frame interrupt
    rom[0x0002] = 0xFE;
    rom[0x0038] = 0x3C; // INC A, the first thing the handler does
    rom[0x0039] = 0xED; // RETI
    rom[0x003A] = 0x4D;

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.cpu.im = 1;
    spec.bus.breaks.interrupt = true;
    // A is $FF out of reset, which INC A would wrap to zero; set it so the
    // check below reads as what it means.
    spec.cpu.a = 0;

    let mut stopped = None;
    for _ in 0..8 {
        if let Stop::Watched(event, at) = spec.run(FRAME_T) {
            stopped = Some((event, at));
            break;
        }
    }

    let (event, at) = stopped.expect("no interrupt was caught");
    assert_eq!(event, Event::Interrupt);
    assert_eq!(
        at, 0x0038,
        "mode 1 vectors to $0038; stopped at ${at:04X} instead"
    );
    assert_eq!(
        spec.cpu.pc, 0x0038,
        "the handler should still be waiting to run"
    );
    assert_eq!(
        spec.cpu.a, 0,
        "the handler's INC A has already run, so the machine stopped too late"
    );
}

/// A write or an OUT happens part-way through an instruction, and the CPU is
/// only stoppable between them, so the machine stops at the end of the one
/// that did it — and says which one that was, rather than leaving the user
/// looking at the instruction after.
#[test]
fn a_write_says_which_instruction_did_it() {
    let mut spec = Spectrum::new();
    // LD HL,$4000 : LD (HL),A — the write is the second instruction. In RAM
    // at $8000, because a poke into the ROM area does not stick.
    for (offset, byte) in [
        (0u16, 0x21u8),
        (1, 0x00),
        (2, 0x40),
        (3, 0x77),
        (4, 0x76), // HALT, so nothing else happens
    ] {
        spec.bus.poke(0x8000 + offset, byte);
    }
    spec.cpu.pc = 0x8000;
    spec.bus.breaks.screen = true;

    match spec.run(FRAME_T) {
        Stop::Watched(Event::Screen(addr), at) => {
            assert_eq!(addr, 0x4000, "wrote to ${addr:04X}");
            assert_eq!(at, 0x8003, "LD (HL),A is at $8003, not ${at:04X}");
            assert_eq!(
                spec.cpu.pc, 0x8004,
                "the instruction that wrote has finished, so PC is past it"
            );
        }
        other => panic!("expected a screen write, got {other:?}"),
    }
}

/// The flags are under the registers, in the same panel, not off beside them.
/// A frame takes the layout of the `Ui` it is shown in, and this one is shown
/// in a row: the flags were being laid out to the right of the register grid
/// and over the top of the stack.
#[test]
fn the_flags_are_below_the_registers() {
    use zx_rustrum::ui::debugger;

    let mut h = Harness::builder()
        .with_size([debugger::WINDOW_W, 900.0])
        .build_ui_state(|ui, app: &mut App| debugger::ui(app, ui), app());
    h.run_steps(4);

    let flags = h.get_by_label_contains("Flags:").rect();
    let last_register = h.get_by_label_contains("HL  ").rect();
    assert!(
        flags.min.y > last_register.min.y,
        "the flags are at y {} and the last register row at y {}",
        flags.min.y,
        last_register.min.y
    );
    assert!(
        flags.min.x < last_register.max.x + 8.0,
        "the flags start at x {}, away to the right of the registers",
        flags.min.x
    );
}

/// AutoDoc is off until it is switched on, and switching it on names the
/// routines being called.
#[test]
fn the_autodoc_toggle_names_routines() {
    let mut app = app();
    // A screen clear at $8000, called from $9000, where the listing is put.
    for (offset, byte) in [
        (0u16, 0x21u8),
        (1, 0x00),
        (2, 0x40),
        (3, 0x11),
        (4, 0x01),
        (5, 0x40),
        (6, 0x01),
        (7, 0x00),
        (8, 0x18),
        (9, 0x36),
        (10, 0x00),
        (11, 0xED),
        (12, 0xB0),
        (13, 0xC9),
    ] {
        app.spec.bus.poke(0x8000 + offset, byte);
    }
    for (offset, byte) in [(0u16, 0xCDu8), (1, 0x00), (2, 0x80), (3, 0xC9)] {
        app.spec.bus.poke(0x9000 + offset, byte);
    }
    app.dbg.follow_pc = false;
    app.dbg.view_addr = 0x9000;

    let mut h = harness(app);
    assert!(
        h.state().dbg.doc.is_empty(),
        "nothing should be guessed at until it is asked for"
    );

    h.get_by_label("AutoDoc").click();
    h.run_steps(3);

    let doc = &h.state().dbg.doc;
    assert_eq!(
        doc.label(0x8000),
        "clear_screen_8000",
        "the routine called from the listing was not named: {doc:?}"
    );
}

/// Guesses are kept with the rest of the notes, marked so they can be told
/// from anything the user wrote.
#[test]
fn guesses_go_into_the_notes_marked_as_guesses() {
    let mut app = app();
    app.dbg.autodoc = true;
    // A call, so there is a routine to name: the address being looked at is
    // not one, only what it calls.
    for (offset, byte) in [(0u16, 0xCDu8), (1, 0x00), (2, 0xA0), (3, 0xC9)] {
        app.spec.bus.poke(0x9000 + offset, byte);
    }
    app.spec.bus.poke(0xA000, 0xC9);
    app.dbg.follow_pc = false;
    app.dbg.view_addr = 0x9000;

    let mut h = harness(app);
    h.run_steps(3);

    let notes = &h.state().notes;
    assert!(!notes.is_empty(), "nothing was written down at all");
    assert!(
        notes.label_is_auto(0xA000),
        "the guess at $A000 is not marked as one: {:?}",
        notes.label(0xA000)
    );
    assert!(
        notes.to_text().contains("@"),
        "the file should mark guesses:\n{}",
        notes.to_text()
    );
}

/// A guess never replaces a line somebody wrote themselves, however good the
/// guess is.
#[test]
fn a_guess_never_replaces_what_the_user_wrote() {
    let mut app = app();
    for (offset, byte) in [(0u16, 0xCDu8), (1, 0x00), (2, 0xA0), (3, 0xC9)] {
        app.spec.bus.poke(0x9000 + offset, byte);
    }
    app.spec.bus.poke(0xA000, 0xC9);
    app.notes.set_label(0xA000, "my_own_name");
    app.notes.set_comment(0xA000, "my own words");
    app.dbg.autodoc = true;
    app.dbg.follow_pc = false;
    app.dbg.view_addr = 0x9000;

    let mut h = harness(app);
    h.run_steps(3);

    let notes = &h.state().notes;
    assert_eq!(notes.label(0xA000), "my_own_name");
    assert_eq!(notes.comment(0xA000), "my own words");
    assert!(
        !notes.label_is_auto(0xA000) && !notes.comment_is_auto(0xA000),
        "the user's own line was marked as a guess"
    );
}

/// The list of names is there to be clicked, and clicking one takes the
/// listing to it.
#[test]
fn clicking_a_label_takes_the_listing_to_it() {
    let mut app = app();
    app.notes.set_label(0xABCD, "the_place");
    app.dbg.view_addr = 0x0000;
    app.dbg.follow_pc = true;

    let mut h = harness(app);
    h.get_by_label_contains("ABCD the_place").click();
    h.run_steps(2);

    assert_eq!(h.state().dbg.view_addr, 0xABCD, "the listing did not move");
    assert!(
        !h.state().dbg.follow_pc,
        "and it should stay there rather than snapping back to PC"
    );
}
