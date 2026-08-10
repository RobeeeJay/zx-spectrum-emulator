//! Playing back RZX recordings.
//!
//! The recordings themselves are somebody's play-through of a copyrighted
//! game, so like `roms/` and `tapes/` they are not in the repository: these
//! tests skip themselves when there is nothing to read.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::notes::Notes;
use zx_rustrum::ui::{App, Roms};

fn recording(name: &str) -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(format!("recordings/{name}.rzx"));
    path.exists().then_some(path)
}

fn app() -> App {
    let roms = Roms {
        rom48: std::fs::read("roms/48.rom").ok(),
        rom128: std::fs::read("roms/128.rom").ok(),
        rom_plus3: std::fs::read("roms/plus3.rom").ok(),
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app
}

/// A recording is a header, a snapshot and a long list of frames.
#[test]
fn a_recording_reads_as_a_snapshot_and_a_list_of_frames() {
    let Some(path) = recording("manic") else {
        return;
    };
    let recording = zx_rustrum::rzx::parse(&std::fs::read(path).unwrap()).unwrap();

    assert!(!recording.creator.is_empty(), "nobody made it?");
    let snapshot = recording
        .snapshot
        .clone()
        .expect("no snapshot to start from");
    assert_eq!(snapshot.extension, "z80");
    assert!(snapshot.data.len() > 1000, "the snapshot was not unpacked");
    assert!(
        recording.len() > 1000,
        "only {} frames: the frames were not unpacked",
        recording.len()
    );
    assert!(
        recording.frames.iter().any(|f| !f.inputs.is_empty()),
        "nothing was ever read from a port, which cannot be right"
    );
}

/// Something that is not a recording is turned away rather than half read.
#[test]
fn something_that_is_not_a_recording_is_refused() {
    let error = zx_rustrum::rzx::parse(b"not an RZX file at all").unwrap_err();
    assert!(error.contains("not an RZX"), "{error}");
}

/// Playing one back runs the machine along the path it took when it was
/// recorded, which is what makes it worth having.
#[test]
fn a_recording_plays_back_without_coming_adrift() {
    let Some(path) = recording("manic") else {
        return;
    };
    let mut app = app();
    app.load_path(&path);
    assert!(app.rzx.is_some(), "it did not start: {}", app.status);

    for _ in 0..2000 {
        app.advance(1.0 / 50.08);
    }

    let played = app.rzx.as_ref().unwrap().frame;
    assert!(played > 1500, "only got to frame {played}");
    let short = app.spec.bus.playback.as_ref().unwrap().short;
    assert_eq!(
        short, 0,
        "the program read from the ports {short} times more than the \
         recording holds, so it is no longer following the recorded path"
    );
    let drawn = (0x4000..0x5800u32)
        .filter(|a| app.peek(*a as u16) != 0)
        .count();
    assert!(drawn > 500, "only {drawn} bytes of screen written");
}

/// A frame is measured in opcode fetches, not in instructions: a prefixed
/// instruction is two or more fetches, and counting whole instructions runs
/// past the end of every frame and reads input that was never recorded.
#[test]
fn a_frame_is_counted_in_opcode_fetches() {
    let mut spec = Spectrum::new();
    // LD A,R is two fetches: the ED prefix and the opcode.
    spec.bus.poke(0x8000, 0xED);
    spec.bus.poke(0x8001, 0x5F);
    spec.bus.poke(0x8002, 0x00); // NOP, one fetch
    spec.cpu.pc = 0x8000;

    let before = spec.bus.fetches;
    let (_, ran) = spec.run_fetches(2);
    assert_eq!(
        spec.bus.fetches - before,
        2,
        "LD A,R should have used up the whole two-fetch budget"
    );
    assert_eq!(ran, 2);
    assert_eq!(
        spec.cpu.pc, 0x8002,
        "and it should have run one instruction"
    );
}

/// The keyboard belongs to the recording while one is playing: what the user
/// types would fight with what was recorded.
#[test]
fn the_keyboard_is_the_recordings_while_it_plays() {
    let source = include_str!("../src/ui/mod.rs");
    let read_keyboard = source
        .split("fn read_keyboard")
        .nth(1)
        .expect("read_keyboard has gone");
    let guard = read_keyboard
        .split("ctx.input")
        .next()
        .expect("it reads no input at all now");
    assert!(
        guard.contains("self.rzx.is_some()"),
        "the host keyboard is still read while a recording plays"
    );
}

/// Notes go beside the recording: it is what is being read.
#[test]
fn notes_are_kept_beside_the_recording() {
    let Some(path) = recording("manic") else {
        return;
    };
    let mut app = app();
    app.tape_path = Some("tapes/somethingelse.tap".into());
    app.load_path(&path);

    assert_eq!(
        app.notes.file(),
        Some(Notes::sidecar(&path).as_path()),
        "the notes should sit beside the recording, not the tape"
    );

    // And they go back to the tape's when the recording is stopped.
    app.stop_recording();
    assert_eq!(
        app.notes.file(),
        Some(Notes::sidecar(std::path::Path::new("tapes/somethingelse.tap")).as_path())
    );
}

/// AutoDoc reads where the recording actually went. Static reading has to
/// guess which bytes are code; a recording says where the program really got
/// to, past the loader and the protection.
#[test]
fn a_recording_tells_autodoc_where_the_program_goes() {
    let Some(path) = recording("manic") else {
        return;
    };
    let mut app = app();
    app.load_path(&path);
    for _ in 0..600 {
        app.advance(1.0 / 50.08);
    }

    let visited = &app.rzx.as_ref().unwrap().visited;
    assert!(
        visited.len() > 20,
        "only {} places reached in 600 frames",
        visited.len()
    );

    app.dbg.autodoc = true;
    app.dbg.follow_pc = true;
    let doc = {
        let entries: Vec<u16> = visited.iter().copied().collect();
        let peek = |a: u16| app.peek(a);
        zx_rustrum::autodoc::analyse(&peek, &entries)
    };
    assert!(
        doc.labels.len() > 5,
        "the recording's own path through the game yielded only {} routines",
        doc.labels.len()
    );
}

/// The file picker does not filter recordings by extension.
///
/// rfd's macOS backend hands the extension list to the panel through an API
/// that wants types the system knows about, and nothing on a Mac claims
/// `.rzx`: with a filter set, the recordings show up greyed out and cannot be
/// chosen at all. The file is checked when it is opened instead.
#[test]
fn the_picker_does_not_filter_recordings_out_of_existence() {
    let source = include_str!("../src/ui/mod.rs");
    let picker = source
        .split("pub fn pick_file")
        .nth(1)
        .expect("pick_file has gone");
    let arm = picker
        .split("Some(FileKind::Recording) =>")
        .nth(1)
        .expect("the picker no longer knows about recordings");
    let arm = arm.split(',').next().unwrap_or_default();
    assert!(
        !arm.contains("add_filter"),
        "a filter on .rzx makes the recordings unselectable on macOS: {arm}"
    );
}

/// Anything that is not a recording still gets turned away when it is opened.
#[test]
fn opening_something_that_is_not_a_recording_says_so() {
    let dir = std::env::temp_dir().join(format!("zxrs-rzx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notreally.rzx");
    std::fs::write(&path, b"this is not a recording").unwrap();

    let mut app = app();
    app.load_path(&path);

    assert!(app.rzx.is_none(), "it should not have started playing");
    assert!(
        app.status.contains("not an RZX"),
        "and it should say why: {}",
        app.status
    );
}

/// The main window says a recording is playing, how far through it is, and
/// offers the choice between the speed it was played at and as fast as this
/// machine will go.
#[test]
fn the_main_window_says_a_recording_is_playing() {
    use egui_kittest::kittest::Queryable;

    let Some(path) = recording("manic") else {
        return;
    };
    let mut app = app();
    app.load_path(&path);

    let mut harness = egui_kittest::Harness::builder()
        .with_size([1600.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(3);

    let text = every_string(&harness);
    assert!(
        text.iter().any(|t| t.contains("REPLAY")),
        "nothing says a recording is playing: {text:?}"
    );
    assert!(
        text.iter().any(|t| t.contains('%')),
        "and nothing says how far through it is"
    );

    // The toggle is off to start with: a recording plays at the speed it was
    // played at unless asked otherwise.
    assert!(!harness.state().rzx.as_ref().unwrap().max_speed);
    harness.get_by_label("Max speed").click();
    harness.run_steps(2);
    assert!(
        harness.state().rzx.as_ref().unwrap().max_speed,
        "the toggle did nothing"
    );
}

/// And at maximum speed it really does get through more of the recording.
#[test]
fn maximum_speed_plays_more_of_the_recording() {
    let Some(path) = recording("manic") else {
        return;
    };

    let mut realtime = app();
    realtime.load_path(&path);
    for _ in 0..40 {
        realtime.advance(1.0 / 50.08);
    }
    let played_realtime = realtime.rzx.as_ref().unwrap().frame;

    let mut flat_out = app();
    flat_out.load_path(&path);
    flat_out.rzx.as_mut().unwrap().max_speed = true;
    for _ in 0..40 {
        flat_out.advance(1.0 / 50.08);
    }
    let played_fast = flat_out.rzx.as_ref().unwrap().frame;

    assert!(
        played_fast > played_realtime * 4,
        "maximum speed managed {played_fast} frames against {played_realtime} \
         at the speed it was recorded"
    );
    // And it is still following the recording, not just running.
    let short = flat_out.spec.bus.playback.as_ref().unwrap().short;
    assert_eq!(short, 0, "it came adrift when hurried along");
}

fn every_string(h: &egui_kittest::Harness<'_, App>) -> Vec<String> {
    use egui_kittest::kittest::NodeT;
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
