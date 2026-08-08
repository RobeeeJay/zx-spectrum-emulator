//! The cassette drawn in the tape window: how the packs wind from one reel to
//! the other, and how fast the hubs turn doing it.

use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::tape::{zx81_block, zx81_name, Block, Tape};
use zx_rustrum::ui::cassette::{overall_progress, reel_scales, spin_rate, written_name};
use zx_rustrum::ui::{App, Roms};

#[test]
fn the_left_reel_starts_full_and_the_right_one_nearly_empty() {
    let (left, right) = reel_scales(0.0);
    assert_eq!(left, 1.0, "the supply reel holds all the tape at the start");
    assert_eq!(right, 0.6, "and the take-up reel only its own hub");
}

#[test]
fn by_the_end_they_have_swapped_over() {
    let (left, right) = reel_scales(1.0);
    assert_eq!(left, 0.6);
    assert_eq!(right, 1.0);
}

#[test]
fn the_packs_change_place_smoothly_and_in_step() {
    // Whatever has left one reel has arrived on the other, so the two always
    // add up to the same amount of tape.
    let total = reel_scales(0.0).0 + reel_scales(0.0).1;
    let mut last = reel_scales(0.0);
    for step in 1..=20 {
        let p = step as f32 / 20.0;
        let (left, right) = reel_scales(p);
        assert!(
            (left + right - total).abs() < 1e-5,
            "tape went missing at {p}"
        );
        assert!(left < last.0, "the supply reel should only ever shrink");
        assert!(right > last.1, "and the take-up reel only grow");
        last = (left, right);
    }
    assert_eq!(
        reel_scales(0.5),
        (0.8, 0.8),
        "level pegging halfway through"
    );
}

#[test]
fn progress_outside_the_tape_does_not_send_the_reels_silly() {
    assert_eq!(reel_scales(-1.0), reel_scales(0.0));
    assert_eq!(reel_scales(9.9), reel_scales(1.0));
}

#[test]
fn a_small_pack_turns_faster_than_a_fat_one() {
    // The tape moves at a constant speed, so the hub with less on it has to
    // turn faster to take up the same length.
    let (left, right) = reel_scales(0.0);
    assert!(
        spin_rate(right, false) > spin_rate(left, false),
        "the empty take-up reel should be spinning fastest at the start"
    );
    let half = reel_scales(0.5);
    assert_eq!(
        spin_rate(half.0, false),
        spin_rate(half.1, false),
        "with equal packs the hubs keep pace"
    );
    assert!(spin_rate(1.0, false) > 0.0);
}

#[test]
fn boosting_the_tape_winds_the_hubs_on_faster() {
    // Half speed while the tape plays at its own pace, half again above that
    // when it is being hurried along, so the picture matches the sound.
    let slow = spin_rate(1.0, false);
    let fast = spin_rate(1.0, true);
    assert!(
        (fast / slow - 3.0).abs() < 1e-5,
        "boosted should be 1.5 against 0.5"
    );
}

#[test]
fn the_label_carries_the_tapes_name_without_the_extension() {
    assert_eq!(written_name("Manic Miner.tzx"), "Manic Miner");
    assert_eq!(written_name("tapes/zx81/JetPac.p"), "JetPac");
    assert_eq!(written_name("no extension"), "no extension");
}

#[test]
fn progress_counts_the_block_being_played_as_well_as_the_ones_before_it() {
    let blocks: Vec<Block> = (0..4)
        .map(|_| Block::Standard {
            pause_ms: 0,
            data: vec![0xff; 400],
        })
        .collect();
    let mut tape = Tape::from_blocks("t".into(), blocks);
    assert_eq!(overall_progress(&tape), 0.0);

    tape.seek(2);
    let at_block = overall_progress(&tape);
    assert!(
        (at_block - 0.5).abs() < 0.01,
        "two blocks of four in, expected about half, got {at_block}"
    );

    // Playing into that block should move it on, but not as far as the next.
    tape.play(0);
    tape.level_at(400_000);
    let within = overall_progress(&tape);
    assert!(
        within > at_block && within < 0.75,
        "part way through block 3 should be between 0.50 and 0.75, got {within}"
    );
}

