//! TZX parsing, pulse generation, and a real load through the 48K ROM's
//! LD-BYTES routine.

use std::path::{Path, PathBuf};

use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::tape::{
    Block, Tape, DATA_PILOT_PULSES, HEADER_PILOT_PULSES, ONE_PULSE, PILOT_PULSE, SYNC1_PULSE,
    SYNC2_PULSE, ZERO_PULSE,
};

fn tape_files() -> Vec<PathBuf> {
    let dir = Path::new("tapes");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("tzx") | Some("tap")
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_tape_in_the_tapes_directory_parses() {
    let files = tape_files();
    if files.is_empty() {
        eprintln!("no tapes/ directory; skipping");
        return;
    }
    for path in files {
        let tape = Tape::load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            !tape.blocks.is_empty(),
            "{}: parsed no blocks",
            path.display()
        );
        assert!(
            tape.blocks.iter().any(|b| b.is_data()),
            "{}: no data blocks",
            path.display()
        );
        // Every block must describe itself without panicking.
        for b in &tape.blocks {
            assert!(!b.describe().is_empty());
        }
    }
}

#[test]
fn the_first_block_of_a_standard_tape_is_a_recognisable_header() {
    let files = tape_files();
    let Some(path) = files
        .iter()
        .find(|p| p.to_string_lossy().contains("Jetpac"))
        .or_else(|| files.first())
    else {
        eprintln!("no tapes; skipping");
        return;
    };
    let tape = Tape::load(path).unwrap();
    let first = tape.blocks.iter().find(|b| b.is_data()).unwrap();
    match first {
        Block::Standard { data, .. } => {
            assert_eq!(data.len(), 19, "a ZX header block is 19 bytes");
            assert_eq!(data[0], 0x00, "header blocks start with flag byte $00");
            let parity = data[..18].iter().fold(0u8, |a, b| a ^ b);
            assert_eq!(parity, data[18], "header checksum");
        }
        other => panic!("expected a standard block first, got {other:?}"),
    }
}

/// Collect the pulse lengths a tape produces, by stepping time forward.
fn pulses_of(tape: &mut Tape, max: usize) -> Vec<u64> {
    tape.play(0);
    let mut out = Vec::new();
    let mut t = 0u64;
    let mut last_edges = tape.edges.len();
    let mut last_edge_t = 0u64;
    // Step in small increments so no pulse is skipped over.
    while out.len() < max && t < 200_000_000 {
        t += 100;
        tape.level_at(t);
        while tape.edges.len() > last_edges {
            let (edge_t, _) = tape.edges[last_edges];
            if last_edges > 0 {
                out.push(edge_t - last_edge_t);
            }
            last_edge_t = edge_t;
            last_edges += 1;
        }
        if !tape.playing {
            break;
        }
    }
    out
}

#[test]
fn a_standard_block_generates_rom_timings() {
    let data = vec![0x00, 0x03, b'A', 0xff];
    let mut tape = Tape::from_blocks(
        "test".into(),
        vec![Block::Standard {
            pause_ms: 100,
            data: data.clone(),
        }],
    );
    let pulses = pulses_of(&mut tape, HEADER_PILOT_PULSES as usize + 40);

    // A header block leads with the long pilot tone.
    assert!(pulses.len() > HEADER_PILOT_PULSES as usize + 2);
    for (i, p) in pulses.iter().take(HEADER_PILOT_PULSES as usize).enumerate() {
        assert_eq!(*p, PILOT_PULSE as u64, "pilot pulse {i}");
    }
    let after_pilot = HEADER_PILOT_PULSES as usize;
    assert_eq!(pulses[after_pilot], SYNC1_PULSE as u64, "sync 1");
    assert_eq!(pulses[after_pilot + 1], SYNC2_PULSE as u64, "sync 2");

    // First data byte is $00, so eight pairs of zero-length pulses.
    for i in 0..16 {
        assert_eq!(
            pulses[after_pilot + 2 + i],
            ZERO_PULSE as u64,
            "bit pulse {i} of byte $00"
        );
    }
    // Second byte is $03: six zero bits then two one bits.
    let b1 = after_pilot + 2 + 16;
    for i in 0..12 {
        assert_eq!(pulses[b1 + i], ZERO_PULSE as u64, "high bits of $03");
    }
    for i in 12..16 {
        assert_eq!(pulses[b1 + i], ONE_PULSE as u64, "low bits of $03");
    }
}

