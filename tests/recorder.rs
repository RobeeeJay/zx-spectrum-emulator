//! Reading what the machine sends through MIC back into blocks.
//!
//! The pulse trains here are built to the ROM's own timings — a pilot of
//! 2,168 T, sync of 667 and 735, and bits as pairs of 855 or 1,710 — which is
//! what SA-BYTES puts out. `tests/tape_save.rs` is the one that runs the real
//! ROM's SAVE; these say the reading is right on its own.

use zx_rustrum::recorder::{self, Recorder, QUIET_T};
use zx_rustrum::tape::Block;

/// The edges a ROM save of these bytes would make, starting at `from`.
fn edges_for(bytes: &[u8], from: u64, pilot: usize) -> Vec<u64> {
    let mut at = from;
    let mut out = vec![at];
    let mut pulse = |len: u64, out: &mut Vec<u64>| {
        at += len;
        out.push(at);
    };
    for _ in 0..pilot {
        pulse(2168, &mut out);
    }
    pulse(667, &mut out);
    pulse(735, &mut out);
    for byte in bytes {
        for bit in (0..8).rev() {
            let len = if byte & (1 << bit) != 0 { 1710 } else { 855 };
            pulse(len, &mut out);
            pulse(len, &mut out);
        }
    }
    out
}

/// A block saved at the ROM's timings reads back as the same bytes: the flag,
/// the data and the checksum, exactly as they went out.
#[test]
fn a_rom_save_reads_back_as_the_bytes_it_sent() {
    let bytes = [0xFF, 0x12, 0x34, 0xED, 0x00, 0x80, 0x7F];
    let mut rec = Recorder::new();
    let edges = edges_for(&bytes, 1_000, 3223);
    for at in &edges {
        rec.edge(*at);
    }
    let last = *edges.last().unwrap();

    assert!(
        rec.finish_if_quiet(last + QUIET_T / 2).is_none(),
        "not while the block might still be going"
    );
    let block = rec.finish_if_quiet(last + QUIET_T).expect("a block");
    match block {
        Block::Standard { data, .. } => assert_eq!(data, bytes, "byte for byte"),
        other => panic!("a standard block, not {other:?}"),
    }
    assert!(!rec.pending(), "and nothing is left over");
}

/// A run of pulses with no pilot in front of it is not a standard block, and
/// is counted rather than turned into a wrong one.
#[test]
fn pulses_that_are_not_a_rom_save_are_not_read() {
    let mut rec = Recorder::new();
    // A beeper tune through MIC: pulses of every length, no pilot.
    let mut at = 0;
    for i in 0..500u64 {
        at += 300 + (i * 37) % 900;
        rec.edge(at);
    }
    assert!(rec.finish_if_quiet(at + QUIET_T).is_none());
    assert_eq!(rec.unread, 1, "it is counted as something not read");
}

/// A reset puts the machine's clock back to zero. A half-heard block is
/// dropped rather than having a pulse measured across the jump — which in
/// unsigned arithmetic is a pulse billions of T-states long, or a panic.
#[test]
fn a_clock_that_goes_backwards_drops_the_half_heard_block() {
    let mut rec = Recorder::new();
    for at in edges_for(&[0xFF, 0x01], 10_000_000, 500).iter().take(300) {
        rec.edge(*at);
    }
    // Reset: the clock starts again from nothing.
    let bytes = [0x00, 0x42];
    let edges = edges_for(&bytes, 100, 3223);
    for at in &edges {
        rec.edge(*at);
    }
    let block = rec
        .finish_if_quiet(*edges.last().unwrap() + QUIET_T)
        .expect("the block after the reset");
    match block {
        Block::Standard { data, .. } => assert_eq!(data, bytes),
        other => panic!("{other:?}"),
    }
}

/// Two blocks saved one after the other come out as two, with the silence
/// between them kept as the second one's pause.
#[test]
fn a_header_and_its_data_come_out_as_two_blocks() {
    let mut rec = Recorder::new();
    let header = edges_for(&[0x00, 0x03], 0, 8063);
    for at in &header {
        rec.edge(*at);
    }
    let header_end = *header.last().unwrap();
    let first = rec
        .finish_if_quiet(header_end + QUIET_T)
        .expect("the header");

    // The ROM waits about a second before the data.
    let data = edges_for(&[0xFF, 0xAA, 0x55], header_end + 3_500_000, 3223);
    for at in &data {
        rec.edge(*at);
    }
    let second = rec
        .finish_if_quiet(*data.last().unwrap() + QUIET_T)
        .expect("the data");
    assert!(matches!(first, Block::Standard { .. }));
    match second {
        Block::Standard { pause_ms, data } => {
            assert_eq!(data, [0xFF, 0xAA, 0x55]);
            assert!(
                (900..=1100).contains(&pause_ms),
                "the second's pause is the second's silence: {pause_ms}ms"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// A `.tap` is each block's length and then the block; a `.tzx` is the
/// signature, the version, and a $10 block for each, pause and all.
#[test]
fn a_recorded_tape_is_written_as_tap_and_tzx() {
    let blocks = vec![
        Block::Standard {
            pause_ms: 1000,
            data: vec![0x00, 0x01, 0x02],
        },
        Block::Standard {
            pause_ms: 500,
            data: vec![0xFF, 0x09],
        },
    ];
    let tap = recorder::to_tap(&blocks).expect("a tap");
    assert_eq!(tap, [3, 0, 0x00, 0x01, 0x02, 2, 0, 0xFF, 0x09]);

    let tzx = recorder::to_tzx(&blocks).expect("a tzx");
    assert_eq!(&tzx[..8], b"ZXTape!\x1A");
    assert_eq!((tzx[8], tzx[9]), (1, 20), "version 1.20");
    assert_eq!(tzx[10], 0x10, "a standard block");
    assert_eq!(u16::from_le_bytes([tzx[11], tzx[12]]), 1000, "its pause");

    // And both read back as the same blocks, through the emulator's own tape
    // reader — which is the reader they have to satisfy.
    for (name, bytes) in [("x.tap", tap), ("x.tzx", tzx)] {
        let tape = zx_rustrum::tape::Tape::from_bytes(name, &bytes).expect(name);
        let datas: Vec<Vec<u8>> = tape
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Standard { data, .. } => Some(data.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            datas,
            vec![vec![0x00, 0x01, 0x02], vec![0xFF, 0x09]],
            "{name}"
        );
    }

    // A tape with anything else on it cannot be a .tap.
    let odd = vec![Block::PureTone {
        len: 2168,
        count: 10,
    }];
    assert!(recorder::to_tap(&odd).is_err());
}
