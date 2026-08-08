//! Profiler: time attribution against hand-counted T-states, plus the runs
//! list and the bar graph in the window.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::profiler::{format_duration, Metric};
use zx_rustrum::ui::{App, Roms};

/// A program with two leaf functions of known cost, called in a loop.
///
/// ```text
/// 0000: LD SP,$C000     ; pushes land in bank 2, which is never contended,
///                     ; so the T-state counts below are exact
/// 0003: CALL $0010      ; 17T
/// 0006: CALL $0020      ; 17T
/// 0009: JR $0003        ; 12T
/// 0010: NOP x5, RET     ; 20T + 10T = 30T inclusive
/// 0020: NOP x10, RET    ; 40T + 10T = 50T inclusive
/// ```
fn looping_rom() -> Vec<u8> {
    let mut rom = vec![0x00; 0x4000];
    rom[0x0000..0x0003].copy_from_slice(&[0x31, 0x00, 0xc0]); // LD SP,$C000
    rom[0x0003..0x0006].copy_from_slice(&[0xcd, 0x10, 0x00]); // CALL $0010
    rom[0x0006..0x0009].copy_from_slice(&[0xcd, 0x20, 0x00]); // CALL $0020
    rom[0x0009..0x000b].copy_from_slice(&[0x18, 0xf8]); // JR $0003
    rom[0x0015] = 0xc9; // RET, after five NOPs from $0010
    rom[0x002a] = 0xc9; // RET, after ten NOPs from $0020
    rom
}

const F1: u16 = 0x0010;
const F2: u16 = 0x0020;

fn machine() -> Spectrum {
    let mut spec = Spectrum::new();
    spec.load_rom(&looping_rom());
    spec
}

/// Stop the run with the CPU back at the top of the loop and nothing on the
/// call stack, so every call recorded has also returned and the totals are
/// exact multiples of the hand-counted costs.
fn stop_cleanly(spec: &mut Spectrum, loop_top: u16) {
    for _ in 0..10_000 {
        if spec.cpu.pc == loop_top && spec.profiler.depth() == 0 {
            break;
        }
        spec.step_instruction();
    }
    let now = spec.bus.total_t();
    spec.profiler.stop(now);
    assert_eq!(spec.profiler.depth(), 0, "test wanted a clean stop");
}

#[test]
fn leaf_functions_get_their_hand_counted_time() {
    let mut spec = machine();
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    for _ in 0..4 {
        spec.run(FRAME_T);
    }
    stop_cleanly(&mut spec, 0x0003);

    let run = spec.profiler.runs.last().unwrap();
    assert_eq!(run.unfinished, 0, "every call completed");
    let f1 = run.funcs[&F1];
    let f2 = run.funcs[&F2];

    assert!(f1.calls > 100, "only {} calls recorded", f1.calls);
    assert_eq!(f1.calls, f2.calls, "both are called once per loop");

    // Five NOPs and a RET is 30 T-states; ten NOPs and a RET is 50.
    assert_eq!(f1.incl_t, f1.calls * 30, "$0010 inclusive");
    assert_eq!(f2.incl_t, f2.calls * 50, "$0020 inclusive");
    // Neither calls anything, so self time is the same.
    assert_eq!(f1.self_t, f1.incl_t);
    assert_eq!(f2.self_t, f2.incl_t);
    assert_eq!(f1.max_depth, 1, "no nesting");
}

#[test]
fn the_ranking_puts_the_biggest_first() {
    let mut spec = machine();
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    for _ in 0..4 {
        spec.run(FRAME_T);
    }
    spec.profiler.stop(spec.bus.total_t());

    let run = spec.profiler.runs.last().unwrap();
    for metric in [Metric::SelfTime, Metric::Inclusive] {
        let ranked = run.ranked(metric);
        assert_eq!(ranked[0].entry, F2, "$0020 costs more, so it leads");
        assert_eq!(ranked[1].entry, F1);
        assert!(
            ranked[0].time(metric) >= ranked[1].time(metric),
            "not sorted by {}",
            metric.label()
        );
    }
}

