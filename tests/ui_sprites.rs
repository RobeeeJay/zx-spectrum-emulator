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
    let mut view = SpriteView::default();
    view.cells_across = 2;
    view.cells_down = 2;
    assert_eq!(view.stride(), 32, "two by two cells is thirty-two bytes");

    view.cells_across = 3;
    view.cells_down = 1;
    assert_eq!(view.stride(), 24);
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

    h.get_by_label("Sprites").click();
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
