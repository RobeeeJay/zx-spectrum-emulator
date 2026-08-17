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

/// The transport is the deck's own buttons; the two speed switches are about
/// the emulator rather than about the tape, so they are a row of their own
/// with their own label rather than the tail of the transport row.
///
/// The geometry alone cannot say this: the tape window is narrower than the
/// two rows together, so the switches wrapped below the transport even when
/// they were part of it. What the row is worth testing for is that it is one
/// row — its label and both switches on a line, and none of the deck's
/// buttons with them.
#[test]
fn the_speed_switches_are_a_row_of_their_own() {
    let h = harness();
    let top = |label: &str| -> f32 {
        h.get_by_label(label)
            .accesskit_node()
            .bounding_box()
            .expect("it should be somewhere")
            .y0 as f32
    };

    // A plain label's text is in the node's value rather than its label, so
    // the row's own label is looked for by walking the tree. Group labels are
    // drawn in capitals, and the main window has a Speed group of its own, so
    // what is being asked is whether *one* of them is on this row.
    let switches = top("Max speed");
    let labelled = h
        .root()
        .children_recursive()
        .filter_map(|node| {
            let node = node.accesskit_node();
            (node.value().as_deref() == Some("SPEED")).then(|| node.bounding_box())?
        })
        .any(|box_| (box_.y0 as f32 - switches).abs() < 0.5);
    assert!(
        labelled,
        "the row the speed switches are on should have its own label"
    );
    assert!(
        (top("Ludicrous speed") - switches).abs() < 0.5,
        "and both switches should be on it"
    );

    let row = switches;
    for label in ["|◀ Start", "▶ Play", "■ Stop", "▶▶ Forward"] {
        assert!(
            (top(label) - row).abs() > 0.5,
            "{label} belongs to the transport and should not be on the speed row"
        );
    }
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

/// The Quality row: two switches for the deck's failings, and the sliders that
/// say how bad each is.
#[test]
fn the_quality_row_sets_the_decks_failings() {
    let mut h = harness();
    assert!(!h.state().quality.speed, "it should start behaving");
    assert!(!h.state().quality.alignment);

    h.get_by_label("Speed").click();
    h.run_steps(2);
    assert!(h.state().quality.speed, "the Speed switch did nothing");

    h.get_by_label("Alignment").click();
    h.run_steps(2);
    assert!(
        h.state().quality.alignment,
        "the Alignment switch did nothing"
    );

    // What the sliders are set to reaches the deck, which is where the pulses
    // are made.
    h.state_mut().quality.speed_wobble = 0.07;
    h.state_mut().quality.alignment_offset = 0.15;
    h.state_mut().advance(1.0 / 50.0);
    let deck = h.state().spec.bus.tape.as_ref().expect("a tape").quality;
    assert!(
        deck.speed && deck.alignment,
        "the deck should have been told: {deck:?}"
    );
    assert!(
        (deck.speed_wobble - 0.07).abs() < 0.001 && (deck.alignment_offset - 0.15).abs() < 0.001,
        "and told how far: {deck:?}"
    );
}
