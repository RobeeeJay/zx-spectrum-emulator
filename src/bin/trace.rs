//! Play a recording and write down what each routine was seen to do.
//!
//! Static analysis can only say what code might do. A recording says what it
//! did: which routines ran, how often, what they wrote, and — because the bus
//! remembers who wrote each byte of the screen — what they actually drew. This
//! writes one JSON object per routine, which is the shape a model can be asked
//! about and a person can read.
//!
//! ```text
//! trace recordings/manic.rzx --frames 3000 > manic.jsonl
//! ```
//!
//! A recording that has come adrift from the machine is refused rather than
//! described: everything below depends on the machine having followed the same
//! path it followed when somebody played it, and once that stops being true
//! the attribution is fiction.

use zx_rustrum::autodoc;
use zx_rustrum::disasm;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut recording = None;
    let mut frames = 3000u32;
    let mut tree = false;
    let mut find_loops = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--frames" => frames = iter.next().and_then(|v| v.parse().ok()).unwrap_or(frames),
            "--tree" => tree = true,
            "--loops" => find_loops = true,
            other => recording = Some(other.to_string()),
        }
    }
    let Some(recording) = recording else {
        eprintln!("usage: trace <recording.rzx> [--frames N]");
        std::process::exit(2);
    };

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
    app.load_path(std::path::Path::new(&recording));
    if app.rzx.is_none() {
        eprintln!("{recording}: {}", app.status);
        std::process::exit(1);
    }

    app.spec.bus.observer.enabled = true;
    if let Some(rzx) = &mut app.rzx {
        rzx.max_speed = true;
    }
    for _ in 0..frames {
        app.advance(1.0 / 50.0);
        if app.rzx.is_none() {
            break;
        }
    }

    let adrift = app.spec.bus.playback.as_ref().map_or(0, |p| p.short);
    let played = app.rzx.as_ref().map_or(0, |rzx| rzx.frame);
    if adrift > 0 {
        eprintln!(
            "{recording}: came adrift after {played} frames — {adrift} reads the \
             recording had no answer for. Nothing written: what the machine did \
             after that is not what was recorded."
        );
        std::process::exit(1);
    }

    let observer = &app.spec.bus.observer;
    let watched = observer.frames as u32;

    if tree {
        call_tree(&app, observer);
        return;
    }
    if find_loops {
        show_loops(&app, observer);
        return;
    }
    let mut written = 0;
    for (entry, seen) in &observer.routines {
        // Only routines that did something worth describing.
        if seen.calls == 0 {
            continue;
        }
        println!("{}", episode(&app, *entry, seen, watched));
        written += 1;
    }
    eprintln!("{written} routines over {played} frames of {recording}");
}

/// The loops the program was seen to go round, and one turn of each.
///
/// Nothing here counts frames. A game may take three frames over a turn, draw
/// into a back buffer and show it when it is ready, or not be tied to the
/// frame at all; the only evidence used is the order routines were entered in.
fn show_loops(app: &App, observer: &zx_rustrum::observe::Observer) {
    let frame_t = app.spec.bus.frame_t();
    let steps: Vec<zx_rustrum::observe::Step> = observer.steps().copied().collect();
    let phases = zx_rustrum::loops::phases(&steps, frame_t, 64);
    if phases.is_empty() {
        eprintln!("nothing repeated often enough to call a loop");
        return;
    }

    println!("{} phases over {} calls\n", phases.len(), steps.len());
    for (n, phase) in phases.iter().enumerate() {
        let name = app.notes.label(phase.head);
        let named = if name.is_empty() {
            format!("${:04X}", phase.head)
        } else {
            format!("{name} (${:04X})", phase.head)
        };
        let turns = phase.frames_per_turn(frame_t);
        println!(
            "Phase {}: loops on {named}, {} turns, {:.2} frames a turn, {} routines",
            n + 1,
            phase.iterations,
            turns,
            phase.routines.len()
        );
        if let Some(turn) = zx_rustrum::loops::turn(&steps, phase, frame_t) {
            // The whole turn as one object, which is the thing worth asking
            // about: a routine on its own says little, and a turn of the loop
            // is the program's unit of work whatever its relationship to the
            // frame turns out to be.
            if let Ok(path) = std::env::var("TURN_JSON") {
                let calls: Vec<String> = turn
                    .steps
                    .iter()
                    .filter(|s| s.enter)
                    .map(|s| {
                        let name = app.notes.label(s.entry);
                        let named = if name.is_empty() {
                            format!("${:04X}", s.entry)
                        } else {
                            format!("{name} (${:04X})", s.entry)
                        };
                        format!(
                            "{{\"depth\":{},\"t\":{},\"routine\":\"{named}\"}}",
                            s.depth, s.t
                        )
                    })
                    .collect();
                let json = format!(
                    "{{\"head\":\"{:04X}\",\"frames_per_turn\":{:.2},\"t_states\":{},\"calls\":[{}]}}",
                    phase.head,
                    turns,
                    turn.to - turn.from,
                    calls.join(",")
                );
                let _ = std::fs::write(format!("{path}.{}.json", n + 1), json);
            }
            let calls = turn.steps.iter().filter(|s| s.enter).count();
            println!(
                "  one turn: {calls} calls over {} T-states",
                turn.to - turn.from
            );
            let mut shown = 0;
            for step in turn.steps.iter().filter(|s| s.enter) {
                let name = app.notes.label(step.entry);
                let named = if name.is_empty() {
                    format!("${:04X}", step.entry)
                } else {
                    name.to_string()
                };
                println!(
                    "  {:indent$}{named}",
                    "",
                    indent = (step.depth.saturating_sub(1)) as usize * 2
                );
                shown += 1;
                if shown >= 40 {
                    println!("  ... {} more", calls - shown);
                    break;
                }
            }
        }
        println!();
    }
}