#[test]
fn nested_calls_split_self_from_inclusive() {
    // $0100 calls $0200, which burns 10 NOPs and returns.
    //   inner: 10 NOPs + RET            = 50T inclusive
    //   outer: NOP + CALL + NOP + RET   = 4 + 17 + 4 + 10 = 35T self
    //          plus the inner call                        = 85T inclusive
    let mut rom = vec![0x00; 0x4000];
    rom[0x0000..0x0003].copy_from_slice(&[0x31, 0x00, 0xc0]); // LD SP,$C000
    rom[0x0003..0x0006].copy_from_slice(&[0xcd, 0x00, 0x01]); // CALL $0100
    rom[0x0006..0x0008].copy_from_slice(&[0x18, 0xfb]); // JR $0003
    rom[0x0100] = 0x00; // NOP
    rom[0x0101..0x0104].copy_from_slice(&[0xcd, 0x00, 0x02]); // CALL $0200
    rom[0x0104] = 0x00; // NOP
    rom[0x0105] = 0xc9; // RET
    rom[0x020a] = 0xc9; // RET, after ten NOPs from $0200

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    for _ in 0..4 {
        spec.run(FRAME_T);
    }
    stop_cleanly(&mut spec, 0x0003);

    let run = spec.profiler.runs.last().unwrap();
    assert_eq!(run.unfinished, 0, "every call completed");
    let outer = run.funcs[&0x0100];
    let inner = run.funcs[&0x0200];
    assert_eq!(outer.calls, inner.calls);
    assert_eq!(inner.incl_t, inner.calls * 50, "inner inclusive");
    assert_eq!(outer.self_t, outer.calls * 35, "outer self time");
    assert_eq!(outer.incl_t, outer.calls * 85, "outer inclusive");
    // Self time plus what it called is the inclusive time.
    assert_eq!(outer.incl_t, outer.self_t + inner.incl_t);
}

#[test]
fn interrupt_handlers_are_profiled_as_calls() {
    // EI, then loop. The ULA interrupt should show up as calls to $0038.
    let mut rom = vec![0x00; 0x4000];
    rom[0x0000..0x0003].copy_from_slice(&[0x31, 0x00, 0xc0]); // LD SP,$C000
    rom[0x0003] = 0xed; // IM 1
    rom[0x0004] = 0x56;
    rom[0x0005] = 0xfb; // EI
    rom[0x0006..0x0008].copy_from_slice(&[0x18, 0xfe]); // JR $0006 (spin)
    rom[0x0038] = 0xfb; // EI
    rom[0x0039] = 0xc9; // RET

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    for _ in 0..10 {
        spec.run(FRAME_T);
    }
    spec.profiler.stop(spec.bus.total_t());

    let run = spec.profiler.runs.last().unwrap();
    let handler = run
        .funcs
        .get(&0x0038)
        .expect("the interrupt handler should have been profiled");
    assert!(
        (8..=11).contains(&handler.calls),
        "expected about one interrupt per frame, got {}",
        handler.calls
    );
    // EI plus RET is 14 T-states inside the handler.
    assert_eq!(handler.self_t, handler.calls * 14);
}

#[test]
fn a_function_that_never_returns_still_gets_its_time() {
    // $0010 loops forever, so it is on the stack when the run stops.
    let mut rom = vec![0x00; 0x4000];
    rom[0x0000..0x0003].copy_from_slice(&[0x31, 0x00, 0xc0]);
    rom[0x0003..0x0006].copy_from_slice(&[0xcd, 0x10, 0x00]); // CALL $0010
    rom[0x0010..0x0012].copy_from_slice(&[0x18, 0xfe]); // JR $0010

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    for _ in 0..4 {
        spec.run(FRAME_T);
    }
    let elapsed = spec.bus.total_t();
    spec.profiler.stop(spec.bus.total_t());

    let run = spec.profiler.runs.last().unwrap();
    assert_eq!(run.unfinished, 1, "one frame was still open");
    let f = run.funcs[&0x0010];
    assert_eq!(f.calls, 1);
    // It owns nearly the whole run: everything bar the three instructions
    // before the call.
    assert!(
        f.self_t > elapsed - 100,
        "only got {} of {elapsed} T-states",
        f.self_t
    );
}

