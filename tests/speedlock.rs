//! Loading a game whose loader is its own, as quickly as that can be done.
//!
//! Speedlock reads the tape itself and decrypts every byte as it goes, so its
//! blocks cannot be handed over the way the ROM's can. What Ludicrous speed
//! does for it is let the machine run: the tape still plays, but the emulator
//! is not held to twenty-four frames of work a host frame while it does.

use std::time::Instant;
use zx_rustrum::flashload;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::tape::Tape;
use zx_rustrum::ui::{App, Roms};

const HEAD_OVER_HEELS: &str = "tapes/Head over Heels (1987)(Ocean)[48-128K].tzx";
const DALEY: &str = "tapes/Daley Thompson's Decathlon - Day 1 (1984)(Ocean Software).zip";

fn tape(name: &str) -> Option<Tape> {
    let bytes = std::fs::read(name).ok()?;
    let (inner, bytes) = if name.ends_with(".zip") {
        zx_rustrum::zip::first_with_extension(&bytes, &["tzx", "tap"])?
    } else {
        (name.to_string(), bytes)
    };
    Tape::from_bytes(&inner, &bytes).ok()
}

/// Type `LOAD ""` and start the tape.
fn start_loading(spec: &mut Spectrum, tape: Tape) {
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
}

/// Load the tape and hand back the machine's memory when the tape stops.
fn load(name: &str, flash: bool) -> Option<(Vec<u8>, u16)> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.reset();
    spec.bus.tape_flash = flash;
    start_loading(&mut spec, tape(name)?);
    // Short steps, so both ways stop at the same moment rather than part way
    // through a slice of work.
    for _ in 0..20_000_000u64 {
        spec.run(500);
        if !spec.bus.tape_playing() {
            break;
        }
    }
    Some((
        (0x4000..=0xFFFFu32)
            .map(|a| spec.bus.mem(a as u16))
            .collect(),
        spec.cpu.pc,
    ))
}

/// The game that comes off the tape is the same game either way.
///
/// Not every byte of memory: the loading screen's colours are cycled while the
/// tape runs, and the stack below SP holds whatever the loader last pushed, so
/// both depend on exactly when the tape ran out. The loaded program does not.
#[test]
fn a_speedlock_tape_loads_to_the_same_thing_in_a_hurry() {
    for name in [HEAD_OVER_HEELS, DALEY] {
        let (Some((slow, slow_pc)), Some((fast, fast_pc))) = (load(name, false), load(name, true))
        else {
            eprintln!("need roms/48.rom and {name}; skipping");
            return;
        };

        let differing = |from: u32, to: u32| -> usize {
            (from..to)
                .filter(|a| slow[(*a - 0x4000) as usize] != fast[(*a - 0x4000) as usize])
                .count()
        };
        assert_eq!(
            differing(0x4000, 0x5800),
            0,
            "{name}: the loading screen's pixels differ"
        );
        // Everything above the screen but the stack, which is at the top of
        // memory while a loader is running.
        assert_eq!(
            differing(0x5B00, 0xFF00),
            0,
            "{name}: the game loaded differently when it was loaded quickly"
        );
        assert_eq!(
            slow_pc >> 8,
            fast_pc >> 8,
            "{name}: it ended up somewhere else entirely: ${slow_pc:04X} against ${fast_pc:04X}"
        );
    }
}

/// And it is quicker — in the only way that counts, which is how long the
/// person watching has to wait.
#[test]
fn a_speedlock_tape_takes_a_fraction_of_the_host_frames() {
    let Some(rom) = std::fs::read("roms/48.rom").ok() else {
        return;
    };
    let Some(tape) = tape(DALEY) else {
        eprintln!("need {DALEY}; skipping");
        return;
    };

    let run = |ludicrous: bool| -> (u32, std::time::Duration) {
        let roms = Roms {
            rom48: Some(rom.clone()),
            ..Roms::default()
        };
        let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
        app.show_ram_map = false;
        app.show_debugger = false;
        app.show_back_buffer = false;
        app.show_tape = false;
        app.spec.load_rom(&rom);
        app.spec.reset();
        app.running = true;
        app.spec.bus.tape_boost = true;
        app.spec.bus.tape_flash = ludicrous;
        start_loading(&mut app.spec, tape.clone());
        let mut host = 0u32;
        let mut worst = std::time::Duration::ZERO;
        while host < 60 * 60 {
            let at = Instant::now();
            app.advance(1.0 / 60.0);
            worst = worst.max(at.elapsed());
            host += 1;
            if !app.spec.bus.tape_playing() {
                break;
            }
        }
        (host, worst)
    };

    let (played, _) = run(false);
    let (hurried, worst) = run(true);
    assert!(
        hurried * 8 < played,
        "loading should take far fewer host frames in a hurry: {hurried} against {played}"
    );
    assert!(
        worst < std::time::Duration::from_millis(250),
        "and the window should still get a look in: the worst frame took {worst:?}"
    );
}

/// The sampling loop nearly every loader is built round, which is how the
/// emulator knows a game is reading the tape for itself.
#[test]
fn the_loader_core_is_recognised() {
    let mut spec = Spectrum::new();
    let core = [
        0x04u8, 0xC8, 0x3E, 0x7F, 0xDB, 0xFE, 0x1F, 0xA9, 0xE6, 0x20, 0x28, 0xF4,
    ];
    for (offset, byte) in core.iter().enumerate() {
        spec.bus.poke(0xFD30 + offset as u16, *byte);
    }

    spec.cpu.pc = 0xFD30;
    assert!(
        flashload::at_sampler(&spec),
        "the loop from the reference should be recognised where it stands"
    );

    spec.cpu.pc = 0xFD31;
    assert!(
        !flashload::at_sampler(&spec),
        "and only at its head, not part way through it"
    );

    spec.bus.poke(0xFD34, 0x00);
    spec.cpu.pc = 0xFD30;
    assert!(
        !flashload::at_sampler(&spec),
        "something that only looks like it is not it"
    );
}
