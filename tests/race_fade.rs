//! Five seconds a frame, with the picture fading behind the beam.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::screen::{self, Fade, View};
use zx_rustrum::ui::{App, Roms, RACE_SPEED};

const VIEW: View = View::CROPPED;

fn colour_at(buf: &[u8], x: usize, y: usize) -> [u8; 3] {
    let at = (y * VIEW.width() + x) * 4;
    [buf[at], buf[at + 1], buf[at + 2]]
}

/// A machine with a screenful of white, painted for a whole frame, so every
/// pixel of the picture is the same colour and only the fade tells them apart.
fn white_screen() -> Spectrum {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00); // NOPs
    spec.cpu.pc = 0;
    for o in 0..0x1800u16 {
        spec.bus.poke(0x4000 + o, 0xFF);
    }
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x07); // white ink on black paper
    }
    // Two frames, so what the beam has painted is white from top to bottom.
    for _ in 0..2 {
        let frame = spec.bus.frame;
        while spec.bus.frame == frame {
            spec.step_instruction();
        }
    }
    spec
}

/// How bright a line is drawn, as a fraction of full white.
fn brightness(spec: &Spectrum, fade: Fade, line: usize) -> f32 {
    let mut out = vec![0u8; VIEW.buffer_len()];
    screen::render_fading(&spec.bus, VIEW, &mut out, false, fade);
    let lit = colour_at(&out, VIEW.border_x + 8, VIEW.border_top + line);
    lit[0] as f32 / screen::PALETTE[7][0] as f32
}

/// A phosphor is brightest where the beam has just been and dimmest where it
/// is about to arrive, and that is the whole point: the gradient down the
/// screen says how long ago each part of the picture was drawn.
#[test]
fn the_picture_fades_behind_the_beam() {
    let mut spec = white_screen();
    // The beam half way down the display.
    let half = spec.bus.first_pixel_t() + 96 * spec.bus.model.t_per_line();
    while spec.bus.tstates < half {
        spec.step_instruction();
    }
    spec.bus.catch_up_painting();
    let fade = Fade {
        now: spec.bus.tstates,
        floor: 0.5,
    };

    // The oldest part of the picture is the line just *below* the beam: it
    // was drawn a whole frame ago bar a line or two, and is about to be drawn
    // again. The bottom of the screen is younger than that — the beam spends
    // a quarter of the frame below the display, in the border and the sync.
    let just_behind = brightness(&spec, fade, 94);
    let long_ago = brightness(&spec, fade, 4);
    let about_to_be_drawn = brightness(&spec, fade, 98);

    assert!(
        just_behind > 0.95,
        "the line the beam has just passed should be near full brightness, \
         not {just_behind}"
    );
    assert!(
        (0.6..0.9).contains(&long_ago),
        "the top of the screen was drawn most of a frame ago and should be \
         part way down to the floor, not {long_ago}"
    );
    assert!(
        about_to_be_drawn < 0.53,
        "the line the beam is about to reach has waited nearly a whole frame \
         and should be at the floor, not {about_to_be_drawn}"
    );
    assert!(
        just_behind > long_ago && long_ago > about_to_be_drawn,
        "brightness should fall the further back the beam is: {just_behind}, \
         {long_ago}, {about_to_be_drawn}"
    );
}

/// How far it fades is the slider's business.
#[test]
fn the_slider_says_how_far_a_pixel_fades() {
    let mut spec = white_screen();
    let half = spec.bus.first_pixel_t() + 96 * spec.bus.model.t_per_line();
    while spec.bus.tstates < half {
        spec.step_instruction();
    }
    spec.bus.catch_up_painting();

    for floor in [0.0f32, 0.25, 0.75] {
        let fade = Fade {
            now: spec.bus.tstates,
            floor,
        };
        let oldest = brightness(&spec, fade, 98);
        assert!(
            (oldest - floor).abs() < 0.06,
            "with the slider at {floor} the oldest line came out at {oldest}"
        );
    }
}

