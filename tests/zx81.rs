//! ZX81: the memory map, the ULA's NOP-substitution video, and booting the
//! real ROM.

use zx_spectrum_emulator::z80::Bus;
use zx_spectrum_emulator::zx81::{
    Ram, Zx81, LINE_T, PICTURE_X, RASTER_H, RASTER_W, SYNC_TO_PICTURE_T,
};

/// Where a character fetched this far into a line lands. The line's T-states
/// are counted from the interrupt that starts it, a little before the visible
/// picture begins.
fn pixel_x(t_in_line: u32) -> usize {
    (t_in_line as usize * 2)
        .checked_sub(PICTURE_X)
        .expect("that fetch is in the blanking, off the left of the picture")
}

fn rom() -> Option<Vec<u8>> {
    std::fs::read("roms/zx81.rom").ok()
}

fn machine(ram: Ram) -> Option<Zx81> {
    let rom = rom()?;
    let mut zx = Zx81::new(ram);
    zx.load_rom(&rom);
    zx.reset();
    Some(zx)
}

// ---------------------------------------------------------------------------
// memory
// ---------------------------------------------------------------------------

#[test]
fn an_eight_k_rom_appears_twice_and_again_above_8000() {
    let mut zx = Zx81::new(Ram::K16);
    zx.load_rom(&vec![0u8; 8192]);
    zx.bus.rom[0] = 0xaa;
    zx.bus.rom[0x1fff] = 0x55;

    assert_eq!(zx.bus.mem(0x0000), 0xaa);
    assert_eq!(zx.bus.mem(0x2000), 0xaa, "mirrored in the second 8K");
    assert_eq!(zx.bus.mem(0x8000), 0xaa, "and again above $8000");
    assert_eq!(zx.bus.mem(0xa000), 0xaa);
    assert_eq!(zx.bus.mem(0x1fff), 0x55);
    assert_eq!(zx.bus.mem(0x3fff), 0x55);
}

#[test]
fn a_sixteen_k_rom_fills_the_page_instead_of_mirroring() {
    let mut zx = Zx81::new(Ram::K16);
    let mut image = vec![0u8; 16384];
    image[0x2ea4] = 0x77; // where this ROM's interrupt handler lives
    zx.load_rom(&image);
    assert_eq!(
        zx.bus.mem(0x2ea4),
        0x77,
        "a 16K ROM must not be folded back into the first 8K"
    );
    assert_eq!(zx.bus.mem(0xaea4), 0x77, "still mirrored above $8000");
}

#[test]
fn one_k_of_ram_repeats_through_its_page() {
    let mut zx = Zx81::new(Ram::K1);
    zx.bus.poke(0x4000, 0x12);
    assert_eq!(zx.bus.mem(0x4000), 0x12);
    assert_eq!(zx.bus.mem(0x4400), 0x12, "1K repeats every 1024 bytes");
    assert_eq!(zx.bus.mem(0x7c00), 0x12);
    assert_eq!(zx.bus.mem(0xc000), 0x12, "and above $8000 as well");

    // Writing to a mirror is writing to the same byte.
    zx.bus.poke(0x4400, 0x34);
    assert_eq!(zx.bus.mem(0x4000), 0x34);
}

#[test]
fn sixteen_k_fills_the_page_without_mirroring() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.poke(0x4000, 0x12);
    zx.bus.poke(0x4400, 0x34);
    assert_eq!(zx.bus.mem(0x4000), 0x12, "these are different bytes now");
    assert_eq!(zx.bus.mem(0x4400), 0x34);
    assert_eq!(zx.bus.mem(0x7fff), 0);
    assert_eq!(zx.bus.mem(0xc000), 0x12, "still mirrored above $8000");
}

#[test]
fn writing_to_the_rom_does_nothing() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.rom[0x100] = 0x99;
    zx.bus.poke(0x0100, 0x11);
    assert_eq!(zx.bus.mem(0x0100), 0x99);
}

// ---------------------------------------------------------------------------
// the ULA's video trick
// ---------------------------------------------------------------------------

/// Put a character bitmap in the ROM where the ULA will look for it.
fn set_char(zx: &mut Zx81, code: u8, rows: [u8; 8]) {
    let base = 0x1e00 + (code as usize & 0x3f) * 8;
    zx.bus.rom[base..base + 8].copy_from_slice(&rows);
}

