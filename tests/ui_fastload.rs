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

/// Two switches rather than three: picking one puts the other out, picking the
/// same one again puts it away, and neither on is the tape's own speed. The
/// fastest brings the machine's own speed with it, since a game with a loader
/// of its own has to be played to whatever else happens.
#[test]
fn the_speeds_are_one_at_a_time_and_switch_off_again() {
    let mut h = harness();
    h.state_mut().spec.bus.tape_boost = false;
    h.run_steps(2);
    assert!(
        !h.state().tape_flash() && !h.state().tape_boost(),
        "it should start at the tape's own speed"
    );
    assert!(
        h.query_by_label("Normal").is_none(),
        "there is no button for the tape's own speed: it is neither switch on"
    );

    // Straight to the fastest, so that bringing the machine's own speed with
    // it is this button's doing and not another's.
    h.get_by_label("Fastload").click();
    h.run_steps(2);
    assert!(
        h.state().tape_flash() && h.state().tape_boost(),
        "Fastload should do both: a game with its own loader still has to be \
         played to"
    );

    h.get_by_label("Max").click();
    h.run_steps(2);
    assert!(
        h.state().tape_boost() && !h.state().tape_flash(),
        "Max should hurry the machine and hand nothing over"
    );

    h.get_by_label("Max").click();
    h.run_steps(2);
    assert!(
        !h.state().tape_flash() && !h.state().tape_boost(),
        "and pressing it again should put it away"
    );
}

/// The speeds sit with the transport rather than on a row of their own.
///
/// They are how fast the tape is got through, which is part of working the
/// deck; the row they had to themselves cost a line of a window whose height
/// the block list is what is left of.
#[test]
fn the_speeds_sit_with_the_transport() {
    let h = harness();
    let top = |label: &str| -> f32 {
        h.get_by_label(label)
            .accesskit_node()
            .bounding_box()
            .expect("it should be somewhere")
            .y0 as f32
    };

    // Anchored on Fastload: the main window's speed dropdown has a "Max" of
    // its own, and the tape window's is not the only one in the tree.
    let row = top("Fastload");
    for label in ["|◀ Start", "▶ Play", "■ Stop", "▶▶ Forward"] {
        assert!(
            (top(label) - row).abs() < 0.5,
            "{label} and the speeds should be on one row"
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
        h.get_by_label("Fastload").accesskit_node().is_disabled(),
        "a ZX81 has no such routine to hand blocks to"
    );
}

/// The Quality row: what the deck does wrong, and how much of it.
///
/// The motor has no switch of its own — two sliders at nothing is a motor that
/// is behaving — so what is tested is that the sliders reach the deck.
#[test]
fn the_quality_row_sets_the_decks_failings() {
    let mut h = harness();
    assert!(!h.state().quality.alignment, "it should start behaving");
    assert!(
        h.query_by_label("Speed").is_none(),
        "the motor's switch is gone: its sliders say it"
    );

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
    assert!(deck.alignment, "the deck should have been told: {deck:?}");
    assert!(
        (deck.wow - 0.03).abs() < 0.001 && (deck.alignment_offset - 0.15).abs() < 0.001,
        "and told how far: {deck:?}"
    );
}