/// Nothing fades unless racing: at fifty frames a second the trail would be a
/// flicker, and every other view of the picture is of a machine that is not
/// being watched a frame at a time.
#[test]
fn the_ordinary_picture_does_not_fade() {
    let spec = white_screen();
    let mut out = vec![0u8; VIEW.buffer_len()];
    screen::render_painting(&spec.bus, VIEW, &mut out, false);
    for line in [4usize, 94, 190] {
        assert_eq!(
            colour_at(&out, VIEW.border_x + 8, VIEW.border_top + line),
            screen::PALETTE[7],
            "line {line} of the ordinary picture is not full brightness"
        );
    }
}

fn test_app<'a>() -> Harness<'a, App> {
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
    h
}

/// Pressing it slows the machine right down and starts it: five seconds to a
/// frame, which is one frame's drawing at a speed it can be read at. Pressing
/// it again puts the speed back where it was found.
#[test]
fn racing_runs_at_five_seconds_a_frame() {
    let mut h = test_app();
    h.state_mut().speed = 2.0;

    h.get_by_label("Race the Beam").click();
    h.run_steps(2);
    assert!(h.state().racing, "the button did not switch racing on");
    assert_eq!(
        h.state().speed,
        RACE_SPEED,
        "racing should run at five seconds a frame"
    );
    assert!(h.state().running, "there is nothing to watch while stopped");

    h.get_by_label("Race the Beam").click();
    h.run_steps(2);
    assert!(!h.state().racing);
    assert_eq!(
        h.state().speed,
        2.0,
        "the speed it was on before racing should come back"
    );
}

/// And the window draws it: the trail is only worth anything if it is what
/// ends up on screen.
#[test]
fn the_window_draws_the_fading_picture_while_racing() {
    let mut h = test_app();
    // A screenful of white for the beam to be drawing.
    {
        let app = h.state_mut();
        app.spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);
        app.spec.cpu.pc = 0;
        for o in 0..0x1800u16 {
            app.spec.bus.poke(0x4000 + o, 0xFF);
        }
        for o in 0x1800..0x1b00u16 {
            app.spec.bus.poke(0x4000 + o, 0x07);
        }
    }
    h.get_by_label("Race the Beam").click();
    h.run_steps(2);

    // Eight seconds of host time: a frame and a half at five seconds each, so
    // the whole picture has been painted white once and the beam is part way
    // down painting it again.
    {
        let app = h.state_mut();
        for _ in 0..60 * 8 {
            app.advance(1.0 / 60.0);
        }
    }
    h.run_steps(2);

    let app = h.state();
    let view = app.view();
    let beam_line =
        (app.spec.bus.tstates - app.spec.bus.first_pixel_t()) / app.spec.bus.model.t_per_line();
    assert!(
        (40..192).contains(&beam_line),
        "the beam should be part way down the display, not on line {beam_line}"
    );
    let drawn = app.picture();
    let brightness = |line: usize| {
        let at = ((view.border_top + line) * view.width() + view.border_x + 8) * 4;
        drawn[at] as f32 / screen::PALETTE[7][0] as f32
    };
    // Two lines the beam has already been over this frame, so both hold the
    // same white and only the fade tells them apart.
    let just_drawn = brightness(beam_line as usize - 10);
    let a_while_ago = brightness(2);
    assert!(
        just_drawn > a_while_ago + 0.05,
        "the picture the window drew does not fade behind the beam: the line \
         behind it is at {just_drawn} and line 2, drawn earlier, at \
         {a_while_ago}"
    );
}

/// Five seconds a frame means a frame takes five seconds: a second of running
/// gets about a fifth of the way through one.
#[test]
fn a_frame_takes_five_seconds() {
    let mut h = test_app();
    h.get_by_label("Race the Beam").click();
    h.run_steps(2);

    let app = h.state_mut();
    let before = (app.spec.bus.frame, app.spec.bus.tstates);
    // A second of host time, in sixtieths.
    for _ in 0..60 {
        app.advance(1.0 / 60.0);
    }
    let done = (app.spec.bus.frame - before.0) as f64 * app.spec.bus.frame_t() as f64
        + app.spec.bus.tstates as f64
        - before.1 as f64;
    let fraction = done / app.spec.bus.frame_t() as f64;
    assert!(
        (0.15..0.25).contains(&fraction),
        "a second of racing got {fraction:.3} of the way through a frame, \
         which is {:.1} seconds a frame",
        1.0 / fraction
    );
}
