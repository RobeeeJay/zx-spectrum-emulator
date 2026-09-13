//! The keyboard window: the machine's own keys, pressable, and lit by the
//! keys of the desk.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::time::{Duration, Instant};
use zx_rustrum::keyboard::{self, Keys, MIN_PRESS};
use zx_rustrum::machine::Spectrum;
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
    app.show_tape = false;
    app.show_keyboard = true;
    app.running = false;
    app
}

fn harness<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1500.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// Every key of the matrix is on the picture, once.
///
/// Eight rows of five is forty keys, and a keyboard missing one of them is a
/// keyboard that cannot type something the machine can read.
#[test]
fn every_key_of_the_matrix_is_drawn_once() {
    for zx81 in [false, true] {
        let keys = keyboard::layout(zx81);
        let mut seen = [[0u32; 5]; 8];
        for key in keys {
            for &(row, bit) in key.press {
                seen[row][bit as usize] += 1;
            }
        }
        for (row, bits) in seen.iter().enumerate() {
            for (bit, count) in bits.iter().enumerate() {
                assert_eq!(
                    *count, 1,
                    "row {row} bit {bit} should be on exactly one key (zx81: {zx81})"
                );
            }
        }
    }
}

/// The legends are the machine's own, and the two machines differ.
///
/// The ZX81 says NEWLINE where the Spectrum says ENTER, has a full stop where
/// the Spectrum has SYMBOL SHIFT, and its keywords are its own: UNPLOT and
/// SCROLL are on it and BORDER and DRAW are not.
#[test]
fn each_machine_has_its_own_legends() {
    let spectrum = keyboard::layout(false);
    let zx81 = keyboard::layout(true);

    let named = |keys: &[keyboard::Key], name: &str| keys.iter().any(|k| k.main == name);
    let word = |keys: &[keyboard::Key], w: &str| keys.iter().any(|k| k.word == w);

    assert!(named(spectrum, "ENTER") && named(spectrum, "SYMBOL SHIFT"));
    assert!(word(spectrum, "BORDER") && word(spectrum, "DRAW"));

    assert!(named(zx81, "NEWLINE") && named(zx81, "."));
    assert!(word(zx81, "UNPLOT") && word(zx81, "SCROLL"));
    assert!(
        !word(zx81, "BORDER") && !word(zx81, "DRAW"),
        "the ZX81 has neither"
    );

    // The shift keys are in the same corner on both, since it is the same
    // matrix underneath.
    assert_eq!(spectrum[30].press, &[(0, 0)]);
    assert_eq!(zx81[30].press, &[(0, 0)]);
}

/// A key pressed on the window is a key the machine sees, and for long enough
/// to see it.
///
/// The ROM reads the keyboard once a frame and wants a key on two scans
/// running, so a press that lasted one host frame would type nothing.
#[test]
fn a_key_pressed_in_the_window_is_held_long_enough_to_be_read() {
    let mut keys = Keys::default();
    let now = Instant::now();
    keys.press(6, 0, now); // ENTER

    let matrix = keys.matrix(now + Duration::from_millis(60));
    assert_eq!(
        matrix[6] & 0x01,
        0,
        "ENTER should still be down three frames later: {:02X}",
        matrix[6]
    );
    assert!(
        MIN_PRESS >= Duration::from_millis(40),
        "and long enough for two of the ROM's scans: {MIN_PRESS:?}"
    );

    let matrix = keys.matrix(now + MIN_PRESS + Duration::from_millis(1));
    assert_eq!(matrix[6] & 0x01, 0x01, "and let go of afterwards");
    assert_eq!(matrix, [0xff; 8], "with nothing else stuck down");
}

