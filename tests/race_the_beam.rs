//! Racing the beam: the picture is shown half-painted, and the two halves say
//! different things.
//!
//! Behind the beam is what the ULA actually put on the screen — the colours as
//! they were at each T-state, which is where a border or attribute effect
//! lives. Ahead of it is what the display file holds now, drawn plainly with
//! one border colour and each cell's attribute as it stands, and dimmed to
//! show it has not been painted yet. So the screen shows the machine's
//! execution on one side of the beam and the program's intention on the other.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::screen::{self, View};

const VIEW: View = View::OVERSCAN;

fn colour_at(buf: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * VIEW.width() + x) * 4;
    [buf[i], buf[i + 1], buf[i + 2]]
}

/// A machine whose last frame was all white paper, and whose memory now holds
/// all black paper, so the two frames are easy to tell apart.
fn two_frames() -> Spectrum {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00); // NOPs
    spec.cpu.pc = 0;

    // Frame one: white paper everywhere, border white.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x38); // black ink on white paper
    }
    spec.bus.border = 7;
    spec.bus.border_start = 7;
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }

    // Frame two: the CPU has since blacked everything out.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x00); // black on black
    }
    spec.bus.border = 0;
    spec.bus.border_start = 0;
    spec
}

#[test]
fn the_beam_splits_the_picture_between_two_frames() {
    let spec = two_frames();
    let mut buf = vec![0u8; VIEW.buffer_len()];

    // Put the beam half way down the display.
    let split_y = VIEW.border_top + screen::SCREEN_H / 2;
    let beam = screen::t_at_pixel(
        VIEW,
        spec.bus.first_pixel_t(),
        spec.bus.model.t_per_line(),
        VIEW.border_x,
        split_y,
    ) as u32;
    screen::render_racing(&spec.bus, VIEW, &mut buf, false, beam);

    // Above the beam: this frame, black.
    let above = colour_at(&buf, VIEW.border_x + 8, split_y - 4);
    assert_eq!(above, [0, 0, 0], "above the beam should be the new frame");

    // Below it: what the display file holds now, which is also black — the
    // previous frame's white is gone, because the part of the screen the beam
    // has not reached shows what is in memory rather than what was painted a
    // frame ago.
    let below = colour_at(&buf, VIEW.border_x + 8, split_y + 4);
    assert_eq!(
        below,
        [0, 0, 0],
        "below the beam should be what memory holds now, not the old frame"
    );
}

