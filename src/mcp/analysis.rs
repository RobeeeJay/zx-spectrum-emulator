//! Reading the program rather than running it: the disassembly, what the
//! machine was seen to do, and the notes that are written about it.
//!
//! The measurements come from `src/observe.rs`, which attributes every write,
//! read, port access and loop to the routine on top of the call stack. That is
//! why these tools say "watch_routines first": with the observer off, nothing
//! is being counted and the answers would be empty rather than wrong.

use crate::mcp::json::Json;
use crate::mcp::tools::{addr, count, flag, text, Session};

/// How many routines a listing hands back before it is cut off. A game makes a
/// few hundred; the first fifty by work done are the ones worth reading.
const MAX_ROUTINES: usize = 50;

pub fn disassemble(session: &mut Session, args: &Json) -> Result<String, String> {
    let start = match args.get("address") {
        Some(_) => addr(args, "address")?,
        None => session.spec.cpu.pc,
    };
    let lines = count(args, "count", 32)?.min(512);
    let comments = flag(args, "comments", true);
    let peek = |a: u16| session.spec.bus.peek_raw(a);

    let mut out = String::new();
    let mut at = start;
    for _ in 0..lines {
        let insn = crate::disasm::disasm(&peek, at);
        let bytes: String = insn
            .bytes
            .iter()
            .map(|b| format!("{b:02X} "))
            .collect::<String>();
        let label = session.notes.label(at);
        let comment = session.notes.comment(at);
        let executed = session.spec.bus.observer.was_executed(at);
        let data = session.spec.bus.observer.is_data(at);

        if comments && !label.is_empty() {
            out.push_str(&format!("{label}:\n"));
        }
        out.push_str(&format!("${at:04X}  {bytes:<12} {:<20}", insn.text));
        let mut notes = Vec::new();
        if executed {
            notes.push("run".to_string());
        } else if data {
            notes.push("read as data".to_string());
        }
        if comments && !comment.is_empty() {
            notes.push(comment.to_string());
        }
        if !notes.is_empty() {
            out.push_str(&format!("; {}", notes.join(" — ")));
        }
        out.push('\n');
        at = at.wrapping_add(insn.len.max(1) as u16);
    }
    out.push_str(&format!("(next address ${at:04X})\n"));
    Ok(out)
}

pub fn watch_routines(session: &mut Session, args: &Json) -> Result<String, String> {
    let on = flag(args, "enabled", true);
    session.spec.bus.observer.enabled = on;
    if flag(args, "clear", false) {
        session.spec.bus.observer.clear();
    }
    Ok(if on {
        "Watching. Every write, read, port access and loop is now attributed to \
         the routine making it; run the machine, then ask for routines. \
         Watching costs a branch on every access, so turn it off when done."
            .to_string()
    } else {
        "Not watching any more. What was already seen is kept; pass clear: true to forget it."
            .to_string()
    })
}

pub fn routines(session: &mut Session, args: &Json) -> Result<String, String> {
    let observer = &session.spec.bus.observer;
    if observer.routines.is_empty() {
        return Err(
            "nothing has been watched yet: call watch_routines, then run the machine".into(),
        );
    }
    let limit = count(args, "limit", MAX_ROUTINES as u32)? as usize;
    let mut list: Vec<_> = observer.routines.values().collect();
    match text(args, "sort")
        .unwrap_or_else(|_| "work".into())
        .as_str()
    {
        "calls" => list.sort_by_key(|r| std::cmp::Reverse(r.calls)),
        "address" => list.sort_by_key(|r| r.entry),
        "writes" => list.sort_by_key(|r| std::cmp::Reverse(r.inclusive.total())),
        _ => list.sort_by_key(|r| std::cmp::Reverse(r.instructions)),
    }
    let frames = observer.frames.max(1);

    let mut out = format!(
        "{} routines seen over {} frames. Columns: entry, calls, instructions, \
         writes as screen/attrs/other (per call, including what it calls), \
         reads, ports, size.\n",
        observer.routines.len(),
        observer.frames
    );
    for r in list.iter().take(limit) {
        let per_call = |v: u32| v as f64 / r.calls.max(1) as f64;
        let size = r
            .spans
            .map_or(0, |(low, high)| high as u32 - low as u32 + 1);
        let label = session.notes.label(r.entry).to_string();
        out.push_str(&format!(
            "${:04X} {label:<16} calls {:>6} ({:.1}/frame)  insns {:>9}  \
             writes {:.0}/{:.0}/{:.0}  reads {:.0}  ports {}in/{}out  size {size}\n",
            r.entry,
            r.calls,
            r.calls as f64 / frames as f64,
            r.instructions,
            per_call(r.inclusive.screen),
            per_call(r.inclusive.attrs),
            per_call(r.inclusive.other),
            per_call(r.inclusive_reads.total()),
            r.ports_in.len(),
            r.ports_out.len(),
        ));
    }
    if list.len() > limit {
        out.push_str(&format!("({} more not shown)\n", list.len() - limit));
    }
    Ok(out)
}