/// A key pressed on the real keyboard lights its equivalent for at least a
/// tenth of a second, whatever the emulator's frame rate is doing.
#[test]
fn a_key_of_the_real_keyboard_stays_lit_for_a_tenth_of_a_second() {
    let mut keys = Keys::default();
    let now = Instant::now();
    keys.lit(2, 0, now); // Q

    assert!(keys.is_lit(2, 0, now), "lit the moment it goes down");
    assert!(
        keys.is_lit(2, 0, now + Duration::from_millis(90)),
        "and still lit after the key has been let go of"
    );
    assert!(
        !keys.is_lit(2, 0, now + Duration::from_millis(110)),
        "and out again a moment later"
    );
    assert!(!keys.is_lit(2, 1, now), "and only that key is lit");
    assert_eq!(
        keys.matrix(now),
        [0xff; 8],
        "lighting a key does not press it: the machine already has it"
    );
}

/// Holding a key on the real keyboard keeps its picture lit, rather than
/// flashing it once.
#[test]
fn a_held_key_stays_lit_while_it_is_held() {
    let mut keys = Keys::default();
    let start = Instant::now();
    for frame in 0..30 {
        let now = start + Duration::from_millis(frame * 20);
        keys.lit(0, 0, now);
        assert!(keys.is_lit(0, 0, now), "still down at frame {frame}");
    }
    let after = start + Duration::from_millis(30 * 20);
    assert!(!keys.is_lit(0, 0, after + MIN_PRESS));
}

/// A shift clicked on its own waits for the key it is shifting, and goes down
/// with it — one pointer cannot hold two keys.
#[test]
fn a_shift_waits_for_the_key_it_shifts() {
    let mut keys = Keys::default();
    let now = Instant::now();
    keys.latch(0, 0); // CAPS SHIFT
    assert!(keys.latched(0, 0), "held, waiting");
    assert_eq!(
        keys.matrix(now)[0] & 0x01,
        0,
        "and down as far as the machine is concerned"
    );

    // Now the key it was held for. Both are down together, and the shift is
    // let go of afterwards rather than at the moment the key is pressed.
    keys.press(4, 0, now); // 0, which with CAPS SHIFT is DELETE
    keys.take_latched(now);
    let matrix = keys.matrix(now + Duration::from_millis(50));
    assert_eq!(matrix[0] & 0x01, 0, "CAPS SHIFT still down with the key");
    assert_eq!(matrix[4] & 0x01, 0, "and the key itself");
    assert!(!keys.latched(0, 0), "the latch is spent");

    assert_eq!(
        keys.matrix(now + MIN_PRESS + Duration::from_millis(1)),
        [0xff; 8],
        "and both are let go of together"
    );

    // Clicked twice, it is simply let go of again.
    keys.latch(7, 1);
    keys.latch(7, 1);
    assert!(!keys.latched(7, 1));
}

/// The keys are drawn to the machine's shape rather than stretched to the
/// window's: ten across, four down, and wider than they are tall.
#[test]
fn the_keys_keep_their_shape_in_any_window() {
    use egui::{pos2, Rect};

    for area in [
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(760.0, 260.0)),
        Rect::from_min_size(pos2(10.0, 40.0), egui::vec2(2000.0, 300.0)),
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 900.0)),
    ] {
        let rects = keyboard::key_rects(area, 6.0);
        assert_eq!(rects.len(), 40);
        let first = rects[0];
        let aspect = first.width() / first.height();
        assert!(
            (aspect - keyboard::KEY_ASPECT).abs() < 0.01,
            "a key is {aspect:.2} wide for its height in a {}x{} window",
            area.width(),
            area.height()
        );
        // Ten across and four down, in reading order, and inside the area.
        assert!(
            rects[9].left() > rects[0].left(),
            "the first row goes right"
        );
        assert!(rects[10].top() > rects[0].top(), "and then down a row");
        assert!(
            area.contains_rect(rects[39]),
            "the last key is still inside the window"
        );
    }
}

