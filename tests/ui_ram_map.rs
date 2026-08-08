//! RAM access map controls.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn harness<'a>() -> Harness<'a, App> {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = true;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

#[test]
fn reads_writes_and_executes_can_each_be_toggled() {
    let mut h = harness();
    h.run_steps(3);
    assert!(h.state().ram.show_read);
    assert!(h.state().ram.show_write);
    assert!(h.state().ram.show_exec);

    h.get_by_label("Read").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_read, "Read should have turned off");
    assert!(h.state().ram.show_write, "and left the others alone");
    assert!(h.state().ram.show_exec);

    h.get_by_label("Write").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_write);

    h.get_by_label("Execute").click();
    h.run_steps(3);
    assert!(!h.state().ram.show_exec);

    // And back on again.
    h.get_by_label("Read").click();
    h.run_steps(3);
    assert!(h.state().ram.show_read);
}

#[test]
fn each_channel_has_its_own_fade_control() {
    let mut h = harness();
    h.run_steps(3);
    // A slider contributes more than one node (the drag value and its label),
    // so count matches rather than expecting exactly one.
    for l in ["read fade", "write fade", "exec fade"] {
        assert!(
            h.query_all_by_label(l).count() > 0,
            "expected a {l} slider in the RAM map window"
        );
    }
}

// ---------------------------------------------------------------------------
// per-bank tracking and the all-memory view
// ---------------------------------------------------------------------------

use zx_rustrum::machine::Model;
use zx_rustrum::tracker::{ram_phys, rom_phys, BANK_SIZE};
use zx_rustrum::ui::ram_map::{chunks, hover_at, View, ROWS_PER_BANK};
use zx_rustrum::z80::Bus;

fn app_128() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: Some(vec![0x00; 0x8000]),
        rom_plus3: Some(vec![0x00; 0x10000]),
        rom_zx81: Some(vec![0x00; 0x2000]),
    };
    let mut app = App::with_roms(
        zx_rustrum::machine::Spectrum::with_model(Model::Spectrum128),
        String::new(),
        roms,
        None,
    );
    app.show_ram_map = true;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

#[test]
fn heat_follows_the_bank_not_the_address() {
    let mut app = app_128();
    // Write to $C000 with bank 1 paged in, then with bank 3.
    app.spec.bus.io_write(0x7ffd, 1);
    app.spec.bus.write(0xc000, 0xaa);
    app.spec.bus.io_write(0x7ffd, 3);
    app.spec.bus.write(0xc001, 0xbb);

    let t = &app.spec.bus.tracker;
    assert_eq!(t.write_count[ram_phys(1, 0)], 1, "bank 1 kept its write");
    assert_eq!(t.write_count[ram_phys(3, 1)], 1, "bank 3 got its own");
    assert_eq!(t.write_count[ram_phys(3, 0)], 0, "and not each other's");
    assert_eq!(t.write_count[ram_phys(1, 1)], 0);

    // A bank paged out keeps its history.
    app.spec.bus.io_write(0x7ffd, 0);
    assert!(app.spec.bus.tracker.write_heat[ram_phys(1, 0)] > 0);
}

#[test]
fn rom_pages_are_tracked_separately() {
    let mut app = app_128();
    app.spec.bus.io_write(0x7ffd, 0x00); // ROM 0
    app.spec.bus.read(0x0000);
    app.spec.bus.io_write(0x7ffd, 0x10); // ROM 1
    app.spec.bus.read(0x0001);

    let t = &app.spec.bus.tracker;
    assert_eq!(t.read_count[rom_phys(0, 0)], 1);
    assert_eq!(t.read_count[rom_phys(1, 1)], 1);
    assert_eq!(t.read_count[rom_phys(1, 0)], 0);
}

#[test]
fn the_all_memory_view_lists_every_bank_and_rom_page() {
    let mut app = app_128();
    app.ram.view = View::AllMemory;
    let blocks = chunks(&app);
    // A 128K has two ROM pages and eight RAM banks.
    assert_eq!(
        blocks.len(),
        10,
        "got {:?}",
        blocks.iter().map(|c| c.label.clone()).collect::<Vec<_>>()
    );
    assert_eq!(blocks[0].label, "ROM0");
    assert_eq!(blocks[1].label, "ROM1");
    assert_eq!(blocks[2].label, "RAM0");
    assert_eq!(blocks[9].label, "RAM7");
    // Blocks are stacked one after another, 64 rows each.
    for (i, c) in blocks.iter().enumerate() {
        assert_eq!(c.row, i * ROWS_PER_BANK);
    }

    // A 48K only has the three banks it can reach.
    let mut app48 = app_128();
    app48.switch_model(Model::Spectrum48);
    let blocks = chunks(&app48);
    let names: Vec<String> = blocks.iter().map(|c| c.label.clone()).collect();
    assert_eq!(names, vec!["ROM0", "RAM5", "RAM2", "RAM0"]);
}

#[test]
fn the_all_memory_view_says_where_each_block_is_paged() {
    let mut app = app_128();
    app.ram.view = View::AllMemory;
    app.spec.bus.io_write(0x7ffd, 0x13); // bank 3 at $C000, ROM 1

    let blocks = chunks(&app);
    let slot_of = |label: &str| {
        blocks
            .iter()
            .find(|c| c.label == label)
            .unwrap_or_else(|| panic!("no {label}"))
            .slot
    };
    assert_eq!(slot_of("ROM1"), Some(0), "ROM 1 answers at $0000");
    assert_eq!(slot_of("ROM0"), None, "ROM 0 is paged out");
    assert_eq!(slot_of("RAM5"), Some(1), "bank 5 is always at $4000");
    assert_eq!(slot_of("RAM2"), Some(2), "bank 2 is always at $8000");
    assert_eq!(slot_of("RAM3"), Some(3), "bank 3 was paged to $C000");
    assert_eq!(slot_of("RAM1"), None, "bank 1 is not paged in");
}

#[test]
fn hovering_the_all_memory_view_reports_the_bank_and_its_address() {
    let mut app = app_128();
    app.ram.view = View::AllMemory;
    app.spec.bus.io_write(0x7ffd, 0x01); // bank 1 at $C000

    // Row 0 of the RAM1 block: two ROM pages then banks 0 and 1.
    let ram1_row = 2 * ROWS_PER_BANK + ROWS_PER_BANK;
    let h = hover_at(&app, 0, ram1_row).expect("should be over RAM1");
    assert_eq!(h.what, "RAM1");
    assert_eq!(h.phys, ram_phys(1, 0));
    assert_eq!(h.addr, Some(0xc000), "bank 1 is paged in at $C000");

    // Bank 6 is not paged in, so it has no address.
    let ram6_row = 2 * ROWS_PER_BANK + 6 * ROWS_PER_BANK + 3;
    let h = hover_at(&app, 5, ram6_row).expect("should be over RAM6");
    assert_eq!(h.what, "RAM6");
    assert_eq!(h.addr, None);
    assert_eq!(h.phys, ram_phys(6, 3 * 256 + 5));
    assert!(h.phys < 8 * BANK_SIZE);
}

#[test]
fn the_view_can_be_switched_from_the_window() {
    let mut h = Harness::builder()
        .with_size([1500.0, 1400.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app_128());
    h.run_steps(3);
    assert!(matches!(h.state().ram.view, View::AddressSpace));

    h.get_by_label("All memory").click();
    h.run_steps(3);
    assert!(matches!(h.state().ram.view, View::AllMemory));

    h.get_by_label("Address space").click();
    h.run_steps(3);
    assert!(matches!(h.state().ram.view, View::AddressSpace));
}
