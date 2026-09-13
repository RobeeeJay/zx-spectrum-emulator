//! The debugger's Go to box, and the other ways of sending the listing
//! somewhere.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

/// An address typed into Go to moves the listing there, with the address in
/// the middle of it. It used to set the address the listing was showing and
/// leave the listing drawing from the row it was already on — which was only
/// recalculated while following PC — so nothing on screen moved.
#[test]
fn go_to_moves_the_listing() {
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
    let before = h.state().dbg.top;

    // The Go to box is the first text field in the debugger.
    h.get_all(egui_kittest::kittest::by().role(egui::accesskit::Role::TextInput))
        .next()
        .expect("the Go to box")
        .click();
    h.run_steps(2);
    h.event(egui::Event::Text("8000".into()));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);

    let dbg = &h.state().dbg;
    assert_eq!(dbg.view_addr, 0x8000);
    // Memory there is all NOPs, a byte each, so the row half a listing above
    // $8000 is exactly that many bytes before it.
    let expected = 0x8000u16.wrapping_sub((dbg.lines / 2) as u16);
    assert_eq!(
        dbg.top, expected,
        "the listing starts at ${:04X}, not ${:04X} where it was, so $8000 is in its middle",
        dbg.top, before
    );
    assert_eq!(dbg.marked, Some(0x8000), "and the row asked for is marked");
}

/// Memory, beside PC, moves the listing to where the memory dump is: the
/// bytes being read, as code.
#[test]
fn memory_moves_the_listing_to_the_memory_dump() {
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
    app.dbg.mem_addr = 0x9000;
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    // The memory dump is headed "Memory" too: the button is the one to press.
    {
        use egui_kittest::kittest::NodeT;
        h.get_all_by_label("Memory")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .expect("a Memory button")
            .click();
    }
    h.run_steps(3);
    let dbg = &h.state().dbg;
    assert_eq!(dbg.view_addr, 0x9000);
    assert_eq!(dbg.marked, Some(0x9000), "the row is marked");
    assert_eq!(
        dbg.top,
        0x9000u16.wrapping_sub((dbg.lines / 2) as u16),
        "and the listing moved there"
    );
}
