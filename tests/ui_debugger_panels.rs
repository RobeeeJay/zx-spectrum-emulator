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
        if let Stop::Watched(event) = spec.run(FRAME_T) {
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
        if let Stop::Watched(event) = spec.run(FRAME_T) {
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
            Stop::Watched(event) => panic!("stopped for {event:?} with every watch off"),
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
        if matches!(app.last_stop, Some(Stop::Watched(_))) {
            break;
        }
    }

    assert!(
        matches!(app.last_stop, Some(Stop::Watched(Event::Interrupt))),
        "never stopped: {:?}",
        app.last_stop
    );
    assert!(!app.running, "a watched stop should stop the machine");
    assert!(app.show_debugger && app.dbg.raise, "and show the debugger");
    assert_eq!(app.status, "Took the frame interrupt");
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
