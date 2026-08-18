//! Running the machine at more than its own clock.

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
        .with_size([1500.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app)
}

/// The clock is offered in MHz, starting at whatever the machine's own is.
///
/// A machine's clock is what everything about it is counted in, so the list is
/// the machine's own and multiples of it rather than a set of numbers that
/// happen to be round: a 48K's own is 3.50MHz and a 128K's 3.55, and "twice"
/// is a different number on each.
#[test]
fn the_clock_is_the_machines_own_and_multiples_of_it() {
    let mut h = harness();
    h.run_steps(3);
    assert_eq!(h.state().clock_mult, 1.0, "it starts as it was built");
    // The dropdown carries its selection in its label, with the arrow after
    // it, so it is matched on what it contains.
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
    assert!(
        (h.state().machine_cpu_hz() - 3_546_900.0).abs() < 1.0,
        "measured from the model rather than assumed"
    );
}

/// Picking a faster clock runs the machine faster: the same host frame does
/// twice the T-states at twice the clock.
#[test]
fn a_faster_clock_does_more_work_in_the_same_time() {
    let mut h = harness();
    h.run_steps(3);

    let ran = |h: &mut Harness<'_, App>| -> u64 {
        let before = h.state().spec.bus.total_t();
        h.state_mut().advance(1.0 / 50.0);
        h.state().spec.bus.total_t() - before
    };

    let normal = ran(&mut h);
    h.state_mut().clock_mult = 2.0;
    let doubled = ran(&mut h);
    h.state_mut().clock_mult = 8.0;
    let eightfold = ran(&mut h);

    assert!(
        (doubled as f64 / normal as f64 - 2.0).abs() < 0.05,
        "twice the clock is twice the work: {doubled} against {normal}"
    );
    assert!(
        (eightfold as f64 / normal as f64 - 8.0).abs() < 0.15,
        "and eight times is eight: {eightfold} against {normal}"
    );
}

/// The mixer is told, so sound comes out at the pitch the machine is running
/// at rather than the pitch it was built for.
///
/// A beeper note is a number of T-states between one toggle and the next. Run
/// the machine at twice its clock and those T-states take half as long, so the
/// note is an octave up — which is what an accelerated machine sounded like,
/// and only happens if the mixer is counting in the same T-states.
#[test]
fn the_mixer_is_told_the_clock_has_changed() {
    let mut h = harness();
    h.run_steps(3);
    let per_sample = |h: &Harness<'_, App>| h.state().spec.bus.audio.t_per_sample();

    let normal = per_sample(&h);
    h.state_mut().clock_mult = 4.0;
    h.state_mut().apply_clock();
    let faster = per_sample(&h);
    assert!(
        (faster / normal - 4.0).abs() < 0.01,
        "four times the clock puts four times the T-states in a sample: \
         {faster} against {normal}"
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
