//! Test a claim about a routine by taking it out and seeing what stops.
//!
//! Somebody — a person or a model — says "$8DAA draws the guardians". That is
//! a falsifiable prediction, and a recording makes it testable: play the same
//! frames twice, once with the routine patched to return immediately, and see
//! what changed on screen. If the guardians vanish, the claim holds. If
//! nothing changes, it does not, whatever the code looks like.
//!
//! ```text
//! verify recordings/manic.rzx 8DAA --frames 600
//! ```
//!
//! The honest caveat is printed with the answer: patching the program changes
//! the path it takes, so the recorded input eventually stops matching what it
//! asks for. A short run stays in step; a long one will not, and the number of
//! reads the recording could not answer is reported so the result can be
//! weighed rather than taken.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

/// The display and attribute files.
const SCREEN: std::ops::Range<u32> = 0x4000..0x5B00;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut frames = 120u32;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--frames" => frames = iter.next().and_then(|v| v.parse().ok()).unwrap_or(frames),
            other => positional.push(other.to_string()),
        }
    }
    let [recording, address] = positional.as_slice() else {
        eprintln!("usage: verify <recording.rzx> <address> [--frames N]");
        std::process::exit(2);
    };
    let Ok(entry) = u16::from_str_radix(address.trim_start_matches('$'), 16) else {
        eprintln!("{address} is not an address");
        std::process::exit(2);
    };

    let with = run(recording, frames, None);
    let without = run(recording, frames, Some(entry));
    let (Some(with), Some(without)) = (with, without) else {
        eprintln!("{recording}: could not be played");
        std::process::exit(1);
    };

    let differ = with
        .screen
        .iter()
        .zip(&without.screen)
        .filter(|(a, b)| a != b)
        .count();
    let cells = changed_cells(&with.screen, &without.screen);

    println!("Taking ${entry:04X} out of {recording} for {frames} frames:");
    println!("  (the routine is patched to RET; everything else is left alone)");
    println!(
        "  {differ} of {} bytes of the screen differ ({:.1}%), across {} character cells",
        with.screen.len(),
        differ as f64 * 100.0 / with.screen.len() as f64,
        cells.len()
    );

    if differ == 0 {
        println!("  Nothing on screen depended on it — whatever it does, it is not drawing.");
    } else if cells.len() < 40 {
        // A handful of cells is something particular: a sprite, a counter, a
        // panel. Where they are says which.
        let (top, bottom) = (
            cells.iter().map(|(_, row)| *row).min().unwrap_or(0),
            cells.iter().map(|(_, row)| *row).max().unwrap_or(0),
        );
        println!("  The cells it accounts for lie between rows {top} and {bottom}.");
        println!("  Something particular is missing rather than the whole picture.");
    } else {
        println!("  Most of the picture depended on it.");
    }

    for (what, run) in [("with it", &with), ("without it", &without)] {
        println!(
            "  {what}: {} frames played, {} reads the recording could not answer",
            run.frames, run.adrift
        );
    }
    if without.adrift > with.adrift {
        println!(
            "  Patching it took the program off the recorded path after a while, \
             so the later frames are the emulator's own doing. Shorten the run \
             to be sure of the result."
        );
    }
}

struct Played {
    screen: Vec<u8>,
    frames: usize,
    adrift: u32,
}

/// Play the recording, optionally with one routine returning immediately.
fn run(recording: &str, frames: u32, patch: Option<u16>) -> Option<Played> {
    let roms = Roms {
        rom48: std::fs::read("roms/48.rom").ok(),
        rom128: std::fs::read("roms/128.rom").ok(),
        rom_plus3: std::fs::read("roms/plus3.rom").ok(),
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.load_path(std::path::Path::new(recording));
    app.rzx.as_ref()?;

    // $C9 is RET: the routine is entered and comes straight back, so
    // everything else about the program is left as it was.
    if let Some(entry) = patch {
        app.spec.bus.poke(entry, 0xC9);
    }
    // One frame at a time, deliberately. At maximum speed a call to advance
    // runs up to twenty-four frames, and the patched program wanders off the
    // recorded path as it goes: a long run measures the wandering rather than
    // the routine.
    for _ in 0..frames {
        app.advance(1.0 / 50.0);
        if app.rzx.is_none() {
            break;
        }
    }

    Some(Played {
        screen: SCREEN.map(|a| app.peek(a as u16)).collect(),
        frames: app.rzx.as_ref().map_or(0, |rzx| rzx.frame),
        adrift: app.spec.bus.playback.as_ref().map_or(0, |p| p.short),
    })
}

/// Which character cells differ, as (column, row).
fn changed_cells(with: &[u8], without: &[u8]) -> Vec<(usize, usize)> {
    let mut cells = std::collections::BTreeSet::new();
    for offset in 0..0x1800usize {
        if with[offset] == without[offset] {
            continue;
        }
        // Undo the display file's layout: thirds, then pixel row, then line.
        let third = offset >> 11;
        let line = (offset >> 8) & 7;
        let row_in_third = (offset >> 5) & 7;
        cells.insert((offset & 31, third * 8 + row_in_third));
        let _ = line;
    }
    // The attributes, which are laid out sensibly.
    for offset in 0x1800..0x1B00usize {
        if with[offset] != without[offset] {
            let cell = offset - 0x1800;
            cells.insert((cell % 32, cell / 32));
        }
    }
    cells.into_iter().collect()
}