#[test]
fn a_data_block_uses_the_short_pilot() {
    let mut tape = Tape::from_blocks(
        "test".into(),
        vec![Block::Standard {
            pause_ms: 0,
            data: vec![0xff, 0x00, 0xff],
        }],
    );
    let pulses = pulses_of(&mut tape, DATA_PILOT_PULSES as usize + 4);
    assert_eq!(pulses[DATA_PILOT_PULSES as usize], SYNC1_PULSE as u64);
}

#[test]
fn turbo_blocks_use_their_own_timings() {
    let mut tape = Tape::from_blocks(
        "test".into(),
        vec![Block::Turbo {
            pilot: 1000,
            sync1: 200,
            sync2: 300,
            zero: 400,
            one: 800,
            pilot_pulses: 10,
            used_bits: 8,
            pause_ms: 0,
            data: vec![0x80],
        }],
    );
    let pulses = pulses_of(&mut tape, 32);
    assert_eq!(&pulses[..10], &[1000u64; 10], "ten pilot pulses");
    assert_eq!(pulses[10], 200, "sync 1");
    assert_eq!(pulses[11], 300, "sync 2");
    // $80 is one set bit then seven clear ones; each bit is two pulses.
    assert_eq!(pulses[12], 800);
    assert_eq!(pulses[13], 800);
    assert_eq!(pulses[14], 400);
}

#[test]
fn loops_jumps_and_stops_are_honoured() {
    let mut tape = Tape::from_blocks(
        "test".into(),
        vec![
            Block::LoopStart(3),
            Block::PureTone { len: 100, count: 2 },
            Block::LoopEnd,
            Block::Pause(0), // stop the tape
            Block::PureTone { len: 100, count: 2 },
        ],
    );
    let pulses = pulses_of(&mut tape, 100);
    // Three passes of two pulses; the gaps between them are all 100T.
    assert!(pulses.len() >= 5, "got {} pulses", pulses.len());
    assert!(pulses.iter().take(5).all(|p| *p == 100));
    assert!(!tape.playing, "a pause-of-zero block must stop the tape");
    assert!(tape.stopped_by_block);
}

#[test]
fn skipping_moves_between_data_blocks() {
    let mut tape = Tape::from_blocks(
        "test".into(),
        vec![
            Block::Standard {
                pause_ms: 0,
                data: vec![0x00],
            },
            Block::Info("text".into()),
            Block::Standard {
                pause_ms: 0,
                data: vec![0xff],
            },
        ],
    );
    assert_eq!(tape.next_data_block(1), 2, "skips over the info block");
    tape.seek(2);
    assert_eq!(tape.next_data_block(-1), 0);
}

/// Drive the real ROM tape loader and check it reads a header off a TZX.
#[test]
fn the_rom_loader_reads_a_header_from_a_real_tzx() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("no roms/48.rom; skipping");
        return;
    };
    let files = tape_files();
    let Some(path) = files.first() else {
        eprintln!("no tapes; skipping");
        return;
    };

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    let tape = Tape::load(path).unwrap();
    let expected = match tape.blocks.iter().find(|b| b.is_data()).unwrap() {
        Block::Standard { data, .. } => data.clone(),
        other => panic!("first data block is not standard: {other:?}"),
    };
    assert_eq!(expected.len(), 19);
    spec.bus.tape = Some(tape);

    // Call LD-BYTES at $0556 directly: A = expected flag byte, IX = target,
    // DE = length, carry set for LOAD (rather than VERIFY).
    const SENTINEL: u16 = 0xfff0;
    const TARGET: u16 = 0x8000;
    spec.cpu.pc = 0x0556;
    spec.cpu.a = 0x00;
    spec.cpu.ix = TARGET;
    spec.cpu.set_de(17);
    spec.cpu.iy = 0x5c3a;
    spec.cpu.sp = 0x7ff0;
    spec.cpu.f |= 0x01; // CF = load
    spec.bus.poke(0x7ff0, SENTINEL as u8);
    spec.bus.poke(0x7ff1, (SENTINEL >> 8) as u8);

    let now = spec.bus.total_t();
    spec.bus.tape.as_mut().unwrap().play(now);

    // The header's pilot tone alone is about 17.5M T-states.
    spec.breakpoints.push(SENTINEL);
    let mut spent = 0u64;
    let mut returned = false;
    while spent < 60_000_000 {
        if matches!(
            spec.run(FRAME_T),
            zx_rustrum::machine::Stop::Breakpoint(SENTINEL)
        ) {
            returned = true;
            break;
        }
        spent += FRAME_T as u64;
    }

    assert!(
        returned,
        "LD-BYTES never returned (ran {spent} T-states, pc now ${:04X})",
        spec.cpu.pc
    );
    assert!(
        spec.cpu.f & 0x01 != 0,
        "LD-BYTES reported a load error (carry clear)"
    );
    let loaded: Vec<u8> = (0..17).map(|i| spec.bus.peek_raw(TARGET + i)).collect();
    assert_eq!(
        loaded,
        expected[1..18].to_vec(),
        "loaded header does not match the tape"
    );
}

