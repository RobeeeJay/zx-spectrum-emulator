//! The debugger and the RAM map, pointed at a ZX81 rather than a Spectrum.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_spectrum_emulator::machine::Spectrum;
use zx_spectrum_emulator::ui::{App, Roms};
use zx_spectrum_emulator::zx81::Ram;

fn zx81_app() -> Option<App> {
    let roms = Roms {
        rom_zx81: Some(std::fs::read("roms/zx81.rom").ok()?),
        ..Default::default()
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.switch_to_zx81(Ram::K16);
    app.on_zx81().then_some(app)
}

fn harness_for<'a>(app: App) -> Harness<'a, App> {
    Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

fn labels(h: &Harness<'_, App>) -> String {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut Vec<String>) {
        if let Some(l) = node.accesskit_node().label() {
            out.push(l.to_string());
        }
        for c in node.children() {
            walk(&c, out);
        }
    }
    let mut v = Vec::new();
    walk(&h.root(), &mut v);
    v.join("\n")
}

// ---- the debugger ----------------------------------------------------------

#[test]
fn the_debugger_shows_the_zx81s_registers_and_code() {
    let Some(mut app) = zx81_app() else {
        eprintln!("no ZX81 ROM; skipping");
        return;
    };
    app.show_debugger = true;
    app.running = true;
    let mut h = harness_for(app);
    h.run_steps(5);

    // The ROM has been running, so the program counter is somewhere in it and
    // the listing shows real instructions rather than a blank pane.
    let pc = h.state().cpu().pc;
    assert!(
        pc < 0x4000,
        "the ZX81 should be running its ROM, PC ${pc:04X}"
    );

    // The clock the debugger reads out is the ZX81's, not the Spectrum's.
    let (frame, t, frame_t) = h.state().machine_clock();
    assert_eq!(frame_t, 207 * 312, "a ZX81 frame, not a Spectrum one");
    assert!(t < frame_t);
    assert!(frame > 0, "the machine should have run");

    // And the listing is disassembled from ZX81 memory: $0000 is the ROM's
    // first instruction, OUT ($FD),A, which switches the NMI generator off.
    assert_eq!(h.state().peek(0x0000), 0xd3);
    assert_eq!(h.state().peek(0x0001), 0xfd);
}

#[test]
fn stepping_moves_the_zx81_on_one_instruction() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_debugger = true;
    app.running = true;
    let mut h = harness_for(app);
    h.run_steps(5);
    h.state_mut().running = false; // otherwise the frames in between run on
    h.run_steps(2);

    let before = (h.state().cpu().pc, h.state().cpu().instructions);
    h.get_by_label("⤓ Step into").click();
    h.run_steps(2);
    let after = (h.state().cpu().pc, h.state().cpu().instructions);

    assert_eq!(after.1, before.1 + 1, "exactly one instruction should run");
    assert_ne!(after.0, before.0, "and the program counter should move");
    assert!(!h.state().running, "stepping stops the machine");
}

#[test]
fn a_breakpoint_can_be_set_on_the_zx81() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_debugger = true;
    assert!(app.breakpoints().is_empty());
    app.breakpoints_mut().push(0x0283);
    assert_eq!(app.breakpoints(), &vec![0x0283]);
    // The Spectrum's list is a different one and stays empty.
    assert!(app.spec.breakpoints.is_empty());

    // It survives a repaint, and the window draws with it set.
    let mut h = harness_for(app);
    h.run_steps(3);
    assert_eq!(h.state().breakpoints(), &vec![0x0283]);
}

#[test]
fn the_debugger_hides_what_a_zx81_does_not_have() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_debugger = true;
    let mut h = harness_for(app);
    h.run_steps(3);
    let text = labels(&h);
    assert!(
        !text.contains("$7FFD"),
        "a ZX81 has no paging latch:\n{text}"
    );
    assert!(!text.contains("AY"), "and no sound chip:\n{text}");
}