/// One frame's calls, in the order they happened and nested as they nest.
///
/// A game repeats itself fifty times a second, so one whole frame is the unit
/// worth looking at: it is the program's turn, from the interrupt to the wait
/// for the next one. The T-state each call was entered at is where the beam
/// was at that moment, which is the thing a Spectrum programmer is arranging
/// their work around.
fn call_tree(app: &App, observer: &zx_rustrum::observe::Observer) {
    let Some(frame) = observer.last_whole_frame() else {
        eprintln!("nothing was watched for a whole frame");
        return;
    };
    let steps = observer.frame_steps(frame);
    if steps.is_empty() {
        eprintln!("frame {frame} has nothing in it");
        return;
    }

    let picture = app.spec.bus.first_pixel_t();
    println!("Frame {frame}: {} calls and returns", steps.len());
    println!("(the beam reaches the picture at T={picture})\n");

    for step in &steps {
        if !step.enter {
            continue;
        }
        let name = app.notes.label(step.entry);
        let named = if name.is_empty() {
            format!("${:04X}", step.entry)
        } else {
            format!("{name} (${:04X})", step.entry)
        };
        // Where the beam was: the one piece of context that turns a call
        // order into an explanation of how the picture is made.
        let where_beam = if step.t < picture {
            "above the picture"
        } else if step.t < picture + 192 * 224 {
            "on the picture"
        } else {
            "below the picture"
        };
        println!(
            "{:indent$}{named:<32} T={:<6} {where_beam}",
            "",
            step.t,
            indent = step.depth as usize * 2,
        );
    }
}

