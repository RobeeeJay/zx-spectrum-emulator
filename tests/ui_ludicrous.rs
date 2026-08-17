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

/// One of three rather than two switches: picking one puts the others out, and
/// picking the fastest brings the machine's own speed with it, since a game
/// with a loader of its own has to be played to whatever else happens.
#[test]
fn the_three_speeds_are_one_at_a_time() {
    let mut h = harness();
    h.state_mut().spec.bus.tape_boost = false;
    h.run_steps(2);
    assert!(
        !h.state().tape_flash() && !h.state().tape_boost(),
        "it should start at the tape's own speed"
    );

    // Straight from Normal to the fastest, so that bringing the machine's own
    // speed with it is this button's doing and not the one before it.
    h.get_by_label("Ludicrous").click();
    h.run_steps(2);
    assert!(
        h.state().tape_flash() && h.state().tape_boost(),
        "Ludicrous should do both: a game with its own loader still has to be \
         played to"
    );

    h.get_by_label("Max").click();
    h.run_steps(2);
    assert!(
        h.state().tape_boost() && !h.state().tape_flash(),
        "Max should hurry the machine and hand nothing over"
    );

    h.get_by_label("Normal").click();
    h.run_steps(2);
    assert!(
        !h.state().tape_flash() && !h.state().tape_boost(),
        "and Normal should put both away"
    );
}

/// The transport is the deck's own buttons; the three speeds are about the
/// emulator rather than about the tape, so they are a row of their own with
/// their own label rather than the tail of the transport row.
///
/// The geometry alone cannot say this: the tape window is narrower than the
/// two rows together, so the switches wrapped below the transport even when
/// they were part of it. What the row is worth testing for is that it is one
/// row — its label and all three of them on a line, and none of the deck's
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
    // Anchored on Ludicrous: the main window's speed dropdown has a "Max" of
    // its own, and the tape window's is not the only one in the tree.
    let switches = top("Ludicrous");
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
        (top("Normal") - switches).abs() < 0.5 && (top("Max") - switches).abs() < 0.5,
        "and all three should be on it"
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
        h.get_by_label("Ludicrous").accesskit_node().is_disabled(),
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
    h.state_mut().quality.wow = 0.03;
    h.state_mut().quality.alignment_offset = 0.15;
    h.state_mut().advance(1.0 / 50.0);
    let deck = h.state().spec.bus.tape.as_ref().expect("a tape").quality;
    assert!(
        deck.speed && deck.alignment,
        "the deck should have been told: {deck:?}"
    );
    assert!(
        (deck.wow - 0.03).abs() < 0.001 && (deck.alignment_offset - 0.15).abs() < 0.001,
        "and told how far: {deck:?}"
    );
}
