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
/// A machine at the start of a frame, with a whole frame of white behind it
/// and black in the display file, so the two are told apart.
fn fresh_frame() -> Spectrum {
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

    // Frame two: the CPU has since blacked everything out. Poking memory is
    // not enough to make it show behind the beam — that is what the ULA put
    // out — so the machine has to run far enough for it to have put it out.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x00); // black on black
    }
    spec.bus.border = 0;
    spec.bus.border_start = 0;
    spec
}

/// The same, run on until the ULA has painted the whole of the black frame.
fn two_frames() -> Spectrum {
    let mut spec = fresh_frame();
    let display_ends = spec.bus.first_pixel_t() + 192 * spec.bus.model.t_per_line();
    let frame = spec.bus.frame;
    while spec.bus.frame == frame && spec.bus.tstates < display_ends {
        spec.step_instruction();
    }
    spec.bus.catch_up_painting();
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

/// Behind the beam the racing view and the crawling view are the same picture.
///
/// Both are the frame the ULA is painting, so hovering over the picture should
/// only dim what is ahead of the cursor — it should not change what is behind
/// it. It used to: the plain view was what the ULA painted and the racing view
/// read the display file, so moving the mouse on to the picture swapped every
/// pixel of it for a different one, and moving it off swapped them back.
#[test]
fn behind_the_beam_is_the_same_picture_as_the_plain_view() {
    let mut spec = two_frames();
    // Memory now differs from what was painted: the program has written a
    // white screen that the ULA has not yet put out.
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x38);
    }

    let mut plain = vec![0u8; VIEW.buffer_len()];
    let mut racing = vec![0u8; VIEW.buffer_len()];
    screen::render_painting(&spec.bus, VIEW, &mut plain, false);
    let split_y = VIEW.border_top + screen::SCREEN_H / 2;
    let beam = screen::t_at_pixel(
        VIEW,
        spec.bus.first_pixel_t(),
        spec.bus.model.t_per_line(),
        VIEW.border_x,
        split_y,
    ) as u32;
    screen::render_racing(&spec.bus, VIEW, &mut racing, false, beam);

    for y in VIEW.border_top..split_y - 1 {
        for x in VIEW.border_x..VIEW.border_x + screen::SCREEN_W {
            assert_eq!(
                colour_at(&racing, x, y),
                colour_at(&plain, x, y),
                "pixel ({x},{y}) behind the beam differs between the two views"
            );
        }
    }
}

