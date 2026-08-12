//! The disassembly's label and comment columns, the memory shortcuts, and the
//! frame split that keeps the debug windows alive across an application
//! switch.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::demo_rom::DEMO_ROM;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::notes::Notes;
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
    Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// The pointer registers are one click away, because looking at what IX is
/// aimed at should not mean reading it off the register panel and typing it
/// back in.
#[test]
fn the_memory_panel_follows_every_pointer_register() {
    let mut app = app();
    app.spec.cpu.set_bc(0x1234);
    app.spec.cpu.set_de(0x5678);
    app.spec.cpu.ix = 0x9ABC;
    app.spec.cpu.iy = 0xDEF0;
    app.spec.cpu.set_hl(0x4321);
    app.spec.cpu.sp = 0xFF00;

    let mut h = harness(app);
    h.run_steps(3);

    for (button, expected) in [
        ("BC", 0x1234u16),
        ("DE", 0x5678),
        ("IX", 0x9ABC),
        ("IY", 0xDEF0),
        ("HL", 0x4321),
        ("SP", 0xFF00),
    ] {
        h.get_by_label(button).click();
        h.run_steps(2);
        assert_eq!(
            h.state().dbg.mem_addr,
            expected,
            "{button} should have pointed the dump at ${expected:04X}"
        );
    }
}

/// Both new columns are there and named, so it is clear which side is which
/// before anything has been typed into either.
#[test]
fn the_listing_has_a_label_column_and_a_comment_column() {
    let mut h = harness(app());
    h.run_steps(3);
    let labels = all_labels(&h);
    assert!(
        labels.contains(&"Label".to_string()),
        "no label column heading: {labels:?}"
    );
    assert!(
        labels.contains(&"Comments".to_string()),
        "no comment column heading"
    );
}

/// What is typed into the columns is kept against the address, and written to
/// the file beside whatever is loaded as soon as the field is left.
#[test]
fn a_label_and_a_comment_are_kept_against_the_address() {
    let dir = tempdir("typed");
    let rom = dir.join("48.rom");
    std::fs::write(&rom, [0u8; 4]).unwrap();

    let mut app = app();
    app.rom_path = Some(rom.clone());
    app.reload_notes();
    app.notes.set_label(0x0000, "reset");
    app.notes.set_comment(0x0000, "where the machine starts");
    app.notes.save_if_dirty().unwrap();

    // Read back through a fresh set of notes, which is what the next session
    // would do.
    let saved = Notes::for_file(&rom);
    assert_eq!(saved.label(0x0000), "reset");
    assert_eq!(saved.comment(0x0000), "where the machine starts");
    assert!(
        dir.join("48.zxrs.txt").exists(),
        "the notes should sit beside the ROM"
    );
}

/// Notes belong to what is being disassembled: the tape when there is one in
/// the deck, and the ROM only when there is not.
#[test]
fn the_tape_owns_the_notes_when_one_is_loaded() {
    let dir = tempdir("which-file");
    let rom = dir.join("48.rom");
    let tape = dir.join("manic.tap");
    std::fs::write(&rom, [0u8; 4]).unwrap();
    std::fs::write(&tape, [0u8; 4]).unwrap();

    let mut app = app();
    app.rom_path = Some(rom.clone());
    app.reload_notes();
    assert_eq!(app.notes.file(), Some(Notes::sidecar(&rom).as_path()));

    app.tape_path = Some(tape.clone());
    app.reload_notes();
    assert_eq!(
        app.notes.file(),
        Some(Notes::sidecar(&tape).as_path()),
        "the tape in the deck is what is being disassembled"
    );
}

/// Switching to another tape saves what was typed against the last one first,
/// rather than dropping it on the floor.
#[test]
fn changing_tapes_writes_the_notes_out_first() {
    let dir = tempdir("switching");
    let first = dir.join("first.tap");
    let second = dir.join("second.tap");
    std::fs::write(&first, [0u8; 4]).unwrap();
    std::fs::write(&second, [0u8; 4]).unwrap();

    let mut app = app();
    app.tape_path = Some(first.clone());
    app.reload_notes();
    app.notes
        .set_comment(0x8000, "unsaved when the tape changes");

    app.tape_path = Some(second.clone());
    app.reload_notes();

    let kept = Notes::for_file(&first);
    assert_eq!(
        kept.comment(0x8000),
        "unsaved when the tape changes",
        "the first tape's notes were lost"
    );
    assert_eq!(
        app.notes.comment(0x8000),
        "",
        "the second tape should start with notes of its own"
    );
}

