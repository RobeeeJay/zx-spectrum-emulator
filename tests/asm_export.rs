//! The program in RAM as assembly source, and that source assembled back.
//!
//! The promise is that the file assembles into the bytes it came from, and
//! the only honest test of that is an assembler. None is part of the build,
//! so these use sjasmplus when one is named in ZXRS_Z80ASM or found on the
//! path, and skip that part otherwise — the same bargain as the ROMs.

use std::path::PathBuf;
use zx_rustrum::asmexport;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::notes::Notes;

fn assembler() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ZXRS_Z80ASM").map(PathBuf::from) {
        return p.exists().then_some(p);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("sjasmplus"))
            .find(|p| p.exists())
    })
}

/// Assemble with sjasmplus to a raw binary, and hand back the bytes.
fn assemble(asm: &std::path::Path, source: &str, name: &str) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("zxrs-asm-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (src, bin) = (dir.join("program.asm"), dir.join("program.bin"));
    std::fs::write(&src, source).unwrap();
    let out = std::process::Command::new(asm)
        .arg("--nologo")
        .arg(format!("--raw={}", bin.display()))
        .arg(&src)
        .output()
        .expect("the assembler runs");
    assert!(
        out.status.success(),
        "the assembler refused it:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&bin).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

/// A program full of the awkward cases, run so the tracker has seen its code.
fn awkward() -> (Spectrum, Notes) {
    let mut spec = Spectrum::new();
    let program: &[(u16, &[u8])] = &[
        (0x8000, &[0x3E, 0x05]),             // LD A,$05
        (0x8002, &[0xDD, 0x21, 0x00, 0x90]), // LD IX,$9000
        (0x8006, &[0xDD, 0x7E, 0x03]),       // LD A,(IX+$03)
        (0x8009, &[0xDD, 0x26, 0x07]),       // LD IXH,$07: undocumented
        (0x800C, &[0xED, 0x4C]),             // NEG, a duplicate encoding
        (0x800E, &[0xED, 0x6B, 0x00, 0x90]), // LD HL,($9000), the long form
        (0x8012, &[0xCB, 0x37]),             // SLL A: undocumented
        (0x8014, &[0xCD, 0x00, 0x81]),       // CALL $8100
        (0x8017, &[0xCD, 0x6B, 0x0D]),       // CALL $0D6B, in the ROM
        (0x801A, &[0x18, 0xFE]),             // JR $801A: never runs
        (0x8100, &[0xC9]),                   // RET
        (0x9000, &[1, 2, 3, 4, 5, 6, 7, 8]), // a table
    ];
    for (at, bytes) in program {
        for (i, b) in bytes.iter().enumerate() {
            spec.bus.poke(at + i as u16, *b);
        }
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.sp = 0x7F00;
    spec.run(FRAME_T);

    let mut notes = Notes::unattached();
    notes.set_label(0x8000, "start");
    notes.set_label(0x8100, "draw sprite");
    notes.set_label(0x8003, "ix_target"); // on an operand byte
    notes.set_label(0x9000, "HL"); // an assembler's own word
    notes.set_label(0x9004, "start"); // taken already
    notes.set_label(0x0D6B, "CLS"); // in the ROM
    notes.set_comment(0x8000, "first line\nsecond line");
    notes.set_comment(0x9000, "the table");
    (spec, notes)
}

/// Its shape, which needs no assembler: an ORG at $4000, labels an assembler
/// will take, the ROM's name as an EQU, the undocumented and the doubly
/// encoded as DEFB with the instruction beside them, and comments kept.
#[test]
fn the_source_is_laid_out_for_an_assembler() {
    let (spec, notes) = awkward();
    let out = asmexport::from_machine(&spec, &notes, None, "awkward");
    let text = &out.text;
    let has = |s: &str| assert!(text.contains(s), "{s:?} in:\n{text}");
    has("        ORG $4000");
    has("draw_sprite:   ; labelled \"draw sprite\"");
    has("CALL draw_sprite");
    has("CLS EQU $0D6B");
    has("CALL CLS");
    has("L_HL:");
    // Prefixed instructions are whole: the byte after the prefix is fetched
    // as an opcode too, and splitting there gave DEFB $DD and an LD HL.
    has("LD IX,L_HL");
    has("LD A,(IX+$03)");
    has("start_9004:");
    has("ix_target EQU $8003");
    has("DEFB $DD,$26,$07");
    has("; LD IXH,$07");
    has("DEFB $ED,$4C");
    has("DEFB $ED,$6B,$00,$90");
    has("DEFB $CB,$37");
    has("; first line");
    has("; second line");
    has("; the table");
    has("CRC32");
    assert!(
        !text.contains("JR $801A"),
        "the JR never ran, so it is data"
    );
    assert!(out.instructions >= 6, "{} instructions", out.instructions);
}

/// Assembled, it is the same 48K it came from, byte for byte.
#[test]
fn the_source_assembles_back_into_the_same_bytes() {
    let Some(asm) = assembler() else {
        eprintln!("no sjasmplus (set ZXRS_Z80ASM); skipping");
        return;
    };
    let (spec, notes) = awkward();
    let out = asmexport::from_machine(&spec, &notes, None, "awkward");
    let bytes = assemble(&asm, &out.text, "awkward");
    let memory: Vec<u8> = (0x4000..=0xFFFFu16).map(|a| spec.bus.peek_raw(a)).collect();
    assert_eq!(bytes.len(), memory.len(), "49,152 bytes from $4000");
    let first = bytes.iter().zip(&memory).position(|(a, b)| a != b);
    assert_eq!(
        first,
        None,
        "first difference at ${:04X}",
        first.unwrap_or(0) + 0x4000
    );
}

/// A real program: Border Break, loaded through the ROM and left running so
/// its code has run, assembles back into the same 48K.
#[test]
fn a_real_program_assembles_back_into_the_same_bytes() {
    let (Some(asm), Ok(rom), Ok(tape)) = (
        assembler(),
        std::fs::read("roms/48.rom"),
        zx_rustrum::tape::Tape::load(std::path::Path::new("tapes/borderbreak.tap")),
    ) else {
        eprintln!("need sjasmplus, roms/48.rom and tapes/borderbreak.tap; skipping");
        return;
    };
    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.reset();
    spec.bus.tape_flash = true;
    for _ in 0..120 {
        spec.run(FRAME_T);
    }
    spec.bus.tape = Some(tape);
    for keys in [
        &[(6usize, 3u8)][..],
        &[(7, 1), (5, 0)][..],
        &[(7, 1), (5, 0)][..],
        &[(6, 0)][..],
    ] {
        for (row, bit) in keys {
            spec.bus.keys[*row] &= !(1 << bit);
        }
        for _ in 0..4 {
            spec.run(FRAME_T);
        }
        for (row, bit) in keys {
            spec.bus.keys[*row] |= 1 << bit;
        }
        for _ in 0..4 {
            spec.run(FRAME_T);
        }
    }
    let now = spec.bus.total_t();
    spec.bus.tape.as_mut().unwrap().play(now);
    for _ in 0..600 {
        spec.run(FRAME_T);
    }
    let ran_in_ram = (0x4000..=0xFFFFu16)
        .filter(|a| spec.bus.tracker.executed[spec.bus.phys_index(*a)])
        .count();
    assert!(
        ran_in_ram > 20,
        "Border Break's code has run: {ran_in_ram} addresses"
    );

    let notes = Notes::unattached();
    let out = asmexport::from_machine(&spec, &notes, None, "Border Break");
    assert!(
        out.text.contains(&asmexport::describe_rom("ROM", &rom)),
        "the header names the ROM it ran against"
    );
    let bytes = assemble(&asm, &out.text, "borderbreak");
    let memory: Vec<u8> = (0x4000..=0xFFFFu16).map(|a| spec.bus.peek_raw(a)).collect();
    assert_eq!(bytes.len(), memory.len());
    let first = bytes.iter().zip(&memory).position(|(a, b)| a != b);
    assert_eq!(
        first,
        None,
        "first difference at ${:04X}",
        first.unwrap_or(0) + 0x4000
    );
    eprintln!(
        "Border Break: {} instructions, {} bytes as data",
        out.instructions, out.data_bytes
    );
}
