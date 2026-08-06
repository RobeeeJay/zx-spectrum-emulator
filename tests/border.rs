//! Border timing, checked against Border Break (introspec/gonzy, 2023), whose
//! output on a real 48K is the photograph in `tapes/bb.png`.
//!
//! The border is drawn by writing port $FE at precise T-states, so getting it
//! right needs the write to land on the correct pair of pixels — the ULA emits
//! two per T-state. Rendering one colour per scanline, as a simpler emulator
//! might, would turn the whole thing into horizontal stripes.

use zx_spectrum_emulator::machine::{Model, Spectrum, FRAME_T};
use zx_spectrum_emulator::screen;
use zx_spectrum_emulator::tape::Tape;

/// These measurements are against the full overscan area, which is what the
/// reference photograph shows.
const VIEW: screen::View = screen::View::OVERSCAN;

/// Colour number (0-7) of a pixel in a rendered frame.
fn colour_at(buf: &[u8], x: usize, y: usize) -> u8 {
    let i = (y * VIEW.width() + x) * 4;
    let rgb = [buf[i], buf[i + 1], buf[i + 2]];
    screen::PALETTE
        .iter()
        .position(|p| *p == rgb)
        .map(|i| (i & 7) as u8)
        .unwrap_or(255)
}

/// Run-length encoding of a horizontal span, as (colour, length).
fn runs(buf: &[u8], y: usize, x0: usize, x1: usize) -> Vec<(u8, usize)> {
    let mut out: Vec<(u8, usize)> = Vec::new();
    for x in x0..x1 {
        let c = colour_at(buf, x, y);
        match out.last_mut() {
            Some((last, n)) if *last == c => *n += 1,
            _ => out.push((c, 1)),
        }
    }
    out
}

fn press(spec: &mut Spectrum, keys: &[(usize, u8)], frames: u32) {
    for phase in 0..2 {
        spec.bus.keys = [0xff; 8];
        if phase == 0 {
            for &(row, bit) in keys {
                spec.bus.keys[row] &= !(1 << bit);
            }
        }
        for _ in 0..frames {
            spec.run(FRAME_T);
        }
    }
}

/// Load Border Break, start it, and render a frame.
fn run_border_break() -> Option<Vec<u8>> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let tape = Tape::load(std::path::Path::new("tapes/borderbreak.tap")).ok()?;

    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.load_rom(&rom);
    spec.reset();
    for _ in 0..150 {
        spec.run(FRAME_T);
    }

    press(&mut spec, &[(6, 3)], 4); // J -> LOAD
    press(&mut spec, &[(7, 1), (5, 0)], 4); // SYMBOL SHIFT + P -> "
    press(&mut spec, &[(7, 1), (5, 0)], 4); // "
    press(&mut spec, &[(6, 0)], 4); // ENTER

    spec.bus.tape = Some(tape);
    let now = spec.bus.total_t();
    spec.bus.tape.as_mut().unwrap().play(now);
    for _ in 0..4000 {
        spec.run(FRAME_T);
        if !spec.bus.tape_playing() {
            break;
        }
    }
    for _ in 0..200 {
        spec.run(FRAME_T);
    }

    // Leave the information screen. The demo wants the key held for a while,
    // so keep it down until the border routine gets going.
    // Leave the information screen. It wants the key held for a while and
    // then pressed again, so do both.
    spec.bus.keys = [0xff; 8];
    spec.bus.keys[7] &= !1; // SPACE
    for _ in 0..2_000_000 {
        spec.step_instruction();
    }
    press(&mut spec, &[(7, 0)], 6);

    let mut started = false;
    for _ in 0..600 {
        spec.run(FRAME_T);
        if spec.bus.border_prev.len() > 100 {
            started = true;
            break;
        }
    }
    assert!(started, "the demo never started drawing the border");
    // Let it reach its steady state, and stop on a frame boundary.
    for _ in 0..50 {
        spec.run(FRAME_T);
    }
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }

    let mut buf = vec![0u8; VIEW.buffer_len()];
    screen::render(&spec.bus, VIEW, &mut buf, false);
    Some(buf)
}