/// The debug windows are declared while the machine is run, not while the main
/// window is drawn.
///
/// eframe skips an application's `ui` when the main window is not visible —
/// which on macOS includes switching away from the application — and then
/// prunes every viewport that frame did not declare. The windows were being
/// destroyed and built again on the way back, which is what made them vanish,
/// reappear and change order.
#[test]
fn the_debug_windows_are_drawn_even_when_the_main_window_is_not() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.show_debugger = true;

    // Cleared inside the debugger window's own frame, so it only goes back to
    // false if that window was drawn.
    app.dbg.raise = true;
    let _ = ctx.run_ui(Default::default(), |ui| {
        app.frame_logic(ui.ctx());
    });

    assert!(
        !app.dbg.raise,
        "the debugger was not drawn by frame_logic, so eframe would prune it \
         the moment the main window stopped being visible"
    );
}

fn all_labels(h: &Harness<'_, App>) -> Vec<String> {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        if let Some(label) = node.accesskit_node().label() {
            out.push(label.to_string());
        }
        if let Some(value) = node.accesskit_node().value() {
            out.push(value.to_string());
        }
        for child in node.children() {
            walk(&child, out);
        }
    }
    let mut found = Vec::new();
    walk(&h.root(), &mut found);
    found
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zxrs-listing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The listing is a window onto the whole address space, not a list with ends:
/// rolling the wheel over it moves through memory an instruction at a time, as
/// far as it goes in either direction.
///
/// It stopped doing that when the listing was given more rows than fit: the
/// extra ones gave the area a scrollbar of its own, and the wheel moved within
/// those rows instead of through memory. So the rows are cut to what fits.
#[test]
fn rolling_the_wheel_travels_through_memory_without_end() {
    let mut app = app();
    app.dbg.view_addr = 0x8000;
    app.dbg.follow_pc = false;
    let mut h = harness(app);
    h.run_steps(3);

    // Over the listing: the wheel is only answered where the listing is.
    let over = h
        .get_all_by_label("Instruction")
        .next()
        .and_then(|node| node.accesskit_node().bounding_box())
        .map(|box_| egui::pos2(box_.x0 as f32 + 20.0, box_.y1 as f32 + 60.0))
        .expect("the listing has a heading over it");

    let roll = |h: &mut Harness<'_, App>, amount: f32| {
        h.input_mut().events.push(egui::Event::PointerMoved(over));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, amount),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(2);
    };

    // Down through memory, then back up past where it started: neither
    // direction runs out.
    let start = h.state().dbg.view_addr;
    roll(&mut h, -200.0);
    let down = h.state().dbg.view_addr;
    assert!(
        down > start,
        "rolling down should move the listing forwards: ${start:04X} to ${down:04X}"
    );

    roll(&mut h, 400.0);
    let up = h.state().dbg.view_addr;
    assert!(
        up < start,
        "and rolling up should go back past where it began: ${down:04X} to ${up:04X}"
    );
}

/// Every row drawn is a row that can be seen. Rows past the bottom of the
/// listing are the ones that put a scrollbar on it.
#[test]
fn the_listing_draws_no_more_rows_than_fit() {
    let mut h = harness(app());
    h.run_steps(3);

    let rows = h.state().dbg.lines as f32;
    let height = rows * zx_rustrum::ui::theme::CONTROL_H;
    assert!(
        height <= zx_rustrum::ui::debugger::LISTING_H,
        "{rows} rows at {} points each is {height}, past the {} the listing has",
        zx_rustrum::ui::theme::CONTROL_H,
        zx_rustrum::ui::debugger::LISTING_H
    );
    assert!(rows >= 8.0, "and there should be a listing at all: {rows}");
}
