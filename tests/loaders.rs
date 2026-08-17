//! Loading games whose loaders are their own.
//!
//! Six of them, which between them cover most of what a commercial tape did:
//! Speedlock (Head over Heels, Daley Thompson's Decathlon), Alkatraz (Cobra,
//! 720 Degrees), Bleepload (Bubble Bobble, Starglider, Starglider 2),
//! Microsphere (Skool Daze, Contact Sam Cruise), Paul Owens (Chase H.Q.), and
//! the ROM's own for the blocks in front of them.
//!
//! None of their blocks can be handed over the way the ROM's can: they read
//! the tape themselves, and some of them decrypt every byte as it arrives.
//! What Ludicrous speed does for them is let the machine run — the tape still
//! plays, but the emulator is not held to twenty-four frames of work a host
//! frame while it does.

use std::time::Instant;
use zx_rustrum::flashload;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::tape::Tape;
use zx_rustrum::ui::{App, Roms};

const HEAD_OVER_HEELS: &str = "tapes/Head over Heels (1987)(Ocean)[48-128K].tzx";
const DALEY: &str = "tapes/Daley Thompson's Decathlon - Day 1 (1984)(Ocean Software).zip";
const COBRA: &str = "tapes/Cobra (1986)(Ocean Software).zip";
const BUBBLE_BOBBLE: &str = "tapes/Bubble Bobble (1987)(Firebird Software)[48-128K].zip";
const STARGLIDER: &str = "tapes/Starglider (1986)(Rainbird Software).zip";
const STARGLIDER_2: &str =
    "tapes/Starglider 2 - The Egrons Strike Back (1989)(Rainbird Software)[48-128K].zip";