// ---- the RAM map -----------------------------------------------------------

#[test]
fn the_ram_map_shows_the_zx81s_memory() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_ram_map = true;
    app.running = true;
    let mut h = harness_for(app);
    h.run_steps(5);

    // Something has been executed and read: the ROM has been running.
    let app = h.state();
    let rom_exec: u32 = (0..0x2000u16)
        .map(|a| app.tracker().exec_heat[app.phys_index(a)] as u32)
        .sum();
    assert!(rom_exec > 0, "no executed bytes were recorded in the ROM");
    let ram_write: u32 = (0x4000..0x8000u16)
        .map(|a| app.tracker().write_heat[app.phys_index(a)] as u32)
        .sum();
    assert!(ram_write > 0, "the ROM writes to RAM as it boots");
}

#[test]
fn the_ram_map_describes_zx81_addresses() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_ram_map = true;
    assert_eq!(app.slot_label(0x0000), "ROM");
    assert_eq!(app.slot_label(0x4000), "RAM");
    assert!(app.is_rom(0x1234));
    assert!(!app.is_rom(0x4321));
    // The mirror above $8000 is the same memory, so it maps to the same byte.
    assert_eq!(app.phys_index(0x4000), app.phys_index(0xc000));
    assert_eq!(app.phys_index(0x0000), app.phys_index(0x8000));
}

#[test]
fn the_ram_map_lists_one_rom_and_one_ram_for_a_zx81() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_ram_map = true;
    let chunks = zx_spectrum_emulator::ui::ram_map::chunks(&app);
    let names: Vec<&str> = chunks.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(names, vec!["ROM", "RAM"], "a ZX81 has nothing to page");
}

#[test]
fn the_heat_maps_fade_on_a_zx81() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_ram_map = true;
    app.running = true;
    let mut h = harness_for(app);
    h.run_steps(5);

    let hottest = |app: &App| -> u8 {
        (0..0x2000u16)
            .map(|a| app.tracker().exec_heat[app.phys_index(a)])
            .max()
            .unwrap_or(0)
    };
    let running_hot = hottest(h.state());
    assert!(
        running_hot > 200,
        "the ROM is being executed, so it should be lit up: {running_hot}"
    );

    // Stop the machine: nothing is touched now, so the marks must fade away
    // rather than staying lit for good.
    h.state_mut().running = false;
    h.run_steps(3);
    let after_a_moment = hottest(h.state());
    assert!(
        after_a_moment < running_hot,
        "the heat did not fade at all: still {after_a_moment}"
    );

    for _ in 0..60 {
        h.step();
    }
    let later = hottest(h.state());
    assert!(
        later < after_a_moment,
        "the heat stopped fading at {after_a_moment}, now {later}"
    );
}

#[test]
fn the_address_space_lights_up_where_the_zx81_has_been() {
    let Some(mut app) = zx81_app() else {
        return;
    };
    app.show_ram_map = true;
    app.running = true;
    let mut h = harness_for(app);
    h.run_steps(5);

    // One pixel per byte of the address space, RGBA: red is writes, green
    // reads, blue executes, over a dim floor that marks ROM from RAM.
    let image = h.state().ram.image().to_vec();
    assert_eq!(image.len(), 65536 * 4, "one pixel per byte");

    let lit = |from: usize, to: usize, channel: usize| -> usize {
        (from..to)
            .filter(|addr| image[addr * 4 + channel] > 40)
            .count()
    };
    assert!(
        lit(0x0000, 0x2000, 2) > 0,
        "no executed bytes shown in the ROM"
    );
    assert!(lit(0x0000, 0x2000, 1) > 0, "no reads shown in the ROM");
    assert!(lit(0x4000, 0x8000, 0) > 0, "no writes shown in the RAM");
    // The mirror above $8000 is the same memory, so it lights up with it.
    assert!(
        lit(0x8000, 0xa000, 2) > 0,
        "the mirror should show the ROM's use"
    );
}