/// The window is opened from the toolbar, and knows which machine it is of.
#[test]
fn the_window_is_opened_from_the_toolbar() {
    let mut app = test_app();
    app.show_keyboard = false;
    let mut h = harness(app);
    h.run_steps(3);
    assert!(!h.state().show_keyboard);

    h.get_by_label("Keyboard").click();
    h.run_steps(3);
    assert!(h.state().show_keyboard, "the toggle opens it");

    // And the keys are there to be clicked, with the machine named above them.
    h.run_steps(3);
    assert!(
        h.query_by_label("SYMBOL SHIFT").is_some(),
        "the Spectrum's own keys are drawn"
    );
    let name = h.state().machine_name();
    assert!(
        h.query_all_by_value(&name).count() > 0,
        "and the window says which machine it is of: {name}"
    );
}

/// Clicking a key in the window types it into the machine.
#[test]
fn clicking_a_key_types_it_into_the_machine() {
    let mut h = harness(test_app());
    h.run_steps(3);
    h.get_by_label("A").click();
    h.run_steps(2);
    // A is row 1, bit 0.
    assert_eq!(
        h.state().spec.bus.keys[1] & 0x01,
        0,
        "A should be down: {:02X}",
        h.state().spec.bus.keys[1]
    );
    // And nothing else with it.
    let down: Vec<_> = (0..8)
        .filter(|r| h.state().spec.bus.keys[*r] != 0xff)
        .collect();
    assert_eq!(down, vec![1], "only A's row is down: {down:?}");
}

/// The window follows the machine: switch to a ZX81 and it is the ZX81's
/// keyboard, without the window being reopened.
#[test]
fn the_window_follows_the_machine() {
    let mut h = harness(test_app());
    h.run_steps(3);
    assert!(h.query_by_label("SYMBOL SHIFT").is_some());

    h.state_mut().switch_to_zx81(zx_rustrum::zx81::Ram::K16);
    h.run_steps(3);
    assert!(h.query_by_label("NEWLINE").is_some(), "the ZX81's keys now");
    assert!(
        h.query_by_label("SYMBOL SHIFT").is_none(),
        "and not the Spectrum's"
    );
}

/// Pressing a key on the real keyboard lights its picture — which is the
/// other half of the window: it says what is being typed as well as taking
/// what is clicked.
#[test]
fn the_real_keyboard_lights_the_picture() {
    let mut h = harness(test_app());
    h.run_steps(3);
    assert!(
        !h.state().keys.is_lit(1, 0, Instant::now()),
        "nothing lit to start with"
    );

    h.key_press(egui::Key::A);
    h.run_steps(2);
    assert!(
        h.state().keys.is_lit(1, 0, Instant::now()),
        "A on the desk lights A on the picture"
    );
    assert!(
        !h.state().keys.is_lit(1, 1, Instant::now()),
        "and only that key"
    );
}

/// A reset lets go of the keyboard.
///
/// A shift clicked in the window waits for the key it is shifting, and it used
/// to go on waiting across a reset: the machine came up with CAPS SHIFT held,
/// answered every key with the shifted one, and read as a machine ignoring the
/// keyboard.
#[test]
fn a_reset_lets_go_of_every_key() {
    let mut app = test_app();
    // A shift clicked and waiting, and a key still within its hold.
    app.keys.latch(0, 0); // CAPS SHIFT
    app.keys.press(1, 0, Instant::now()); // A
    assert!(app.keys.latched(0, 0));
    assert_ne!(app.keys.matrix(Instant::now()), [0xFF; 8]);

    app.reset_machine();

    assert!(!app.keys.latched(0, 0), "the shift is let go of");
    assert_eq!(
        app.keys.matrix(Instant::now()),
        [0xFF; 8],
        "and nothing is held down on the machine that comes up"
    );
    assert!(
        !app.keys.is_lit(0, 0, Instant::now()),
        "nor lit on the picture of the keyboard"
    );
}

