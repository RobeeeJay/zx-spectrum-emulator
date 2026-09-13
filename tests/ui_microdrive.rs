//! The Microdrive and Hardware windows.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::hardware::Peripheral;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::microdrive::Cartridge;
use zx_rustrum::ui::disk::Mounted;
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
    app.running = false;
    app
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1600.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("zxrs-microdrive-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn a_cartridge_file(name: &str) -> std::path::PathBuf {
    let path = scratch(name);
    let mut cart = Cartridge::blank("Games", 20);
    // A file on it, so there is a catalogue to show.
    cart.sectors[0].record[0] = 0x04;
    cart.sectors[0].record[2] = 0x00;
    cart.sectors[0].record[3] = 0x02;
    cart.sectors[0].record[4..14].copy_from_slice(b"Cybernoid ");
    std::fs::write(&path, cart.to_bytes()).unwrap();
    path
}

/// The microdrives hang off the Interface 1, so fitting one is what brings
/// them into being — and unplugging it takes them away again.
#[test]
fn the_microdrives_come_with_the_interface() {
    let mut app = test_app();
    assert!(app.spec.bus.if1.is_none());

    app.fit(Peripheral::Interface1, true);
    let if1 = app.spec.bus.if1.as_ref().expect("an interface");
    assert_eq!(if1.drive_count(), 1, "one drive to start with");

    app.set_microdrives(4);
    assert_eq!(app.spec.bus.if1.as_ref().unwrap().drive_count(), 4);

    app.fit(Peripheral::Interface1, false);
    assert!(app.spec.bus.if1.is_none(), "and it goes with the interface");
    assert!(!app.show_microdrive, "so does its window");
}

/// A cartridge is put in the way a disk is: read-only, a copy, or the file
/// itself, and the question is asked rather than guessed at.
#[test]
fn a_cartridge_is_mounted_one_of_three_ways() {
    let mut app = test_app();
    app.fit(Peripheral::Interface1, true);
    let path = a_cartridge_file("games.mdr");
    let before = std::fs::read(&path).unwrap();

    app.open_cartridge(&path, 0);
    assert!(app.pending_cartridge.is_some(), "it asks first");
    assert!(
        zx_rustrum::ui::microdrivewin::mounted(&app, 0).is_none(),
        "and nothing is in the drive yet"
    );

    app.mount_pending_cartridge(Mounted::ReadOnly, None);
    let cart = zx_rustrum::ui::microdrivewin::mounted(&app, 0).expect("a cartridge");
    assert_eq!(cart.name(), "Games");
    assert!(
        !app.spec.bus.if1.as_ref().unwrap().drives[0].writable(),
        "read-only means the machine cannot write to it"
    );

    // Nothing is written back, even if something marks it changed.
    app.spec.bus.if1.as_mut().unwrap().drives[0]
        .cartridge
        .as_mut()
        .unwrap()
        .dirty = true;
    assert!(!app.save_cartridge(0));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is as it was"
    );
}

/// Writing to a copy leaves the original alone, and the copy is made when the
/// cartridge goes in rather than at the first write.
#[test]
fn writing_to_a_copy_leaves_the_original_cartridge_alone() {
    let mut app = test_app();
    app.fit(Peripheral::Interface1, true);
    let path = a_cartridge_file("original.mdr");
    let original = std::fs::read(&path).unwrap();
    let copy = scratch("copy.mdr");

    app.open_cartridge(&path, 0);
    app.mount_pending_cartridge(Mounted::Copy, Some(copy.clone()));
    assert!(copy.exists(), "the copy is made straight away");

    {
        let cart = app.spec.bus.if1.as_mut().unwrap().drives[0]
            .cartridge
            .as_mut()
            .unwrap();
        cart.sectors[5].record[15] = 0x99;
        cart.dirty = true;
    }
    assert!(app.save_cartridge(0), "the copy is written");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "and the original is untouched"
    );
    let written = Cartridge::parse(&std::fs::read(&copy).unwrap()).unwrap();
    assert_eq!(written.sectors[5].record[15], 0x99);
}

