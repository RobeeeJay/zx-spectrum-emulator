//! Typing a comment into the debugger's listing.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

/// A new comment takes spaces. Every keystroke used to be trimmed before the
/// field was drawn again, so the space typed at the end of "a" was gone by the
/// time "b" arrived: adding a comment gave "ab", while editing between two
/// words of an old one worked.
#[test]
fn a_new_comment_takes_spaces() {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: None,
        rom_plus3: None,
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_debugger = true;
    app.running = false;
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    let top = h.state().dbg.top;

    // The comments are the listing's multi-line fields, the top row's first.
    h.get_all(egui_kittest::kittest::by().role(egui::accesskit::Role::MultilineTextInput))
        .next()
        .expect("the top row's comment")
        .click();
    h.run_steps(2);
    for text in ["a", " ", "b"] {
        h.event(egui::Event::Text(text.into()));
        h.run_steps(2);
    }
    assert_eq!(
        h.state().notes.comment(top),
        "a b",
        "the comment at ${top:04X}"
    );
}