#[test]
fn a_fetch_above_8000_draws_a_character_and_runs_as_a_nop() {
    let mut zx = Zx81::new(Ram::K16);
    zx.cpu.i = 0x1e;
    set_char(&mut zx, 0x01, [0b1010_1010; 8]);

    // A display byte in RAM, reached through the mirror at $C000.
    zx.bus.poke(0x4000, 0x01);
    zx.cpu.pc = 0xc000;
    zx.bus.line = 10;
    zx.bus.t_in_line = 20;

    let t0 = zx.bus.tstates;
    zx.step_instruction();

    assert_eq!(zx.cpu.pc, 0xc001, "the CPU saw a NOP, so PC just moved on");
    assert_eq!(zx.bus.tstates - t0, 4, "and it took a NOP's four T-states");
    assert_eq!(zx.bus.video_bytes, 1);

    // Eight pixels, alternating, starting two per T-state along the line.
    let y = 10;
    let x0 = pixel_x(20);
    let row: Vec<u8> = (0..8).map(|i| zx.bus.fb[y * RASTER_W + x0 + i]).collect();
    assert_eq!(row, vec![1, 0, 1, 0, 1, 0, 1, 0]);
}

#[test]
fn bit_seven_of_a_character_inverts_it() {
    let mut zx = Zx81::new(Ram::K16);
    zx.cpu.i = 0x1e;
    set_char(&mut zx, 0x01, [0b1100_0000; 8]);
    zx.bus.poke(0x4000, 0x81); // character 1, inverted
    zx.cpu.pc = 0xc000;
    zx.bus.line = 5;
    zx.bus.t_in_line = 40;
    zx.step_instruction();

    let x0 = pixel_x(40);
    let row: Vec<u8> = (0..8).map(|i| zx.bus.fb[5 * RASTER_W + x0 + i]).collect();
    assert_eq!(row, vec![0, 0, 1, 1, 1, 1, 1, 1], "inverse video");
}

#[test]
fn the_line_counter_picks_the_row_of_the_character() {
    let mut zx = Zx81::new(Ram::K16);
    zx.cpu.i = 0x1e;
    // A different pattern on each of the eight rows.
    set_char(
        &mut zx,
        0x01,
        [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01],
    );
    zx.bus.poke(0x4000, 0x01);

    for lcnt in 0..8u8 {
        zx.bus.fb.iter_mut().for_each(|p| *p = 0);
        zx.bus.lcnt = lcnt;
        zx.bus.line = lcnt as u32;
        zx.bus.t_in_line = 40;
        zx.cpu.pc = 0xc000;
        zx.step_instruction();

        let x0 = pixel_x(40);
        let row: Vec<usize> = (0..8)
            .filter(|i| zx.bus.fb[lcnt as usize * RASTER_W + x0 + i] != 0)
            .collect();
        assert_eq!(row, vec![lcnt as usize], "row {lcnt} of the character");
    }
}

#[test]
fn a_halt_in_the_display_file_is_executed_rather_than_drawn() {
    let mut zx = Zx81::new(Ram::K16);
    zx.cpu.i = 0x1e;
    zx.bus.poke(0x4000, 0x76); // HALT: bit 6 set, so the ULA lets it through
    zx.cpu.pc = 0xc000;
    zx.step_instruction();

    assert!(zx.cpu.halted, "the CPU should halt, ending the line");
    assert_eq!(zx.bus.video_bytes, 0, "and nothing should be drawn");
}

#[test]
fn fetches_below_8000_are_ordinary_instructions() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.rom[0] = 0x3e; // LD A,$2A
    zx.bus.rom[1] = 0x2a;
    zx.cpu.pc = 0;
    zx.step_instruction();
    assert_eq!(zx.cpu.a, 0x2a, "the ULA only interferes above $8000");
    assert_eq!(zx.bus.video_bytes, 0);
}

// ---------------------------------------------------------------------------
// sync and timing
// ---------------------------------------------------------------------------

