//! The +3's drive, for a program driving the emulator.
//!
//! A disk is a different thing from a tape to take apart: it has a catalogue,
//! the sectors are addressable, and what a game reads off it can be watched
//! sector by sector. What is not here is a way to mount a disk writable
//! without saying so — every writable mount names where the writes go.

use crate::disk::Disk;
use crate::fdc::{Drive, Speed};
use crate::mcp::json::Json;
use crate::mcp::tools::{addr_of, count, flag, text, Session};

/// Put a disk in the drive.
pub fn mount_disk(session: &mut Session, args: &Json) -> Result<String, String> {
    let path = std::path::PathBuf::from(text(args, "path")?);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // A zip with a disk in it is a disk, which is how a download of a game
    // usually arrives. Nothing is written back into an archive, so a disk out
    // of one is read-only unless copy_to says where a writable copy goes.
    let from_archive = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let (inner, bytes) = if from_archive {
        crate::zip::first_with_extension(&bytes, &["dsk"])
            .ok_or_else(|| format!("{} holds no disk image", path.display()))?
    } else {
        (path.display().to_string(), bytes)
    };
    let disk = Disk::parse(&bytes)?;
    let what = if from_archive {
        format!("{inner} from {}: {}", path.display(), disk.describe())
    } else {
        disk.describe()
    };

    if !session.spec.bus.model.has_disk() {
        return Err(format!(
            "a {} has no disk drive: set_machine +3 first",
            session.spec.bus.model.name()
        ));
    }

    // Read-only unless told otherwise, and a writable mount says where the
    // writes go. A game writes its high scores to the disk it loaded from, and
    // doing that to somebody's file without being asked is not on.
    let writable = flag(args, "writable", false);
    let copy_to = args.get("copy_to").and_then(|p| p.as_str());
    let (path, protected) = match (writable, copy_to) {
        (_, Some(to)) => {
            std::fs::write(to, disk.to_bytes()).map_err(|e| format!("{to}: {e}"))?;
            (Some(std::path::PathBuf::from(to)), false)
        }
        (true, None) if from_archive => {
            return Err(format!(
                "{} is an archive, and a disk cannot be written back into one. Give \
                 copy_to a filename and the writes will go there.",
                path.display()
            ))
        }
        (true, None) => (Some(path.clone()), false),
        (false, None) => (Some(path.clone()), true),
    };
    let where_ = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    session.spec.bus.fdc.drives[0] = Some(Drive::new(disk, path, protected));
    Ok(format!(
        "{what}. {}",
        if protected {
            "Read-only: the machine is told it is write-protected and the file is not \
             touched. Pass writable, or copy_to a new file, to let it be written."
                .to_string()
        } else {
            format!("Writable: writes go to {where_}.")
        }
    ))
}

/// A blank disk, formatted as the machine's own FORMAT formats one.
pub fn new_disk(session: &mut Session, args: &Json) -> Result<String, String> {
    if !session.spec.bus.model.has_disk() {
        return Err(format!(
            "a {} has no disk drive: set_machine +3 first",
            session.spec.bus.model.name()
        ));
    }
    let disk = Disk::blank("zx-rustrum");
    let what = disk.describe();
    let path = match args.get("path").and_then(|p| p.as_str()) {
        Some(path) => {
            std::fs::write(path, disk.to_bytes()).map_err(|e| format!("{path}: {e}"))?;
            Some(std::path::PathBuf::from(path))
        }
        None => None,
    };
    let where_ = match &path {
        Some(p) => format!(" and written to {}", p.display()),
        None => ", in memory only — pass a path to keep it".to_string(),
    };
    session.spec.bus.fdc.drives[0] = Some(Drive::new(disk, path, false));
    Ok(format!("A blank disk: {what}{where_}."))
}

/// Take it out, writing it back first if it has changed.
pub fn eject_disk(session: &mut Session, _args: &Json) -> Result<String, String> {
    let Some(drive) = session.spec.bus.fdc.drives[0].take() else {
        return Err("the drive is empty".into());
    };
    let mut out = String::from("Ejected");
    if drive.disk.dirty && !drive.write_protected {
        if let Some(path) = &drive.path {
            match std::fs::write(path, drive.disk.to_bytes()) {
                Ok(()) => out.push_str(&format!(", and wrote {}", path.display())),
                Err(e) => out.push_str(&format!(", but could not write {}: {e}", path.display())),
            }
        }
    }
    Ok(out)
}

/// What is in the drive, and what the drive is doing.
pub fn disk_info(session: &mut Session, _args: &Json) -> Result<String, String> {
    let fdc = &session.spec.bus.fdc;
    let Some(drive) = fdc.drives[0].as_ref() else {
        return Ok(format!(
            "The drive is empty. Speed: {}. mount_disk or new_disk puts one in.",
            speed_name(fdc.speed)
        ));
    };
    let mut out = format!(
        "{}{}\n{} — {}\n",
        drive
            .path
            .as_ref()
            .map(|p| format!("{}\n", p.display()))
            .unwrap_or_default(),
        drive.disk.describe(),
        drive.disk.format().describe(),
        if drive.write_protected {
            "read-only"
        } else {
            "writable"
        }
    );
    out.push_str(&format!(
        "Head at track {}, motor {}, speed {}. {} sectors read and {} written recently.\n",
        fdc.head_at(0),
        if fdc.motor { "on" } else { "off" },
        speed_name(fdc.speed),
        fdc.reads.len(),
        fdc.writes.len()
    ));
    match drive.disk.made_for() {
        Some(crate::disk::MadeFor::Spectrum) => {
            out.push_str("The files on it have +3DOS headers: a Spectrum disk.\n")
        }
        Some(crate::disk::MadeFor::Amstrad) => out.push_str(
            "The files on it have AMSDOS headers: this is an Amstrad CPC disk. The two \
             machines use the same disks, the same controller and the same filesystem, so \
             it mounts and catalogues perfectly well here — and a +3 cannot load it.\n",
        ),
        Some(crate::disk::MadeFor::Headerless) => {
            out.push_str("The first file has no header, which is what a game's own loader reads.\n")
        }
        None => {}
    }
    if drive.disk.dirty {
        out.push_str("It has been written to since it was mounted.\n");
    }
    Ok(out)
}