/// Ahead of the beam is the display file as it stands. A program that has
/// rewritten the screen since the last frame sees its new picture there, which
/// is the point: it is what the machine is about to paint.
#[test]
fn ahead_of_the_beam_is_what_memory_holds_now() {
    let mut spec = two_frames();
    // Memory now says white paper again, differing from both the last frame
    // and the black above the beam.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x38);
    }
    let mut buf = vec![0u8; VIEW.buffer_len()];
    let split_y = VIEW.border_top + screen::SCREEN_H / 2;
    let beam = screen::t_at_pixel(
        VIEW,
        spec.bus.first_pixel_t(),
        spec.bus.model.t_per_line(),
        VIEW.border_x,
        split_y,
    ) as u32;
    screen::render_racing(&spec.bus, VIEW, &mut buf, false, beam);

    let white = screen::PALETTE[7];
    let dimmed = [
        (white[0] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (white[1] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (white[2] as f32 * screen::STALE_BRIGHTNESS) as u8,
    ];
    let below = colour_at(&buf, VIEW.border_x + 8, split_y + 4);
    assert_eq!(
        below, dimmed,
        "ahead of the beam is memory as it stands, dimmed to say it is not \
         painted yet"
    );
    assert!(
        (below[0] as f32 / white[0] as f32 - 2.0 / 3.0).abs() < 0.01,
        "dimming should take off a third: {below:?} against {white:?}"
    );
}

#[test]
fn the_split_happens_part_way_along_a_line() {
    let spec = two_frames();
    let mut buf = vec![0u8; VIEW.buffer_len()];

    // Beam a quarter of the way across a line in the middle of the display.
    let y = VIEW.border_top + 100;
    let x = VIEW.border_x + 64;
    let beam = screen::t_at_pixel(
        VIEW,
        spec.bus.first_pixel_t(),
        spec.bus.model.t_per_line(),
        x,
        y,
    ) as u32;
    screen::render_racing(&spec.bus, VIEW, &mut buf, false, beam);

    // Both halves are drawn from memory now, so what marks the beam is the
    // dimming rather than a difference of content.
    let before = colour_at(&buf, x - 16, y);
    let after = colour_at(&buf, x + 16, y);
    assert_eq!(before, [0, 0, 0], "the left of the line is painted");
    assert_eq!(
        after,
        [0, 0, 0],
        "the right of it is memory, and also black"
    );
}

/// A border effect is a colour change part-way down the frame. Behind the beam
/// it is there, because that is what the ULA painted; ahead of it there is
/// only the colour the program has set, because nothing has been painted with
/// it yet. That contrast is the whole purpose of racing the beam.
#[test]
fn a_border_effect_shows_behind_the_beam_and_not_ahead_of_it() {
    use zx_rustrum::z80::Bus;

    let mut spec = two_frames();
    // Part-way down this frame, the program turns the border red.
    while spec.bus.tstates < spec.bus.first_pixel_t() + 40 * 224 {
        spec.step_instruction();
    }
    spec.bus.io_write(0x00FE, 2);
    while spec.bus.tstates < spec.bus.first_pixel_t() + 120 * 224 {
        spec.step_instruction();
    }

    let mut buf = vec![0u8; VIEW.buffer_len()];
    let split_y = VIEW.border_top + 100;
    let beam = screen::t_at_pixel(
        VIEW,
        spec.bus.first_pixel_t(),
        spec.bus.model.t_per_line(),
        VIEW.border_x,
        split_y,
    ) as u32;
    screen::render_racing(&spec.bus, VIEW, &mut buf, false, beam);

    let red = screen::PALETTE[2];
    let above = colour_at(&buf, 4, VIEW.border_top + 60);
    assert_eq!(
        above, red,
        "the border the ULA painted red should be red behind the beam"
    );

    let ahead = colour_at(&buf, 4, split_y + 40);
    let dimmed_red = [
        (red[0] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (red[1] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (red[2] as f32 * screen::STALE_BRIGHTNESS) as u8,
    ];
    assert_eq!(
        ahead, dimmed_red,
        "ahead of the beam it is the one colour the border is set to now"
    );
}

#[test]
fn a_beam_at_the_end_of_the_frame_shows_it_all() {
    let spec = two_frames();
    let mut buf = vec![0u8; VIEW.buffer_len()];
    screen::render_racing(&spec.bus, VIEW, &mut buf, false, FRAME_T - 1);
    // Everything is the new frame: black paper, black border.
    for (x, y) in [(4, 4), (VIEW.border_x + 8, VIEW.border_top + 8), (300, 280)] {
        assert_eq!(colour_at(&buf, x, y), [0, 0, 0], "pixel ({x},{y})");
    }
}

#[test]
fn pixel_positions_and_t_states_agree() {
    let first = 14335u32;
    let per_line = 224u32;

    // The first pixel of the display is at the fetch clock plus the lead.
    assert_eq!(
        screen::t_at_pixel(VIEW, first, per_line, VIEW.border_x, VIEW.border_top),
        first as i64 + screen::DISPLAY_LEAD_T
    );
    // Two pixels per T-state.
    assert_eq!(
        screen::t_at_pixel(VIEW, first, per_line, VIEW.border_x + 2, VIEW.border_top)
            - screen::t_at_pixel(VIEW, first, per_line, VIEW.border_x, VIEW.border_top),
        1
    );
    // One line further down is one line of T-states later.
    assert_eq!(
        screen::t_at_pixel(VIEW, first, per_line, VIEW.border_x, VIEW.border_top + 1)
            - screen::t_at_pixel(VIEW, first, per_line, VIEW.border_x, VIEW.border_top),
        per_line as i64
    );
    // The left border comes before the display.
    assert!(
        screen::t_at_pixel(VIEW, first, per_line, 0, VIEW.border_top)
            < first as i64 + screen::DISPLAY_LEAD_T
    );
}

#[test]
fn racing_works_while_paused() {
    // Nothing about the split depends on the emulator running: the same frame
    // rendered with two different beam positions differs.
    //
    // The paper has to be something other than black for that to show. Ahead
    // of the beam is the same memory as behind it, only dimmed, and a dimmed
    // black is black.
    let mut spec = two_frames();
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x38); // black ink on white paper
    }
    let mut early = vec![0u8; VIEW.buffer_len()];
    let mut late = vec![0u8; VIEW.buffer_len()];
    let t = |y: usize| {
        screen::t_at_pixel(
            VIEW,
            spec.bus.first_pixel_t(),
            spec.bus.model.t_per_line(),
            VIEW.border_x,
            y,
        ) as u32
    };
    screen::render_racing(&spec.bus, VIEW, &mut early, false, t(VIEW.border_top + 50));
    screen::render_racing(&spec.bus, VIEW, &mut late, false, t(VIEW.border_top + 150));

    let y = VIEW.border_top + 100;
    assert_ne!(
        colour_at(&early, VIEW.border_x + 8, y),
        colour_at(&late, VIEW.border_x + 8, y),
        "moving the beam should change what is shown, with nothing running"
    );
}

/// Where the beam is at a T-state, and which T-state a pixel is drawn at, are
/// the same question asked in opposite directions.
#[test]
fn the_beam_position_and_the_pixel_time_agree() {
    use zx_rustrum::screen::{pixel_at_t, t_at_pixel, View};

    let view = View::CROPPED;
    let (first, per_line) = (14335u32, 224u32);

    // Every other pixel across the display area, since two go out per T-state,
    // and a spread of lines down it. Only the display area: the border to the
    // left of a line is emitted before that line's pixels and belongs to the
    // T-states of the line above, so asking where the beam is at that moment
    // rightly answers with the line above.
    for py in [
        view.border_top,
        view.border_top + 1,
        view.border_top + 100,
        view.border_top + 191,
    ] {
        for px in (view.border_x..view.border_x + 256).step_by(2) {
            let t = t_at_pixel(view, first, per_line, px, py);
            if t < 0 {
                continue;
            }
            let (bx, by) = pixel_at_t(view, first, per_line, t as u32);
            assert_eq!(
                (bx, by),
                (px as i64, py as i64),
                "pixel ({px}, {py}) is drawn at T {t}, which puts the beam at \
                 ({bx}, {by})"
            );
        }
    }
}

/// The beam is where the machine has actually reached, and it travels down the
/// picture as the frame goes on.
///
/// It is only worth drawing while the machine is going slowly enough to see:
/// at full speed a frame of work happens between one repaint and the next, so
/// the beam would sit at the top of the frame saying nothing. Slow draw, and
/// any speed under a hundred per cent, leave it part-way through a frame.
#[test]
fn the_beam_travels_down_the_picture_as_the_frame_goes_on() {
    use zx_rustrum::screen::{pixel_at_t, View};

    let view = View::CROPPED;
    let spec = zx_rustrum::machine::Spectrum::new();
    let (first, per_line) = (spec.bus.first_pixel_t(), spec.bus.model.t_per_line());
    let beam = |t: u32| pixel_at_t(view, first, per_line, t);

    // At the start of a frame it is above the picture: the border at the top
    // is drawn before the first pixel of the display.
    let (_, top) = beam(0);
    assert!(
        top < view.border_top as i64,
        "the frame starts above the picture, and the beam is at line {top}"
    );

    // And it works its way down, a line at a time, from wherever the first
    // pixel of the display puts it.
    let (_, mut last) = beam(first);
    for line in 1..192i64 {
        let (_, y) = beam(first + (line as u32) * per_line);
        assert_eq!(y, last + 1, "line {line} should be one below the last");
        last = y;
    }
    assert!(
        last >= view.border_top as i64,
        "by the end it should be well inside the picture, not at line {last}"
    );

    // Across a line it moves left to right, two pixels a T-state. Taken from
    // the middle of a line, so ten T-states later is still the same line.
    let middle = zx_rustrum::screen::t_at_pixel(
        view,
        first,
        per_line,
        view.border_x + 100,
        view.border_top + 50,
    ) as u32;
    let (x0, y0) = beam(middle);
    let (x1, y1) = beam(middle + 10);
    assert_eq!(y1, y0, "ten T-states later is still the same line");
    assert_eq!(x1 - x0, 20, "and ten T-states is twenty pixels");
}
