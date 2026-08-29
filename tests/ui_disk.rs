//! Putting a disk in the +3 from the window: read-only, a copy, or the file
//! itself — and the question that decides which.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::disk::Disk;
use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::ui::disk::{copy_name, Mounted};
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
    app.switch_model(Model::Plus3);
    app
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1600.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// A directory of this test's own, so nothing writes near the user's disks.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("zxrs-disk-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn a_disk_file(name: &str) -> std::path::PathBuf {
    let path = scratch(name);
    let mut disk = Disk::blank("original");
    // Something to tell an original from a copy by.
    disk.track_mut(0, 0).unwrap().sectors[0].data[0] = 0x11;
    std::fs::write(&path, disk.to_bytes()).unwrap();
    path
}

/// Opening a disk does not put it in the drive: it asks first. Asking
/// afterwards would mean a write could already have happened.
#[test]
fn opening_a_disk_asks_before_anything_is_mounted() {
    let mut app = test_app();
    let path = a_disk_file("ask.dsk");
    app.open_disk(&path);

    assert!(
        app.pending_disk.is_some(),
        "it should be waiting to be told"
    );
    assert!(
        app.spec.bus.fdc.drives[0].is_none(),
        "and nothing is in the drive yet"
    );

    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label("Read-only").is_some(),
        "the question should be on the screen"
    );
    assert!(h
        .get_all_by_label_contains("Write to a copy")
        .next()
        .is_some());
    assert!(h.query_by_label("Write to this file").is_some());
}

/// Read-only means the machine is told the disk is write-protected, and the
/// file is never touched.
#[test]
fn a_read_only_disk_is_write_protected_and_the_file_is_left_alone() {
    let mut app = test_app();
    let path = a_disk_file("read-only.dsk");
    let before = std::fs::read(&path).unwrap();
    app.open_disk(&path);
    app.mount_pending(Mounted::ReadOnly, None);

    let drive = app.spec.bus.fdc.drives[0].as_ref().expect("a disk");
    assert!(drive.write_protected, "the machine is told it cannot write");
    assert_eq!(app.disk_mounted, Some(Mounted::ReadOnly));

    // Even if something writes to it, nothing is saved: there is nowhere for
    // it to go and the machine was told so.
    app.spec.bus.fdc.drives[0].as_mut().unwrap().disk.dirty = true;
    assert!(!app.save_disk(), "a read-only disk is not written back");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is as it was"
    );
}

/// Writing to a copy leaves the original alone, and the copy is made when the
/// disk goes in rather than at the first write: a copy that does not exist
/// until something changes is a copy nobody can find.
#[test]
fn writing_to_a_copy_leaves_the_original_untouched() {
    let mut app = test_app();
    let path = a_disk_file("game.dsk");
    let original = std::fs::read(&path).unwrap();
    let copy = scratch("game (copy).dsk");
    app.open_disk(&path);
    app.mount_pending(Mounted::Copy, Some(copy.clone()));

    assert!(copy.exists(), "the copy is made straight away");
    let drive = app.spec.bus.fdc.drives[0].as_ref().expect("a disk");
    assert!(!drive.write_protected, "and it can be written to");
    assert_eq!(drive.path.as_deref(), Some(copy.as_path()));

    // A write goes to the copy.
    {
        let drive = app.spec.bus.fdc.drives[0].as_mut().unwrap();
        drive.disk.track_mut(0, 0).unwrap().sectors[0].data[0] = 0x22;
        drive.disk.dirty = true;
    }
    assert!(app.save_disk(), "the copy is written");

    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "the original is untouched"
    );
    let written = Disk::parse(&std::fs::read(&copy).unwrap()).unwrap();
    assert_eq!(
        written.track(0, 0).unwrap().sectors[0].data[0],
        0x22,
        "and the change is in the copy"
    );
}

/// Writing to the file itself is what a work disk wants.
#[test]
fn writing_in_place_changes_the_file_it_came_from() {
    let mut app = test_app();
    let path = a_disk_file("work.dsk");
    app.open_disk(&path);
    app.mount_pending(Mounted::InPlace, None);

    {
        let drive = app.spec.bus.fdc.drives[0].as_mut().unwrap();
        drive.disk.track_mut(1, 0).unwrap().sectors[2].data[7] = 0x33;
        drive.disk.dirty = true;
    }
    assert!(app.save_disk());
    let written = Disk::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written.track(1, 0).unwrap().sectors[2].data[7], 0x33);
    assert!(
        !app.spec.bus.fdc.drives[0].as_ref().unwrap().disk.dirty,
        "and it knows it has been saved"
    );
}