#[test]
fn a_run_records_when_it_started_and_how_long_it_lasted() {
    let mut spec = machine();
    assert!(spec.profiler.runs.is_empty());

    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    assert!(spec.profiler.running);
    assert_eq!(spec.profiler.runs.len(), 1, "starting adds an entry");
    assert!(spec.profiler.runs[0].wall.is_none(), "no duration yet");
    // The label is a local date and time, e.g. "2026-07-30 09:41:12".
    let label = spec.profiler.runs[0].started_label.clone();
    assert_eq!(label.len(), 19, "unexpected timestamp {label:?}");
    assert_eq!(&label[4..5], "-");
    assert_eq!(&label[10..11], " ");
    assert_eq!(&label[13..14], ":");

    for _ in 0..4 {
        spec.run(FRAME_T);
    }
    spec.profiler.stop(spec.bus.total_t());

    assert!(!spec.profiler.running);
    let run = &spec.profiler.runs[0];
    assert!(run.wall.is_some(), "stopping fills in the running time");
    // Four frames' worth, give or take the instruction the last frame ended on.
    let four_frames = 4 * FRAME_T as u64;
    assert!(
        (four_frames..four_frames + 200).contains(&run.emulated_t),
        "emulated time was {}",
        run.emulated_t
    );
    assert!(run.instructions > 1000);

    // Starting again adds a second entry rather than replacing the first.
    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    assert_eq!(spec.profiler.runs.len(), 2);
    assert_eq!(spec.profiler.selected, Some(1));
}

#[test]
fn nothing_is_recorded_while_stopped() {
    let mut spec = machine();
    for _ in 0..2 {
        spec.run(FRAME_T);
    }
    assert!(spec.profiler.runs.is_empty(), "no run, no data");

    spec.profiler.start(spec.bus.total_t(), 3_500_000.0);
    spec.run(FRAME_T);
    spec.profiler.stop(spec.bus.total_t());
    let first = spec.profiler.runs[0].instructions;

    for _ in 0..2 {
        spec.run(FRAME_T);
    }
    assert_eq!(
        spec.profiler.runs[0].instructions, first,
        "a stopped run must not keep accumulating"
    );
}

#[test]
fn durations_are_formatted_for_people() {
    assert_eq!(format_duration(0.0123), "12 ms");
    assert_eq!(format_duration(1.5), "1.50 s");
    assert_eq!(format_duration(90.0), "1 m 30.0 s");
}

// ---------------------------------------------------------------------------
// the window
// ---------------------------------------------------------------------------

fn app() -> App {
    let mut app = App::with_roms(machine(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_profiler = true;
    app
}

fn harness<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1400.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

#[test]
fn the_window_starts_and_stops_a_run() {
    let mut h = harness(app());
    h.run_steps(3);
    assert!(h.state().spec.profiler.runs.is_empty());

    h.get_by_label("● Start").click();
    h.run_steps(3);
    assert!(h.state().spec.profiler.running, "Start should begin a run");
    assert_eq!(h.state().spec.profiler.runs.len(), 1);

    // Let the emulator run so there is something to measure.
    h.state_mut().running = true;
    h.run_steps(5);

    h.get_by_label("■ Stop").click();
    h.run_steps(3);
    let p = &h.state().spec.profiler;
    assert!(!p.running, "Stop should end the run");
    assert!(p.runs[0].wall.is_some(), "and record how long it took");
    assert!(
        p.runs[0].funcs.contains_key(&F1),
        "and have collected functions"
    );
}

#[test]
fn clicking_a_bar_opens_the_disassembly_of_that_function() {
    let mut app = app();
    app.spec.profiler.start(app.spec.bus.total_t(), 3_500_000.0);
    for _ in 0..4 {
        app.spec.run(FRAME_T);
    }
    let now = app.spec.bus.total_t();
    app.spec.profiler.stop(now);

    let mut h = harness(app);
    h.run_steps(3);
    assert!(!h.state().show_debugger);

    // The hottest function is $0020, so its bar is the first one.
    h.get_by_label("$0020").click();
    h.run_steps(3);

    let state = h.state();
    assert!(state.show_debugger, "the debugger window should open");
    assert_eq!(state.dbg.view_addr, F2, "showing that function");
    assert!(!state.dbg.follow_pc, "and not chasing the PC");
}

#[test]
fn the_run_list_shows_the_start_time_and_can_be_reselected() {
    let mut app = app();
    for _ in 0..2 {
        let now = app.spec.bus.total_t();
        app.spec.profiler.start(now, 3_500_000.0);
        app.spec.run(FRAME_T);
        let now = app.spec.bus.total_t();
        app.spec.profiler.stop(now);
    }
    let first_label = app.spec.profiler.runs[0].started_label.clone();

    let mut h = harness(app);
    h.run_steps(3);
    assert_eq!(h.state().spec.profiler.selected, Some(1), "newest selected");

    // Rows are labelled with their index and start time.
    h.get_by_label_contains(&format!("1  {first_label}"))
        .click();
    h.run_steps(3);
    assert_eq!(h.state().spec.profiler.selected, Some(0));
}
