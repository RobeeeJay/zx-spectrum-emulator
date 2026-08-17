//! Tape window behaviour: a freshly loaded tape stays stopped, and the block
//! list keeps the block being played in view.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::tape::{Block, Tape};
use zx_rustrum::ui::tape::needs_scroll;
use zx_rustrum::ui::{App, Roms};

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
    assert!(
        !tape.playing,
        "a freshly loaded tape must not start playing"
    );
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
fn a_tape_never_starts_itself() {
    // Loading a tape puts it in the deck and nothing more, as with a real one.
    let Some(path) = a_tape_file() else {
        return;
    };
    let mut app = test_app();
    app.load_path(&path);
    assert!(
        !app.spec.bus.tape_playing(),
        "loading a tape must not start it"
    );
    assert!(
        app.status.contains("press Play"),
        "and the status should say so: {}",
        app.status
    );
}

#[test]
fn the_block_list_follows_playback() {
    // A row that has scrolled out of view is brought back; a visible one is
    // left alone, so scrolling by hand sticks while the tape stays put.
    assert!(needs_scroll(false, false), "off-screen row must scroll");
    assert!(!needs_scroll(false, true), "visible row must not");
    assert!(needs_scroll(true, true), "an explicit request wins");
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
        !labels.iter().any(|l| l.contains("block 31 / 40")),
        "how far through the tape is shown on the block's own row now, not in \
         a readout of its own: {labels:?}"
    );
}