/// A border effect is a colour change part-way down the frame. Behind the beam
/// it is there, because that is what the ULA painted; ahead of it there is
/// only the colour the program has set, because nothing has been painted with
/// it yet. That contrast is the whole purpose of racing the beam.
#[test]
fn a_border_effect_shows_behind_the_beam_and_not_ahead_of_it() {
    use zx_rustrum::z80::Bus;

    let mut spec = fresh_frame();
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

/// When the frame interrupt goes off, the beam is at the top of the frame —
/// above the picture, in the border the ULA draws before the first line.
///
/// A television-sized view crops most of that border away, so at that moment
/// there is no beam to draw at all. The readout says where it is regardless,
/// because "no beam anywhere" and "the beam is up in the border" look the same
/// on screen and are not the same thing.
#[test]
fn the_beam_is_above_the_picture_when_the_interrupt_fires() {
    use zx_rustrum::machine::{Event, Spectrum, Stop, FRAME_T};
    use zx_rustrum::screen::{pixel_at_t, View};

    // A program that sits in HALT, so the interrupt is taken the moment it is
    // offered rather than at the end of some long instruction.
    let mut spec = Spectrum::new();
    spec.bus.poke(0x8000, 0xFB);
    spec.bus.poke(0x8001, 0x76);
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xFF00;
    spec.cpu.im = 1;
    spec.bus.breaks.interrupt = true;

    let mut stopped = false;
    for _ in 0..8 {
        if let Stop::Watched(Event::Interrupt, _) = spec.run(FRAME_T) {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "the interrupt watch should have stopped it");

    let t = spec.bus.tstates;
    assert!(
        t < 64,
        "the interrupt goes off at the top of the frame, not at T {t}"
    );

    for view in [View::CROPPED, View::OVERSCAN] {
        let (_, y) = pixel_at_t(
            view,
            spec.bus.first_pixel_t(),
            spec.bus.model.t_per_line(),
            t,
        );
        let line = y - view.border_top as i64;
        assert!(
            line < -60,
            "the beam should be sixty-odd lines above the first line of the \
             display, not {line}"
        );
    }
}

/// What the screen shows is what the ULA painted, line by line — not what the
/// display file holds at the moment somebody looks at it.
///
/// A game that races the beam draws a line just before the beam reaches it and
/// rubs it out just after the beam has passed, so the line is on the screen for
/// the whole frame and in the display file for only part of it. Reading the
/// display file to draw the picture makes such a line blink on and off, which
/// is what Space Harrier's text was doing.
#[test]
fn the_picture_is_what_the_beam_painted() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    let line = 74u16;
    // Where that line lives in the display file, and when the ULA paints it.
    let at = 0x4000 | ((line & 0xc0) << 5) | ((line & 0x07) << 8) | ((line & 0x38) << 2);
    let paints_at = spec.bus.first_pixel_t() + line as u32 * spec.bus.model.t_per_line();

    for _ in 0..3 {
        // Ahead of the beam: draw, in white on black so there is something to
        // see.
        let attr = 0x5800 + (line / 8) * 32;
        spec.bus.tstates = paints_at - 4000;
        for cell in 0..8u16 {
            spec.bus.write(at + cell, 0xFF);
            spec.bus.write(attr + cell, 0x07);
        }
        // Behind it: rub out.
        spec.bus.tstates = paints_at + 4000;
        for cell in 0..8u16 {
            spec.bus.write(at + cell, 0x00);
        }
        spec.bus.tstates = spec.bus.frame_t();
        spec.bus.end_frame();

        // The display file has nothing there, and the picture has the line.
        assert_eq!(
            spec.bus.video(at - 0x4000),
            0x00,
            "the program rubbed it out, so the display file holds nothing"
        );
        assert_eq!(
            spec.bus.video_painted(at - 0x4000),
            0xFF,
            "but the beam had already painted it, so the picture keeps it"
        );

        // And the picture that is actually drawn shows it, which is the half
        // of this that the screen depends on.
        let view = zx_rustrum::screen::View::CROPPED;
        let mut out = vec![0u8; view.width() * view.height() * 4];
        zx_rustrum::screen::render(&spec.bus, view, &mut out, false);
        let py = view.border_top + line as usize;
        let px = view.border_x + 4;
        let pixel = &out[(py * view.width() + px) * 4..][..3];
        assert_ne!(
            pixel,
            [0, 0, 0],
            "the line the beam painted should be lit on the rendered screen"
        );
    }
}

/// How the picture is paced: its contents change once per emulated frame, and
/// it is presented at whatever rate the host repaints.
///
/// The window asks for a repaint every pass, so how often it is *drawn* is the
/// desktop's business — sixty times a second, or a hundred and twenty. What is
/// drawn changes only when the ULA finishes a frame, so no repaint can ever
/// catch a half-painted picture, however the two rates line up.
#[test]
fn the_picture_changes_once_per_emulated_frame() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    // A program that changes the screen every frame, so each finished picture
    // is different from the last.
    let mut spec = Spectrum::new();
    for (offset, byte) in [0x3Cu8, 0x32, 0x00, 0x40, 0x18, 0xFA].iter().enumerate() {
        spec.bus.poke(0x8000 + offset as u16, *byte);
    }
    spec.cpu.pc = 0x8000;
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = true;

    for host in [60.0f32, 120.0] {
        let start = app.frame_count();
        let mut pictures = 0usize;
        let mut last: Vec<u8> = Vec::new();
        for _ in 0..120 {
            app.advance(1.0 / host);
            let now = app.spec.bus.screen_prev.clone();
            if now != last {
                pictures += 1;
                last = now;
            }
        }
        let frames = (app.frame_count() - start) as usize;
        assert!(
            pictures <= frames + 1,
            "at {host} Hz, {pictures} different pictures came out of {frames} \
             finished frames, so something was shown that no frame had finished"
        );
        assert!(
            pictures + 1 >= frames,
            "at {host} Hz, {frames} frames finished but only {pictures} pictures \
             were shown, so finished frames went missing"
        );
    }
}