/// A copy is named after the disk it is a copy of, and does not overwrite one
/// that is already there.
#[test]
fn a_copy_is_named_after_the_original_and_does_not_overwrite_one() {
    let path = a_disk_file("named.dsk");
    let first = copy_name(&path);
    assert_eq!(
        first.file_name().unwrap().to_string_lossy(),
        "named (copy).dsk"
    );

    std::fs::write(&first, b"in the way").unwrap();
    let second = copy_name(&path);
    assert_eq!(
        second.file_name().unwrap().to_string_lossy(),
        "named (copy 2).dsk",
        "a copy that is already there is not written over"
    );
    let _ = std::fs::remove_file(&first);
}

/// A blank disk is made where the user says, formatted as the machine formats
/// one, and goes in writable — somebody who has just made a disk means to
/// write to it.
#[test]
fn a_new_disk_is_blank_writable_and_saved_where_it_was_asked_for() {
    let mut app = test_app();
    let path = scratch("brand new.dsk");
    app.new_disk(&path);

    assert!(path.exists(), "the file is written straight away");
    let drive = app.spec.bus.fdc.drives[0].as_ref().expect("a disk");
    assert!(!drive.write_protected, "writable by default");
    assert_eq!(app.disk_mounted, Some(Mounted::InPlace));
    assert_eq!(drive.path.as_deref(), Some(path.as_path()));

    let disk = Disk::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disk.describe(), "40 tracks, 1 side, 360 sectors, 180K");
    assert!(
        disk.track(0, 0)
            .unwrap()
            .sectors
            .iter()
            .all(|s| s.data.iter().all(|b| *b == zx_rustrum::disk::FILLER)),
        "and empty"
    );
}

/// Ejecting writes the disk back if it has changed, and empties the drive.
#[test]
fn ejecting_writes_the_disk_back() {
    let mut app = test_app();
    let path = scratch("eject.dsk");
    app.new_disk(&path);
    {
        let drive = app.spec.bus.fdc.drives[0].as_mut().unwrap();
        drive.disk.track_mut(0, 0).unwrap().sectors[0].data[0] = 0x44;
        drive.disk.dirty = true;
    }
    app.eject_disk();

    assert!(app.spec.bus.fdc.drives[0].is_none(), "the drive is empty");
    assert_eq!(app.disk_mounted, None);
    let written = Disk::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(written.track(0, 0).unwrap().sectors[0].data[0], 0x44);
}

/// The Disk section is only on a machine that has a drive.
#[test]
fn the_disk_controls_are_only_on_a_machine_with_a_drive() {
    let mut app = test_app();
    app.switch_model(Model::Spectrum48);
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label("Insert…").is_none(),
        "a 48K has no disk drive"
    );

    h.state_mut().switch_model(Model::Plus3);
    h.run_steps(3);
    assert!(
        h.query_by_label("Insert…").is_some(),
        "a +3 has one, and the buttons appear with it"
    );
    assert!(h.query_by_label("New disk…").is_some());
}

/// Cancelling puts nothing in the drive and forgets the disk.
#[test]
fn cancelling_the_question_puts_nothing_in() {
    let mut app = test_app();
    let path = a_disk_file("cancel.dsk");
    app.open_disk(&path);
    let mut h = harness_for(app);
    h.run_steps(3);
    h.get_by_label("Cancel").click();
    h.run_steps(3);

    assert!(h.state().pending_disk.is_none());
    assert!(h.state().spec.bus.fdc.drives[0].is_none());
}

/// Switching machine takes the disk out and writes it back: a 48K has nowhere
/// to put one, and the writes so far are the user's.
#[test]
fn changing_machine_writes_the_disk_back_and_empties_the_drive() {
    let mut app = test_app();
    let path = scratch("switch.dsk");
    app.new_disk(&path);
    {
        let drive = app.spec.bus.fdc.drives[0].as_mut().unwrap();
        drive.disk.track_mut(2, 0).unwrap().sectors[1].data[3] = 0x55;
        drive.disk.dirty = true;
    }

    app.switch_model(Model::Spectrum48);
    assert!(app.spec.bus.fdc.drives[0].is_none(), "the drive is empty");
    assert_eq!(app.disk_mounted, None);
    let written = Disk::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        written.track(2, 0).unwrap().sectors[1].data[3],
        0x55,
        "and what was written is on the disk"
    );
}