/// Collect every label in the accessibility tree.
/// Every piece of text on screen. egui puts a plain label's text in the
/// accessibility node's value rather than its label, and a progress bar's
/// caption likewise, so both are collected.
fn labels(h: &Harness<'_, App>) -> Vec<String> {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let node_ref = node.accesskit_node();
        if let Some(l) = node_ref.label() {
            out.push(l.to_string());
        }
        if let Some(v) = node_ref.value() {
            out.push(v.to_string());
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

// ---------------------------------------------------------------------------
// progress through the current block
// ---------------------------------------------------------------------------

use zx_rustrum::tape::{
    DATA_PILOT_PULSES, HEADER_PILOT_PULSES, ONE_PULSE, PILOT_PULSE, SYNC1_PULSE, SYNC2_PULSE,
    ZERO_PULSE,
};

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
        assert!(
            p >= last - 0.001,
            "progress went backwards: {last} then {p}"
        );
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

    // A block that makes no sound takes no time, so there is never any of it
    // left to play. It reads as finished rather than as nothing at all, which
    // is what keeps the progress on a block's row as the tape passes through.
    let info = Tape::from_blocks("info".into(), vec![Block::Info("hello".into())]);
    assert_eq!(info.block_progress(), Some(1.0));

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
fn the_block_being_played_is_shaded_as_far_as_the_tape_has_got() {
    // Progress is drawn over the block's own row rather than in a bar of its
    // own, so what there is to check is the shape of that overlay.
    use egui::{pos2, Rect};
    use zx_rustrum::ui::tape::played_rect;

    let row = Rect::from_min_max(pos2(10.0, 20.0), pos2(210.0, 36.0));
    let none = played_rect(row, 0.0);
    assert_eq!(none.width(), 0.0, "nothing played, nothing shaded");
    assert_eq!(none.height(), row.height());

    let half = played_rect(row, 0.5);
    assert_eq!(half.width(), 100.0);
    assert_eq!(half.min, row.min, "it fills from the start of the row");

    assert_eq!(played_rect(row, 1.0).width(), row.width());
    // A block that somehow reports past its end does not spill over the row.
    assert_eq!(played_rect(row, 4.0).width(), row.width());
    assert_eq!(played_rect(row, -1.0).width(), 0.0);
}

#[test]
fn the_window_no_longer_carries_progress_bars_of_its_own() {
    let Some(path) = a_tape_file() else {
        return;
    };
    let mut app = test_app();
    app.load_path(&path);
    let mut h = harness_for(app);
    h.run_steps(3);

    let text = labels(&h).join("\n");
    assert!(
        !text.contains("block 1 / "),
        "the overall bar should be gone: {text}"
    );
    assert!(
        text.contains("▶ Play") || text.contains("⏸ Pause"),
        "the transport itself should still be there: {text}"
    );
}

// ---- ZX81 tapes ------------------------------------------------------------

fn zx81_app() -> Option<App> {
    let mut app = test_app();
    app.roms.rom_zx81 = Some(std::fs::read("roms/zx81.rom").ok()?);
    app.switch_to_zx81(zx_rustrum::zx81::Ram::K16);
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
    assert!(
        text.contains("ZX81"),
        "no ZX81 block in the window:\n{text}"
    );
    assert!(
        text.contains("CHESSQUEEN"),
        "the block should name the program:\n{text}"
    );
}

#[test]
fn pressing_play_on_a_zx81_starts_the_tape_on_the_zx81s_clock() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    let path = std::path::Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    if !path.exists() {
        return;
    }
    app.load_path(path);
    // Real time, so the only thing that can move the tape a long way at once
    // is the bug being tested for.
    *app.tape_boost_mut() = false;
    // Let the ZX81 get well ahead of the Spectrum's stopped clock, which is
    // what made Play look like it had fast-forwarded through the tape.
    app.running = true;
    let mut harness = harness_for(app);
    for _ in 0..30 {
        harness.step();
    }
    {
        let app = harness.state();
        assert!(
            app.machine_t() > 1_000_000,
            "the ZX81 should have run for a while, not {}",
            app.machine_t()
        );
        assert!(
            app.machine_t() > app.spec.bus.total_t(),
            "the two clocks should have diverged"
        );
    }

    harness.get_by_label("▶ Play").click();
    harness.step();
    for _ in 0..10 {
        harness.step();
    }

    let app = harness.state();
    let tape = app.tape_ref().unwrap();
    assert!(tape.playing, "the tape should be playing");
    // A few frames in, a tape that takes twenty seconds has barely started.
    let progress = tape.block_progress().unwrap_or(1.0);
    assert!(
        progress < 0.05,
        "the tape jumped {:.0}% in on Play",
        progress * 100.0
    );
    assert!(tape.pulses > 0, "no pulses came out");

    // The oscilloscope sweeps from the newest edge that still has a whole
    // window of signal after it. Against the wrong clock every edge is in the
    // future, nothing triggers, and the trace is a flat line.
    let now = app.machine_t();
    let window = (app.cpu_hz() * 0.001) as u64; // a millisecond sweep
    let trigger = tape
        .edges
        .iter()
        .rev()
        .find(|(t, l)| *l && t.saturating_add(window) <= now);
    let trigger = trigger.expect("nothing for the oscilloscope to trigger on");
    assert!(
        trigger.0 + 20 * window > now,
        "the newest usable edge is {} T-states back — the trace would be stale",
        now - trigger.0
    );
}

/// The whole thing through the app's own loop: type `LOAD ""` at a ZX81,
/// press Play in the tape window, and let it run until the program arrives.
/// This is the path that broke when the tape was played against the wrong
/// machine's clock.
#[test]
fn a_zx81_loads_a_program_through_the_app() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    let path = std::path::Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    let Ok(file) = std::fs::read(path) else {
        return;
    };
    app.load_path(path);
    app.running = true;
    let mut harness = harness_for(app);

    // Give the ROM time to reach its cursor.
    for _ in 0..60 {
        harness.step();
    }

    // A key has to be held for a few frames, as on the real keyboard.
    fn tap(harness: &mut Harness<'_, App>, key: egui::Key, shift: bool) {
        let modifiers = egui::Modifiers {
            shift,
            ..Default::default()
        };
        harness.key_down_modifiers(modifiers, key);
        for _ in 0..8 {
            harness.step();
        }
        harness.key_up_modifiers(modifiers, key);
        for _ in 0..8 {
            harness.step();
        }
    }

    tap(&mut harness, egui::Key::J, false); // LOAD
    tap(&mut harness, egui::Key::P, true); // "
    tap(&mut harness, egui::Key::P, true); // "
    tap(&mut harness, egui::Key::Enter, false);

    harness.get_by_label("▶ Play").click();
    harness.step();
    assert!(
        harness.state().tape_ref().unwrap().playing,
        "Play did nothing"
    );

    // Twenty-odd seconds of tape at the 8x boost, with room to spare.
    for _ in 0..4000 {
        harness.step();
        if !harness.state().tape_is_playing() {
            break;
        }
    }

    let app = harness.state();
    assert!(!app.tape_is_playing(), "the tape never reached the end");
    let zx = app.zx81.as_ref().expect("still a ZX81");
    // The BASIC program area, which the running program leaves alone.
    let basic = 0x74;
    let loaded: Vec<u8> = (basic..file.len())
        .map(|i| zx.bus.ram[(0x0009 + i) & 0x3fff])
        .collect();
    assert_eq!(
        loaded,
        file[basic..],
        "the program in memory is not the one on the tape"
    );
}

#[test]
fn an_empty_deck_shows_the_same_window() {
    // Taking the tape out should not change the shape of the window: the
    // controls are still there, inert, and the list says what is missing.
    let mut app = test_app();
    app.spec.bus.tape = None;
    let mut h = harness_for(app);
    h.run_steps(3);

    let text = labels(&h).join("\n");
    assert!(
        text.contains("No tape loaded"),
        "the block list should say so: {text}"
    );
    assert!(
        text.contains("▶ Play") && text.contains("Blocks"),
        "and the rest of the window should be as it always is: {text}"
    );
}

