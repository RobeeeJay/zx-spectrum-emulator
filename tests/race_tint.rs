//! Marking writes by which side of the beam they landed on.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::{Model, Spectrum, Tint, Tints};
use zx_rustrum::screen::{self, Fade, Tinting, View};
use zx_rustrum::ui::{App, Roms};
use zx_rustrum::z80::Bus;

const VIEW: View = View::CROPPED;

/// The address of the first byte of a display line.
fn line_addr(line: u16) -> u16 {
    0x4000 | ((line & 0xc0) << 5) | ((line & 0x07) << 8) | ((line & 0x38) << 2)
}

/// A machine with the marks switched on and the beam part way down line 100.
fn machine_at_line(line: u16) -> Spectrum {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.tints = Some(Tints::default());
    spec.bus.tstates = spec.bus.first_pixel_t() + line as u32 * spec.bus.model.t_per_line() + 100;
    spec.bus.catch_up_painting();
    spec
}

/// Writing to a part of the picture the beam has been over already means the
/// change will not be seen until the next frame — which is what a flickering
/// sprite is. Writing ahead of the beam means it will be shown this frame.
/// Neither can be told from the finished picture, so the write is marked.
#[test]
fn writes_are_marked_by_which_side_of_the_beam_they_land_on() {
    let mut spec = machine_at_line(100);

    spec.bus.write(line_addr(50), 0xFF);
    spec.bus.write(line_addr(150), 0xFF);

    let tints = spec.bus.tints.as_ref().expect("marks are on");
    assert_eq!(
        tints.at(line_addr(50) - 0x4000).0,
        Tint::Late,
        "line 50 was written after the beam had passed it"
    );
    assert_eq!(
        tints.at(line_addr(150) - 0x4000).0,
        Tint::Early,
        "line 150 was written before the beam reached it"
    );
}

/// Once the beam has been over a write, the write has been shown and there is
/// nothing left to say about it, however recently it was made.
#[test]
fn the_beam_going_over_a_mark_clears_it() {
    let mut spec = machine_at_line(100);
    let addr = line_addr(150);
    spec.bus.write(addr, 0xFF);
    assert_eq!(
        spec.bus.tints.as_ref().unwrap().at(addr - 0x4000).0,
        Tint::Early,
        "it should be marked to begin with"
    );

    // The beam moves on past line 150.
    spec.bus.tstates = spec.bus.first_pixel_t() + 160 * spec.bus.model.t_per_line();
    spec.bus.catch_up_painting();

    assert_eq!(
        spec.bus.tints.as_ref().unwrap().at(addr - 0x4000).0,
        Tint::None,
        "the beam has shown it, so the mark should have gone"
    );
}

/// An attribute governs eight lines and is fetched again on each of them, so
/// it is late once the beam has started the row it belongs to.
#[test]
fn an_attribute_is_late_once_its_row_has_begun() {
    let mut spec = machine_at_line(100);
    // The attribute row line 100 is in, and one well below.
    let started = 0x5800 + (100 / 8) * 32;
    let to_come = 0x5800 + (180 / 8) * 32;

    spec.bus.write(started, 0x07);
    spec.bus.write(to_come, 0x07);

    let tints = spec.bus.tints.as_ref().unwrap();
    assert_eq!(tints.at(started - 0x4000).0, Tint::Late);
    assert_eq!(tints.at(to_come - 0x4000).0, Tint::Early);
}

fn drawn(spec: &Spectrum, over: u64, line: usize) -> [u8; 3] {
    let mut out = vec![0u8; VIEW.buffer_len()];
    let fade = Fade {
        // No phosphor fade in the way of the colours being checked.
        now: spec.bus.tstates,
        floor: 1.0,
    };
    let tinting = spec.bus.tints.as_ref().map(|tints| Tinting {
        tints,
        now: spec.bus.total_t(),
        over,
    });
    screen::render_fading(&spec.bus, VIEW, &mut out, false, fade, tinting);
    let at = ((VIEW.border_top + line) * VIEW.width() + VIEW.border_x + 4) * 4;
    [out[at], out[at + 1], out[at + 2]]
}

/// The marks are drawn: red for a write the beam has passed, green for one it
/// has not reached, blending into the colour the pixel should be over the two
/// seconds the user was given.
#[test]
fn a_mark_is_drawn_and_blends_into_the_colour_it_should_be() {
    let mut spec = machine_at_line(100);
    // White ink on black paper everywhere, so a set pixel is white and the
    // marks are the only other colour on screen.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x07);
    }
    spec.bus.write(line_addr(50), 0xFF); // behind the beam
    spec.bus.write(line_addr(150), 0xFF); // ahead of it

    let over = 28_000u64;
    // Not exactly the mark's colour: writing takes a few T-states, so the
    // blend has already begun by a fraction of a percent.
    let near = |got: [u8; 3], want: [u8; 3], what: &str| {
        for c in 0..3 {
            assert!(
                (got[c] as i32 - want[c] as i32).abs() < 12,
                "{what}: drawn {got:?}, expected about {want:?}"
            );
        }
    };
    near(
        drawn(&spec, over, 50),
        screen::LATE,
        "a write the beam had passed should be drawn as late",
    );
    near(
        drawn(&spec, over, 150),
        screen::EARLY,
        "a write ahead of the beam should be drawn as early",
    );

    // Half way through the blend, half way between the mark and the truth.
    spec.bus.tstates += over as u32 / 2;
    let half = drawn(&spec, over, 50);
    let white = screen::PALETTE[7];
    assert!(
        (half[0] as i32 - (screen::LATE[0] as i32 + white[0] as i32) / 2).abs() < 12
            && half[1] > 40
            && half[1] < white[1],
        "half way through the blend the colour should be half way there, not {half:?}"
    );

    // And after it, the colour it should be — still shown, because the beam
    // has not been over it since: a late write is not on the screen, and the
    // picture would pop back to what was there before it if the mark ended
    // when its colour did.
    spec.bus.tstates += over as u32;
    assert_eq!(
        drawn(&spec, over, 50),
        white,
        "once the blend is done the pixel should be its own colour"
    );

    // Until the beam comes round to it, next frame, and the picture is simply
    // the picture again.
    spec.bus.tstates = 0;
    spec.bus.frame += 1;
    spec.bus.end_frame();
    spec.bus.tstates = spec.bus.first_pixel_t() + 60 * spec.bus.model.t_per_line();
    spec.bus.catch_up_painting();
    assert_eq!(
        spec.bus
            .tints
            .as_ref()
            .unwrap()
            .at(line_addr(50) - 0x4000)
            .0,
        Tint::None,
        "the beam has been over it, so the mark should have gone"
    );
}

/// The marks are only kept while somebody is looking at them: every write to
/// the display file pays for them.
#[test]
fn the_marks_are_kept_only_while_racing() {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    let mut h = Harness::builder()
        .with_size([1500.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(
        h.state().spec.bus.tints.is_none(),
        "nothing should be marking writes until asked"
    );

    h.get_by_label("Race the Beam").click();
    h.run_steps(2);
    assert!(
        h.state().spec.bus.tints.is_some(),
        "racing should mark the writes"
    );

    h.get_by_label("Race the Beam").click();
    h.run_steps(2);
    assert!(
        h.state().spec.bus.tints.is_none(),
        "the marks should be put away with the mode"
    );
}
