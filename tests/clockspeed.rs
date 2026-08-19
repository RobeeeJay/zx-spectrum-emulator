//! The CPU's clock, in the window: an accelerator rather than a new crystal.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::ui::{App, Roms, CLOCK_MULTIPLES};

fn harness<'a>() -> Harness<'a, App> {
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
    app.running = true;
    Harness::builder()
        .with_size([1800.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// The dropdown is the machine's own clock and multiples of it, worked out
/// from the model: a 48K's own is 3.50MHz and a 128K's 3.55, so "twice" is a
/// different number on each.
#[test]
fn the_clock_is_the_machines_own_and_multiples_of_it() {
    let mut h = harness();
    h.run_steps(3);
    assert_eq!(
        h.state().clock_mult,
        1.0,
        "it starts as the machine was built"
    );
    assert!(
        h.get_all_by_label_contains("3.50MHz").next().is_some(),
        "a 48K's own clock is 3.5MHz"
    );

    h.state_mut().switch_model(Model::Spectrum128);
    h.run_steps(3);
    assert!(
        h.get_all_by_label_contains("3.55MHz").next().is_some(),
        "and a 128K's is 3.5469"
    );
    h.state_mut().clock_mult = 2.0;
    assert!(
        (h.state().clock_hz() - 7_093_800.0).abs() < 1.0,
        "twice a 128K's is not twice a 48K's"
    );
}

/// The steps are doublings, because that is what the hardware offered.
#[test]
fn the_clock_steps_are_doublings() {
    assert_eq!(CLOCK_MULTIPLES, &[1.0, 2.0, 4.0, 8.0]);
    for pair in CLOCK_MULTIPLES.windows(2) {
        assert_eq!(pair[1], pair[0] * 2.0, "each is the one before it twice");
    }
}

/// A faster CPU does more inside the same frame rather than being given more
/// frames: the ULA is not wound up with it.
#[test]
fn a_faster_cpu_does_more_inside_the_same_frame() {
    let ran = |mult: f32| -> (u64, u32) {
        let mut h = harness();
        h.run_steps(3);
        h.state_mut().clock_mult = mult;
        let before_t = h.state().spec.bus.total_t();
        let before_ops = h.state().spec.bus.fetches;
        h.state_mut().advance(1.0 / 50.0);
        (
            h.state().spec.bus.total_t() - before_t,
            h.state().spec.bus.fetches - before_ops,
        )
    };

    let (plain_t, plain_ops) = ran(1.0);
    for mult in [2.0f32, 4.0, 8.0] {
        let (t, ops) = ran(mult);
        assert!(
            (t as f64 / plain_t as f64 - 1.0).abs() < 0.02,
            "the machine's own time should pass at the same rate at {mult}x: \
             {t} against {plain_t}"
        );
        let ratio = ops as f64 / plain_ops as f64;
        assert!(
            (ratio - mult as f64).abs() < 0.2,
            "and {mult}x should get about {mult} times as much done: {ratio:.2}"
        );
    }
}

/// The mixer is not told anything, because the samples are made of the
/// machine's T-states and those still pass at 3.5MHz.
///
/// A beeper note still comes out an octave up at 2×, because the loop that
/// makes it comes round in half the T-states — which is what an accelerated
/// machine sounded like, and falls out rather than being arranged.
#[test]
fn the_mixer_stays_on_the_machines_own_clock() {
    let mut h = harness();
    h.run_steps(3);
    let per_sample = |h: &Harness<'_, App>| h.state().spec.bus.audio.t_per_sample();

    let normal = per_sample(&h);
    h.state_mut().clock_mult = 4.0;
    h.state_mut().apply_clock();
    assert_eq!(
        per_sample(&h),
        normal,
        "the mixer counts the ULA's T-states, and those have not moved"
    );
}

/// A tape playing holds the CPU at the machine's own clock whatever the
/// dropdown says, and the window says why.
///
/// Every loader counts turns of its own loop against pulses that are in ULA
/// time, so at 4× it counts four times as many for the same pulse and nothing
/// loads at all. That is what an accelerated machine did — which is why they
/// had a switch, and here the switch throws itself.
#[test]
fn the_cpu_is_held_at_one_times_while_a_tape_plays() {
    let mut h = harness();
    h.run_steps(3);
    h.state_mut().clock_mult = 4.0;
    h.run_steps(2);
    assert_eq!(h.state().turbo(), 4, "nothing in the way to start with");
    assert!(h.state().turbo_held_because().is_none());
    assert_eq!(
        h.state().spec.bus.turbo,
        4,
        "and the machine is running at it"
    );

    let mut tape = zx_rustrum::tape::Tape::from_blocks(
        "t".into(),
        // Long enough that it is still running when it is looked at: a
        // hundred pulses is three frames, and the window draws more than that.
        vec![zx_rustrum::tape::Block::PureTone {
            len: 2168,
            count: 60_000,
        }],
    );
    tape.play(0);
    h.state_mut().spec.bus.tape = Some(tape);
    h.run_steps(2);
    assert_eq!(h.state().turbo(), 1, "held while the tape runs");
    assert_eq!(
        h.state().turbo_held_because(),
        Some("a tape is loading"),
        "and the window says why"
    );
    assert_eq!(
        h.state().spec.bus.turbo,
        1,
        "which is what the machine is actually running at"
    );

    // Stopped again, and it is back to what was asked for.
    h.state_mut().tape_mut().unwrap().stop();
    h.run_steps(2);
    assert_eq!(h.state().turbo(), 4);
    assert_eq!(h.state().spec.bus.turbo, 4);
}