/// While the machine crawls, the picture builds down the screen under the beam.
///
/// The picture normally holds the last finished frame, so that no repaint ever
/// catches one half drawn. Under slow draw that is wrong: an emulated frame
/// takes seconds, and holding the finished one freezes the picture for all of
/// them while the beam crawls over it saying work is being done.
#[test]
fn the_picture_builds_under_the_beam_while_the_machine_crawls() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00); // NOPs
    spec.cpu.pc = 0;
    // One frame of nothing, so what was finished is blank.
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }

    // Now a screenful of white, and the machine half way down painting it.
    for o in 0..0x1800u16 {
        spec.bus.poke(0x4000 + o, 0xFF);
    }
    for o in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + o, 0x07); // white ink on black paper
    }
    let half = spec.bus.first_pixel_t() + 96 * spec.bus.model.t_per_line();
    while spec.bus.tstates < half {
        spec.step_instruction();
    }
    spec.bus.catch_up_painting();

    let (top, bottom) = (VIEW.border_top + 40, VIEW.border_top + 150);
    let mut building = vec![0u8; VIEW.buffer_len()];
    screen::render_painting(&spec.bus, VIEW, &mut building, false);
    assert_eq!(
        colour_at(&building, VIEW.border_x + 8, top),
        screen::PALETTE[7],
        "the beam has passed line {top}, so the picture should have it"
    );
    assert_eq!(
        colour_at(&building, VIEW.border_x + 8, bottom),
        [0, 0, 0],
        "and it has not reached line {bottom}, which is still the last frame"
    );

    // The finished-frame picture is the one that would sit still: nothing of
    // this frame is in it, however far down the beam has got.
    let mut finished = vec![0u8; VIEW.buffer_len()];
    screen::render(&spec.bus, VIEW, &mut finished, false);
    assert_eq!(
        colour_at(&finished, VIEW.border_x + 8, top),
        [0, 0, 0],
        "the finished frame holds none of this one, which is why it is not          what is shown while the machine crawls"
    );

    // And that is what the window draws when slow draw is on: the picture the
    // beam is drawn over, not the one from the frame before.
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_debugger = false;
    app.show_tape = false;
    app.running = false;
    assert!(!app.crawling(), "a machine at full speed is not crawling");
    app.spec.bus.slow.enabled = true;
    assert!(
        app.crawling(),
        "slow draw is watching the picture being drawn"
    );

    let mut harness = egui_kittest::Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(3);
    let view = harness.state().view();
    let drawn = harness.state().picture();
    let at = |y: usize| {
        let i = (y * view.width() + view.border_x + 8) * 4;
        [drawn[i], drawn[i + 1], drawn[i + 2]]
    };
    assert_eq!(
        at(top),
        screen::PALETTE[7],
        "the window should draw the frame being painted while slow draw is on"
    );
    assert_eq!(
        at(bottom),
        [0, 0, 0],
        "and not what has not been painted yet"
    );
}

/// The beam is followed a character cell at a time, not a line at a time.
///
/// The ULA fetches a cell every four T-states, and a game racing the beam
/// writes to a cell the moment that cell has been fetched — several times
/// within one line. Copying a whole line the moment the beam entered it took
/// the version from before all of those writes, so anything drawn behind the
/// beam within a line was a frame late.
#[test]
fn the_painted_frame_follows_the_beam_cell_by_cell() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::z80::Bus;

    let mut spec = Spectrum::new();
    let line = 100u16;
    let at = 0x4000 | ((line & 0xc0) << 5) | ((line & 0x07) << 8) | ((line & 0x38) << 2);
    let line_starts = spec.bus.first_pixel_t() + line as u32 * spec.bus.model.t_per_line();

    // Part-way along the line: the ULA has fetched the first sixteen cells.
    spec.bus.tstates = line_starts + 16 * 4;
    // One cell behind the beam and one ahead of it.
    spec.bus.write(at + 4, 0xAA);
    spec.bus.write(at + 24, 0x55);

    spec.bus.tstates = spec.bus.frame_t();
    spec.bus.end_frame();

    assert_eq!(
        spec.bus.video_painted(at - 0x4000 + 4),
        0x00,
        "the beam had already fetched that cell, so what was written after it \
         belongs to the next frame"
    );
    assert_eq!(
        spec.bus.video_painted(at - 0x4000 + 24),
        0x55,
        "and it had not reached this one, so what was written reaches the screen"
    );
}