#[test]
fn the_raster_advances_a_line_every_207_t_states() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.rom[0] = 0x00; // NOP
    zx.bus.rom[1] = 0x18;
    zx.bus.rom[2] = 0xfd; // JR -3
    zx.cpu.pc = 0;

    let start_line = zx.bus.line;
    let start_t = zx.bus.tstates;
    while zx.bus.tstates - start_t < LINE_T as u64 {
        zx.step_instruction();
    }
    assert_eq!(
        zx.bus.line,
        start_line + 1,
        "one line per {LINE_T} T-states"
    );
}

#[test]
fn reading_the_keyboard_starts_the_sync_and_a_write_ends_it() {
    let mut zx = Zx81::new(Ram::K16);
    assert!(!zx.bus.vsync);

    zx.bus.io_read(0xfe);
    assert!(zx.bus.vsync, "an IN from $FE starts the vertical sync");
    zx.bus.lcnt = 5;
    zx.bus.io_read(0xfe);
    assert_eq!(zx.bus.lcnt, 0, "which holds the line counter at zero");

    // Held for as long as the ROM holds it, which is several lines.
    zx.bus.tick_for_test(LINE_T * 4);
    let frame = zx.bus.frame;
    zx.bus.io_write(0xff, 0);
    assert!(!zx.bus.vsync, "any OUT ends it");
    assert_eq!(zx.bus.frame, frame + 1, "and that finishes the picture");
    assert_eq!(zx.bus.line, 0, "with the raster back at the top");
}

#[test]
fn a_brief_sync_pulse_does_not_restart_the_picture() {
    // The hi-res routines read the keyboard and write a port several times a
    // line. That raises the sync for a few microseconds, which a television
    // ignores; treating it as a vertical sync would restart the picture
    // hundreds of times a second and only the top row would ever be drawn.
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.tick_for_test(LINE_T * 8);
    let (frame, line) = (zx.bus.frame, zx.bus.line);

    for _ in 0..20 {
        zx.bus.io_read(0xfe); // sync on
        zx.bus.io_write(0xff, 0); // and straight off again
    }

    assert_eq!(zx.bus.frame, frame, "the picture was restarted");
    assert!(
        zx.bus.line >= line,
        "the raster jumped back to the top: {} to {}",
        line,
        zx.bus.line
    );
}

#[test]
fn the_nmi_generator_is_switched_by_the_two_ports() {
    let mut zx = Zx81::new(Ram::K16);
    assert!(!zx.bus.nmi_on);

    zx.bus.io_write(0xfe, 0); // $FE turns it on
    assert!(zx.bus.nmi_on);
    zx.bus.io_write(0xfd, 0); // $FD turns it off
    assert!(!zx.bus.nmi_on);
}

#[test]
fn the_nmi_generator_fires_once_a_line() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.rom[0] = 0x00;
    zx.bus.rom[1] = 0x18;
    zx.bus.rom[2] = 0xfd; // NOP; JR -3
    zx.cpu.pc = 0;
    zx.bus.io_write(0xfe, 0); // NMI generator on
    zx.bus.nmi_pending = false;

    let mut nmis = 0;
    let start = zx.bus.tstates;
    while zx.bus.tstates - start < LINE_T as u64 * 4 {
        if zx.bus.nmi_pending {
            nmis += 1;
            zx.bus.nmi_pending = false;
        }
        zx.step_instruction();
    }
    assert!(
        (3..=5).contains(&nmis),
        "expected about one NMI per line, got {nmis}"
    );
}

#[test]
fn the_interrupt_comes_from_bit_six_of_the_refresh_register() {
    let mut zx = Zx81::new(Ram::K16);
    // A handler that just returns, so we can count how often it is reached.
    zx.bus.rom[0x38] = 0xfb; // EI
    zx.bus.rom[0x39] = 0xed; // RETN
    zx.bus.rom[0x3a] = 0x45;
    zx.bus.rom[0] = 0x00;
    zx.bus.rom[1] = 0x18;
    zx.bus.rom[2] = 0xfd;
    zx.cpu.pc = 0;
    zx.cpu.iff1 = true;
    zx.cpu.im = 1;
    zx.cpu.r = 0x3e; // just below the point where bit 6 falls... it rises first

    let mut taken = 0;
    for _ in 0..400 {
        let pc_before = zx.cpu.pc;
        zx.step_instruction();
        if pc_before != 0x38 && zx.cpu.pc == 0x38 {
            taken += 1;
        }
    }
    assert!(
        taken >= 2,
        "the refresh register should keep producing interrupts, got {taken}"
    );
}

