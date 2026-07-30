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