/// The window draws a drive per cartridge, with what is on the one selected.
#[test]
fn the_window_shows_the_drives_and_the_catalogue() {
    let mut app = test_app();
    app.fit(Peripheral::Interface1, true);
    app.set_microdrives(3);
    let path = a_cartridge_file("shown.mdr");
    app.open_cartridge(&path, 0);
    app.mount_pending_cartridge(Mounted::ReadOnly, None);
    app.show_microdrive = true;

    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label("Load…").is_some(),
        "the controls are there"
    );
    assert!(h.query_by_label("Eject").is_some());
    assert!(
        h.query_by_label("Cybernoid").is_some(),
        "and the catalogue of the cartridge in the drive"
    );
    // Three drives means a drive picker.
    assert!(h.query_by_label("2").is_some(), "one button per drive");
}

/// Without an Interface 1 there are no microdrives, and the window says where
/// to plug one in rather than showing an empty box.
#[test]
fn the_window_says_when_there_is_no_interface() {
    let mut app = test_app();
    app.show_microdrive = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.get_all_by_label_contains("No Interface 1 is fitted")
            .next()
            .is_some(),
        "it should say what is missing"
    );
}

/// The Hardware window says, for each peripheral, what it is and how far it is
/// emulated. A switch that turns on nothing is worse than no switch.
#[test]
fn the_hardware_window_says_what_each_peripheral_does() {
    let mut app = test_app();
    app.show_hardware = true;
    let mut h = harness_for(app);
    h.run_steps(3);

    for what in Peripheral::ALL {
        assert!(
            h.query_by_label(what.name()).is_some(),
            "{} should be on the list",
            what.name()
        );
    }
    // The ones that do nothing yet say so, in the window.
    assert!(
        h.get_all_by_label_contains("not emulated").next().is_some(),
        "the switches with nothing behind them are marked"
    );

    // And fitting one from the window fits it on the machine. In sections
    // the list is longer than the window's 720 points, and the SpecDrum,
    // under Audio, starts out below the bottom of it: it is scrolled to, as
    // somebody would, before it is clicked.
    let over = h
        .get_by_label("Kempston mouse")
        .accesskit_node()
        .bounding_box()
        .expect("the top of the list");
    h.event(egui::Event::PointerMoved(egui::pos2(
        over.x0 as f32 + 20.0,
        over.y0 as f32 + 5.0,
    )));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -700.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.get_by_label("Cheetah SpecDrum").click();
    h.run_steps(2);
    assert!(h.state().spec.bus.hardware.fitted(Peripheral::SpecDrum));
}

/// What is plugged in, and which machine it is plugged into, are remembered
/// between launches.
#[test]
fn the_machine_and_its_peripherals_are_remembered() {
    use zx_rustrum::machine::Model;

    let mut app = test_app();
    app.switch_model(Model::Spectrum128);
    app.fit(Peripheral::SpecDrum, true);
    app.fit(Peripheral::Interface1, true);
    app.set_microdrives(3);
    app.save_window_state();

    let saved = app.prefs.to_text();
    assert!(saved.contains("machine = \"128K\""), "{saved}");
    assert!(saved.contains("specdrum"), "{saved}");
    assert!(saved.contains("microdrives = \"3\""), "{saved}");

    // A machine coming up with those preferences comes up as that machine.
    let mut next = test_app();
    next.prefs = zx_rustrum::prefs::Prefs::parse(&saved);
    next.apply_prefs();
    assert_eq!(next.spec.bus.model, Model::Spectrum128, "the same machine");
    assert!(next.spec.bus.hardware.fitted(Peripheral::SpecDrum));
    assert!(next.spec.bus.hardware.fitted(Peripheral::Interface1));
    assert_eq!(next.spec.bus.if1.as_ref().unwrap().drive_count(), 3);
}

