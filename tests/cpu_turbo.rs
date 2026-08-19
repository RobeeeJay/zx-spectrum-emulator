//! A faster CPU against the same ULA.
//!
//! The frame stays 69,888 T-states and the interrupt stays where it was; what
//! changes is how much the CPU gets through inside one. See
//! `docs/cpu-turbo.md`.

use zx_rustrum::machine::{Spectrum, CPU_HZ, FRAME_T};

/// A machine running `NOP`s out of uncontended RAM, so what is being measured
/// is the CPU's own cost and nothing else.
fn nops(turbo: u32) -> Spectrum {
    let mut spec = Spectrum::new();
    for addr in 0x8000..0x9000u16 {
        spec.bus.poke(addr, 0x00);
    }
    // A jump back to the top, so it runs for ever without leaving the page.
    spec.bus.poke(0x8FFD, 0xC3);
    spec.bus.poke(0x8FFE, 0x00);
    spec.bus.poke(0x8FFF, 0x80);
    spec.cpu.pc = 0x8000;
    spec.bus.turbo = turbo;
    spec
}

/// The ULA is not touched: a frame is what it always was, and the interrupt
/// comes when it always did.
#[test]
fn the_frame_is_the_same_length_at_every_speed() {
    for turbo in [1u32, 2, 4, 8] {
        let mut spec = nops(turbo);
        let frames_before = spec.bus.frame;
        // Ten frames of the ULA's own time.
        for _ in 0..10 {
            spec.run(FRAME_T);
        }
        assert_eq!(
            spec.bus.frame - frames_before,
            10,
            "at {turbo}x, ten frames of T-states should still be ten frames"
        );
        assert_eq!(
            spec.bus.frame_t(),
            FRAME_T,
            "and a frame is still 69,888 T-states"
        );
    }
    // Which is 50.08 pictures a second whatever the CPU is doing.
    let fps = CPU_HZ / FRAME_T as f64;
    assert!((fps - 50.08).abs() < 0.01, "{fps}");
}

/// What the switch buys: more instructions inside the same frame.
#[test]
fn a_faster_cpu_gets_more_done_in_a_frame() {
    let ran = |turbo: u32| -> u32 {
        let mut spec = nops(turbo);
        let before = spec.bus.fetches;
        spec.run(FRAME_T);
        spec.bus.fetches - before
    };
    let plain = ran(1);
    for turbo in [2u32, 4, 8] {
        let faster = ran(turbo);
        let ratio = faster as f64 / plain as f64;
        assert!(
            (ratio - turbo as f64).abs() < 0.02,
            "{turbo}x should run {turbo} times as many instructions: {ratio:.3} \
             ({faster} against {plain})"
        );
    }
}

/// Nothing is lost to rounding. A cycle costs a fraction of a T-state and the
/// remainder is carried, so a long run comes out exact rather than a little
/// short — which would be a machine running slower than it says it does.
#[test]
fn the_cycles_that_do_not_divide_are_carried() {
    for turbo in [2u32, 4, 8] {
        let mut plain = nops(1);
        let mut fast = nops(turbo);
        // Every NOP is four T-states, and four divides by all of these; a
        // longer instruction is what makes the remainder matter, so the page
        // is filled with `LD A,(HL)` — seven cycles — instead.
        for spec in [&mut plain, &mut fast] {
            for addr in 0x8000..0x8FFD_u16 {
                spec.bus.poke(addr, 0x7E);
            }
            spec.cpu.h = 0x90;
            spec.cpu.l = 0x00;
        }
        let million = 1_000_000;
        let cost = |spec: &mut Spectrum| -> u64 {
            let before = spec.bus.total_t();
            let start = spec.bus.fetches;
            while spec.bus.fetches - start < million {
                spec.step_instruction();
            }
            spec.bus.total_t() - before
        };
        let slow = cost(&mut plain);
        let quick = cost(&mut fast);
        assert_eq!(
            quick,
            slow / turbo as u64,
            "a million instructions at {turbo}x should cost exactly a \
             {turbo}th of the time: {quick} against {slow}"
        );
    }
}

/// Above 1× the ULA does not hold the CPU off the bus at all, which is the
/// decided policy: an accelerated machine is not sharing the bus on the ULA's
/// terms any more.
#[test]
fn there_is_no_contention_above_one_times() {
    // `LD A,(HL)` reading contended RAM, at the worst T-state of the frame.
    let cost_of = |turbo: u32, from: u16| -> u32 {
        let mut spec = Spectrum::new();
        spec.bus.turbo = turbo;
        spec.bus.poke(0x8000, 0x7E);
        spec.cpu.pc = 0x8000;
        spec.cpu.h = (from >> 8) as u8;
        spec.cpu.l = from as u8;
        // Where the delay table says six T-states.
        spec.bus.tstates = 14335;
        let before = spec.bus.tstates;
        spec.step_instruction();
        spec.bus.tstates - before
    };

    // The machine as built: reading the screen's own RAM costs more.
    let contended = cost_of(1, 0x4000);
    let free = cost_of(1, 0x8000);
    assert!(
        contended > free,
        "at 1x a contended read should still be stalled: {contended} against \
         {free}"
    );

    // Accelerated: the same instruction wherever it reads from.
    for turbo in [2u32, 4, 8] {
        assert_eq!(
            cost_of(turbo, 0x4000),
            cost_of(turbo, 0x8000),
            "at {turbo}x there is no contention to tell them apart"
        );
        assert_eq!(
            cost_of(turbo, 0x8000),
            free / turbo,
            "and an uncontended read costs a {turbo}th of what it did"
        );
    }
}

