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
    h.get_by_label("Fastload").click();
    h.run_steps(2);
    assert!(
        h.state().tape_flash() && h.state().tape_boost(),
        "Fastload should do both: a game with its own loader still has to be \
         played to"
    );

    h.get_by_label("Max CPU").click();
    h.run_steps(2);
    assert!(
        h.state().tape_boost() && !h.state().tape_flash(),
        "Max CPU should hurry the machine and hand nothing over"
    );

    h.get_by_label("Normal").click();
    h.run_steps(2);
    assert!(
        !h.state().tape_flash() && !h.state().tape_boost(),
        "and Normal should put both away"
    );
}

/// The top of the window is two rows, and five with the deck's failings on
/// show: what the deck is doing, how fast it is being got through, and then
/// one row for each of the three things a real deck does wrong.
///
/// The geometry alone cannot say this — the window is narrower than any two of
/// the rows together, so they would stack anyway — so what is asked is that
/// each label is on a line with the things that belong to it, in order.
#[test]
fn the_top_of_the_window_is_five_labelled_rows() {
    let mut h = harness();
    assert!(
        h.query_by_label("Alignment").is_none(),
        "the deck's failings should be put away until they are asked for"
    );
    h.get_by_label("Quality").click();
    h.run_steps(2);
    let h = h;
    let top = |label: &str| -> f32 {
        h.get_by_label(label)
            .accesskit_node()
            .bounding_box()
            .expect("it should be somewhere")
            .y0 as f32
    };
    // A plain label's text is in the node's value rather than its label, and
    // group labels are drawn in capitals. The main window has rows of its own,
    // so what is asked is whether *one* of each is on the row in question.
    let rows_of = |text: &str| -> Vec<f32> {
        h.root()
            .children_recursive()
            .filter_map(|node| {
                let node = node.accesskit_node();
                (node.value().as_deref() == Some(text)).then(|| node.bounding_box())?
            })
            .map(|box_| box_.y0 as f32)
            .collect()
    };
    let labelled =
        |text: &str, row: f32| -> bool { rows_of(text).iter().any(|y| (y - row).abs() < 0.5) };

    let playback = top("|◀ Start");
    let speed = top("Fastload");
    let alignment = top("Alignment");
    let noise = top("Noise");
    let quality = top("Wobble");

    assert!(
        labelled("PLAYBACK", playback) && labelled("SPEED", speed),
        "the transport and the speeds should each have their own label"
    );
    assert!(
        (top("Quality") - speed).abs() < 0.5,
        "and the switch that shows the failings belongs with the speeds"
    );
    for label in ["▶▶ Forward", "■ Stop"] {
        assert!(
            (top(label) - playback).abs() < 0.5,
            "{label} belongs with the transport"
        );
    }
    for label in ["Normal", "Max CPU"] {
        assert!(
            (top(label) - speed).abs() < 0.5,
            "{label} belongs with the speeds"
        );
    }
    assert!(
        playback < speed && speed < quality && quality < alignment && alignment < noise,
        "and they should be in that order down the window: {playback} {speed} \
         {quality} {alignment} {noise}"
    );
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
    h.get_by_label("Quality").click();
    h.run_steps(2);

    h.get_by_label("Wobble").click();
    h.run_steps(2);
    assert!(h.state().quality.wobble, "the Wobble switch did nothing");

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