const SKOOL_DAZE: &str = "tapes/Skool Daze (1985)(Microsphere).zip";
const SAM_CRUISE: &str = "tapes/Contact Sam Cruise (1986)(Microsphere).zip";
const CHASE_HQ: &str = "tapes/Chase H.Q. (1989)(Ocean Software)[48-128K].zip";
const SEVEN_TWENTY: &str = "tapes/720 Degrees (1986)(U.S. Gold).zip";

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
/// Head over Heels, because it stays in its loader until the tape runs out:
/// the comparison is then of two machines at the same point in the load. Daley
/// Thompson's is already running its game by the time its tape ends, so its
/// own variables have moved on by different amounts and there is nothing exact
/// to compare — what that one has to show is further down, that it runs.
///
/// Not every byte even so: the stack below SP holds whatever the loader last
/// pushed, and the system variables hold a frame counter, both of which depend
/// on how long the load took rather than on what was loaded.
#[test]
fn a_speedlock_tape_loads_to_the_same_thing_in_a_hurry() {
    let name = HEAD_OVER_HEELS;
    let (Some((slow, _)), Some((fast, _))) = (load(name, false), load(name, true)) else {
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
    assert_eq!(
        differing(0x6000, 0xFF00),
        0,
        "{name}: the game loaded differently when it was loaded quickly"
    );
}

/// And the game starts.
///
/// Head over Heels loads its parts and then listens to the tape again: it
/// samples the EAR line 255 times and builds a table at $9000 from what it
/// hears, and a line that reads a dead zero gives it a table of zeros. It then
/// wipes memory a byte at a time, which is what a black screen and a machine
/// that never comes back looks like. The line is not dead on a real machine —
/// the tape is still rolling long after its last block, and the loudspeaker
/// feeds back into it besides.
#[test]
fn head_over_heels_starts_after_loading() {
    starts_after_loading(HEAD_OVER_HEELS);
}

/// And so does the other one, whose loader starts the game while the tape is
/// still running.
#[test]
fn daley_thompson_starts_after_loading() {
    starts_after_loading(DALEY);
}

fn starts_after_loading(name: &str) {
    let Ok(rom) = std::fs::read("roms/48.rom") else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    if tape(name).is_none() {
        eprintln!("need {name}; skipping");
        return;
    }
    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.reset();
    spec.bus.tape_flash = true;
    start_loading(&mut spec, tape(name).unwrap());
    while spec.bus.tape_playing() {
        spec.run(20_000);
    }
    for _ in 0..600 {
        spec.run(FRAME_T);
    }
    // What says it loaded is the picture. A machine that has fallen back to
    // BASIC has the report line and nothing else; a game that has loaded has
    // its title or its menu. Where the machine is executing says less than it
    // looks: Bubble Bobble waits at its menu inside the ROM's keyboard scan,
    // which is where BASIC waits too.
    assert!(
        !(0xFC00..=0xFFFF).contains(&spec.cpu.pc),
        "{name} should not still be in a loader at ${:04X}",
        spec.cpu.pc
    );
    let drawn = (0x4000..0x5800u16)
        .filter(|a| spec.bus.mem(*a) != 0)
        .count();
    assert!(
        drawn > 500,
        "and there should be a picture, not {drawn} bytes"
    );
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

/// Alkatraz — Cobra and 720 Degrees — loads too.
///
/// Its loader reads all eight bits of a block's last byte and then waits for
/// the edge that closes the last pulse. A block whose last pulse leaves the
/// line low used to end with no edge at all, because the silence behind it is
/// low as well; the loader waited for ever, and its protection took that for a
/// snapped tape and wiped itself.
#[test]
fn cobra_loads() {
    starts_after_loading(COBRA);
}

#[test]
fn seven_twenty_degrees_loads() {
    starts_after_loading(SEVEN_TWENTY);
}

/// The silence at the end of a tape is got through too.
///
/// Max speed comes back to normal for the pause a tape ends on, so that a
/// loader finishing sounds and looks as it should. Ludicrous speed is a
/// promise to get it over with, and Out Run Europa ends with twenty-two
/// seconds of it — which used to be twenty-two seconds of watching a stopped
/// picture, a thousand host frames of doing nothing at all.
#[test]
fn the_last_pause_is_hurried_through_as_well() {
    use zx_rustrum::machine::Model;
    use zx_rustrum::tape::Block;

    let Ok(rom) = std::fs::read("roms/48.rom") else {
        return;
    };
    // A short block and then ten seconds of silence to finish on.
    let tape = Tape::from_blocks(
        "t".into(),
        vec![Block::Standard {
            pause_ms: 10_000,
            data: vec![0xFF, 1, 2, 3],
        }],
    );

    let run = |ludicrous: bool| -> u32 {
        let roms = Roms {
            rom48: Some(rom.clone()),
            ..Roms::default()
        };
        let mut app = App::with_roms(
            Spectrum::with_model(Model::Spectrum48),
            String::new(),
            roms,
            None,
        );
        app.show_ram_map = false;
        app.show_debugger = false;
        app.show_back_buffer = false;
        app.show_tape = false;
        app.spec.load_rom(&rom);
        app.spec.reset();
        app.running = true;
        app.spec.bus.tape_boost = true;
        app.spec.bus.tape_flash = ludicrous;
        for _ in 0..60 {
            app.advance(1.0 / 60.0);
        }
        app.spec.bus.tape = Some(tape.clone());
        let now = app.spec.bus.total_t();
        app.spec.bus.tape.as_mut().unwrap().play(now);
        let mut host = 0;
        while host < 60 * 60 {
            app.advance(1.0 / 60.0);
            host += 1;
            if !app.spec.bus.tape_playing() {
                break;
            }
        }
        host
    };

    let played = run(false);
    let hurried = run(true);
    assert!(
        hurried * 10 < played,
        "the silence should be got through in a hurry: {hurried} host frames \
         against {played}"
    );
}

/// Bleepload — Firebird's and Rainbird's — is a couple of hundred blocks of
/// about 270 bytes each with a few milliseconds between them, and the two
/// sync pulses swap round from one tape to the next: Bubble Bobble's are
/// 735 then 667, Starglider's the other way about.
#[test]
fn bubble_bobble_loads() {
    starts_after_loading(BUBBLE_BOBBLE);
}

#[test]
fn starglider_loads() {
    starts_after_loading(STARGLIDER);
}

/// Starglider 2 wraps its blocks in groups and puts a four-millisecond pause
/// and a long tone in front of them.
#[test]
fn starglider_2_loads() {
    starts_after_loading(STARGLIDER_2);
}

/// Microsphere's is a whole game in one block. Skool Daze is 82,109 bytes of
/// turbo block at twice the ROM's rate — 422 and 843 T-states a bit — behind
/// nothing but a header and a BASIC line.
#[test]
fn skool_daze_loads() {
    starts_after_loading(SKOOL_DAZE);
}

/// Contact Sam Cruise does the same thing the other way about: a small turbo
/// block carrying the loader, and then 49,465 bytes as an ordinary standard
/// block, read by the game rather than by the ROM.
#[test]
fn contact_sam_cruise_loads() {
    starts_after_loading(SAM_CRUISE);
}

/// Paul Owens's, which Chase H.Q. uses: every block at the same settings —
/// 2,196 pilot pulses, 667 and 735 for the sync, 735 and 1,590 a bit — and
/// the levels sitting behind the game as four-byte blocks with their data,
/// each announced by a text block in the tape itself.
#[test]
fn chase_hq_loads() {
    starts_after_loading(CHASE_HQ);
}

/// A head out of square takes the quick loaders first.
///
/// Skool Daze writes its bits in 422 and 843 T-states, which is 4.1 kHz at the
/// short end; Chase H.Q. uses 735 and 1,590, which is 2.4 kHz. Bring the
/// filter's corner down to about 1.2 kHz and the first stops loading while the
/// second does not notice — which is what a misaligned deck did to a shelf of
/// tapes, and why the fast loaders were the ones people had trouble with.
#[test]
fn a_misaligned_head_stops_the_quick_loader_and_not_the_slow_one() {
    use zx_rustrum::tape::Quality;

    let out_of_square = Quality {
        alignment: true,
        alignment_offset: 0.7,
        ..Quality::default()
    };
    let corner = out_of_square.cutoff(0);
    assert!(
        (1000.0..1500.0).contains(&corner),
        "the corner should sit between the two loaders, not {corner:.0} Hz"
    );

    let Some(quick) = loaded_screen(SKOOL_DAZE, out_of_square) else {
        eprintln!("need roms/48.rom and the tapes; skipping");
        return;
    };
    let Some(slow) = loaded_screen(CHASE_HQ, out_of_square) else {
        return;
    };
    assert!(
        quick < 500,
        "Skool Daze should not load through a head that far out: {quick} \
         bytes of screen"
    );
    assert!(
        slow > 500,
        "and Chase H.Q. should: only {slow} bytes of screen"
    );
}

/// Load a tape with the deck behaving however it is told to, and hand back how
/// much of the screen ended up drawn.
fn loaded_screen(name: &str, quality: zx_rustrum::tape::Quality) -> Option<usize> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let mut spec = Spectrum::new();
    spec.load_rom(&rom);
    spec.reset();
    spec.bus.tape_flash = true;
    let mut tape = tape(name)?;
    tape.quality = quality;
    start_loading(&mut spec, tape);
    let mut frames = 0;
    while frames < 40_000 {
        spec.run(FRAME_T);
        frames += 1;
        if !spec.bus.tape_playing() {
            break;
        }
    }
    for _ in 0..300 {
        spec.run(FRAME_T);
    }
    Some(
        (0x4000..0x5800u16)
            .filter(|a| spec.bus.mem(*a) != 0)
            .count(),
    )
}
