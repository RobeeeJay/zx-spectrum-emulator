//! Picking a byte out of the picture, and typing over it in the dump.

use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;
use zx_rustrum::machine::{screen_bitmap_addr, Spectrum};
use zx_rustrum::ui::debugger::Column;
use zx_rustrum::ui::{App, Roms};

fn app() -> App {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
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
    h.run_steps(3);
    h
}

fn type_text(h: &mut Harness<'_, App>, text: &str) {
    h.input_mut()
        .events
        .push(egui::Event::Text(text.to_string()));
    h.run_steps(2);
}

/// A pixel on the screen has a byte behind it, and working out which one by
/// counting rows and thirds is a job nobody should be doing by hand.
#[test]
fn clicking_the_picture_picks_out_the_byte_behind_it() {
    let mut app = app();
    app.show_debugger = false;
    let view = app.view();

    // The top left of the picture proper, past the border.
    app.show_pixel_in_memory(view.border_x, view.border_top);
    assert_eq!(
        app.dbg.mem_addr, 0x4000,
        "the first byte of the display file"
    );
    assert_eq!(app.dbg.selected, Some((0x4000, Column::Hex)));
    assert!(app.show_debugger, "and the debugger should come up with it");

    // A pixel three rows down and ten cells along, which the display file's
    // thirds-and-rows order puts nowhere near the start.
    let (x, y) = (view.border_x + 10 * 8 + 3, view.border_top + 3 * 8 + 5);
    app.show_pixel_in_memory(x, y);
    assert_eq!(
        app.dbg.mem_addr,
        screen_bitmap_addr(29, 10),
        "row 29 of the display, cell 10"
    );

    // The border has no byte behind it, so nothing moves.
    let was = app.dbg.mem_addr;
    app.show_pixel_in_memory(1, 1);
    assert_eq!(
        app.dbg.mem_addr, was,
        "a click on the border changes nothing"
    );
}

/// Two hex digits make a byte, the first going in as it is typed so what is on
/// screen is what the second completes, and the second moving on to the next
/// byte — which is how a hex editor has always worked.
#[test]
fn typing_hex_over_a_byte_changes_it() {
    let mut app = app();
    app.dbg.mem_addr = 0x8000;
    app.dbg.selected = Some((0x8000, Column::Hex));
    let mut h = harness(app);

    type_text(&mut h, "3");
    assert_eq!(
        h.state().peek(0x8000) & 0xF0,
        0x30,
        "the first digit is the top half of the byte"
    );
    type_text(&mut h, "E");
    assert_eq!(h.state().peek(0x8000), 0x3E, "and the second completes it");
    assert_eq!(
        h.state().dbg.selected,
        Some((0x8001, Column::Hex)),
        "then it moves on to the next byte"
    );

    // Lower case is the same digit, and anything that is not a digit is not a
    // byte: it should leave the memory alone.
    type_text(&mut h, "ff");
    assert_eq!(h.state().peek(0x8001), 0xFF);
    let before = h.state().peek(0x8002);
    type_text(&mut h, "zz");
    assert_eq!(h.state().peek(0x8002), before, "z is not a hex digit");
}

/// The characters beside the numbers take characters: typing into them writes
/// what was typed, which is how a message in a program is changed.
#[test]
fn typing_into_the_text_column_writes_characters() {
    let mut app = app();
    app.dbg.mem_addr = 0x9000;
    app.dbg.selected = Some((0x9000, Column::Text));
    let mut h = harness(app);

    type_text(&mut h, "H");
    type_text(&mut h, "i");
    type_text(&mut h, "!");
    assert_eq!(
        (
            h.state().peek(0x9000),
            h.state().peek(0x9001),
            h.state().peek(0x9002)
        ),
        (b'H', b'i', b'!'),
        "each character goes into its own byte"
    );
    assert_eq!(h.state().dbg.selected, Some((0x9003, Column::Text)));
}

/// Nothing is typed over while a field has the keyboard. The address box and
/// the listing's labels are full of characters that are also hex digits.
#[test]
fn typing_into_a_field_does_not_reach_the_memory() {
    let mut app = app();
    app.dbg.mem_addr = 0x8000;
    app.dbg.selected = Some((0x8000, Column::Hex));
    let mut h = harness(app);

    let before = h.state().peek(0x8000);
    // Whatever the window puts the keyboard in — the address box is the first
    // field in the memory panel.
    h.state_mut().dbg.mem_text = "8000".into();
    h.run_steps(2);
    let field = h
        .root()
        .children_recursive()
        .find(|node| format!("{:?}", node.accesskit_node().role()).contains("TextInput"))
        .expect("the panel has a field in it");
    field.focus();
    h.run_steps(2);
    type_text(&mut h, "AB");
    assert_eq!(
        h.state().peek(0x8000),
        before,
        "what was typed belongs to the field that had the keyboard"
    );
}

/// Clicking a byte in the dump picks it out, and clicking the character beside
/// it picks out the same byte in the other column.
#[test]
fn clicking_a_byte_in_the_dump_picks_it_out() {
    use egui_kittest::kittest::Queryable;

    let mut app = app();
    app.dbg.mem_addr = 0x4000;
    app.spec.bus.poke(0x4002, 0x41);
    let mut h = harness(app);
    assert_eq!(
        h.state().dbg.selected,
        None,
        "nothing is picked out to begin"
    );

    // The dump shows the byte as "41 " in the numbers and "A" beside them.
    h.get_all_by_label("41 ").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(
        h.state().dbg.selected,
        Some((0x4002, Column::Hex)),
        "clicking the number picks that byte out"
    );

    h.get_all_by_label("A").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(
        h.state().dbg.selected,
        Some((0x4002, Column::Text)),
        "and clicking the character beside it picks out the same byte"
    );
}

/// Typing past the bottom of what is on show brings the dump along, so the
/// byte being typed into stays visible.
#[test]
fn typing_off_the_end_brings_the_dump_along() {
    let mut app = app();
    app.dbg.mem_addr = 0x8000;
    app.dbg.selected = Some((0x807F, Column::Text));
    let mut h = harness(app);

    type_text(&mut h, "x");
    assert_eq!(h.state().dbg.selected, Some((0x8080, Column::Text)));
    assert_eq!(
        h.state().dbg.mem_addr,
        0x8080,
        "the dump should have followed it"
    );
}
