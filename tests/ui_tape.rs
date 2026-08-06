//! Tape window behaviour: a freshly loaded tape stays stopped, and the block
//! list keeps the block being played in view.

use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;
use zx_spectrum_emulator::machine::Spectrum;
use zx_spectrum_emulator::tape::{Block, Tape};
use zx_spectrum_emulator::ui::tape::needs_scroll;
use zx_spectrum_emulator::ui::{App, Roms};

fn test_app() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: Some(vec![0x00; 0x8000]),
        rom_plus3: Some(vec![0x00; 0x10000]),
        rom_zx81: Some(vec![0x00; 0x2000]),
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = true;
    app.running = false;
    app
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    // Big enough that the embedded tape window has room for its block list;
    // in the real app it is a separate, resizable OS window.
    Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

fn a_tape_file() -> Option<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir("tapes")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("tzx") | Some("tap")
            )
        })
        .collect();
    v.sort();
    v.into_iter().next()
}

#[test]
fn loading_a_tape_leaves_it_stopped() {
    let Some(path) = a_tape_file() else {
        eprintln!("no tapes/; skipping");
        return;
    };
    let mut app = test_app();
    app.load_path(&path);

    let tape = app.spec.bus.tape.as_ref().expect("tape should be loaded");
    assert!(!tape.playing, "a freshly loaded tape must not start playing");
    assert!(!app.spec.bus.tape_playing());
    assert_eq!(tape.block, 0, "and should be at the start");
    assert!(
        app.status.contains("press Play"),
        "the status should say what to do next: {}",
        app.status
    );
}

#[test]
fn loading_a_tape_does_not_advance_it_over_time() {
    let Some(path) = a_tape_file() else {
        return;
    };
    let mut app = test_app();
    app.load_path(&path);
    app.running = true;

    let mut h = harness_for(app);
    h.run_steps(10);

    let tape = h.state().spec.bus.tape.as_ref().unwrap();
    assert!(!tape.playing, "still stopped after running for a while");
    assert_eq!(tape.pulses, 0, "a stopped tape emits no pulses");
}

#[test]
fn play_on_load_is_off_by_default_but_can_be_turned_on() {
    let Some(path) = a_tape_file() else {
        return;
    };
    let mut app = test_app();
    assert!(!app.tape.auto_play_on_load, "off by default");

    app.tape.auto_play_on_load = true;
    app.load_path(&path);
    assert!(
        app.spec.bus.tape_playing(),
        "with the option on, loading should start playback"
    );
}

#[test]
fn the_block_list_follows_playback() {
    // Following is on by default, and a row that has scrolled out of view is
    // brought back; a visible row is left alone so manual scrolling sticks.
    assert!(needs_scroll(false, true, false), "off-screen row must scroll");
    assert!(!needs_scroll(false, true, true), "visible row must not");
    assert!(needs_scroll(true, false, true), "an explicit request wins");
    assert!(
        !needs_scroll(false, false, false),
        "with following off, nothing is forced"
    );
}

#[test]
fn the_window_marks_the_block_being_played() {
    let mut app = test_app();
    let blocks = (0..40)
        .map(|i| Block::Standard {
            pause_ms: 0,
            data: vec![i as u8; 20],
        })
        .collect();
    let mut tape = Tape::from_blocks("many-blocks.tzx".into(), blocks);
    tape.seek(30);
    app.spec.bus.tape = Some(tape);
    assert!(app.tape.follow_current, "following is on by default");

    let mut h = harness_for(app);
    h.run_steps(3);

    // The current block (31 of 40, one-based in the list) must be on screen.
    let labels = {
        fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
            if let Some(l) = node.accesskit_node().label() {
                out.push(l.to_string());
            }
            for c in node.children() {
                walk(&c, out);
            }
        }
        let mut v = Vec::new();
        walk(&h.root(), &mut v);
        v
    };
    assert!(
        labels.iter().any(|l| l.trim_start().starts_with("31 ")),
        "the playing block should be listed: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.contains("block 31 / 40")),
        "and the progress readout should agree: {labels:?}"
    );
}

/// Collect every label in the accessibility tree.
fn labels(h: &Harness<'_, App>) -> Vec<String> {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        if let Some(l) = node.accesskit_node().label() {
            out.push(l.to_string());
        }
        for c in node.children() {
            walk(&c, out);
        }
    }
    let mut v = Vec::new();
    walk(&h.root(), &mut v);
    v
}