/// The I/O patterns are the reference's at 1× and a plain four T-states above
/// it, since the stalls are what the four patterns are.
#[test]
fn the_io_patterns_go_with_the_contention() {
    let cost_of = |turbo: u32, port: u16| -> u32 {
        let mut spec = Spectrum::new();
        spec.bus.turbo = turbo;
        // IN A,(n) — the port's high byte comes from A.
        spec.bus.poke(0x8000, 0xDB);
        spec.bus.poke(0x8001, (port & 0xFF) as u8);
        spec.cpu.a = (port >> 8) as u8;
        spec.cpu.pc = 0x8000;
        spec.bus.tstates = 14335;
        let before = spec.bus.tstates;
        spec.step_instruction();
        spec.bus.tstates - before
    };

    // $7FFE is the ULA's own port with its high byte in the contended range —
    // the one every tape loader reads, and the most stalled of the four
    // patterns. $FFFF is neither, and is the four T-states the Z80 asks for
    // and nothing else.
    let stalled = cost_of(1, 0x7FFE);
    let plain = cost_of(1, 0xFFFF);
    assert!(
        stalled > plain,
        "at 1x the ULA stalls the read it is asked for: {stalled} against \
         {plain}"
    );
    for turbo in [2u32, 4, 8] {
        assert_eq!(
            cost_of(turbo, 0x7FFE),
            cost_of(turbo, 0xFFFF),
            "above 1x there are no stalls to tell the patterns apart"
        );
        assert_eq!(
            cost_of(turbo, 0xFFFF),
            plain / turbo,
            "and what is left is the instruction, a {turbo}th of the cost"
        );
    }
}

/// The machine still keeps its own place: an accelerated CPU reaches the
/// interrupt at the same moment in the frame, and gets through more before it.
#[test]
fn the_interrupt_still_comes_with_the_frame() {
    for turbo in [1u32, 4] {
        let mut spec = nops(turbo);
        spec.cpu.iff1 = true;
        spec.cpu.iff2 = true;
        // Run to just before the frame ends, then over the boundary.
        spec.run(FRAME_T - 1000);
        let frame = spec.bus.frame;
        spec.run(2000);
        assert_eq!(
            spec.bus.frame,
            frame + 1,
            "at {turbo}x the frame should have turned over once"
        );
    }
}

/// A tape does not load with the CPU accelerated, and that is the machine
/// rather than a fault.
///
/// Every loader — the ROM's own as much as a game's — counts turns of its own
/// loop against the pulses coming off the tape. The pulses are in ULA time and
/// the ULA has not moved, so at 4× a loader counts four times as many turns
/// for the same pulse and every length it knows is wrong. That is what
/// happens on an accelerated machine, which is why they had a switch, and it
/// is why the emulator holds the CPU at 1× while a tape is playing.
#[test]
fn a_loader_cannot_count_pulses_with_the_cpu_accelerated() {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    let path = "tapes/Head over Heels (1987)(Ocean)[48-128K].tzx";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("need {path}; skipping");
        return;
    };
    let Ok(tape) = zx_rustrum::tape::Tape::from_bytes(path, &bytes) else {
        return;
    };

    let loaded = |turbo: u32| -> (usize, u16) {
        let mut spec = Spectrum::new();
        spec.load_rom(&rom);
        spec.reset();
        spec.bus.turbo = turbo;
        spec.bus.tape_flash = true;
        // `LOAD ""` typed at the machine.
        for _ in 0..120 {
            spec.run(FRAME_T);
        }
        spec.bus.tape = Some(tape.clone());
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
        while spec.bus.tape_playing() {
            spec.run(20_000);
        }
        for _ in 0..600 {
            spec.run(FRAME_T);
        }
        let drawn = (0x4000..0x5800u16)
            .filter(|a| spec.bus.mem(*a) != 0)
            .count();
        (drawn, spec.cpu.pc)
    };

    // The machine as built loads it.
    let (drawn, pc) = loaded(1);
    assert!(
        drawn > 500,
        "at 1x the tape should load: {drawn} bytes of screen, pc ${pc:04X}"
    );

    // Accelerated, the loader is left counting: nothing reaches the screen and
    // the machine is still in the ROM's edge loop when the tape has run out.
    let (drawn, pc) = loaded(4);
    assert!(
        drawn < 100,
        "at 4x a loader cannot measure the pulses: {drawn} bytes of screen"
    );
    assert!(
        (0x0530..=0x0620).contains(&pc),
        "and is left waiting for an edge it cannot recognise, at ${pc:04X}"
    );
}
