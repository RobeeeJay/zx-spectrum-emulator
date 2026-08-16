//! Handing tape blocks straight to the ROM's loader.

use zx_rustrum::flashload::LD_BYTES;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::tape::{Block, Tape};

/// A standard block: the flag byte, the data, and the checksum that makes the
/// lot exclusive-or to zero.
fn block(flag: u8, data: &[u8]) -> Block {
    let mut bytes = vec![flag];
    bytes.extend_from_slice(data);
    let checksum = bytes.iter().fold(0u8, |acc, b| acc ^ b);
    bytes.push(checksum);
    Block::Standard {
        pause_ms: 1000,
        data: bytes,
    }
}

/// A machine with just enough ROM to be recognised as the one that owns
/// LD-BYTES: the six instructions the check reads, and a RET at the end of
/// them in case anything falls through.
fn machine(blocks: Vec<Block>) -> Spectrum {
    let mut rom = vec![0xC9u8; 0x4000];
    rom[LD_BYTES as usize..LD_BYTES as usize + 8]
        .copy_from_slice(&[0x14, 0x08, 0x15, 0xF3, 0x3E, 0x0F, 0xD3, 0xFE]);
    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.bus.tape = Some(Tape::from_blocks("test".into(), blocks));
    spec.bus.tape_flash = true;
    spec.bus.tape.as_mut().unwrap().play(0);
    spec
}

/// Call the loader as the ROM's own callers do: the flag it wants in A, the
/// length in DE, the address in IX, carry set for loading rather than
/// verifying, and a return address on the stack.
fn call_loader(spec: &mut Spectrum, flag: u8, at: u16, length: u16) {
    spec.cpu.sp = 0x7FFE;
    spec.bus.poke(0x7FFE, 0x34);
    spec.bus.poke(0x7FFF, 0x12); // returns to $1234
    spec.cpu.a = flag;
    spec.cpu.f |= 0x01;
    spec.cpu.ix = at;
    spec.cpu.d = (length >> 8) as u8;
    spec.cpu.e = length as u8;
    spec.cpu.pc = LD_BYTES;
    spec.step_instruction();
}

/// A tape loads at 1,500 baud however fast the machine is run: the pulses take
/// as long as they take. The only way to be quicker is not to play them —
/// hand the block to the routine that was going to read it, and return.
#[test]
fn a_block_is_handed_over_whole() {
    let mut spec = machine(vec![block(0xFF, &[1, 2, 3, 4, 5, 6, 7, 8])]);
    call_loader(&mut spec, 0xFF, 0x8000, 8);

    for (offset, want) in [1u8, 2, 3, 4, 5, 6, 7, 8].iter().enumerate() {
        assert_eq!(
            spec.bus.mem(0x8000 + offset as u16),
            *want,
            "byte {offset} of the block should be in memory"
        );
    }
    assert_eq!(spec.cpu.f & 0x01, 1, "carry set says it loaded");
    assert_eq!(spec.cpu.ix, 0x8008, "IX ends past the bytes loaded");
    assert_eq!(
        ((spec.cpu.d as u16) << 8) | spec.cpu.e as u16,
        0,
        "DE ends at nothing left to load"
    );
    assert_eq!(
        spec.cpu.pc, 0x1234,
        "and the routine returned to its caller"
    );
}

/// A program looking for its data steps over the headers in between, and so
/// does this: the block handed over is the next one with the flag asked for.
#[test]
fn a_block_with_the_wrong_flag_is_stepped_over() {
    let mut spec = machine(vec![
        block(0x00, &[9; 17]), // a header
        block(0xFF, &[7; 4]),  // the data that goes with it
        block(0x00, &[9; 17]), // and another header behind that
    ]);
    call_loader(&mut spec, 0xFF, 0x8000, 4);

    assert_eq!(spec.bus.mem(0x8000), 7, "the data block should have loaded");
    assert_eq!(spec.cpu.f & 0x01, 1, "and reported success");
    assert_eq!(
        spec.bus.tape.as_ref().unwrap().block,
        2,
        "the deck should be on the block after the one handed over"
    );
}