/// A run of one border colour: the colour number and how many pixels of it.
type Run = (u8, usize);
/// One row of the reference photo: which row, then the runs down the left and
/// right borders. Black is colour 0 and the demo's red is 2.
type ReferenceRow = (usize, &'static [Run], &'static [Run]);

/// Rows of `tapes/bb.png`.
const REFERENCE_ROWS: &[ReferenceRow] = &[
    (40, &[(0, 64)], &[(0, 64)]),
    (100, &[(0, 64)], &[(2, 32), (0, 24), (2, 8)]),
    (181, &[(2, 16), (0, 48)], &[(2, 32), (0, 24), (2, 8)]),
    (
        200,
        &[(0, 8), (2, 24), (0, 32)],
        &[(2, 32), (0, 24), (2, 8)],
    ),
    (257, &[(2, 64)], &[(2, 64)]),
];

#[test]
fn border_break_matches_a_real_48k() {
    let Some(buf) = run_border_break() else {
        eprintln!("need roms/48.rom and tapes/borderbreak.tap; skipping");
        return;
    };

    for (row, left, right) in REFERENCE_ROWS {
        assert_eq!(
            runs(&buf, *row, 0, VIEW.border_x),
            left.to_vec(),
            "left border of row {row}"
        );
        assert_eq!(
            runs(&buf, *row, VIEW.border_x + screen::SCREEN_W, VIEW.width()),
            right.to_vec(),
            "right border of row {row}"
        );
    }
}

#[test]
fn the_border_carries_detail_on_most_rows() {
    let Some(buf) = run_border_break() else {
        return;
    };
    // Rows whose left or right border changes colour at least once: border art
    // rendered per scanline instead of per T-state would have almost none.
    let detailed = (0..VIEW.height())
        .filter(|y| {
            runs(&buf, *y, 0, VIEW.border_x).len() > 1
                || runs(&buf, *y, VIEW.border_x + screen::SCREEN_W, VIEW.width()).len() > 1
        })
        .count();
    assert!(
        detailed > 100,
        "only {detailed} rows have any border detail"
    );
}

/// The ULA emits two pixels per T-state, so writes 4 T-states apart must show
/// up as 8-pixel bands within a single scanline.
#[test]
fn border_writes_land_two_pixels_per_t_state() {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);

    // OUT (C),A with BC = $00FE is 12 T-states, so alternating two colours
    // gives 24 T-state stripes: 48 pixels of each.
    let prog: [u8; 12] = [
        0x01, 0xfe, 0x00, // LD BC,$00FE
        0x3e, 0x02, // LD A,2   (red)
        0xed, 0x79, // OUT (C),A
        0x3e, 0x00, // LD A,0   (black)
        0xed, 0x79, // OUT (C),A
        0x18, // JR ...
    ];
    spec.bus.rom[..prog.len()].copy_from_slice(&prog);
    spec.bus.rom[12] = 0xf6; // JR -10, back to the LD A,2 at $0003
    spec.cpu.pc = 0;

    // Run a whole frame so the border log covers it.
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }
    while spec.bus.frame == frame + 1 {
        spec.step_instruction();
    }

    let mut buf = vec![0u8; VIEW.buffer_len()];
    screen::render(&spec.bus, VIEW, &mut buf, false);

    // Row 20 is in the top border, where nothing is contended, so the loop
    // runs at a fixed 50 T-states: red for 19 of them and black for 31.
    let row = runs(&buf, 20, 0, VIEW.width());
    assert!(
        row.len() > 6,
        "alternating the border mid-line should give stripes, got {row:?}"
    );
    let colours: std::collections::HashSet<u8> = row.iter().map(|(c, _)| *c).collect();
    assert_eq!(colours, [0u8, 2].into_iter().collect(), "{row:?}");
    let widths: Vec<usize> = row[1..row.len() - 1].iter().map(|(_, n)| *n).collect();
    assert!(
        widths.iter().all(|w| *w == 38 || *w == 62),
        "19 and 31 T-states are 38 and 62 pixels, got {widths:?}"
    );
    assert!(widths.contains(&38) && widths.contains(&62), "{widths:?}");
}