#[test]
fn a_zx81_tape_reads_as_one_long_block() {
    let block = zx81_block(&zx81_name("CHESS"), &[0; 500]);
    let mut tape = Tape::from_blocks("chess.p".into(), vec![block]);
    tape.play(0);
    tape.level_at(20_000_000);
    let p = overall_progress(&tape);
    assert!(
        (0.05..0.95).contains(&p),
        "a single block should still show progress within itself, got {p}"
    );
}

// ---- in the window ---------------------------------------------------------

fn app_with_tape() -> App {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_profiler = false;
    app.show_tape = true;
    app.running = false;
    let block = Block::Standard {
        pause_ms: 0,
        data: vec![0xff; 800],
    };
    app.set_tape(Some(Tape::from_blocks(
        "Manic Miner.tzx".into(),
        vec![block],
    )));
    app
}

#[test]
fn the_hubs_only_turn_while_the_tape_is_moving() {
    let app = app_with_tape();
    let mut h = Harness::builder()
        .with_size([900.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    let still = (h.state().tape.left_spin, h.state().tape.right_spin);
    h.run_steps(5);
    assert_eq!(
        (h.state().tape.left_spin, h.state().tape.right_spin),
        still,
        "a stopped tape should leave the cogs where they are"
    );

    let now = h.state().machine_t();
    h.state_mut().tape_mut().unwrap().play(now);
    h.run_steps(5);
    let moved = (h.state().tape.left_spin, h.state().tape.right_spin);
    assert!(moved.0 > still.0, "the left cog should have turned");
    assert!(moved.1 > still.1, "and the right one too");
    assert!(
        moved.1 > moved.0,
        "the take-up reel starts nearly empty, so it turns faster"
    );
}

#[test]
fn progress_goes_by_time_rather_than_by_block() {
    // A header is nineteen bytes and the data after it several thousand, so
    // counting blocks would put the reels a quarter of the way through a tape
    // that has barely started.
    let header = Block::Standard {
        pause_ms: 0,
        data: vec![0x00; 19],
    };
    let data = Block::Standard {
        pause_ms: 0,
        data: vec![0xff; 6914],
    };
    let mut tape = Tape::from_blocks("t".into(), vec![header.clone(), data.clone(), header, data]);

    tape.seek(1);
    let after_first_header = overall_progress(&tape);
    assert!(
        after_first_header < 0.1,
        "a header is a moment of the tape, not a quarter of it: {after_first_header}"
    );

    tape.seek(2);
    let halfway = overall_progress(&tape);
    assert!(
        (0.4..0.6).contains(&halfway),
        "the first header and data are about half the tape: {halfway}"
    );
}

#[test]
fn the_mark_is_a_rainbow_on_a_dark_shell() {
    // The window icon is drawn rather than shipped, so it is worth checking it
    // comes out as something rather than an empty square.
    let size = 64;
    let rgba = zx_rustrum::logo::rgba(size);
    assert_eq!(rgba.len(), size * size * 4);

    // The corners are outside the rounded shell, so they stay transparent.
    let at = |x: usize, y: usize| {
        let i = (y * size + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    assert_eq!(at(0, 0)[3], 0, "the corner should be cut away");
    assert_eq!(at(size / 2, size / 2)[3], 255, "the middle should be solid");

    // Every one of the seven colours should appear somewhere.
    for colour in zx_rustrum::logo::RAINBOW {
        let found = rgba
            .chunks(4)
            .any(|p| p[0] == colour[0] && p[1] == colour[1] && p[2] == colour[2] && p[3] == 255);
        assert!(found, "{colour:?} is missing from the mark");
    }
}