fn has_row(h: &Harness<'_, App>, one_based: usize) -> bool {
    let prefix = format!("{one_based:3}  ");
    labels(h).iter().any(|l| l.starts_with(&prefix))
}

#[test]
fn the_list_scrolls_to_the_block_as_playback_moves_on() {
    let mut app = test_app();
    let blocks = (0..60)
        .map(|i| Block::Standard {
            pause_ms: 0,
            data: vec![i as u8; 16],
        })
        .collect();
    app.spec.bus.tape = Some(Tape::from_blocks("long.tzx".into(), blocks));

    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(has_row(&h, 1), "the whole list is built");

    // The tape moves on to a later block, as it would while loading. The
    // request is per-frame, so check the very next frame.
    h.state_mut().spec.bus.tape.as_mut().unwrap().seek(54);
    h.step();

    assert_eq!(
        h.state().tape.scroll_requested_for,
        Some(54),
        "the list should scroll the newly playing block into view"
    );
    assert_eq!(
        h.state().tape.last_block,
        Some(54),
        "and remember where playback is"
    );
    assert!(has_row(&h, 55), "the row exists: {:?}", labels(&h));
}

#[test]
fn following_can_be_turned_off() {
    let mut app = test_app();
    let blocks = (0..60)
        .map(|i| Block::Standard {
            pause_ms: 0,
            data: vec![i as u8; 16],
        })
        .collect();
    app.spec.bus.tape = Some(Tape::from_blocks("long.tzx".into(), blocks));
    app.tape.follow_current = false;

    let mut h = harness_for(app);
    h.run_steps(3);
    h.state_mut().spec.bus.tape.as_mut().unwrap().seek(54);
    h.step();
    assert_eq!(
        h.state().tape.scroll_requested_for, None,
        "with following off the list should stay where the user left it"
    );
}

// ---------------------------------------------------------------------------
// progress through the current block
// ---------------------------------------------------------------------------

use zx_spectrum_emulator::tape::{DATA_PILOT_PULSES, HEADER_PILOT_PULSES, ONE_PULSE, PILOT_PULSE,
    SYNC1_PULSE, SYNC2_PULSE, ZERO_PULSE};

#[test]
fn a_blocks_length_adds_up() {
    // A header block: the long pilot, two sync pulses, 19 bytes of data and a
    // one-second pause.
    let block = Block::Standard {
        pause_ms: 1000,
        data: vec![0x00; 19],
    };
    let seg = block.segment_times();
    assert_eq!(seg.pilot, HEADER_PILOT_PULSES as u64 * PILOT_PULSE as u64);
    assert_eq!(seg.sync, SYNC1_PULSE as u64 + SYNC2_PULSE as u64);
    assert_eq!(
        seg.data,
        19 * 8 * (ZERO_PULSE as u64 + ONE_PULSE as u64),
        "an even mix of bit lengths"
    );
    assert_eq!(seg.pause, 1000 * 3500);
    assert_eq!(block.duration_t(), seg.total());

    // A data block uses the short pilot, so it is quicker to start.
    let data_block = Block::Standard {
        pause_ms: 0,
        data: vec![0xff; 19],
    };
    assert_eq!(
        data_block.segment_times().pilot,
        DATA_PILOT_PULSES as u64 * PILOT_PULSE as u64
    );
    assert!(data_block.duration_t() < block.duration_t());
}

#[test]
fn block_progress_runs_from_nothing_to_everything() {
    let mut tape = Tape::from_blocks(
        "one block".into(),
        vec![Block::Standard {
            pause_ms: 0,
            data: vec![0x5a; 64],
        }],
    );
    assert_eq!(tape.block_progress(), Some(0.0), "before it starts");

    tape.play(0);
    let mut last = 0.0;
    let mut seen_middle = false;
    let mut t = 0u64;
    let total = tape.blocks[0].duration_t();
    while t < total + 1000 {
        t += total / 200;
        tape.level_at(t);
        let p = tape.block_progress().unwrap_or(1.0);
        assert!(p >= last - 0.001, "progress went backwards: {last} then {p}");
        if (0.4..0.6).contains(&p) {
            seen_middle = true;
        }
        last = p;
    }
    assert!(seen_middle, "should pass through the middle");
    assert!(last > 0.99, "should finish at the end, got {last}");
}

