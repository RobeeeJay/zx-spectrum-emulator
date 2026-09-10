//! The microdrives, for a program driving the emulator.
//!
//! A cartridge is a loop of tape in 543-byte sectors with a catalogue in it,
//! so it takes apart the way a disk does: what is on it, what state it is in,
//! and which sectors do not add up. The drives are numbered from 1, as the
//! machine numbers them — `LOAD *"m";1;"name"` is drive 1.
//!
//! Everything here needs an Interface 1 with its ROM: without one the machine
//! has no microdrive commands at all, so the errors say to fit one rather than
//! reporting an empty drive.

use crate::if1::Drive;
use crate::mcp::json::Json;
use crate::mcp::tools::{count, flag, text, Session};
use crate::microdrive::Cartridge;

/// The interface, or an error saying how to get one.
fn interface(session: &mut Session) -> Result<&mut crate::if1::If1, String> {
    if session.spec.bus.if1.is_none() {
        return Err(
            "no Interface 1 is fitted, so the machine has no microdrives and no commands \
             for them. fit {\"what\": \"if1\"} puts one on."
                .into(),
        );
    }
    Ok(session.spec.bus.if1.as_mut().expect("checked"))
}

/// Which drive an argument means, counted from 1 as the machine counts them.
fn which(session: &mut Session, args: &Json) -> Result<usize, String> {
    let drives = session.spec.bus.if1.as_ref().map_or(0, |i| i.drive_count());
    let drive = count(args, "drive", 1)? as usize;
    if drive == 0 || drive > drives {
        return Err(format!(
            "there is no drive {drive}: the chain has {drives}. fit takes a microdrives \
             count of up to eight."
        ));
    }
    Ok(drive - 1)
}

/// Put a cartridge in a drive.
pub fn mount_cartridge(session: &mut Session, args: &Json) -> Result<String, String> {
    interface(session)?;
    let at = which(session, args)?;
    let path = std::path::PathBuf::from(text(args, "path")?);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;

    // A cartridge out of a zip is how a download usually arrives, and nothing
    // is written back into an archive.
    let from_archive = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    let (inner, bytes) = if from_archive {
        crate::zip::first_with_extension(&bytes, &["mdr"])
            .ok_or_else(|| format!("{} holds no .mdr cartridge", path.display()))?
    } else {
        (path.display().to_string(), bytes)
    };
    let cartridge = Cartridge::parse(&bytes)?;
    let what = cartridge.describe();
    let name = cartridge.name();

    // Read-only unless told otherwise, and a writable mount says where the
    // writes go: a program writes to the cartridge it loaded from.
    let writable = flag(args, "writable", false);
    let copy_to = args.get("copy_to").and_then(|p| p.as_str());
    let (path, read_only) = match (writable, copy_to) {
        (_, Some(to)) => {
            std::fs::write(to, cartridge.to_bytes()).map_err(|e| format!("{to}: {e}"))?;
            (Some(std::path::PathBuf::from(to)), false)
        }
        (true, None) if from_archive => {
            return Err(format!(
                "{} is an archive, and a cartridge cannot be written back into one. Give \
                 copy_to a filename and the writes will go there.",
                path.display()
            ))
        }
        (true, None) => (Some(path.clone()), false),
        (false, None) => (Some(path.clone()), true),
    };
    let tab = cartridge.write_protected;
    let where_ = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let drives = session.spec.bus.if1.as_mut().expect("checked");
    drives.drives[at] = Drive::loaded(cartridge, path, read_only);

    Ok(format!(
        "Drive {}: {name} — {what}, from {inner}. {}{}",
        at + 1,
        if read_only {
            "Read-only: the file is not touched. Pass writable, or copy_to a new file, to \
             let the machine write to it."
                .to_string()
        } else {
            format!("Writable: writes go to {where_}.")
        },
        if tab {
            " The cartridge's own write-protect tab is broken off, so the machine will \
             refuse to write to it whatever this says."
        } else {
            ""
        }
    ))
}

/// A blank formatted cartridge.
pub fn new_cartridge(session: &mut Session, args: &Json) -> Result<String, String> {
    interface(session)?;
    let at = which(session, args)?;
    let sectors = count(args, "sectors", 180)?.clamp(1, crate::microdrive::MAX_SECTORS as u32);
    let name = args
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("blank")
        .to_string();
    let cartridge = Cartridge::blank(&name, sectors as usize);
    let what = cartridge.describe();
    let path = match args.get("path").and_then(|p| p.as_str()) {
        Some(path) => {
            std::fs::write(path, cartridge.to_bytes()).map_err(|e| format!("{path}: {e}"))?;
            Some(std::path::PathBuf::from(path))
        }
        None => None,
    };
    let where_ = match &path {
        Some(p) => format!(" and written to {}", p.display()),
        None => ", in memory only — pass a path to keep it".to_string(),
    };
    let drives = session.spec.bus.if1.as_mut().expect("checked");
    drives.drives[at] = Drive::loaded(cartridge, path, false);
    Ok(format!(
        "Drive {}: a blank cartridge, {what}{where_}. The machine's own FORMAT writes one \
         sector of test pattern and reports a kilobyte less free than this does.",
        at + 1
    ))
}