#[test]
fn the_windows_that_were_open_are_opened_again() {
    use zx_rustrum::prefs::Prefs;

    let mut app = test_app();
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_tape = false;
    app.prefs = Prefs::parse("open_windows = \"debugger,tape\"\n");
    app.apply_prefs();

    assert!(app.show_debugger, "the debugger was open last time");
    assert!(app.show_tape, "so was the tape");
    assert!(!app.show_ram_map, "the RAM map was not");
    assert!(!app.show_profiler);
}

/// Pause and Stop are different things to a tape deck.
///
/// Pause holds the tape still with the head on it, so the hiss goes on and the
/// scope has something to draw; Stop lifts the head off, which is silence. The
/// two used to be the same call, and a paused deck therefore went as quiet as
/// a stopped one.
#[test]
fn pause_leaves_the_head_on_the_tape_and_stop_lifts_it() {
    let mut app = test_app();
    app.spec.bus.tape = Some(Tape::from_blocks(
        "t".into(),
        vec![Block::PureTone {
            len: 2168,
            count: 100,
        }],
    ));
    app.quality.noise = true;
    app.quality.noise_level = 0.9;
    let mut h = harness_for(app);
    h.run_steps(2);

    h.get_by_label("▶ Play").click();
    h.run_steps(2);
    assert!(
        h.state().tape_ref().unwrap().head_down,
        "Play should put the head down"
    );

    h.get_by_label("⏸ Pause").click();
    h.run_steps(2);
    let deck = h.state().tape_ref().unwrap();
    assert!(!deck.playing, "Pause should stop the tape moving");
    assert!(deck.head_down, "but leave the head where it was");

    h.get_by_label("■ Stop").click();
    h.run_steps(2);
    assert!(
        !h.state().tape_ref().unwrap().head_down,
        "Stop should take the head off the tape"
    );
}

/// The Noise switch and its slider reach the deck, which is where the hiss is
/// made.
#[test]
fn the_noise_switch_tells_the_deck_to_hiss() {
    let mut app = test_app();
    app.spec.bus.tape = Some(Tape::from_blocks(
        "t".into(),
        vec![Block::PureTone {
            len: 2168,
            count: 100,
        }],
    ));
    let mut h = harness_for(app);
    h.run_steps(2);
    assert!(!h.state().quality.noise, "it should start quiet");

    h.get_by_label("Noise").click();
    h.run_steps(2);
    assert!(h.state().quality.noise, "the Noise switch did nothing");

    h.state_mut().quality.noise_level = 0.4;
    h.state_mut().advance(1.0 / 50.0);
    let deck = h.state().spec.bus.tape.as_ref().expect("a tape").quality;
    assert!(
        deck.noise && (deck.noise_level - 0.4).abs() < 0.001,
        "the deck should have been told how loud: {deck:?}"
    );
}

/// A block's row offers to stop the deck in front of it.
///
/// A tape stops where its author put a stop block, which is where the loader
/// they wrote wanted it; somebody taking a game apart wants the deck to stop
/// somewhere else. The button is on the row because the row is where the block
/// is, and it appears on hover so the list stays a list.
#[test]
fn a_row_offers_to_put_a_stop_in_front_of_its_block() {
    let mut app = test_app();
    app.spec.bus.tape = Some(Tape::from_blocks(
        "t".into(),
        vec![
            Block::PureTone {
                len: 2168,
                count: 100,
            },
            Block::PureTone {
                len: 1000,
                count: 50,
            },
        ],
    ));
    let mut h = harness_for(app);
    h.run_steps(2);
    assert!(
        h.query_by_label("⏸ Pause before").is_none(),
        "the button should only be there under the pointer"
    );

    // With the deck on the second block, so pressing the button on the first
    // row can be told apart from clicking the row: a click seeks there.
    h.state_mut().tape_mut().unwrap().seek(1);
    h.run_steps(2);

    // Over the first block's row, which is the one the window has room for.
    let over = h
        .get_all_by_label_contains("1  Pure tone")
        .next()
        .and_then(|node| node.accesskit_node().bounding_box())
        .map(|box_| egui::pos2(box_.x0 as f32 + 20.0, box_.y0 as f32 + 4.0))
        .expect("the first block should be listed");
    h.input_mut().events.push(egui::Event::PointerMoved(over));
    h.run_steps(2);

    h.get_by_label("⏸ Pause before").click();
    h.run_steps(2);

    let deck = h.state().tape_ref().unwrap();
    assert_eq!(deck.blocks.len(), 3, "a block should have been put in");
    assert!(
        matches!(deck.blocks[0], Block::Pause(0)),
        "and it should be a stop-the-tape block, in front of the first: {}",
        deck.blocks[0].describe()
    );
    assert_eq!(
        deck.block, 2,
        "the deck should still be on the block it was on, which has moved \
         along one — pressing the button should not seek the way clicking \
         the row does"
    );
}
