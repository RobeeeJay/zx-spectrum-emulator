//! HALT2INT (Mark Woodmass) run end to end and compared against what real
//! hardware prints.
//!
//! The test measures the R register at the moment an interrupt is taken after
//! a HALT placed at various addresses and reached at exact T-states, which
//! pins down interrupt timing, memory contention, the halt state's refresh
//! address and the floating bus all at once. The two reference tables are the
//! photographs of a real 48K in `tapes/Real 48k (early).png` and
//! `tapes/Real 48k (late).png`.

use zx_spectrum_emulator::machine::{screen_bitmap_offset, Model, Spectrum, FRAME_T};
use zx_spectrum_emulator::tape::Tape;

/// Decode the screen by matching each character cell against the ROM font,
/// which lives at $3D00 of the BASIC ROM.
fn screen_text(spec: &Spectrum) -> Vec<String> {
    let mut out = Vec::new();
    for row in 0..24u16 {
        let mut line = String::new();
        for col in 0..32u16 {
            let mut cell = [0u8; 8];
            for y in 0..8u16 {
                cell[y as usize] = spec.bus.video(screen_bitmap_offset(row * 8 + y, col));
            }
            let mut ch = if cell.iter().all(|b| *b == 0) { ' ' } else { '?' };
            'find: for page in 0..spec.bus.rom.len() / 0x4000 {
                for c in 32u8..128 {
                    let base = page * 0x4000 + 0x3d00 + (c as usize - 32) * 8;
                    if (0..8).all(|i| spec.bus.rom[base + i] == cell[i]) {
                        ch = c as char;
                        break 'find;
                    }
                }
            }
            line.push(ch);
        }
        out.push(line.trim_end().to_string());
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

/// Boot a 48K, type `LOAD ""`, play the tape and wait for the results.
fn run_halt2int(late: bool) -> Option<Vec<String>> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let tape = Tape::load(std::path::Path::new("tapes/halt2int.tap")).ok()?;

    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.set_late_timing(late);
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

    for _ in 0..6000 {
        spec.run(FRAME_T);
        if screen_text(&spec).iter().any(|l| l.contains("OK,")) {
            return Some(screen_text(&spec));
        }
    }
    panic!("HALT2INT did not finish");
}

/// What a real 48K with early timing prints.
const EARLY: &[&str] = &[
    "Float: Early    HALT: Early",
    "ADDR  CYCLE R   ADDR  CYCLE R",
    "16384/12000:0C  32767/12000:0C",
    "32768/12000:0C  49151/12000:0C",
    "49152/12000:0C  65535/12000:0C",
    "",
    "16384/14335:44  32767/14335:43",
    "32768/14335:45  49151/14335:45",
    "49152/14335:45  65535/14335:45",
    "",
    "16384/14336:44  32767/14336:43",
    "32768/14336:44  49151/14336:44",
    "49152/14336:44  65535/14336:44",
    "",
    "16384/14562:1C  32767/14562:0B",
    "32768/14562:0C  49151/14562:0C",
    "49152/14562:0C  65535/14562:0C",
    "",
    "16384/57239:5D  32767/57239:5D",
    "32768/57239:5F  49151/57239:5F",
    "49152/57239:5F  65535/57239:5F",
];

/// And with late timing: the display runs one T-state later relative to the
/// interrupt, which moves the values on the boundary rows.
const LATE: &[&str] = &[
    "Float: Late     HALT: Late",
    "ADDR  CYCLE R   ADDR  CYCLE R",
    "16384/12000:0C  32767/12000:0C",
    "32768/12000:0C  49151/12000:0C",
    "49152/12000:0C  65535/12000:0C",
    "",
    "16384/14335:45  32767/14335:45",
    "32768/14335:45  49151/14335:45",
    "49152/14335:45  65535/14335:45",
    "",
    "16384/14336:44  32767/14336:43",
    "32768/14336:44  49151/14336:44",
    "49152/14336:44  65535/14336:44",
    "",
    "16384/14562:1C  32767/14562:0B",
    "32768/14562:0C  49151/14562:0C",
    "49152/14562:0C  65535/14562:0C",
    "",
    "16384/57239:5E  32767/57239:5F",
    "32768/57239:5F  49151/57239:5F",
    "49152/57239:5F  65535/57239:5F",
];