// ---------------------------------------------------------------------------
// full load through BASIC
// ---------------------------------------------------------------------------

/// Hold a set of (row, bit) keys down for `frames` frames, then release them
/// for the same number, which is one keypress as far as the ROM is concerned.
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

const K_ENTER: (usize, u8) = (6, 0);
const K_J: (usize, u8) = (6, 3);
const K_P: (usize, u8) = (5, 0);
const K_SYMBOL: (usize, u8) = (7, 1);

/// Type `LOAD ""`, press ENTER, start the tape and let a real game load.
#[test]
fn a_real_tape_loads_through_basic() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("no roms/48.rom; skipping");
        return;
    };
    let files = tape_files();
    let Some(path) = files
        .iter()
        .find(|p| p.to_string_lossy().contains("Jetpac"))
        .or_else(|| files.first())
    else {
        eprintln!("no tapes; skipping");
        return;
    };

    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.reset();
    // Let the ROM finish its power-on self test and reach the BASIC prompt.
    for _ in 0..120 {
        spec.run(FRAME_T);
    }

    press(&mut spec, &[K_J], 4); // LOAD (K mode)
    press(&mut spec, &[K_SYMBOL, K_P], 4); // "
    press(&mut spec, &[K_SYMBOL, K_P], 4); // "
    press(&mut spec, &[K_ENTER], 4);

    let tape = Tape::load(path).unwrap();
    let blocks = tape.blocks.len();
    spec.bus.tape = Some(tape);
    let now = spec.bus.total_t();
    spec.bus.tape.as_mut().unwrap().play(now);

    // Jetpac is about 16K of tape: well under two emulated minutes.
    let mut frames = 0;
    while frames < 8000 {
        spec.run(FRAME_T);
        frames += 1;
        let tape = spec.bus.tape.as_ref().unwrap();
        if tape.finished() || (!tape.playing && tape.block + 1 >= blocks) {
            break;
        }
    }

    let tape = spec.bus.tape.as_ref().unwrap();
    eprintln!(
        "frames {frames}, block {}/{blocks}, pulses {}, playing {}, stopped_by_block {}",
        tape.block, tape.pulses, tape.playing, tape.stopped_by_block
    );
    assert!(
        tape.block > 0,
        "the tape never advanced past its first block"
    );
    assert!(
        tape.pulses > 100_000,
        "only {} pulses played — the loader was not reading",
        tape.pulses
    );

    // A loaded game has filled video RAM with something.
    let nonzero = (0x4000..0x5b00u32)
        .filter(|a| spec.bus.peek_raw(*a as u16) != 0)
        .count();
    assert!(
        nonzero > 2000,
        "video RAM looks empty after loading ({nonzero} non-zero bytes)"
    );
}

#[test]
fn a_blocks_progress_runs_through_the_pause_it_ends_with() {
    // A standard block is sound followed by silence, and the silence can be
    // most of it. Progress used to jump to the end as soon as the sound
    // stopped and sit there, so the marker on the block's row appeared to
    // vanish part way through.
    let block = Block::Standard {
        pause_ms: 1000,
        data: vec![0xff; 200],
    };
    let total = block.duration_t();
    let mut tape = Tape::from_blocks("t".into(), vec![block]);
    tape.play(0);

    let mut readings = Vec::new();
    for step in 0..=20u64 {
        tape.level_at(step * total / 20);
        readings.push(
            tape.block_progress()
                .expect("a block being played always has progress to show"),
        );
    }

    assert!(
        readings.windows(2).all(|w| w[1] >= w[0]),
        "progress should only ever move forwards: {readings:?}"
    );
    // Half way through this block the sound has finished and the pause has
    // started, so this is the part that used to be stuck.
    let (before, after) = (readings[10], readings[15]);
    assert!(
        after > before + 0.1,
        "progress should keep moving through the silence, {before} to {after}"
    );
    assert!(
        readings.last().is_some_and(|p| *p > 0.85),
        "and reach the end of the block: {readings:?}"
    );
}