/// Take it out, writing it back first if the machine changed it.
pub fn eject_cartridge(session: &mut Session, args: &Json) -> Result<String, String> {
    interface(session)?;
    let at = which(session, args)?;
    let drives = session.spec.bus.if1.as_mut().expect("checked");
    let drive = std::mem::take(&mut drives.drives[at]);
    let Some(cartridge) = drive.cartridge else {
        return Err(format!("drive {} is empty", at + 1));
    };
    let mut out = format!("Ejected from drive {}", at + 1);
    if cartridge.dirty && !drive.read_only && !cartridge.write_protected {
        if let Some(path) = &drive.path {
            match std::fs::write(path, cartridge.to_bytes()) {
                Ok(()) => out.push_str(&format!(", and wrote {}", path.display())),
                Err(e) => out.push_str(&format!(", but could not write {}: {e}", path.display())),
            }
        }
    }
    Ok(out)
}

/// The chain: what is in each drive, and which one is turning.
pub fn microdrive_info(session: &mut Session, _args: &Json) -> Result<String, String> {
    let if1 = interface(session)?;
    let mut out = format!(
        "Interface 1 with {} microdrive{}. Its ROM is {}.\n",
        if1.drive_count(),
        if if1.drive_count() == 1 { "" } else { "s" },
        if if1.rom.is_some() {
            "loaded"
        } else {
            "MISSING — the machine has no microdrive commands without it"
        }
    );
    if if1.paged {
        out.push_str("Its ROM is paged in over the bottom 8K right now.\n");
    }
    let turning = if1.selected;
    for (i, drive) in if1.drives.iter().enumerate() {
        let running = if1.motor_on() && turning == i + 1;
        match &drive.cartridge {
            Some(cartridge) => out.push_str(&format!(
                "  drive {}: {} — {}{}{}\n",
                i + 1,
                cartridge.name(),
                cartridge.describe(),
                if drive.writable() { "" } else { ", read-only" },
                if running {
                    format!(", turning, sector {} under the head", drive.sector())
                } else {
                    String::new()
                }
            )),
            None => out.push_str(&format!("  drive {}: empty\n", i + 1)),
        }
    }
    out.push_str(
        "\nThe machine reaches them as \"m\": LOAD *\"m\";1;\"name\", SAVE *, CAT 1, \
         FORMAT \"m\";1;\"name\". type_text can type those.\n",
    );
    Ok(out)
}

/// What is on a cartridge: its files, its free space, and its bad sectors.
pub fn cartridge_catalogue(session: &mut Session, args: &Json) -> Result<String, String> {
    interface(session)?;
    let at = which(session, args)?;
    let if1 = session.spec.bus.if1.as_ref().expect("checked");
    let Some(cartridge) = if1.drives[at].cartridge.as_ref() else {
        return Err(format!(
            "drive {} is empty: mount_cartridge or new_cartridge puts one in",
            at + 1
        ));
    };

    let mut out = format!(
        "Drive {}: {} — {}\n",
        at + 1,
        cartridge.name(),
        cartridge.describe()
    );
    let files = cartridge.catalogue();
    if files.is_empty() {
        out.push_str("Nothing on it.\n");
    } else {
        out.push_str("\nname        sectors  bytes\n");
        for file in &files {
            out.push_str(&format!(
                "{:<12}{:>7}{:>7}\n",
                file.name, file.sectors, file.bytes
            ));
        }
    }
    let bad = cartridge.bad_checksums();
    if bad.is_empty() {
        out.push_str("\nEvery sector adds up.\n");
    } else {
        out.push_str(&format!(
            "\n{} sector{} do not add up, which on a real cartridge means wear rather than \
             a mistake: {}\n",
            bad.len(),
            if bad.len() == 1 { "" } else { "s" },
            bad.iter()
                .take(12)
                .map(|(i, (h, d, r))| format!(
                    "{i}({}{}{})",
                    if *h { "" } else { "header " },
                    if *d { "" } else { "descriptor " },
                    if *r { "" } else { "data" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(out)
}
