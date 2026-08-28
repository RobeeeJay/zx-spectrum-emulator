//! What memory has been used, and how: the access map the RAM window draws,
//! and the back buffer it detects.
//!
//! The tracker counts every read, write and fetch against the physical
//! location rather than the address, so a bank keeps its history while it is
//! paged out. What it answers is "where does this program keep things" — which
//! pages it writes constantly, which it only reads, and which it never touches
//! at all.

use crate::mcp::json::Json;
use crate::mcp::tools::{addr, count, flag, Session};

/// How the counts are summarised: per 256-byte page, because a table of 65,536
/// numbers says less than a table of 256.
pub fn memory_activity(session: &mut Session, args: &Json) -> Result<String, String> {
    let from = match args.get("from") {
        Some(_) => addr(args, "from")?,
        None => 0x4000,
    };
    let to = match args.get("to") {
        Some(_) => addr(args, "to")?,
        None => 0xFFFF,
    };
    let quiet = flag(args, "include_quiet", false);
    let limit = count(args, "limit", 40)? as usize;

    let tracker = &session.spec.bus.tracker;
    let mut pages: Vec<(u16, u32, u32, bool)> = Vec::new();
    let mut page = from & 0xFF00;
    loop {
        let (mut reads, mut writes, mut executed) = (0u32, 0u32, false);
        for offset in 0..256u16 {
            let at = page.wrapping_add(offset);
            if at < from || at > to {
                continue;
            }
            let phys = session.spec.bus.phys_index(at);
            reads += tracker.read_count[phys];
            writes += tracker.write_count[phys];
            executed |= tracker.exec_heat[phys] > 0;
        }
        if quiet || reads > 0 || writes > 0 || executed {
            pages.push((page, reads, writes, executed));
        }
        if page >= (to & 0xFF00) || page == 0xFF00 {
            break;
        }
        page = page.wrapping_add(0x100);
    }
    if pages.is_empty() {
        return Ok(format!(
            "nothing between ${from:04X} and ${to:04X} has been touched. The counts start \
             at reset and the heat fades every frame, so a machine that has just been \
             loaded has little to show."
        ));
    }
    pages.sort_by_key(|(_, reads, writes, _)| std::cmp::Reverse(reads + writes));

    let mut out = String::from(
        "Memory by 256-byte page, busiest first. Reads and writes are counted from the \
         last reset; \"run\" means something was executed there.\n",
    );
    let sp = session.spec.cpu.sp;
    for (page, reads, writes, executed) in pages.iter().take(limit) {
        // Where the machine's own furniture is, so a page that is the stack or
        // the system variables is not reported as "variables" and left for
        // somebody to work out.
        let known = match page {
            0x0000..=0x3F00 => Some("the ROM"),
            0x4000..=0x5700 => Some("the display file"),
            0x5800 => Some("the attributes"),
            0x5B00 => Some("the printer buffer"),
            0x5C00 => Some("the system variables"),
            _ if *page == (sp & 0xFF00) => Some("the stack is in here"),
            _ => None,
        };
        let what = match (*writes > 0, *reads > 0, *executed) {
            (_, _, true) => "code",
            (true, _, _) if *writes > *reads * 4 => "written far more than read — a buffer",
            (true, true, _) => "read and written — variables",
            (false, true, _) => "only read — a table, or graphics",
            _ => "",
        };
        out.push_str(&format!(
            "  ${page:04X}xx  reads {reads:>9}  writes {writes:>9}  {}\n",
            // A page the machine itself defines is called what it is; the
            // guess is for the rest, where nobody has said.
            match known {
                Some(name) => name.to_string(),
                None => what.to_string(),
            }
        ));
    }
    if pages.len() > limit {
        out.push_str(&format!("  ({} more pages)\n", pages.len() - limit));
    }

    match session.spec.bus.tracker.back_buffer() {
        Some(region) => out.push_str(&format!(
            "\nBack buffer: ${:04X}-${:04X}, confidence {:.0}%. A screen being built \
             somewhere other than the display file and blitted across.\n",
            region.start,
            region.start as u32 + region.len as u32 - 1,
            session.spec.bus.tracker.detected_confidence * 100.0
        )),
        None => out.push_str(
            "\nNo back buffer detected. That is not proof there is none: the detector \
             wants a long run of heavily written pages outside video RAM whose contents \
             are then copied into it.\n",
        ),
    }
    Ok(out)
}