// ---------------------------------------------------------------------------
// a whole picture, driven the way the ROM drives it
// ---------------------------------------------------------------------------

/// A display routine in the same shape as the ROM's: preset R so the interrupt
/// arrives at the end of the row, jump into the display file so the ULA turns
/// it into pixels, and let the interrupt start the next line.
///
/// This is what makes the picture cycle exact — the characters land where the
/// CPU's fetches put them — so it is worth driving directly rather than only
/// through a ROM.
fn synthetic_display() -> Zx81 {
    synthetic_display_with(false)
}

/// `sync_pulses` makes the driver read the keyboard and write a port at the
/// start of every line, as the hi-res routines do.
fn synthetic_display_with(sync_pulses: bool) -> Zx81 {
    let mut zx = Zx81::new(Ram::K16);

    // A character whose bitmap is solid on every row, so any pixel drawn is
    // unambiguous.
    let mut rom = vec![0u8; 8192];
    let driver: [u8; 14] = [
        0x3e, 0x1e, // LD A,$1E     character set at $1E00
        0xed, 0x47, // LD I,A
        0xed, 0x56, // IM 1
        0x31, 0xff, 0x4f, // LD SP,$4FFF
        0xfb, // EI
        // line:
        0x3e, 0x50, // LD A,$50     so bit 6 of R falls after the row
        0xed, 0x4f, // LD R,A
    ];
    rom[..driver.len()].copy_from_slice(&driver);
    // The per-line code starts at $000A, where the interrupt handler sends it.
    let mut at = driver.len();
    if sync_pulses {
        rom[at] = 0xdb; // IN A,($FE)   raises the sync, as hi-res code does
        rom[at + 1] = 0xfe;
        rom[at + 2] = 0xd3; // OUT ($FF),A  and drops it a few T-states later
        rom[at + 3] = 0xff;
        at += 4;
    }
    rom[at] = 0x21; // LD HL,$C100  the display file, through the mirror
    rom[at + 1] = 0x00;
    rom[at + 2] = 0xc1;
    rom[at + 3] = 0xe9; // JP (HL)      hand the display file to the ULA
                        // The interrupt handler drops the return address and starts the next line.
    rom[0x38] = 0xe1; // POP HL
    rom[0x39] = 0xfb; // EI
    rom[0x3a] = 0xc3; // JP $000A
    rom[0x3b] = 0x0a;
    rom[0x3c] = 0x00;
    // Solid character $01.
    for row in 0..8 {
        rom[0x1e08 + row] = 0xff;
    }
    zx.load_rom(&rom);
    zx.reset();

    // 32 characters and a HALT to end the line.
    for i in 0..32u16 {
        zx.bus.poke(0x4100 + i, 0x01);
    }
    zx.bus.poke(0x4120, 0x76);
    zx
}

/// A driver in the style of the hi-res routines: no interrupts at all. It
/// raises and drops the sync itself to start each line, runs 32 display bytes
/// through the mirror, and jumps back — the jump's opcode has bit 6 set, so the
/// ULA executes it instead of turning it into a NOP. Where the picture lands is
/// therefore decided entirely by the sync, which is the point of the exercise.
fn synthetic_display_hires() -> Zx81 {
    let mut zx = Zx81::new(Ram::K16);
    let mut rom = vec![0u8; 8192];
    // Twenty-eight T-states pass between releasing the sync and the first
    // character, which is what Forty Niner's routine takes.
    let driver: [u8; 22] = [
        0x3e, 0x1e, // LD A,$1E     character set at $1E00
        0xed, 0x47, // LD I,A
        0xf3, // DI
        0x31, 0xff, 0x4f, // LD SP,$4FFF
        0x00, 0x00, // padding, so the line loop starts at $000A
        // line:
        0xdb, 0xfe, // IN A,($FE)   sync low                     11T
        0xd3, 0xff, // OUT ($FF),A  released                     11T
        0x3e, 0x00, // LD A,$00                                   7T
        0x3e, 0x00, // LD A,$00                                   7T
        0x21, 0x00, 0xc1, // LD HL,$C100                         10T
        0xe9, // JP (HL)                                          4T
    ];
    rom[..driver.len()].copy_from_slice(&driver);
    for row in 0..8 {
        rom[0x1e08 + row] = 0xff; // character $01 is solid
    }
    zx.load_rom(&rom);
    zx.reset();

    for i in 0..32u16 {
        zx.bus.poke(0x4100 + i, 0x01);
    }
    // Bit 6 set means the ULA executes these rather than displaying them.
    // Five LD B,B pad a turn of the loop past the 207 T-states of a line, so
    // the raster moves down one row per turn as it does for the real games,
    // which emit exactly one sync per row.
    for i in 0..5 {
        zx.bus.poke(0x4120 + i, 0x40); // LD B,B
    }
    zx.bus.poke(0x4125, 0xc3); // JP $000A
    zx.bus.poke(0x4126, 0x0a);
    zx.bus.poke(0x4127, 0x00);
    zx
}