fn check(expected: &[&str], actual: &[String], what: &str) {
    for (i, want) in expected.iter().enumerate() {
        assert_eq!(
            actual.get(i).map(String::as_str).unwrap_or(""),
            *want,
            "{what} timing, screen line {i}:\n  got      {:?}\n  expected {want:?}\nfull screen:\n{}",
            actual.get(i).map(String::as_str).unwrap_or(""),
            actual.join("\n")
        );
    }
}

#[test]
fn halt2int_matches_a_real_48k_with_early_timing() {
    let Some(text) = run_halt2int(false) else {
        eprintln!("need roms/48.rom and tapes/halt2int.tap; skipping");
        return;
    };
    check(EARLY, &text, "early");
}

#[test]
fn halt2int_matches_a_real_48k_with_late_timing() {
    let Some(text) = run_halt2int(true) else {
        eprintln!("need roms/48.rom and tapes/halt2int.tap; skipping");
        return;
    };
    check(LATE, &text, "late");
}

/// The halt state performs its M1 cycles at PC — the byte *after* the HALT —
/// so a HALT at the end of a contended page refreshes from an uncontended one.
/// This is what HALT2INT's $7FFF column measures.
#[test]
fn the_halt_state_refreshes_from_the_byte_after_the_halt() {
    for (halt_at, contended) in [(0x7fffu16, false), (0x4000u16, true)] {
        let mut spec = Spectrum::with_model(Model::Spectrum48);
        spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);
        spec.bus.poke(halt_at, 0x76); // HALT
        spec.cpu.pc = halt_at;

        // Park in the middle of the display, where contention is at its worst.
        spec.bus.tstates = spec.bus.first_pixel_t() + 100 * 224;
        spec.step_instruction(); // the HALT instruction itself
        assert!(spec.cpu.halted);

        // Time only the halt state, so the contention on the initial fetch of
        // the HALT opcode does not muddy the measurement.
        let start = spec.bus.tstates;
        for _ in 0..16 {
            spec.step_instruction();
        }
        let elapsed = spec.bus.tstates - start;

        if contended {
            assert!(
                elapsed > 16 * 4,
                "a HALT at ${halt_at:04X} refreshes from ${:04X} and should be \
                 slowed by contention, but took {elapsed}T",
                halt_at.wrapping_add(1)
            );
        } else {
            assert_eq!(
                elapsed,
                16 * 4,
                "a HALT at ${halt_at:04X} refreshes from ${:04X}, which is uncontended",
                halt_at.wrapping_add(1)
            );
        }
    }
}

#[test]
fn leaving_the_halt_state_pushes_the_address_after_it() {
    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.rom.iter_mut().for_each(|b| *b = 0x00);
    spec.bus.poke(0x8000, 0x76); // HALT
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0xc000;
    spec.cpu.iff1 = true;
    spec.cpu.im = 1;

    spec.step_instruction();
    assert!(spec.cpu.halted);
    assert_eq!(spec.cpu.pc, 0x8001, "PC sits after the HALT while halted");

    spec.bus.irq_pending = true;
    spec.bus.tstates = 0;
    spec.step_instruction();

    assert!(!spec.cpu.halted, "the interrupt wakes it");
    assert_eq!(spec.cpu.pc, 0x0039, "and runs the handler at $0038");
    let pushed = u16::from_le_bytes([spec.bus.peek_raw(0xbffe), spec.bus.peek_raw(0xbfff)]);
    assert_eq!(pushed, 0x8001, "the return address is the byte after the HALT");
}
