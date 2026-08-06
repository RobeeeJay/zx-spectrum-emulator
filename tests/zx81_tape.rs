//! ZX81 tapes: the .p/.81/.p81 formats, the pulse train they turn into, and a
//! real load driven by the ROM's own loader.

use std::path::Path;
use zx_spectrum_emulator::tape::{
    zx81_block, zx81_name, Block, Tape, ZX81_BIT_GAP, ZX81_HALF_PULSE, ZX81_ONE_PULSES,
    ZX81_ZERO_PULSES,
};
use zx_spectrum_emulator::zx81::{Ram, Zx81};

// ---- the file formats ------------------------------------------------------

#[test]
fn a_name_is_encoded_in_the_zx81_character_set() {
    // A=$26, so CAT is $28,$26,$45... and the last byte carries bit 7.
    assert_eq!(zx81_name("A"), vec![0x26 | 0x80]);
    assert_eq!(zx81_name("AB"), vec![0x26, 0x27 | 0x80]);
    assert_eq!(zx81_name("0"), vec![0x1c | 0x80]);
    // Case does not survive: the ZX81 has no lower case.
    assert_eq!(zx81_name("jetpac"), zx81_name("JETPAC"));
}

#[test]
fn characters_the_zx81_does_not_have_are_dropped() {
    assert_eq!(zx81_name("3D_Maze!"), zx81_name("3DMAZE"));
    // Trailing spaces would otherwise be saved as part of the name.
    assert_eq!(zx81_name("CHESS   "), zx81_name("CHESS"));
    // Something has to go out, or the loader never sees a name at all.
    assert_eq!(zx81_name("!!!"), zx81_name("L"));
    assert_eq!(zx81_name(""), zx81_name("L"));
}

#[test]
fn a_p_file_becomes_one_block_of_name_then_program() {
    let dir = tmp("p-file");
    let program = vec![0x11u8; 300];
    std::fs::write(dir.join("JetPac.p"), &program).unwrap();
    let tape = Tape::load(&dir.join("JetPac.p")).unwrap();
    assert_eq!(tape.blocks.len(), 1);
    match &tape.blocks[0] {
        Block::Zx81 { name, data, .. } => {
            assert_eq!(name, "JETPAC");
            // The name goes out in front of the program, on the same stream.
            assert_eq!(data.len(), program.len() + 6);
            assert_eq!(&data[6..], &program[..]);
        }
        other => panic!("expected a ZX81 block, got {other:?}"),
    }
}

#[test]
fn a_p81_file_carries_its_own_name() {
    let dir = tmp("p81-file");
    // "AB" as ZX81 codes, bit 7 marking the end, then the program.
    let mut file = vec![0x26, 0x27 | 0x80];
    file.extend_from_slice(&[1, 2, 3, 4]);
    std::fs::write(dir.join("whatever.p81"), &file).unwrap();
    let tape = Tape::load(&dir.join("whatever.p81")).unwrap();
    match &tape.blocks[0] {
        Block::Zx81 { name, data, .. } => {
            assert_eq!(
                name, "AB",
                "the name comes from the file, not the file name"
            );
            assert_eq!(data, &file);
        }
        other => panic!("expected a ZX81 block, got {other:?}"),
    }
}

#[test]
fn a_p81_without_an_end_of_name_byte_is_rejected() {
    let dir = tmp("p81-bad");
    std::fs::write(dir.join("bad.p81"), vec![0x26; 200]).unwrap();
    let err = match Tape::load(&dir.join("bad.p81")) {
        Err(e) => e,
        Ok(_) => panic!("a .p81 with no end-of-name byte should not load"),
    };
    assert!(err.contains("end-of-name"), "unhelpful message: {err}");
}

#[test]
fn a_zx81_block_reports_its_size_and_name() {
    let block = zx81_block(&zx81_name("CHESS"), &[0; 100]);
    let text = block.describe();
    assert!(text.contains("ZX81"), "{text}");
    assert!(text.contains("CHESS"), "{text}");
    assert!(block.is_data(), "it makes a sound, so it is a data block");
}

// ---- the pulse train -------------------------------------------------------

