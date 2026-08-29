//! Putting a disk in the +3 from the window: read-only, a copy, or the file
//! itself — and the question that decides which.

use eframe::egui;
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

/// The disk window is offered on a machine with a drive and nowhere else.
#[test]
fn the_disk_window_is_only_offered_on_a_machine_with_a_drive() {
    let mut app = test_app();
    app.switch_model(Model::Spectrum48);
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.query_by_label("Disk").is_none(),
        "a 48K has no disk drive"
    );

    h.state_mut().switch_model(Model::Plus3);
    h.run_steps(3);
    assert!(
        h.query_by_label("Disk").is_some(),
        "a +3 has one, and the window can be opened"
    );
}

/// Loading a disk and making one are in the File menu, where the other things
/// that come off the disc are.
#[test]
fn the_file_menu_offers_a_disk() {
    let mut h = harness_for(test_app());
    h.run_steps(3);
    h.get_by_label("File").click();
    h.run_steps(3);
    assert!(h.query_by_label("Load disk…").is_some());
    assert!(h.query_by_label("Create blank disk…").is_some());
}

/// Putting a disk in opens the window that shows the drive: that is the moment
/// somebody wants to watch it.
#[test]
fn a_disk_going_in_opens_the_window() {
    let mut app = test_app();
    assert!(!app.show_disk);
    let path = a_disk_file("opens.dsk");
    app.open_disk(&path);
    app.mount_pending(Mounted::ReadOnly, None);
    assert!(app.show_disk, "the window opens with the disk");
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

/// The window draws the drive, its light, the disk's sectors and what is on
/// it. Drawn by hand rather than out of widgets, so what the test can see is
/// the shapes and the text.
#[test]
fn the_window_shows_the_drive_the_sectors_and_the_catalogue() {
    let mut app = test_app();
    let path = scratch("window.dsk");
    app.new_disk(&path);
    // A file in the directory, so there is a catalogue to draw.
    {
        let disk = &mut app.spec.bus.fdc.drives[0].as_mut().unwrap().disk;
        let directory = &mut disk.track_mut(0, 0).unwrap().sectors[0].data;
        let entry = &mut directory[..32];
        entry.fill(0);
        entry[1..9].copy_from_slice(b"MYGAME  ");
        entry[9..12].copy_from_slice(b"BAS");
        entry[15] = 24; // 3K
    }
    app.show_disk = true;

    let mut h = harness_for(app);
    h.run_steps(3);

    // The controls.
    assert!(h.query_by_label("Normal").is_some(), "the speeds are there");
    assert!(h.query_by_label("Fastload").is_some());
    assert!(h.query_by_label("Eject").is_some());
    // The catalogue, where the tape window lists blocks.
    assert!(h.query_by_label("MYGAME.BAS").is_some(), "the file");
    assert!(
        h.get_all_by_label_contains("3K").next().is_some(),
        "and how big it is"
    );
    assert!(
        h.get_all_by_label_contains("free").next().is_some(),
        "and what is left"
    );
}

/// The speeds are what they say: Normal makes the machine wait for the drive,
/// Fastload does not.
#[test]
fn the_speed_buttons_choose_how_the_drive_behaves() {
    use zx_rustrum::fdc::Speed;

    let mut app = test_app();
    let path = scratch("speed.dsk");
    app.new_disk(&path);
    app.show_disk = true;
    assert_eq!(
        app.spec.bus.fdc.speed,
        Speed::Fastload,
        "a drive with no waits, until somebody asks for them"
    );

    let mut h = harness_for(app);
    h.run_steps(3);
    h.get_by_label("Normal").click();
    h.run_steps(2);
    assert_eq!(h.state().spec.bus.fdc.speed, Speed::Normal);

    h.get_by_label("Fastload").click();
    h.run_steps(2);
    assert_eq!(h.state().spec.bus.fdc.speed, Speed::Fastload);
}

/// With no disk in the drive the window says so rather than drawing an empty
/// grid that looks like a disk of nothing.
#[test]
fn an_empty_drive_says_so() {
    let mut app = test_app();
    app.show_disk = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.get_all_by_label_contains("No disk in the drive")
            .next()
            .is_some(),
        "it should say the drive is empty"
    );
}

/// A disk that is not in one of the machine's formats has no catalogue, and
/// the window says why rather than printing rubbish out of its sectors.
#[test]
fn a_disk_in_another_format_says_why_there_is_no_catalogue() {
    let mut app = test_app();
    let path = scratch("foreign.dsk");
    app.new_disk(&path);
    {
        let disk = &mut app.spec.bus.fdc.drives[0].as_mut().unwrap().disk;
        for sector in &mut disk.track_mut(0, 0).unwrap().sectors {
            sector.r = 0x01;
        }
    }
    app.show_disk = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(
        h.get_all_by_label_contains("Not a +3 format disk")
            .next()
            .is_some(),
        "it should say what it cannot read"
    );
}