pub fn routine(session: &mut Session, args: &Json) -> Result<String, String> {
    let entry = addr(args, "address")?;
    let observed = session
        .spec
        .bus
        .observer
        .routines
        .get(&entry)
        .ok_or_else(|| format!("${entry:04X} has not been seen called while watching"))?;
    let frames = session.spec.bus.observer.frames.max(1);
    let per_call = |v: u32| v as f64 / observed.calls.max(1) as f64;

    let mut out = String::new();
    let label = session.notes.label(entry);
    out.push_str(&format!(
        "${entry:04X} {label}\ncalled {} times over {} frames ({:.2} a frame), \
         {} instructions, deepest nesting {}\n",
        observed.calls,
        session.spec.bus.observer.frames,
        observed.calls as f64 / frames as f64,
        observed.instructions,
        observed.max_depth
    ));
    match observed.spans {
        Some((low, high)) => out.push_str(&format!(
            "reaches ${low:04X}-${high:04X} ({} bytes) — where it got to, not a promise: \
             a routine that jumps over a table reaches past the table\n",
            high as u32 - low as u32 + 1
        )),
        None => out.push_str("extent not established\n"),
    }
    out.push_str(&format!(
        "writes per call: {:.1} to the display file, {:.1} to attributes, {:.1} elsewhere \
         (itself alone: {:.1}/{:.1}/{:.1})\n",
        per_call(observed.inclusive.screen),
        per_call(observed.inclusive.attrs),
        per_call(observed.inclusive.other),
        per_call(observed.writes.screen),
        per_call(observed.writes.attrs),
        per_call(observed.writes.other),
    ));
    out.push_str(&format!(
        "reads per call: {:.1} (itself alone {:.1})\n",
        per_call(observed.inclusive_reads.total()),
        per_call(observed.reads.total())
    ));
    if let Some((low, high)) = observed.wrote_between {
        out.push_str(&format!("wrote between ${low:04X} and ${high:04X}\n"));
    }
    if let Some(byte) = observed.filled_with {
        out.push_str(&format!("filled with ${byte:02X}\n"));
    }
    if !observed.ports_in.is_empty() || !observed.ports_out.is_empty() {
        out.push_str(&format!(
            "ports: in {} ({} reads), out {} ({} writes)\n",
            ports(&observed.ports_in),
            observed.port_reads,
            ports(&observed.ports_out),
            observed.port_writes
        ));
    }
    if !observed.loops.is_empty() {
        let mut loops: Vec<_> = observed.loops.iter().collect();
        loops.sort_by_key(|(_, times)| std::cmp::Reverse(**times));
        out.push_str("loops (address, times round): ");
        out.push_str(
            &loops
                .iter()
                .take(8)
                .map(|(at, times)| format!("${at:04X}×{times}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    if !observed.exits.is_empty() {
        out.push_str(&format!("exits: {}\n", addresses(&observed.exits)));
    }
    if !observed.after.is_empty() {
        out.push_str(&format!("ends at: {}\n", addresses(&observed.after)));
    }
    if !observed.hot.is_empty() {
        out.push_str(&format!(
            "busiest addresses: {}\n",
            addresses(&observed.hot)
        ));
    }
    out.push_str(&format!(
        "entry registers seen: AF {} BC {} DE {} HL {}\n",
        seen(&observed.entry_af),
        seen(&observed.entry_bc),
        seen(&observed.entry_de),
        seen(&observed.entry_hl)
    ));
    Ok(out)
}

fn ports(list: &[u16]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    list.iter()
        .take(8)
        .map(|p| format!("${p:04X}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn addresses(list: &[u16]) -> String {
    list.iter()
        .take(12)
        .map(|a| format!("${a:04X}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn seen(range: &crate::observe::Seen) -> String {
    if range.low == range.high {
        format!("${:04X}", range.low)
    } else {
        format!("${:04X}-${:04X}", range.low, range.high)
    }
}

pub fn call_graph(session: &mut Session, args: &Json) -> Result<String, String> {
    let observer = &session.spec.bus.observer;
    if observer.edges.is_empty() {
        return Err("no calls have been watched: call watch_routines, then run the machine".into());
    }
    let limit = count(args, "limit", 60)? as usize;
    let mut edges: Vec<_> = observer
        .edges
        .iter()
        .map(|((from, to), edge)| (*from, *to, edge.calls))
        .collect();
    edges.sort_by_key(|e| std::cmp::Reverse(e.2));
    let mut out = format!(
        "{} call edges between {} routines, the busiest first.\n",
        observer.edges.len(),
        observer.routines.len()
    );
    for (from, to, calls) in edges.iter().take(limit) {
        let from_label = session.notes.label(*from);
        let to_label = session.notes.label(*to);
        out.push_str(&format!(
            "${from:04X} {from_label:<14} -> ${to:04X} {to_label:<14} {calls} calls\n"
        ));
    }
    if edges.len() > limit {
        out.push_str(&format!("({} more not shown)\n", edges.len() - limit));
    }
    Ok(out)
}

pub fn code_map(session: &mut Session, args: &Json) -> Result<String, String> {
    let min = count(args, "min_length", 8)? as u16;
    let observer = &session.spec.bus.observer;
    let code = observer.code_runs();
    let data = observer.data_blocks(min);
    if code.is_empty() && data.is_empty() {
        return Err(
            "nothing has been watched yet: call watch_routines, then run the machine".into(),
        );
    }
    let mut out = format!(
        "What was executed, and what was only read. {} runs of code, {} blocks of data.\n\
         This is what the machine did while it was watched, not everything that exists.\n",
        code.len(),
        data.len()
    );
    out.push_str("code:\n");
    for (low, high) in code.iter().take(60) {
        out.push_str(&format!(
            "  ${low:04X}-${high:04X}  {} bytes\n",
            *high as u32 - *low as u32 + 1
        ));
    }
    out.push_str("data (read, never executed):\n");
    for (low, high) in data.iter().take(60) {
        let length = *high as u32 - *low as u32 + 1;
        let where_ = match low {
            0x4000..=0x57FF => " — in the display file",
            0x5800..=0x5AFF => " — in the attributes",
            _ => "",
        };
        out.push_str(&format!(
            "  ${low:04X}-${high:04X}  {length} bytes{where_}\n"
        ));
    }
    Ok(out)
}

pub fn autodoc(session: &mut Session, args: &Json) -> Result<String, String> {
    let mut entries: Vec<u16> = Vec::new();
    if let Some(list) = args.get("entries").and_then(|e| e.as_array()) {
        for item in list {
            entries.push(crate::mcp::tools::addr_of(item)?);
        }
    }
    if entries.is_empty() {
        // Whatever has been watched, busiest first: the routines a game
        // spends its time in are the ones worth a name.
        let mut seen: Vec<_> = session.spec.bus.observer.routines.values().collect();
        seen.sort_by_key(|r| std::cmp::Reverse(r.instructions));
        entries = seen.iter().take(40).map(|r| r.entry).collect();
    }
    if entries.is_empty() {
        entries.push(session.spec.cpu.pc);
    }
    let peek = |a: u16| session.spec.bus.peek_raw(a);
    let doc = crate::autodoc::analyse(&peek, &entries);
    if doc.is_empty() {
        return Ok(
            "AutoDoc found nothing it was willing to name. Code with no tell is \
                   left unnamed rather than guessed at."
                .to_string(),
        );
    }
    let mut out = String::from(
        "AutoDoc's guesses. These are guesses: it says so in the wording where the \
         evidence is thin, and a routine with no tell is left out.\n",
    );
    for entry in &entries {
        let label = doc.label(*entry);
        let comment = doc.comment(*entry);
        if label.is_empty() && comment.is_empty() {
            continue;
        }
        out.push_str(&format!("${entry:04X}  {label}"));
        if !comment.is_empty() {
            out.push_str(&format!("  — {comment}"));
        }
        out.push('\n');
    }
    Ok(out)
}

pub fn set_comment(session: &mut Session, args: &Json) -> Result<String, String> {
    let at = addr(args, "address")?;
    let mut did = Vec::new();
    if let Some(label) = args.get("label").and_then(|l| l.as_str()) {
        session.notes.set_label(at, label);
        did.push(format!("label {label:?}"));
    }
    if let Some(comment) = args.get("comment").and_then(|c| c.as_str()) {
        session.notes.set_comment(at, comment);
        did.push(format!("comment {comment:?}"));
    }
    if did.is_empty() {
        return Err("set_comment needs a label, a comment, or both".into());
    }
    Ok(format!(
        "${at:04X}: {}. Typed rather than guessed, so nothing will overwrite it.",
        did.join(", ")
    ))
}

pub fn comments(session: &mut Session, args: &Json) -> Result<String, String> {
    let from = match args.get("from") {
        Some(_) => addr(args, "from")?,
        None => 0,
    };
    let to = match args.get("to") {
        Some(_) => addr(args, "to")?,
        None => 0xFFFF,
    };
    let mut lines = Vec::new();
    for (at, label, auto) in session.notes.labelled() {
        if at < from || at > to {
            continue;
        }
        let comment = session.notes.comment(at);
        let mark = if auto { " (a guess)" } else { "" };
        lines.push(format!(
            "${at:04X}  {label}{mark}{}",
            if comment.is_empty() {
                String::new()
            } else {
                format!("  — {comment}")
            }
        ));
    }
    if lines.is_empty() {
        return Ok(format!(
            "Nothing written between ${from:04X} and ${to:04X}. There are {} notes altogether.",
            session.notes.len()
        ));
    }
    Ok(lines.join("\n"))
}

pub fn save_comments(session: &mut Session, _args: &Json) -> Result<String, String> {
    match session.notes.file() {
        None => Err(
            "the notes are not attached to a file: load a tape or a snapshot first, \
                     and they are written beside it"
                .into(),
        ),
        Some(path) => {
            let path = path.to_path_buf();
            match session.notes.save_if_dirty() {
                Ok(true) => Ok(format!("Written to {}", path.display())),
                Ok(false) => Ok(format!(
                    "Nothing had changed since the last write to {}",
                    path.display()
                )),
                Err(e) => Err(format!("{}: {e}", path.display())),
            }
        }
    }
}
