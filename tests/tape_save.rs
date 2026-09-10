//! SAVE from the real 48K ROM onto a blank tape, then LOAD it back.
//!
//! `tests/recorder.rs` says the reading is right against pulse trains built to
//! the ROM's timings; this one lets the ROM make them. A program is typed in,
//! saved onto a blank tape with `SAVE "x"`, the machine is cleared with NEW,
//! and `LOAD ""` played off the same tape has to put the program back byte for
//! byte. Skips itself without `roms/48.rom`.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::recorder::Recorder;
use zx_rustrum::tape::{Block, Tape};

const SYM: (usize, u8) = (7, 1);
const ENTER: (usize, u8) = (6, 0);
/// In K mode these are keywords: E is REM, S is SAVE, A is NEW, J is LOAD.
const E: (usize, u8) = (2, 2);
const S: (usize, u8) = (1, 1);
const A: (usize, u8) = (1, 0);
const J: (usize, u8) = (6, 3);
const P: (usize, u8) = (5, 0);
const X: (usize, u8) = (0, 2);
const H: (usize, u8) = (6, 4);
const I: (usize, u8) = (5, 2);
const ONE: (usize, u8) = (3, 0);
const ZERO: (usize, u8) = (4, 0);

/// Hold keys down long enough for the ROM to believe in them, then let go.
fn hold(spec: &mut Spectrum, keys: &[(usize, u8)]) {
    for (row, bit) in keys {
        spec.bus.keys[*row] &= !(1 << bit);
    }
    for _ in 0..6 {
        spec.run(FRAME_T);
    }
    for (row, bit) in keys {
        spec.bus.keys[*row] |= 1 << bit;
    }
    for _ in 0..6 {
        spec.run(FRAME_T);
    }
}

fn quote(spec: &mut Spectrum) {
    hold(spec, &[SYM, P]);
}

/// The BASIC program as it sits in memory, from PROG up to VARS.
fn program(spec: &Spectrum) -> Vec<u8> {
    let word = |at: u16| u16::from_le_bytes([spec.bus.peek_raw(at), spec.bus.peek_raw(at + 1)]);
    let (prog, vars) = (word(0x5C53), word(0x5C4B));
    (prog..vars).map(|a| spec.bus.peek_raw(a)).collect()
}

/// What the machine saves goes onto a blank tape as a header and a data
/// block, and the same tape loads the program back into a cleared machine.
#[test]
fn a_program_saved_onto_a_blank_tape_loads_back() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    let mut spec = Spectrum::new();
    spec.set_model(Model::Spectrum48, &rom);
    spec.reset();
    for _ in 0..200 {
        spec.run(FRAME_T);
    }

    // 10 REM hi
    for key in [ONE, ZERO, E, H, I, ENTER] {
        hold(&mut spec, &[key]);
    }
    let typed = program(&spec);
    assert!(
        typed.len() > 4 && typed[4] == 0xEA,
        "the line went in as REM: {typed:02X?}"
    );

    // What the Blank button does.
    spec.bus.tape = Some(Tape::from_blocks("Blank tape".into(), Vec::new()));
    spec.bus.recorder = Some(Recorder::new());

    // SAVE "x", then any key when it asks for the tape to be started.
    hold(&mut spec, &[S]);
    quote(&mut spec);
    hold(&mut spec, &[X]);
    quote(&mut spec);
    hold(&mut spec, &[ENTER]);
    for _ in 0..20 {
        spec.run(FRAME_T);
    }
    hold(&mut spec, &[ENTER]);
    let mut frames = 0;
    while spec.bus.tape.as_ref().unwrap().blocks.len() < 2 && frames < 1500 {
        spec.run(FRAME_T);
        frames += 1;
    }
    let tape = spec.bus.tape.as_ref().unwrap();
    let recorder = spec.bus.recorder.as_ref().unwrap();
    assert_eq!(
        tape.blocks.len(),
        2,
        "a header and a data block after {frames} frames, with {} runs not read",
        recorder.unread
    );
    let datas: Vec<&Vec<u8>> = tape
        .blocks
        .iter()
        .map(|b| match b {
            Block::Standard { data, .. } => data,
            other => panic!("a standard block, not {other:?}"),
        })
        .collect();
    let header = datas[0];
    assert_eq!(header.len(), 19, "a header is 19 bytes: {header:02X?}");
    assert_eq!(
        (header[0], header[1]),
        (0x00, 0x00),
        "a header, of a program"
    );
    assert_eq!(&header[2..12], b"x         ", "named as it was typed");
    let body = datas[1];
    assert_eq!(body[0], 0xFF, "the data block's flag");
    assert_eq!(
        &body[1..body.len() - 1],
        &typed[..],
        "the program, as it was in memory"
    );
    let check = body.iter().fold(0u8, |a, b| a ^ b);
    assert_eq!(check, 0, "the checksum the ROM added holds");

    // NEW, then LOAD "" off the same tape, rewound.
    for _ in 0..50 {
        spec.run(FRAME_T);
    }
    hold(&mut spec, &[A]);
    hold(&mut spec, &[ENTER]);
    for _ in 0..100 {
        spec.run(FRAME_T);
    }
    assert!(program(&spec).is_empty(), "NEW cleared the program");

    hold(&mut spec, &[J]);
    quote(&mut spec);
    quote(&mut spec);
    hold(&mut spec, &[ENTER]);
    let now = spec.bus.total_t();
    let tape = spec.bus.tape.as_mut().unwrap();
    tape.rewind();
    tape.play(now);
    let mut frames = 0;
    while program(&spec) != typed && frames < 1500 {
        spec.run(FRAME_T);
        frames += 1;
    }
    assert_eq!(
        program(&spec),
        typed,
        "the program loaded back after {frames} frames"
    );
    assert_eq!(
        spec.bus.tape.as_ref().unwrap().blocks.len(),
        2,
        "and loading it recorded nothing more onto the tape"
    );
}
