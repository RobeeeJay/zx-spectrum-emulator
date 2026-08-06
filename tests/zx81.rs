//! ZX81: the memory map, the ULA's NOP-substitution video, and booting the
//! real ROM.

use zx_spectrum_emulator::z80::Bus;
use zx_spectrum_emulator::zx81::{Ram, Zx81, LINE_T, RASTER_H, RASTER_W};

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

/// Non-blank pixels in the picture last completed.
fn ink(zx: &Zx81) -> usize {
    zx.bus.fb_prev.iter().filter(|p| **p != 0).count()
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
    let x0 = 20 * 2;
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
    zx.bus.t_in_line = 0;
    zx.step_instruction();

    let row: Vec<u8> = (0..8).map(|i| zx.bus.fb[5 * RASTER_W + i]).collect();
    assert_eq!(row, vec![0, 0, 1, 1, 1, 1, 1, 1], "inverse video");
}

#[test]
fn the_line_counter_picks_the_row_of_the_character() {
    let mut zx = Zx81::new(Ram::K16);
    zx.cpu.i = 0x1e;
    // A different pattern on each of the eight rows.
    set_char(&mut zx, 0x01, [0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01]);
    zx.bus.poke(0x4000, 0x01);

    for lcnt in 0..8u8 {
        zx.bus.fb.iter_mut().for_each(|p| *p = 0);
        zx.bus.lcnt = lcnt;
        zx.bus.line = lcnt as u32;
        zx.bus.t_in_line = 0;
        zx.cpu.pc = 0xc000;
        zx.step_instruction();

        let row: Vec<usize> = (0..8)
            .filter(|i| zx.bus.fb[lcnt as usize * RASTER_W + i] != 0)
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

    let frame = zx.bus.frame;
    zx.bus.io_write(0xff, 0);
    assert!(!zx.bus.vsync, "any OUT ends it");
    assert_eq!(zx.bus.frame, frame + 1, "and that finishes the picture");
    assert_eq!(zx.bus.line, 0, "with the raster back at the top");
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
    rom[14] = 0x21; // LD HL,$C100  the display file, through the mirror
    rom[15] = 0x00;
    rom[16] = 0xc1;
    rom[17] = 0xe9; // JP (HL)      hand the display file to the ULA
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

    assert!(counts.len() >= 4, "expected several drawn lines, got {counts:?}");
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