/// Fitting a Multiface puts one on the bus, and the main window grows a red
/// button for it.
///
/// The button is on the front of the emulator rather than in the Hardware
/// window because that is what it is for: it is pressed while a game is
/// running, and going to find a window first is not that.
#[test]
fn fitting_a_multiface_puts_its_red_button_on_the_main_window() {
    let mut app = test_app();
    assert!(
        harness_for(test_app())
            .query_by_label("Red button")
            .is_none(),
        "no Multiface, no button"
    );

    app.fit(Peripheral::MultifaceOne, true);
    let fitted = app
        .spec
        .bus
        .multifaces
        .iter()
        .any(|mf| mf.model == zx_rustrum::multiface::Model::One);
    assert!(fitted, "the box should be on the back");

    let ready = app.has_rom_for(Peripheral::MultifaceOne);
    let mut h = harness_for(app);
    h.run_steps(3);

    if !ready {
        // Without a ROM there is nothing behind the button, and a button that
        // does nothing is what the Hardware window exists to avoid.
        assert!(
            h.query_by_label("Red button").is_none(),
            "no ROM, no button"
        );
        return;
    }

    h.get_by_label("Red button").click();
    h.run_steps(2);
    // What the press did is not something to look for in the latches a frame
    // later: by then the NMI has been taken, the menu is running from the stub
    // it puts in the machine's RAM, and the box has paged itself out again.
    // The status line says whether the button was taken.
    assert!(
        h.state().status.contains("Red button"),
        "pressing it should have stopped the machine: {}",
        h.state().status
    );
    assert!(!h.state().status_is_error, "{}", h.state().status);

    // Taking the box out takes its button with it.
    let mut app = h.into_state();
    app.fit(Peripheral::MultifaceOne, false);
    assert!(app.spec.bus.multifaces.is_empty(), "and out it comes");
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label("Red button").is_none(),
        "nothing left to press"
    );
}

/// What is plugged into the back stays plugged in when the machine changes.
///
/// Changing model rebuilds the bus, and the peripherals used to be left behind
/// by it: a 128K chosen from the toolbar came up with the Interface 1 gone and
/// the cartridges with it.
#[test]
fn the_peripherals_stay_on_the_back_when_the_machine_changes() {
    use zx_rustrum::machine::Model;

    let mut app = test_app();
    app.fit(Peripheral::Interface1, true);
    app.fit(Peripheral::MultifaceOne, true);
    app.switch_model(Model::Spectrum128);

    assert!(app.spec.bus.if1.is_some(), "the interface is still there");
    assert_eq!(app.spec.bus.multifaces.len(), 1, "and so is the Multiface");
    assert!(app.spec.bus.hardware.fitted(Peripheral::Interface1));
}

/// The line saying what is on a cartridge sits under the loop of tape, not
/// through it.
///
/// The drive is a picture, so nothing about it is queryable: what can be
/// checked is the spacing the drawing uses, which is why it is worked out
/// apart from the painting. At 74 points tall the block put that line of text
/// straight through the marks — the text is centred on its baseline, so half
/// of it was above the line it was meant to be below.
#[test]
fn the_line_under_a_cartridge_clears_the_loop_of_tape() {
    use egui::{pos2, vec2, Rect};
    use zx_rustrum::ui::microdrivewin::{parts, CARTRIDGE_H, INFO_TEXT};

    // The block as the window allocates it — the height the drawing uses, so
    // that the height is what is being checked — less the five points of case
    // around it.
    let block = Rect::from_min_size(pos2(0.0, 0.0), vec2(520.0, CARTRIDGE_H));
    let face = block.shrink(5.0);
    let parts = parts(face);

    let text_top = parts.info - INFO_TEXT / 2.0;
    assert!(
        text_top >= parts.tape.bottom(),
        "the text starts at {text_top} and the tape ends at {}: they overlap by {}",
        parts.tape.bottom(),
        parts.tape.bottom() - text_top
    );
    assert!(
        parts.tape.top() >= parts.label.bottom(),
        "and the tape is under the label, not through it"
    );
    assert!(
        parts.info + INFO_TEXT / 2.0 <= face.bottom(),
        "and the text is inside the block"
    );
}
