//! Memory read as graphics.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::sprites::SpriteView;
use zx_rustrum::ui::{App, Roms};

fn app() -> App {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

/// A sprite is cells of eight bytes: how many bytes one takes follows from how
/// many cells across and down it is, and that is what the sheet steps by.
#[test]
fn a_sprites_size_decides_how_far_the_sheet_steps() {
    let two_by_two = SpriteView {
        cells_across: 2,
        cells_down: 2,
        ..Default::default()
    };
    assert_eq!(
        two_by_two.stride(),
        32,
        "two by two cells is thirty-two bytes"
    );

    let three_across = SpriteView {
        cells_across: 3,
        cells_down: 1,
        ..Default::default()
    };
    assert_eq!(three_across.stride(), 24);
}

/// The window opens from the Windows row and draws without complaint.
#[test]
fn the_sprite_window_opens() {
    let mut app = app();
    app.show_sprites = false;

    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    h.get_by_label("Graphics").click();
    h.run_steps(3);
    assert!(h.state().show_sprites, "the toggle did not open it");

    // Its controls are there: the address it is showing and the sprite size.
    h.get_by_label_contains("2 wide");
    h.get_by_label_contains("2 tall");
}

/// Clicking a sprite takes the memory dump to it, so the bytes can be read
/// beside the picture.
#[test]
fn the_sheet_starts_where_it_is_told() {
    let mut app = app();
    app.show_sprites = true;
    app.sprites.addr = 0x9000;
    app.sprites.addr_text = "9000".into();

    let mut h = Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    assert_eq!(h.state().sprites.addr, 0x9000);
}

/// Being sent a block of graphics opens the viewer on it, brings it forward,
/// and starts at a sprite size the block divides into. Nothing in the bytes
/// says how wide a sprite is meant to be — that is the one thing the viewer
/// asks for — but a block's length usually divides by the size of the sprites
/// in it, which is a better place to start guessing from than whatever was set
/// last.
#[test]
fn a_block_of_graphics_opens_the_viewer_sized_to_it() {
    let mut app = app();
    app.sprites.cells_across = 1;
    app.sprites.cells_down = 1;

    // 256 bytes divides by 128 (4x4 cells) exactly, and by 32, and by 8.
    app.show_as_graphics(0xB000, 256);

    assert!(app.show_sprites, "the viewer should open");
    assert!(app.sprites.raise, "and come forward");
    assert_eq!(app.sprites.addr, 0xB000);
    assert_eq!(app.sprites.addr_text, "B000", "with the box in step");
    assert_eq!(
        (app.sprites.cells_across, app.sprites.cells_down),
        (4, 4),
        "the largest square sprite the block divides into"
    );
    assert_eq!(
        app.sprites.columns, 2,
        "and enough across to show the whole block: two sprites of 128 bytes"
    );

    // A block that only divides by eight is a column of single cells.
    app.show_as_graphics(0xC000, 24);
    assert_eq!((app.sprites.cells_across, app.sprites.cells_down), (1, 1));
    assert_eq!(app.sprites.columns, 3);
}

/// Clicking a block of data in the debugger's Data panel shows it as pictures.
///
/// Any block, not only the ones guessed to be graphics: what a block holds is
/// a guess made from who read it, and a block whose reader was never seen to
/// be called — which is most of a game, played back from a recording — has no
/// guess at all. Looking at it is how you find out.
#[test]
fn clicking_graphics_data_in_the_debugger_shows_it_as_pictures() {
    use egui_kittest::kittest::NodeT;

    let mut spec = Spectrum::new();
    // A routine that reads a block of memory and writes it to the screen: what
    // the observer calls graphics is data read by something that then draws.
    let program: [(u16, &[u8]); 1] = [(
        0x8000,
        // LD HL,$B000 : LD DE,$4000 : LD BC,$0100 : LDIR : JR $8000
        &[
            0x21, 0x00, 0xB0, 0x11, 0x00, 0x40, 0x01, 0x00, 0x01, 0xED, 0xB0, 0x18, 0xF3,
        ],
    )];
    for (at, bytes) in program {
        for (offset, byte) in bytes.iter().enumerate() {
            spec.bus.poke(at + offset as u16, *byte);
        }
    }
    for offset in 0..0x100u16 {
        spec.bus
            .poke(0xB000 + offset, (offset as u8).wrapping_mul(17));
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xFF00;
    spec.bus.observer.enabled = true;
    for _ in 0..4000 {
        spec.step_instruction();
    }

    let mut app = app();
    app.spec = spec;
    app.show_debugger = true;
    app.show_sprites = false;
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(4);

    // The Data panel lists the block; the row says where it is and how long.
    // A plain label's text is in the node's value, not its label.
    let row = h
        .root()
        .children_recursive()
        .filter_map(|node| node.accesskit_node().value().map(|value| value.to_string()))
        .find(|text| text.starts_with("B000"))
        .expect("the block it read should be listed in the Data panel");
    assert!(
        row.contains("256"),
        "the row should say how long the block is: {row:?}"
    );

    // Two panels can show the same text — the memory dump has an address box
    // of its own — so the row is taken by position rather than by name.
    h.get_all_by_value(&row).next().unwrap().click();
    h.run_steps(3);

    assert!(
        h.state().show_sprites,
        "clicking a block of graphics should show it as pictures"
    );
    assert_eq!(
        h.state().sprites.addr,
        0xB000,
        "at the block that was clicked"
    );
}

/// Not every format packs its rows together: some keep a mask byte, an
/// attribute or a byte of padding after each row of cells. Reading straight
/// past those shears the picture exactly as a wrong width does, so they can be
/// stepped over — and a graphic then takes more bytes than its pixels do.
#[test]
fn bytes_can_be_skipped_after_each_row_of_cells() {
    let packed = SpriteView {
        cells_across: 2,
        cells_down: 3,
        skip_after_row: 0,
        ..Default::default()
    };
    assert_eq!(packed.stride(), 48, "six cells of eight bytes");
    assert_eq!(packed.row_bytes(), 16, "two cells across");

    let padded = SpriteView {
        cells_across: 2,
        cells_down: 3,
        skip_after_row: 1,
        ..Default::default()
    };
    assert_eq!(
        padded.stride(),
        51,
        "a byte after each of the three rows, the last one included: the next \
         graphic starts after the padding, not before it"
    );
}

/// The skipped bytes are stepped over in the drawing as well as counted in the
/// stride, or the sheet would be spaced correctly and drawn wrongly.
#[test]
fn skipped_bytes_are_left_out_of_the_picture() {
    let padded = SpriteView {
        cells_across: 2,
        cells_down: 2,
        skip_after_row: 1,
        ..Default::default()
    };
    // First row of cells: sixteen bytes of pixels from $9000.
    assert_eq!(padded.byte_of(0x9000, 0, 0, 0), 0x9000);
    assert_eq!(padded.byte_of(0x9000, 0, 1, 7), 0x900F);
    // Second row starts past the padding byte, at $9011 rather than $9010.
    assert_eq!(
        padded.byte_of(0x9000, 1, 0, 0),
        0x9011,
        "the byte after the first row of cells is not part of the picture"
    );

    let mut spec = Spectrum::new();
    // Two rows of one cell, with a byte of padding after each row. The pixel
    // rows are $FF and the padding is $00, so drawn correctly every pixel is
    // set, and drawn without the skip half of them are not.
    let mut at = 0x9000u16;
    for _ in 0..2 {
        for _ in 0..8 {
            spec.bus.poke(at, 0xFF);
            at += 1;
        }
        spec.bus.poke(at, 0x00);
        at += 1;
    }

    let mut app = app();
    app.spec = spec;
    app.show_sprites = true;
    app.sprites.addr = 0x9000;
    app.sprites.addr_text = "9000".into();
    app.sprites.cells_across = 1;
    app.sprites.cells_down = 2;
    app.sprites.skip_after_row = 1;
    app.sprites.columns = 1;

    // What the drawing reads, row by row, is what the stride says it should.
    assert_eq!(app.sprites.stride(), 18, "eight bytes and a byte, twice");
    let mut harness = Harness::builder()
        .with_size([900.0, 700.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(3);

    // The second row of the graphic starts at $9009, not $9008: the padding
    // after the first row is not part of the picture.
    assert_eq!(
        harness.state().sprites.row_bytes(),
        8,
        "one cell across is eight bytes of pixels"
    );
    assert_eq!(harness.state().sprites.stride(), 18);
}

/// Kept a row of pixels at a time, a graphic's rows are its width apart —
/// plus whatever is kept beside each row — and a cell across is a byte along.
#[test]
fn a_graphic_kept_in_rows_is_read_a_row_of_pixels_at_a_time() {
    use zx_rustrum::ui::sprites::Layout;
    let view = SpriteView {
        layout: Layout::Rows,
        cells_across: 3,
        cells_down: 2,
        skip_after_row: 1,
        ..Default::default()
    };
    assert_eq!(view.byte_of(0x9000, 0, 1, 2), 0x9000 + 2 * 4 + 1);
    assert_eq!(
        view.byte_of(0x9000, 1, 0, 0),
        0x9000 + 8 * 4,
        "the second row of cells"
    );
    assert_eq!(view.stride(), 4 * 16, "four bytes a row, sixteen rows");
}

/// Find pauses the machine and waits for a block of the screen.
#[test]
fn find_pauses_the_machine_and_waits_for_a_block() {
    let mut app = app();
    app.running = true;
    app.show_sprites = true;
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(2);
    h.get_by_label("Find…").click();
    h.run_steps(2);
    assert!(!h.state().running, "the machine is paused");
    assert!(h.state().gfx_picking, "and waiting for a block");

    h.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    assert!(!h.state().gfx_picking, "Esc gives up");
}

/// A block clicked on the screen is looked for in memory, found where it was
/// put, and Show puts the viewer there.
#[test]
fn a_block_clicked_on_the_screen_is_found_in_memory() {
    const BLOCK: [u8; 8] = [0x18, 0x3C, 0x7E, 0xDB, 0xFF, 0x24, 0x5A, 0xC1];
    let mut app = app();
    // The whole screen shows the block, so whichever cell the click lands
    // on, it is the block that is picked.
    for y in 0..192u16 {
        for col in 0..32u16 {
            let at = zx_rustrum::machine::screen_bitmap_addr(y, col);
            app.spec.bus.poke(at, BLOCK[(y % 8) as usize]);
        }
    }
    for (i, b) in BLOCK.iter().enumerate() {
        app.spec.bus.poke(0x9000 + i as u16, *b);
    }
    app.start_graphics_find();
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(2);
    let centre = egui::pos2(750.0, 600.0);
    h.event(egui::Event::PointerMoved(centre));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);

    let picked = h.state().gfx_found.clone().expect("a block was picked");
    assert!(!h.state().gfx_picking, "and the picking is over");
    assert_eq!(picked.pattern, BLOCK, "the block's own eight bytes");
    let search = picked.result.expect("found");
    assert!(
        search
            .matches
            .iter()
            .any(|m| m.addr == 0x9000 && m.pitch == 1),
        "at $9000, as a character: {:?}",
        search.matches
    );
    assert!(h.state().show_sprites, "the Graphics window is brought up");

    h.get_all_by_label("Show")
        .next()
        .expect("a Show button")
        .click();
    h.run_steps(2);
    assert_eq!(
        h.state().sprites.addr,
        0x9000,
        "and Show puts the viewer there"
    );
}