/// And the machine it comes up as says how long it will be before the ROM
/// reads the keyboard, which is not the same on every model.
#[test]
fn the_wait_after_a_reset_is_the_machines_own() {
    use zx_rustrum::machine::Model;

    let mut app = test_app();
    app.speed = 1.0;
    app.switch_model(Model::Spectrum48);
    app.reset_machine();
    let status = app.starting_up_for_test();
    assert!(status.contains("1.7s"), "a 48K checks 48K of RAM: {status}");

    app.switch_model(Model::Plus3);
    let status = app.starting_up_for_test();
    assert!(
        status.contains("+3"),
        "and says which machine it is: {status}"
    );
    assert!(
        status.contains("1.1s"),
        "a +3 is quicker about it, and saying 1.7 would be wrong: {status}"
    );
}

/// The rows are spaced far enough apart for the word printed under each key.
///
/// The red words — CAT, FORMAT, INVERSE — go under the keys rather than on
/// them, which is where the case prints them and the only place left: the face
/// already carries four legends. That only works if the gap between rows is
/// taller than the text in it.
#[test]
fn the_rows_leave_room_for_the_word_under_each_key() {
    use egui::{pos2, vec2, Rect};

    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 320.0));
    let rects = keyboard::key_rects_with(area, 6.0, 13.0);

    let first = rects[0];
    let below = rects[keyboard::ACROSS];
    let gap = below.top() - first.bottom();
    assert!(
        gap >= 12.0,
        "the row gap is {gap} and the word under a key wants about nine points \
         of it"
    );
    assert_eq!(
        rects[1].left() - rects[0].right(),
        6.0,
        "and the keys are no further apart across the row than they were"
    );
}

fn picked_out(h: &Harness<'_, App>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_all_by_label(label)
        // egui gives a button's selected state to accessibility as toggled.
        .any(|n| n.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True))
}

/// Looking for a word picks out the key it is on and the shifts it takes:
/// BEEP is under Z, so it takes extended mode — both shifts — and then SYMBOL
/// SHIFT. The window says so in words as well.
#[test]
fn the_search_picks_out_a_key_and_its_shifts() {
    let mut h = harness(test_app());
    h.state_mut().key_search = "beep".into();
    h.run_steps(2);
    for key in ["Z", "CAPS SHIFT", "SYMBOL SHIFT"] {
        assert!(picked_out(&h, key), "{key} is picked out for BEEP");
    }
    for key in ["P", "S", "ENTER"] {
        assert!(!picked_out(&h, key), "{key} has nothing to do with BEEP");
    }
    assert!(
        h.query_all_by_value("BEEP: extended mode, then SYMBOL SHIFT with Z")
            .next()
            .is_some(),
        "and how to type it is written out"
    );

    h.state_mut().key_search = "print".into();
    h.run_steps(2);
    assert!(
        picked_out(&h, "P") && picked_out(&h, "C"),
        "PRINT and LPRINT"
    );
    assert!(
        picked_out(&h, "CAPS SHIFT"),
        "LPRINT is above C, which is extended mode"
    );
}

/// What is typed into the search box is for the box: it does not reach the
/// machine's keyboard, or looking for BEEP would type it into the program.
#[test]
fn typing_into_the_search_does_not_type_on_the_machine() {
    use egui_kittest::kittest::NodeT;
    let mut h = harness(test_app());
    h.run_steps(2);
    h.get_by_role(egui::accesskit::Role::TextInput).click();
    h.run_steps(2);
    for (key, text) in [(egui::Key::B, "b"), (egui::Key::E, "e")] {
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.event(egui::Event::Text(text.into()));
        h.run_steps(1);
        assert_eq!(
            h.state().spec.bus.keys,
            [0xFF; 8],
            "{text} went into the box and not to the machine"
        );
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    assert_eq!(h.state().key_search, "be");
    let _ = h
        .get_by_role(egui::accesskit::Role::TextInput)
        .accesskit_node();
}