/// A block shorter than the program asked for is the tape loading error every
/// mistyped POKE ends in, and the routine has to report it as one.
#[test]
fn a_block_that_is_the_wrong_length_fails_as_the_rom_would() {
    let mut spec = machine(vec![block(0xFF, &[1, 2, 3])]);
    call_loader(&mut spec, 0xFF, 0x8000, 40);

    assert_eq!(
        spec.cpu.f & 0x01,
        0,
        "carry clear says the load failed, which is what a short block is"
    );
    assert_eq!(spec.cpu.pc, 0x1234, "and it still returns to its caller");
}

/// Only where the ROM that owns the address is the one paged in. A program is
/// free to put anything it likes at $0556, and a 128K is running a different
/// ROM there until a game pages the other one back.
#[test]
fn the_loader_is_only_answered_when_that_rom_is_there() {
    let mut spec = machine(vec![block(0xFF, &[1, 2, 3, 4])]);
    // A ROM with something else at the address.
    let rom = vec![0x00u8; 0x4000];
    spec.load_rom(&rom);
    call_loader(&mut spec, 0xFF, 0x8000, 4);

    assert_ne!(
        spec.bus.mem(0x8000),
        1,
        "nothing should have been loaded: that is not the loader"
    );
    assert_ne!(spec.cpu.pc, 0x1234, "and nothing returned early");
}

/// With the switch off the tape plays, whatever is under the head.
#[test]
fn nothing_is_handed_over_unless_it_is_switched_on() {
    let mut spec = machine(vec![block(0xFF, &[1, 2, 3, 4])]);
    spec.bus.tape_flash = false;
    call_loader(&mut spec, 0xFF, 0x8000, 4);

    assert_ne!(spec.bus.mem(0x8000), 1, "the block should not have loaded");
    assert_ne!(
        spec.cpu.pc, 0x1234,
        "the machine should be running the routine, not past it"
    );
}

/// End to end, against a real tape: the same machine state at the end, in two
/// frames instead of two and a half thousand.
#[test]
fn a_real_tape_loads_in_a_frame_or_two() {
    let (Ok(rom), Ok(tape)) = (
        std::fs::read("roms/48.rom"),
        Tape::load(std::path::Path::new("tapes/borderbreak.tap")),
    ) else {
        eprintln!("need roms/48.rom and tapes/borderbreak.tap; skipping");
        return;
    };

    let run = |flash: bool, tape: Tape| -> (u32, u16, usize) {
        let mut spec = Spectrum::new();
        spec.load_rom(&rom);
        spec.reset();
        spec.bus.tape_flash = flash;
        for _ in 0..120 {
            spec.run(FRAME_T);
        }
        spec.bus.tape = Some(tape);
        // LOAD ""
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
        let mut frames = 0;
        while frames < 60_000 {
            spec.run(FRAME_T);
            frames += 1;
            if !spec.bus.tape_playing() {
                break;
            }
        }
        // Let the loaded program get going.
        for _ in 0..300 {
            spec.run(FRAME_T);
        }
        let drawn = (0x4000..0x5800u16)
            .filter(|a| spec.bus.mem(*a) != 0)
            .count();
        (frames, spec.cpu.pc, drawn)
    };

    let (slow_frames, slow_pc, slow_drawn) = run(false, tape.clone());
    let (fast_frames, fast_pc, fast_drawn) = run(true, tape);

    assert!(
        fast_frames < 10,
        "the tape should be over in a frame or two, not {fast_frames}"
    );
    assert!(
        slow_frames > 1000,
        "and played it should take the best part of a minute, not {slow_frames} frames"
    );
    assert_eq!(
        (fast_pc, fast_drawn),
        (slow_pc, slow_drawn),
        "the machine should end up in the same place either way"
    );
}