/// Play a tape out and return the length of every level change, in T-states.
fn pulse_lengths(tape: &mut Tape, limit: u64) -> Vec<u64> {
    tape.play(0);
    let mut runs = Vec::new();
    let mut last_edge = 0;
    let mut level = false;
    for t in 0..limit {
        let now = tape.level_at(t);
        if now != level {
            level = now;
            runs.push(t - last_edge);
            last_edge = t;
        }
        if !tape.playing {
            break;
        }
    }
    runs
}

#[test]
fn a_zero_bit_is_four_pulses_and_a_one_bit_is_nine() {
    // $80 as the name means one bit, then the program byte $00 gives eight
    // zero bits — enough to see both burst lengths.
    let mut tape = Tape::from_blocks("t".into(), vec![zx81_block(&[0x80], &[0x00])]);
    let runs = pulse_lengths(&mut tape, 400_000);

    // Runs come in half-pulses; a burst ends with a long run carrying the gap.
    let bursts: Vec<usize> = runs
        .split_inclusive(|r| *r > ZX81_HALF_PULSE as u64)
        .map(|b| b.len() / 2)
        .collect();
    assert_eq!(
        &bursts[..9],
        &[
            ZX81_ONE_PULSES as usize, // the name byte's top bit, which is set
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
            ZX81_ZERO_PULSES as usize,
        ]
    );
}

#[test]
fn every_half_pulse_is_the_same_length_and_each_bit_ends_with_a_gap() {
    let mut tape = Tape::from_blocks("t".into(), vec![zx81_block(&[0x80], &[0xff])]);
    let runs = pulse_lengths(&mut tape, 400_000);

    let gap = ZX81_HALF_PULSE as u64 + ZX81_BIT_GAP as u64;
    // The first run is the leading edge at t=0, and the last is the block's
    // pause running on from the final bit's gap; the rest are one or the other.
    for r in &runs[1..runs.len() - 1] {
        assert!(
            *r == ZX81_HALF_PULSE as u64 || *r == gap,
            "unexpected run of {r}T"
        );
    }
    // Two bytes went out — the name and the data — so sixteen bits, the last
    // of whose gaps is swallowed by the pause.
    assert_eq!(runs.iter().filter(|r| **r == gap).count(), 15);
}

#[test]
fn the_bit_gap_is_long_enough_to_be_seen_as_the_end_of_a_burst() {
    // The ROM's loader ends a bit after 26 passes of a 51T loop with the tape
    // low, and gives up entirely after about 17400T of silence. The gap has to
    // sit between the two.
    let gap = ZX81_BIT_GAP as u64 + ZX81_HALF_PULSE as u64;
    assert!(gap > 26 * 51, "gap of {gap}T is too short to end a bit");
    assert!(
        gap < 17_000,
        "gap of {gap}T would look like the end of the tape"
    );
    // And the low half of an ordinary pulse must not end a bit by itself.
    assert!((ZX81_HALF_PULSE as u64) < 26 * 51);
}

#[test]
fn a_block_knows_how_long_it_takes_to_play() {
    let block = zx81_block(&[0x80], &[0x00; 100]);
    let seconds = block.duration_t() as f64 / zx_spectrum_emulator::zx81::CPU_HZ;
    // The ZX81 saves at roughly 50 bytes a second, so 101 bytes is about two
    // seconds. Anything wildly off means the pulse arithmetic is wrong.
    assert!(
        (1.0..4.0).contains(&seconds),
        "101 bytes should take a couple of seconds, not {seconds:.1}"
    );
    // The progress bar needs the time to be all data, with a pause after it.
    let seg = block.segment_times();
    assert_eq!(seg.pilot, 0, "a ZX81 tape has no pilot tone");
    assert!(seg.data > 0 && seg.pause > 0);
}

// ---- the real thing --------------------------------------------------------

fn rom() -> Option<Vec<u8>> {
    std::fs::read("roms/zx81.rom").ok()
}