#[test]
fn progress_is_reported_for_each_kind_of_block() {
    // A pure tone is half done after half its pulses.
    let mut tape = Tape::from_blocks(
        "tone".into(),
        vec![Block::PureTone {
            len: 1000,
            count: 100,
        }],
    );
    tape.play(0);
    tape.level_at(50 * 1000);
    let p = tape.block_progress().unwrap();
    assert!((p - 0.5).abs() < 0.05, "halfway through a tone, got {p}");

    // A block that makes no sound has no progress to show.
    let info = Tape::from_blocks("info".into(), vec![Block::Info("hello".into())]);
    assert_eq!(info.block_progress(), None);

    // A pause block is all pause.
    let pause = Tape::from_blocks("pause".into(), vec![Block::Pause(500)]);
    assert_eq!(pause.blocks[0].duration_t(), 500 * 3500);
    assert_eq!(pause.block_progress(), Some(0.0));
}

#[test]
fn seeking_resets_the_block_progress() {
    let mut tape = Tape::from_blocks(
        "two".into(),
        vec![
            Block::Standard {
                pause_ms: 0,
                data: vec![0x00; 32],
            },
            Block::Standard {
                pause_ms: 0,
                data: vec![0xff; 32],
            },
        ],
    );
    tape.play(0);
    tape.level_at(tape.blocks[0].duration_t() / 2);
    assert!(tape.block_progress().unwrap() > 0.1);

    tape.seek(1);
    assert_eq!(
        tape.block_progress(),
        Some(0.0),
        "a fresh block starts from the beginning"
    );
}

#[test]
fn the_window_shows_a_bar_for_the_current_block() {
    let Some(path) = a_tape_file() else {
        return;
    };
    let mut app = test_app();
    app.load_path(&path);
    let mut h = harness_for(app);
    h.run_steps(3);

    let text = labels(&h).join("\n");
    assert!(
        text.contains("block 1 / "),
        "the overall bar should be there: {text}"
    );
    assert!(
        text.contains("0%") || text.contains("left"),
        "and one for the block itself: {text}"
    );
}

// ---- ZX81 tapes ------------------------------------------------------------

fn zx81_app() -> Option<App> {
    let mut app = test_app();
    app.roms.rom_zx81 = Some(std::fs::read("roms/zx81.rom").ok()?);
    app.switch_to_zx81(zx_spectrum_emulator::zx81::Ram::K16);
    app.on_zx81().then_some(app)
}

#[test]
fn a_zx81_program_goes_into_the_zx81s_deck() {
    let Some(mut app) = zx81_app() else {
        eprintln!("no ZX81 ROM; skipping");
        return;
    };
    let path = std::path::Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    if !path.exists() {
        return;
    }
    app.load_path(path);

    assert!(app.tape_ref().is_some(), "no tape in the deck");
    assert!(
        app.spec.bus.tape.is_none(),
        "it went into the Spectrum's deck instead of the ZX81's"
    );
    assert!(!app.tape_is_playing(), "a tape should wait to be played");
    assert!(
        app.status.contains("LOAD"),
        "the status should say how to load it, not {:?}",
        app.status
    );
}

#[test]
fn opening_a_zx81_program_on_a_spectrum_switches_machine() {
    let mut app = test_app();
    let Ok(rom) = std::fs::read("roms/zx81.rom") else {
        return;
    };
    app.roms.rom_zx81 = Some(rom);
    let path = std::path::Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    if !path.exists() {
        return;
    }
    assert!(!app.on_zx81());
    app.load_path(path);
    assert!(app.on_zx81(), "a .p should bring up a ZX81");
    assert!(app.tape_ref().is_some());
}

#[test]
fn the_tape_window_lists_the_zx81_block() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    let path = std::path::Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    if !path.exists() {
        return;
    }
    app.load_path(path);
    let mut harness = harness_for(app);
    harness.run_steps(3);
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        if let Some(l) = node.accesskit_node().label() {
            out.push(l.to_string());
        }
        for c in node.children() {
            walk(&c, out);
        }
    }
    let mut labels = Vec::new();
    walk(&harness.root(), &mut labels);
    let text = labels.join("\n");
    assert!(text.contains("ZX81"), "no ZX81 block in the window:\n{text}");
    assert!(
        text.contains("CHESSQUEEN"),
        "the block should name the program:\n{text}"
    );
}