/// One routine, as a JSON object.
fn episode(app: &App, entry: u16, seen: &zx_rustrum::observe::Observed, frames: u32) -> String {
    let observer = &app.spec.bus.observer;
    let peek = |a: u16| app.peek(a);
    let features = autodoc::read_routine(&peek, entry);
    let (rule_label, rule_comment) = autodoc::describe(&features);
    let measured = autodoc::describe_measured(seen, frames);

    let listing: Vec<String> = {
        let mut at = entry;
        let mut lines = Vec::new();
        // The whole routine, not the top of it. Capped at 24, the model was
        // being shown the opening condition check of MOVEWILLY and asked what
        // the routine does, which is not a question anybody could answer.
        for _ in 0..200 {
            let insn = disasm::disasm(&peek, at);
            lines.push(format!("{at:04X}  {}", insn.text));
            if autodoc::ends_routine(&insn.text) {
                break;
            }
            at = at.wrapping_add(insn.len.max(1) as u16);
        }
        lines
    };

    let callers: Vec<String> = observer
        .edges
        .keys()
        .filter(|(_, to)| *to == entry)
        .map(|(from, _)| format!("{from:04X}"))
        .collect();
    let callees: Vec<String> = observer
        .edges
        .keys()
        .filter(|(from, _)| *from == entry)
        .map(|(_, to)| format!("{to:04X}"))
        .collect();

    let name = app.notes.label(entry);
    let art = drawn_cell(app, entry);

    let mut fields = vec![
        format!("\"address\":\"{entry:04X}\""),
        format!("\"name\":{}", json(name)),
        format!("\"calls\":{}", seen.calls),
        format!("\"frames_seen\":{}", seen.frames),
        format!("\"of_frames\":{frames}"),
        format!(
            "\"writes\":{{\"screen\":{},\"attrs\":{},\"other\":{}}}",
            seen.writes.screen, seen.writes.attrs, seen.writes.other
        ),
        format!(
            "\"inclusive\":{{\"screen\":{},\"attrs\":{},\"other\":{}}}",
            seen.inclusive.screen, seen.inclusive.attrs, seen.inclusive.other
        ),
        format!("\"longest_loop\":{}", seen.longest_loop()),
        // The span of addresses it wrote to. A game that draws into a buffer
        // and copies it to the screen later writes nothing to the display
        // file, and without this reads as a routine that thinks rather than
        // draws — which is what happened to every drawing routine in Manic
        // Miner, because that is exactly what it does.
        match seen.wrote_between {
            Some((low, high)) => format!("\"wrote_between\":[\"{low:04X}\",\"{high:04X}\"]"),
            None => "\"wrote_between\":null".to_string(),
        },
        format!(
            "\"entry_hl\":[\"{:04X}\",\"{:04X}\"]",
            seen.entry_hl.low, seen.entry_hl.high
        ),
        format!("\"ports_in\":{}", ports(&seen.ports_in)),
        format!("\"ports_out\":{}", ports(&seen.ports_out)),
        format!("\"rule_says\":{}", json(&rule_label)),
        format!("\"rule_comment\":{}", json(&rule_comment)),
        format!(
            "\"measured\":{}",
            measured
                .map(|(_, comment)| json(&comment))
                .unwrap_or_else(|| "null".into())
        ),
        format!("\"callers\":{}", strings(&callers)),
        format!("\"callees\":{}", strings(&callees)),
        format!("\"listing\":{}", strings(&listing)),
    ];
    if let Some(art) = art {
        // A sprite is thirty-two bytes; drawn as characters a model can read
        // it without anything having to understand a picture.
        fields.push(format!("\"drew\":{}", strings(&art)));
    }
    format!("{{{}}}", fields.join(","))
}

/// A character cell this routine drew, as eight rows of text.
fn drawn_cell(app: &App, entry: u16) -> Option<Vec<String>> {
    let observer = &app.spec.bus.observer;
    // The cell it wrote most of: the one where its work is clearest.
    let mut best: Option<(usize, usize, u32)> = None;
    for row in 0..24 {
        for column in 0..32 {
            let mine = observer
                .drew_cell(column, row)
                .into_iter()
                .find(|(who, _)| *who == entry)
                .map(|(_, bytes)| bytes)
                .unwrap_or(0);
            if mine > best.map_or(0, |(_, _, n)| n) {
                best = Some((column, row, mine));
            }
        }
    }
    let (column, row, _) = best?;

    // Two cells by two around the busiest one: a sprite is rarely one cell,
    // and the thing that makes this evidence worth having is being able to see
    // what was drawn. Requiring four bytes in a single cell left twenty-five
    // episodes out of twenty-six with no picture at all.
    let (first_column, last_column) = (column.saturating_sub(1), (column + 2).min(32));
    let (first_row, last_row) = (row.saturating_sub(1), (row + 2).min(24));

    let mut art = Vec::new();
    for r in first_row..last_row {
        let third = r / 8;
        for line in 0..8 {
            let y = (r % 8) * 8 + line;
            let mut text = String::new();
            for c in first_column..last_column {
                let addr = 0x4000 + (third << 11) + (y << 5) + c;
                let byte = app.peek(addr as u16);
                for bit in 0..8 {
                    text.push(if byte & (0x80 >> bit) != 0 { '#' } else { '.' });
                }
            }
            art.push(text);
        }
    }
    Some(art)
}

fn ports(ports: &[u16]) -> String {
    let list: Vec<String> = ports.iter().map(|p| format!("\"{p:04X}\"")).collect();
    format!("[{}]", list.join(","))
}

fn strings(items: &[String]) -> String {
    let list: Vec<String> = items.iter().map(|s| json(s)).collect();
    format!("[{}]", list.join(","))
}

fn json(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c if (c as u32) < 0x20 => vec![' '],
            c => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}