/// A ZX81 sitting at the `K` cursor, with `LOAD ""` typed in and entered.
fn ready_to_load() -> Option<Zx81> {
    let mut zx = Zx81::new(Ram::K16);
    zx.load_rom(&rom()?);
    zx.reset();
    for _ in 0..200 {
        zx.run(zx.frame_t());
    }
    let key = |zx: &mut Zx81, keys: [u8; 8]| {
        zx.bus.keys = keys;
        for _ in 0..12 {
            zx.run(zx.frame_t());
        }
        zx.bus.keys = [0xff; 8];
        for _ in 0..12 {
            zx.run(zx.frame_t());
        }
    };
    let down = |row: usize, bit: u8| {
        let mut k = [0xffu8; 8];
        k[row] &= !(1 << bit);
        k
    };
    key(&mut zx, down(6, 3)); // J, which is LOAD in keyword mode
    for _ in 0..2 {
        let mut k = [0xffu8; 8];
        k[0] &= !1; // shift
        k[5] &= !1; // P, giving a quote
        key(&mut zx, k);
    }
    key(&mut zx, down(6, 0)); // NEWLINE
    Some(zx)
}

/// Run until the tape runs out, or give up.
fn play_out(zx: &mut Zx81, frames: usize) -> usize {
    for frame in 0..frames {
        zx.run(zx.frame_t());
        zx.bus.tape_tick();
        if !zx.bus.tape.as_ref().is_some_and(|t| t.playing) {
            return frame;
        }
    }
    frames
}

#[test]
fn the_rom_loads_a_program_off_the_tape() {
    let path = Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    let (Some(mut zx), Ok(file)) = (ready_to_load(), std::fs::read(path)) else {
        eprintln!("no ZX81 ROM or test tape; skipping");
        return;
    };
    let mut tape = Tape::load(path).unwrap();
    tape.play(zx.bus.tstates);
    zx.bus.tape = Some(tape);

    let frames = play_out(&mut zx, 3000);
    assert!(frames < 3000, "the tape never finished");

    // The BASIC program area is what the loader put there; the system
    // variables below it are already being churned by the running program.
    let basic = 0x74;
    let loaded: Vec<u8> = (basic..file.len())
        .map(|i| zx.bus.ram[(0x0009 + i) & 0x3fff])
        .collect();
    assert_eq!(
        loaded,
        file[basic..],
        "the program in memory is not the one on the tape"
    );
}

#[test]
fn the_loaded_program_runs_and_draws_something() {
    let path = Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    let (Some(mut zx), true) = (ready_to_load(), path.exists()) else {
        eprintln!("no ZX81 ROM or test tape; skipping");
        return;
    };
    let mut tape = Tape::load(path).unwrap();
    tape.play(zx.bus.tstates);
    zx.bus.tape = Some(tape);
    play_out(&mut zx, 3000);
    for _ in 0..200 {
        zx.run(zx.frame_t());
    }

    // 1K ZX Chess draws a board: a lot more ink than the bare `K` cursor, and
    // spread over a good part of the screen rather than one corner.
    let ink: Vec<usize> = zx
        .bus
        .fb_prev
        .iter()
        .enumerate()
        .filter(|(_, p)| **p != 0)
        .map(|(i, _)| i)
        .collect();
    assert!(ink.len() > 500, "only {} pixels of ink", ink.len());
    let w = zx_spectrum_emulator::zx81::RASTER_W;
    let rows = ink.last().unwrap() / w - ink[0] / w;
    assert!(rows > 40, "the picture is only {rows} lines tall");
}

#[test]
fn the_tape_reports_progress_as_it_plays() {
    let path = Path::new("tapes/zx81/1KZXChess.1.ChessQueen.p");
    let Ok(mut tape) = Tape::load(path) else {
        return;
    };
    tape.play(0);
    assert_eq!(tape.block_progress(), Some(0.0));
    let mut seen = vec![];
    for t in (0..120_000_000u64).step_by(200_000) {
        tape.level_at(t);
        if let Some(p) = tape.block_progress() {
            seen.push(p);
        }
        if !tape.playing {
            break;
        }
    }
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "progress went backwards"
    );
    assert!(
        seen.last().copied().unwrap_or(0.0) > 0.9,
        "progress only reached {:?}",
        seen.last()
    );
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zx81-tape-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