fn speed_name(speed: Speed) -> &'static str {
    match speed {
        Speed::Normal => "Normal — the drive's own waits, as a real one makes them",
        Speed::Fastload => "Fastload — no waits at all",
    }
}

/// How the drive behaves about time.
pub fn disk_speed(session: &mut Session, args: &Json) -> Result<String, String> {
    let wanted = text(args, "speed")?;
    session.spec.bus.fdc.speed = match wanted.to_ascii_lowercase().as_str() {
        "normal" | "real" => Speed::Normal,
        "fastload" | "fast" => Speed::Fastload,
        other => return Err(format!("no speed {other:?}: normal or fastload")),
    };
    Ok(format!(
        "Speed: {}.",
        speed_name(session.spec.bus.fdc.speed)
    ))
}

/// What is on the disk, as CAT would print it.
pub fn disk_catalogue(session: &mut Session, _args: &Json) -> Result<String, String> {
    let drive = session.spec.bus.fdc.drives[0]
        .as_ref()
        .ok_or("the drive is empty")?;
    let Some(files) = drive.disk.catalogue() else {
        return Err(
            "not a +3 format disk: its sectors are numbered some other way, so there is no \
             catalogue to read. read_sector still works."
                .into(),
        );
    };
    if files.is_empty() {
        return Ok(format!(
            "No files. {}K free.",
            drive.disk.free_kilobytes().unwrap_or(0)
        ));
    }
    let mut out = String::new();
    for file in &files {
        out.push_str(&format!("{:<14} {:>4}K", file.name, file.kilobytes));
        if file.read_only {
            out.push_str("  read-only");
        }
        if file.system {
            // The machine's own CAT does not list these; a program taking the
            // disk apart wants to know they are there.
            out.push_str("  hidden from CAT");
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{}K free.\n",
        drive.disk.free_kilobytes().unwrap_or(0)
    ));
    Ok(out)
}

/// A sector, by where it is and what it is called.
pub fn read_sector(session: &mut Session, args: &Json) -> Result<String, String> {
    let track = count(args, "track", 0)? as u8;
    let side = count(args, "side", 0)? as u8;
    let drive = session.spec.bus.fdc.drives[0]
        .as_ref()
        .ok_or("the drive is empty")?;
    let disk_track = drive
        .disk
        .track(track, side)
        .ok_or_else(|| format!("no track {track} on side {side}"))?;
    // By sector number if given, otherwise by position on the track: a disk
    // that numbers its sectors oddly is read the same way the controller reads
    // it, by the number in the address mark.
    let sector = match args.get("sector") {
        Some(value) => {
            let wanted = addr_of(value)? as u8;
            disk_track
                .sectors
                .iter()
                .find(|s| s.r == wanted)
                .ok_or_else(|| {
                    format!(
                        "no sector ${wanted:02X} on track {track}; it holds {}",
                        disk_track
                            .sectors
                            .iter()
                            .map(|s| format!("${:02X}", s.r))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?
        }
        None => disk_track.sectors.first().ok_or("that track is empty")?,
    };
    let mut out = format!(
        "Track {track}, side {side}, sector ${:02X} ({} bytes, size code {}){}\n",
        sector.r,
        sector.data.len(),
        sector.n,
        if sector.st1 != 0 || sector.st2 != 0 {
            format!(
                " — the disk records an error here: ST1 ${:02X}, ST2 ${:02X}",
                sector.st1, sector.st2
            )
        } else {
            String::new()
        }
    );
    let from = count(args, "offset", 0)? as usize;
    let length = count(args, "length", 512)?.min(crate::mcp::tools::MAX_READ as u32) as usize;
    let end = (from + length).min(sector.data.len());
    out.push_str(&crate::mcp::memory::hex_dump(
        from as u16,
        &sector.data[from.min(sector.data.len())..end],
    ));
    Ok(out)
}

/// Which sectors the machine has been reading and writing lately.
pub fn disk_activity(session: &mut Session, _args: &Json) -> Result<String, String> {
    let fdc = &session.spec.bus.fdc;
    if fdc.reads.is_empty() && fdc.writes.is_empty() {
        return Ok(
            "Nothing has been read or written recently. The map fades over a couple of \
             seconds, so this is what the machine is doing now rather than everything it \
             has ever done."
                .into(),
        );
    }
    let mut out = String::from(
        "Sectors touched recently, brightest first — 255 is this moment, 0 is faded away.\n",
    );
    let mut lines: Vec<(u8, String)> = Vec::new();
    for ((track, side, sector), heat) in &fdc.reads {
        lines.push((
            *heat,
            format!("  read   track {track:>2} side {side} sector ${sector:02X}  {heat}"),
        ));
    }
    for ((track, side, sector), heat) in &fdc.writes {
        lines.push((
            *heat,
            format!("  write  track {track:>2} side {side} sector ${sector:02X}  {heat}"),
        ));
    }
    lines.sort_by_key(|(heat, _)| std::cmp::Reverse(*heat));
    for (_, line) in lines.iter().take(40) {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}