#[test]
fn a_row_of_characters_lands_contiguously_and_in_the_same_place_each_line() {
    let mut zx = synthetic_display();
    // Let it settle, then watch a few lines.
    for _ in 0..200 {
        zx.step_instruction();
    }

    let mut starts = Vec::new();
    let mut counts = Vec::new();
    for _ in 0..24 {
        let line = zx.bus.line;
        let mut first = None;
        let mut count = 0;
        while zx.bus.line == line {
            let before = zx.bus.video_bytes;
            let t = zx.bus.t_in_line;
            zx.step_instruction();
            if zx.bus.video_bytes > before {
                first.get_or_insert(t);
                count += 1;
            }
        }
        if let Some(f) = first {
            starts.push(f);
            counts.push(count);
        }
    }

    assert!(
        counts.len() >= 4,
        "expected several drawn lines, got {counts:?}"
    );
    assert!(
        counts.iter().all(|c| *c == 32),
        "every line should draw all 32 characters, got {counts:?}"
    );
    assert!(
        starts.windows(2).all(|w| w[0] == w[1]),
        "the rows must start at the same point on every line, got {starts:?}"
    );
}

#[test]
fn the_drawn_block_is_256_pixels_wide_and_solid() {
    let mut zx = synthetic_display();
    for _ in 0..2000 {
        zx.step_instruction();
    }

    // Find a line with ink in the frame being drawn.
    let row = (0..RASTER_H)
        .find(|y| (0..RASTER_W).any(|x| zx.bus.fb[y * RASTER_W + x] != 0))
        .expect("something should have been drawn");
    let inked: Vec<usize> = (0..RASTER_W)
        .filter(|x| zx.bus.fb[row * RASTER_W + x] != 0)
        .collect();

    assert_eq!(
        inked.len(),
        256,
        "32 characters of 8 pixels should be 256 pixels wide"
    );
    assert_eq!(
        *inked.last().unwrap() - inked[0],
        255,
        "and contiguous, with no gaps between characters"
    );
}

#[test]
fn the_line_counter_walks_the_rows_of_the_character() {
    let mut zx = synthetic_display();
    for _ in 0..200 {
        zx.step_instruction();
    }
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..2000 {
        zx.step_instruction();
        seen.insert(zx.bus.lcnt);
    }
    assert_eq!(
        seen.len(),
        8,
        "all eight rows of the character should be used, saw {seen:?}"
    );
}

// ---------------------------------------------------------------------------
// the real ROM
// ---------------------------------------------------------------------------