/// A disk inside a zip: that is how a download of a game usually arrives.
///
/// The archive builder is the same one `tests/zip.rs` uses — bytes written by
/// hand, so what is tested is the reader rather than whatever wrote them.
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    for (name, data) in files {
        let at = out.len();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        directory.extend_from_slice(&0u16.to_le_bytes());
        directory.extend_from_slice(&[0; 4]);
        directory.extend_from_slice(&0u32.to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&[0; 8]);
        directory.extend_from_slice(&[0; 4]);
        directory.extend_from_slice(&(at as u32).to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let directory_at = out.len();
    let count = files.len() as u16;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&(directory_at as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A zip holding a disk mounts the disk.
#[test]
fn a_disk_inside_a_zip_can_be_mounted() {
    let mut app = test_app();
    let mut disk = Disk::blank("in a zip");
    disk.track_mut(0, 0).unwrap().sectors[0].data[0] = 0x77;
    let path = scratch("game.zip");
    std::fs::write(
        &path,
        archive(&[
            ("readme.txt", b"about the game"),
            ("GAME.DSK", &disk.to_bytes()),
        ]),
    )
    .unwrap();

    app.open_disk(&path);
    let pending = app.pending_disk.as_ref().expect("a disk out of the zip");
    assert_eq!(
        pending.source,
        zx_rustrum::ui::disk::Source::InArchive {
            archive: path.clone(),
            inner: "GAME.DSK".into(),
        }
    );
    assert_eq!(
        pending.disk.track(0, 0).unwrap().sectors[0].data[0],
        0x77,
        "and it is the disk that was in there"
    );

    // Read-only mounts it as it is.
    app.mount_pending(Mounted::ReadOnly, None);
    let drive = app.spec.bus.fdc.drives[0].as_ref().expect("a disk");
    assert!(drive.write_protected);
    assert_eq!(drive.disk.track(0, 0).unwrap().sectors[0].data[0], 0x77);
}

/// A disk out of a zip cannot be written back into it, and the copy goes
/// beside the archive under the name it had inside.
#[test]
fn a_disk_out_of_a_zip_is_copied_rather_than_written_back() {
    let mut app = test_app();
    let disk = Disk::blank("zipped");
    let path = scratch("writable.zip");
    let copy = scratch("INSIDE.dsk");
    std::fs::write(&path, archive(&[("INSIDE.DSK", &disk.to_bytes())])).unwrap();

    app.open_disk(&path);
    let pending = app.pending_disk.as_ref().unwrap();
    assert!(
        !pending.source.writable_in_place(),
        "there is nowhere in an archive to write a disk back to"
    );
    assert_eq!(
        pending.source.copy_name(),
        copy,
        "a copy goes beside the archive, named as it was inside"
    );

    // Asking for it anyway is refused, and the disk stays waiting rather than
    // being dropped.
    app.mount_pending(Mounted::InPlace, None);
    assert!(app.pending_disk.is_some(), "still waiting to be told");
    assert!(app.spec.bus.fdc.drives[0].is_none());
    assert!(
        app.status.contains("cannot be written back"),
        "{}",
        app.status
    );

    // A copy works, and the writes go to it.
    app.mount_pending(Mounted::Copy, None);
    assert!(copy.exists(), "the copy is made");
    let drive = app.spec.bus.fdc.drives[0].as_ref().unwrap();
    assert!(!drive.write_protected);
    assert_eq!(drive.path.as_deref(), Some(copy.as_path()));
    let _ = std::fs::remove_file(&copy);
}

/// The question a zipped disk asks has no "write to this file" on it, since
/// there is no file to write to.
#[test]
fn the_question_for_a_zipped_disk_offers_only_a_copy() {
    let mut app = test_app();
    let disk = Disk::blank("zipped");
    let path = scratch("asks.zip");
    std::fs::write(&path, archive(&[("ASKS.DSK", &disk.to_bytes())])).unwrap();
    app.open_disk(&path);

    let mut h = harness_for(app);
    h.run_steps(3);
    assert!(h.query_by_label("Read-only").is_some());
    assert!(h
        .get_all_by_label_contains("Write to a copy")
        .next()
        .is_some());
    assert!(
        h.query_by_label("Write to this file").is_none(),
        "there is nowhere to write it back to"
    );
    assert!(
        h.get_all_by_label_contains("came out of a zip")
            .next()
            .is_some(),
        "and it should say why"
    );
}

/// A zip with no disk in it says so rather than mounting nothing.
#[test]
fn a_zip_with_no_disk_says_so() {
    let mut app = test_app();
    let path = scratch("empty.zip");
    std::fs::write(&path, archive(&[("readme.txt", b"nothing to load")])).unwrap();
    app.open_disk(&path);
    assert!(app.pending_disk.is_none());
    assert!(app.status.contains("no disk image"), "{}", app.status);
}

/// The disk in the drive is labelled the way the cassette is: the file's name
/// without its extension, in the same hand, and in white — the disk is dark
/// plastic and dark ink on it cannot be read.
#[test]
fn the_disk_is_labelled_in_the_same_hand_as_the_cassette() {
    let mut app = test_app();
    let path = scratch("Chuckie Egg.dsk");
    app.new_disk(&path);
    app.show_disk = true;

    let mut h = harness_for(app);
    h.run_steps(3);

    // What is drawn, rather than what was asked for: the label is painted
    // shapes, so the text goes into the frame's galleys.
    let shapes = h.output().shapes.clone();
    let mut written: Vec<(String, egui::Color32)> = Vec::new();
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Color32)>) {
        match shape {
            egui::epaint::Shape::Text(text) => {
                out.push((text.galley.text().to_string(), text.fallback_color))
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    for clipped in &shapes {
        walk(&clipped.shape, &mut written);
    }

    let label = written
        .iter()
        .find(|(text, _)| text == "Chuckie Egg")
        .unwrap_or_else(|| {
            panic!(
                "the disk should carry its name without the extension; what was written was {:?}",
                written.iter().map(|(t, _)| t).collect::<Vec<_>>()
            )
        });
    assert_eq!(
        label.1,
        zx_rustrum::ui::theme::WHITE,
        "and in white, since the disk is dark"
    );
}