/// A frame rendered part-way through still shows the previous frame below the
/// point the ULA has reached, rather than going black.
#[test]
fn a_partly_drawn_frame_keeps_the_rest_of_the_previous_one() {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);
    // Set the border red once per frame, at the very start.
    let prog: [u8; 7] = [
        0x3e, 0x02, // LD A,2
        0xd3, 0xfe, // OUT ($FE),A
        0x18, 0xfe, // JR -2 (spin)
        0x00,
    ];
    spec.bus.rom[..prog.len()].copy_from_slice(&prog);
    spec.cpu.pc = 0;

    // One complete frame of red.
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }
    // Now stop a little way into the next frame.
    let target = spec.bus.total_t() + 20_000;
    while spec.bus.total_t() < target {
        spec.step_instruction();
    }

    let mut buf = vec![0u8; VIEW.buffer_len()];
    screen::render(&spec.bus, VIEW, &mut buf, false);
    // The bottom of the picture comes from the frame that has already been
    // drawn, so it is red rather than black.
    assert_eq!(
        colour_at(&buf, 4, VIEW.height() - 4),
        2,
        "the undrawn part of the frame should still show the previous one"
    );
}

/// Cropping the border must show the same picture, just with less of the
/// border around it — the same pixel at the same place relative to the display.
#[test]
fn cropping_the_border_keeps_everything_lined_up() {
    let full = screen::View::OVERSCAN;
    let cropped = screen::View::CROPPED;
    assert!(
        cropped.width() < full.width() && cropped.height() < full.height(),
        "cropping should show less: {cropped:?} vs {full:?}"
    );

    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);
    // Alternate the border so there is something to line up, and put a
    // recognisable pattern in the display.
    let prog: [u8; 13] = [
        0x01, 0xfe, 0x00, // LD BC,$00FE
        0x3e, 0x02, // LD A,2
        0xed, 0x79, // OUT (C),A
        0x3e, 0x05, // LD A,5
        0xed, 0x79, // OUT (C),A
        0x18, 0xf6, // JR -10
    ];
    spec.bus.rom[..prog.len()].copy_from_slice(&prog);
    spec.bus.poke(0x4000, 0b1010_1010);
    spec.bus.poke(0x5800, 0x47); // bright white on black
    spec.cpu.pc = 0;

    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }
    while spec.bus.frame == frame + 1 {
        spec.step_instruction();
    }

    let mut big = vec![0u8; full.buffer_len()];
    let mut small = vec![0u8; cropped.buffer_len()];
    screen::render(&spec.bus, full, &mut big, false);
    screen::render(&spec.bus, cropped, &mut small, false);

    // Every pixel of the cropped view is the matching pixel of the full one.
    let dx = full.border_x - cropped.border_x;
    let dy = full.border_top - cropped.border_top;
    for y in 0..cropped.height() {
        for x in 0..cropped.width() {
            let a = (y * cropped.width() + x) * 4;
            let b = ((y + dy) * full.width() + (x + dx)) * 4;
            assert_eq!(
                small[a..a + 3],
                big[b..b + 3],
                "pixel ({x},{y}) of the cropped view"
            );
        }
    }

    // And the display itself is present in both.
    let top_left_of_display = |buf: &[u8], v: screen::View| {
        let i = (v.border_top * v.width() + v.border_x) * 4;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    assert_eq!(
        top_left_of_display(&small, cropped),
        top_left_of_display(&big, full)
    );
}