/// Extent of the ink in the last completed picture, as (x0, x1, y0, y1).
fn ink_extent(zx: &Zx81) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut x1, mut y0, mut y1) = (RASTER_W, 0usize, RASTER_H, 0usize);
    let mut any = false;
    for y in 0..RASTER_H {
        for x in 0..RASTER_W {
            if zx.bus.fb_prev[y * RASTER_W + x] != 0 {
                any = true;
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    any.then_some((x0, x1, y0, y1))
}

fn booted(ram: Ram) -> Option<Zx81> {
    let mut zx = machine(ram)?;
    for _ in 0..200 {
        zx.run(zx.frame_t());
    }
    Some(zx)
}

#[test]
fn the_rom_boots_and_syncs_the_picture() {
    let Some(zx) = booted(Ram::K16) else {
        eprintln!("no roms/zx81.rom; skipping");
        return;
    };
    // The ROM drives the sync itself, so frames should come at about the rate
    // the raster does rather than from the emulator timing out.
    assert!(
        (150..=260).contains(&zx.bus.frame),
        "expected roughly one frame per frame's worth of time, got {}",
        zx.bus.frame
    );
    assert!(!zx.bus.vsync, "and not be stuck in the sync pulse");
}

#[test]
fn the_rom_draws_the_cursor_at_the_bottom_left() {
    let Some(zx) = booted(Ram::K16) else {
        return;
    };
    let (x0, x1, y0, y1) = ink_extent(&zx).expect("nothing was drawn");

    // A freshly booted ZX81 shows one thing: the inverse K cursor on the input
    // line, which lives at the bottom of the screen.
    assert_eq!(x1 - x0 + 1, 8, "the cursor is one character wide");
    assert_eq!(y1 - y0 + 1, 8, "and one character tall");
    // The picture is centred in the raster, so its first column is at
    // (RASTER_W - 256) / 2.
    let first_column = (RASTER_W - 256) / 2;
    assert!(
        (first_column..first_column + 40).contains(&x0),
        "at the left of the display area, which starts at {first_column}, got x {x0}"
    );
    assert!(
        y0 > 180,
        "and near the bottom, where the input line is, got y {y0}"
    );
}

#[test]
fn every_drawn_line_starts_at_the_same_point() {
    let Some(mut zx) = booted(Ram::K16) else {
        return;
    };

    // Start at a frame boundary, so a whole picture is watched rather than
    // whatever is left of the one in progress.
    let frame = zx.bus.frame;
    while zx.bus.frame == frame {
        zx.step_instruction();
    }

    // Watch a whole frame and note where each line's characters begin.
    let frame = zx.bus.frame;
    let mut starts = std::collections::BTreeSet::new();
    let mut counts = Vec::new();
    let (mut count, mut first, mut last_line) = (0u32, None, zx.bus.line);
    while zx.bus.frame == frame {
        let before = zx.bus.video_bytes;
        let (line, t) = (zx.bus.line, zx.bus.t_in_line);
        zx.step_instruction();
        if zx.bus.video_bytes > before {
            if line != last_line {
                if count > 0 {
                    counts.push(count);
                }
                count = 0;
                first = None;
                last_line = line;
            }
            if first.is_none() {
                first = Some(t);
                starts.insert(t);
            }
            count += 1;
        }
    }
    if count > 0 {
        counts.push(count);
    }

    assert!(!counts.is_empty(), "no line drew anything");
    assert_eq!(
        starts.len(),
        1,
        "the rows must all begin at the same T-state, got {starts:?}"
    );
    assert!(
        counts.iter().all(|c| *c == 32),
        "a line of the display file is 32 characters, got {counts:?}"
    );
}

#[test]
fn the_unexpanded_machine_boots_too() {
    let Some(zx) = booted(Ram::K1) else {
        return;
    };
    assert!(
        zx.bus.frame > 150,
        "the 1K machine should sync as well, got {}",
        zx.bus.frame
    );
    assert!(ink_extent(&zx).is_some(), "and draw its cursor");
}

#[test]
fn typing_puts_something_on_the_screen() {
    let Some(mut zx) = booted(Ram::K16) else {
        return;
    };
    let before = zx.bus.fb_prev.iter().filter(|p| **p != 0).count();

    // Hold a key long enough for the ROM to see it, then let go.
    for phase in 0..2 {
        zx.bus.keys = [0xff; 8];
        if phase == 0 {
            zx.bus.keys[3] &= !1; // the "1" key
        }
        for _ in 0..30 {
            zx.run(zx.frame_t());
        }
    }
    for _ in 0..30 {
        zx.run(zx.frame_t());
    }

    let after = zx.bus.fb_prev.iter().filter(|p| **p != 0).count();
    assert!(
        after > before,
        "pressing a key should put a character on the screen ({before} then {after} pixels)"
    );
}

#[test]
fn a_program_too_big_for_1k_is_refused() {
    let mut zx = Zx81::new(Ram::K1);
    let err = zx.load_p(&vec![0u8; 4096]).unwrap_err();
    assert!(err.contains("16K"), "should say what is wrong: {err}");

    let mut big = Zx81::new(Ram::K16);
    assert!(big.load_p(&vec![0u8; 4096]).is_ok());
}

#[test]
fn a_p_file_lands_at_4009() {
    let mut zx = Zx81::new(Ram::K16);
    let mut image = vec![0u8; 32];
    image[0] = 0xab;
    image[31] = 0xcd;
    zx.load_p(&image).unwrap();
    assert_eq!(zx.bus.mem(0x4009), 0xab);
    assert_eq!(zx.bus.mem(0x4009 + 31), 0xcd);
}

#[test]
fn sync_pulses_between_lines_do_not_stop_the_picture_being_drawn() {
    // The hi-res games raise and drop the sync several times a line while
    // building the picture. If each pulse were taken for a vertical sync the
    // raster would keep jumping back to the top and only the first row would
    // ever appear — which is exactly what a blank screen looks like.
    let mut plain = synthetic_display_with(false);
    let mut pulsing = synthetic_display_with(true);
    for _ in 0..400 {
        plain.run(plain.frame_t());
        pulsing.run(pulsing.frame_t());
    }

    let rows = |zx: &Zx81| -> usize {
        (0..RASTER_H)
            .filter(|y| (0..RASTER_W).any(|x| zx.bus.fb_prev[y * RASTER_W + x] != 0))
            .count()
    };
    let (plain_rows, pulsing_rows) = (rows(&plain), rows(&pulsing));
    assert!(plain_rows > 100, "the plain driver drew {plain_rows} rows");
    assert!(
        pulsing_rows > plain_rows / 2,
        "with sync pulses only {pulsing_rows} rows were drawn, against {plain_rows} without"
    );
}

#[test]
fn releasing_the_sync_puts_the_beam_at_a_fixed_point_in_the_line() {
    // However far into a line the sync is released, the beam ends up in the
    // same place: the ULA holds its counters in reset while the sync is low.
    // That place is not the left edge, because the sync pulse and back porch
    // take up the start of the line.
    for offset in [0, 7, 33, 101, 206] {
        let mut zx = Zx81::new(Ram::K16);
        zx.bus.tick_for_test(offset);
        zx.bus.io_read(0xfe); // sync low
        zx.bus.tick_for_test(LINE_T * 4); // held: a vertical sync
        zx.bus.io_write(0xff, 0); // released
        assert_eq!(
            zx.bus.t_in_line, SYNC_TO_PICTURE_T,
            "released {offset} T-states into a line and the raster kept the offset"
        );
    }
}

#[test]
fn a_program_drawing_its_own_lines_lands_where_the_rom_does() {
    // The hi-res routines pace themselves from their own sync rather than from
    // the interrupt the ROM uses. Both have to put the picture in the same
    // place, or switching between a hi-res screen and an ordinary one shifts
    // the display sideways.
    let column0 = |zx: &Zx81| -> Option<usize> {
        (0..RASTER_H)
            .filter_map(|y| (0..RASTER_W).find(|x| zx.bus.fb_prev[y * RASTER_W + x] != 0))
            .min()
    };

    let mut interrupt_paced = synthetic_display();
    let mut sync_paced = synthetic_display_hires();
    for _ in 0..400 {
        interrupt_paced.run(interrupt_paced.frame_t());
        sync_paced.run(sync_paced.frame_t());
    }
    let (a, b) = (
        column0(&interrupt_paced).expect("nothing drawn"),
        column0(&sync_paced).expect("nothing drawn"),
    );
    assert!(
        a.abs_diff(b) <= 2,
        "the interrupt-paced picture starts at {a} and the sync-paced one at {b}"
    );
}

#[test]
fn the_picture_lands_in_the_same_place_after_the_timing_is_nudged() {
    // A driver that paces itself from the sync draws a rectangle: every row
    // starts at the same column. If the sync does not put the horizontal
    // counter back to the left edge, the rows drift sideways one after
    // another and the whole image slides about from frame to frame — the
    // jitter this guards against.
    let edges = |zx: &Zx81| -> Vec<usize> {
        (0..RASTER_H)
            .filter_map(|y| (0..RASTER_W).find(|x| zx.bus.fb_prev[y * RASTER_W + x] != 0))
            .collect()
    };
    let mut zx = synthetic_display_hires();
    for _ in 0..300 {
        zx.run(zx.frame_t());
    }
    let rows = edges(&zx);
    assert!(rows.len() > 100, "the driver drew {} rows", rows.len());
    let first = rows[0];
    assert!(
        rows.iter().all(|x| *x == first),
        "the rows do not line up: {:?}",
        &rows[..rows.len().min(8)]
    );

    for nudge in [1, 3, 11] {
        zx.bus.tick_for_test(nudge);
        for _ in 0..100 {
            zx.run(zx.frame_t());
        }
        let rows = edges(&zx);
        assert!(
            rows.iter().all(|x| *x == first),
            "a {nudge} T-state nudge left the picture at {:?}, not {first}",
            &rows[..rows.len().min(8)]
        );
    }
}

// ---------------------------------------------------------------------------
// what a television makes of a display that is not being driven properly
// ---------------------------------------------------------------------------

fn ink(zx: &Zx81) -> usize {
    zx.bus.fb.iter().filter(|p| **p != 0).count()
}

#[test]
fn a_sync_arriving_far_too_early_blanks_the_beam_where_it_stands() {
    // The tape loader pulses the sync every few microseconds. A television's
    // line oscillator will not lock to that, so the beam carries on and each
    // pulse leaves a black bar where it was — the ZX81's loading pattern.
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.tick_for_test(LINE_T * 4);
    let line_before = zx.bus.line;

    for _ in 0..40 {
        zx.bus.io_read(0xfe); // sync low
        zx.bus.tick_for_test(8);
        zx.bus.io_write(0xff, 0); // and up again, far too soon to be a line
        zx.bus.tick_for_test(60);
    }

    assert!(ink(&zx) > 0, "the pulses left no mark on the picture");
    assert!(
        zx.bus.line > line_before,
        "the raster stopped moving: line stuck at {}",
        zx.bus.line
    );
}

#[test]
fn a_sync_arriving_when_a_line_is_due_is_a_line_sync_and_leaves_no_mark() {
    // The hi-res routines pulse the sync once a row. That is an ordinary
    // horizontal sync: the beam retraces, off the screen, and nothing is drawn.
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.tick_for_test(LINE_T * 4);
    for _ in 0..4 {
        zx.bus.io_read(0xfe);
        zx.bus.tick_for_test(8);
        zx.bus.io_write(0xff, 0);
        zx.bus.tick_for_test(LINE_T - 8); // a whole line before the next
    }
    assert_eq!(ink(&zx), 0, "a line sync should not mark the picture");
}

#[test]
fn the_vertical_sync_leaves_no_mark_either() {
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.tick_for_test(LINE_T * 4);
    zx.bus.io_read(0xfe);
    zx.bus.tick_for_test(LINE_T * 5); // held, as the ROM holds it
    zx.bus.io_write(0xff, 0);
    assert_eq!(
        ink(&zx),
        0,
        "the beam is off the screen retracing, so nothing is drawn"
    );
}

#[test]
fn the_picture_is_painted_over_rather_than_wiped() {
    // A television paints line by line over what is already on the screen. If
    // the frame were wiped instead, a display that keeps restarting — a tape
    // loading, where the sync comes and goes — would show a fragment of a
    // picture on an empty screen rather than what a set really shows.
    let mut zx = Zx81::new(Ram::K16);
    zx.bus.line = 200;
    zx.bus.t_in_line = 100;
    zx.bus.io_read(0xfe);
    zx.bus.tick_for_test(8);
    zx.bus.io_write(0xff, 0); // a bar, low down the picture
    let marked = ink(&zx);
    assert!(marked > 0);

    // Now the picture restarts from the top, as a false vertical sync makes it.
    zx.bus.io_read(0xfe);
    zx.bus.tick_for_test(LINE_T * 5);
    zx.bus.io_write(0xff, 0);
    assert_eq!(zx.bus.line, 0, "back to the top");
    assert_eq!(
        ink(&zx),
        marked,
        "the mark further down the screen should still be there"
    );
}