/// A TZX custom info block names itself in sixteen bytes, not ten.
///
/// Reading ten took the block's length from the last four characters of its
/// name — spaces, which is a block claiming 0x20202020 bytes — and the rest of
/// the tape was unreadable behind it. Sidewize carries one, holding its POKEs,
/// and would not load at all.
#[test]
fn a_custom_info_block_is_stepped_over_whole() {
    use zx_rustrum::tape::{parse_tzx, Block};

    let mut tzx: Vec<u8> = Vec::new();
    tzx.extend_from_slice(b"ZXTape!\x1a");
    tzx.extend_from_slice(&[1, 20]);

    // A custom info block: sixteen bytes of name, a length, then the info.
    let info = b"POKE 40000,0";
    tzx.push(0x35);
    tzx.extend_from_slice(b"POKEs           ");
    tzx.extend_from_slice(&(info.len() as u32).to_le_bytes());
    tzx.extend_from_slice(info);

    // And a standard block after it, which is what goes missing when the
    // block before is not stepped over properly.
    let data = vec![0xFFu8, 0x01, 0x02, 0xFC];
    tzx.push(0x10);
    tzx.extend_from_slice(&1000u16.to_le_bytes());
    tzx.extend_from_slice(&(data.len() as u16).to_le_bytes());
    tzx.extend_from_slice(&data);

    let blocks = parse_tzx(&tzx).expect("it should read");
    assert_eq!(blocks.len(), 2, "the custom info and the block after it");
    match &blocks[0] {
        Block::Info(text) => assert!(
            text.contains("POKEs"),
            "the block should be named by all sixteen bytes: {text:?}"
        ),
        other => panic!("expected the custom info, got {other:?}"),
    }
    match &blocks[1] {
        Block::Standard { data: found, .. } => assert_eq!(found, &data),
        other => panic!("expected the standard block, got {other:?}"),
    }
}

/// Every pulse ends with the line changing, including the last one of a block.
///
/// The silence behind a block is at the low level, so a block whose last pulse
/// was already low used to end with no change at all — and a loader waiting
/// for that closing edge waited for ever. Cobra's Alkatraz loader reads all
/// eight bits of its last byte and then hangs on it, and its protection takes
/// the silence for a snapped tape: it wipes itself, beeps, and resets the
/// machine.
#[test]
fn the_last_pulse_of_a_block_is_finished_before_the_silence() {
    use zx_rustrum::tape::{Block, Tape};

    // One byte of zeros: an even number of pulses, so the line is back where
    // it started — low — when the data runs out.
    let mut tape = Tape::from_blocks(
        "t".into(),
        vec![Block::PureData {
            data: vec![0x00],
            zero: 500,
            one: 1000,
            used_bits: 8,
            pause_ms: 100,
        }],
    );
    tape.play(0);

    // Every level change the deck makes, and when.
    let mut edges = Vec::new();
    let mut level = tape.level_at(0);
    for t in (0..500_000u64).step_by(10) {
        let now = tape.level_at(t);
        if now != level {
            edges.push(t);
            level = now;
        }
    }

    // Eight bits, two pulses each, is sixteen edges. The seventeenth finishes
    // the sixteenth pulse at its own length; the eighteenth is the line
    // dropping into the silence a millisecond later.
    assert_eq!(
        edges.len(),
        17,
        "expected fifteen changes within the sixteen pulses, the edge that \
         closes the last of them, and the drop into silence, not {edges:?}"
    );
    let closing = edges[15] - edges[14];
    assert!(
        (490..=510).contains(&closing),
        "the last pulse should be finished off after its own 500 T-states, \
         not {closing}"
    );
    let silence = edges[16] - edges[15];
    assert!(
        (3490..=3510).contains(&silence),
        "and the line drops into the silence a millisecond later, not after \
         {silence} T-states"
    );
}
