//! The tape window's two speed switches.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::tape::{Block, Tape};
use zx_rustrum::ui::{App, Roms};

fn harness<'a>() -> Harness<'a, App> {
    // A stand-in ZX81 ROM, so the machine can actually be switched to one.
    let roms = Roms {
        rom_zx81: Some(vec![0x00; 0x2000]),
        ..Roms::default()
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    // The transport controls are inert with an empty deck, as they should be.
    app.spec.bus.tape = Some(Tape::from_blocks(
        "test".into(),
        vec![Block::Standard {
            pause_ms: 1000,
            data: vec![0xFF, 1, 2, 3],
        }],
    ));
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = true;
    app.running = false;
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    h
}

/// Ludicrous speed is a switch of its own, and it brings Max speed with it:
/// a game with a loader of its own cannot be handed its blocks, so it still
/// wants the machine run as fast as it will go.
#[test]
fn ludicrous_speed_switches_on_and_brings_max_speed_with_it() {
    let mut h = harness();
    h.state_mut().spec.bus.tape_boost = false;
    h.run_steps(2);
    assert!(!h.state().tape_flash(), "it should start switched off");

    h.get_by_label("Ludicrous speed").click();
    h.run_steps(2);
    assert!(h.state().tape_flash(), "the switch did nothing");
    assert!(
        h.state().tape_boost(),
        "and it should have brought max speed with it"
    );

    h.get_by_label("Ludicrous speed").click();
    h.run_steps(2);
    assert!(!h.state().tape_flash(), "it should switch off again");
}

/// The ZX81's ROM is a different one and has no LD-BYTES to answer, so there
/// is nothing to offer.
#[test]
fn a_zx81_is_not_offered_it() {
    let mut h = harness();
    h.state_mut().switch_to_zx81(zx_rustrum::zx81::Ram::K16);
    h.run_steps(3);

    assert!(
        h.get_by_label("Ludicrous speed")
            .accesskit_node()
            .is_disabled(),
        "a ZX81 has no such routine to hand blocks to"
    );
}
