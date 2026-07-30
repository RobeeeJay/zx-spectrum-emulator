//! Racing the beam: hovering the picture shows the frame half-drawn, with the
//! part the ULA has not reached yet still showing the frame before it, dimmed.

use zx_spectrum_emulator::machine::{Model, Spectrum, FRAME_T};
use zx_spectrum_emulator::screen::{self, View};

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

    // Below it: the previous frame's white paper, a third darker.
    let below = colour_at(&buf, VIEW.border_x + 8, split_y + 4);
    let white = screen::PALETTE[7];
    let expected = [
        (white[0] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (white[1] as f32 * screen::STALE_BRIGHTNESS) as u8,
        (white[2] as f32 * screen::STALE_BRIGHTNESS) as u8,
    ];
    assert_eq!(below, expected, "below the beam should be the old frame, dimmed");
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

    let before = colour_at(&buf, x - 16, y);
    let after = colour_at(&buf, x + 16, y);
    assert_eq!(before, [0, 0, 0], "the left of the line is the new frame");
    assert_ne!(after, [0, 0, 0], "the right of it is still the old one");
    assert_ne!(before, after, "the beam should be visible on the line");

    // The line below is entirely the old frame.
    assert_eq!(colour_at(&buf, x - 16, y + 2), after);
}

#[test]
fn the_border_is_split_and_dimmed_too() {
    let spec = two_frames();
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

    // The old frame's border was white, the new one's is black.
    let old_border = colour_at(&buf, 4, split_y + 20);
    let new_border = colour_at(&buf, 4, split_y - 20);
    assert_eq!(new_border, [0, 0, 0]);
    let white = screen::PALETTE[7];
    assert_eq!(
        old_border,
        [
            (white[0] as f32 * screen::STALE_BRIGHTNESS) as u8,
            (white[1] as f32 * screen::STALE_BRIGHTNESS) as u8,
            (white[2] as f32 * screen::STALE_BRIGHTNESS) as u8,
        ]
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
    let spec = two_frames();
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
