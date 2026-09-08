//! Putting a cartridge in a microdrive, and what happens to it afterwards.
//!
//! The same three ways in as a disk, for the same reason: a program writes to
//! the cartridge it loaded from, and the first time that happens should not be
//! a surprise. Read-only, a copy, or the file itself — and a cartridge out of
//! a zip is offered the first two, since an archive is not a place to keep a
//! changing cartridge.

use eframe::egui;

use crate::if1::Drive;
use crate::microdrive::Cartridge;
use crate::prefs::FileKind;
use crate::ui::disk::{Mounted, Source};
use crate::ui::theme;
use crate::ui::App;

/// A cartridge read and waiting to be told what to do about writes.
pub struct Pending {
    pub source: Source,
    pub cartridge: Cartridge,
}

impl App {
    /// Read a cartridge and ask what should be done with it.
    pub fn open_cartridge(&mut self, path: &std::path::Path, drive: usize) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return self.set_status(format!("{}: {e}", path.display()), true),
        };
        // A zip with a cartridge in it is a cartridge: that is how they arrive.
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            let Some((inner, inner_bytes)) = crate::zip::first_with_extension(&bytes, &["mdr"])
            else {
                return self.set_status(format!("{} holds no cartridge", path.display()), true);
            };
            let source = Source::InArchive {
                archive: path.to_path_buf(),
                inner,
            };
            return self.open_cartridge_bytes(source, &inner_bytes, drive);
        }
        self.open_cartridge_bytes(Source::File(path.to_path_buf()), &bytes, drive);
    }

    /// The same, for a cartridge already read out of somewhere.
    pub fn open_cartridge_bytes(&mut self, source: Source, bytes: &[u8], drive: usize) {
        let name = source.name();
        match Cartridge::parse(bytes) {
            Ok(cartridge) => {
                self.prefs.remember_file(FileKind::Cartridge, source.path());
                self.set_status(format!("{name} — {}", cartridge.describe()), false);
                self.pending_cartridge = Some((Pending { source, cartridge }, drive));
                self.selected_drive = drive;
            }
            Err(e) => self.set_status(format!("{name}: {e}"), true),
        }
    }

    /// Put the waiting cartridge in, one of the three ways.
    pub fn mount_pending_cartridge(&mut self, how: Mounted, copy_to: Option<std::path::PathBuf>) {
        let Some((pending, drive)) = self.pending_cartridge.take() else {
            return;
        };
        let origin = Some(pending.source.path().to_path_buf());
        let (path, read_only) = match how {
            Mounted::ReadOnly => (origin, true),
            Mounted::InPlace if !pending.source.writable_in_place() => {
                self.set_status(
                    "A cartridge inside a zip cannot be written back into it: write to a \
                     copy instead."
                        .into(),
                    true,
                );
                self.pending_cartridge = Some((pending, drive));
                return;
            }
            Mounted::InPlace => (origin, false),
            Mounted::Copy => {
                let to = copy_to.unwrap_or_else(|| pending.source.copy_name_with("mdr"));
                if let Err(e) = std::fs::write(&to, pending.cartridge.to_bytes()) {
                    self.set_status(format!("Could not write {}: {e}", to.display()), true);
                    return;
                }
                self.prefs.remember_file(FileKind::Cartridge, &to);
                (Some(to), false)
            }
        };
        let name = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if let Some(if1) = self.spec.bus.if1.as_mut() {
            if let Some(slot) = if1.drives.get_mut(drive) {
                *slot = Drive::loaded(pending.cartridge, path, read_only);
            }
        }
        self.show_microdrive = true;
        self.set_status(
            format!("{name} in drive {} — {}", drive + 1, how.label()),
            false,
        );
    }

    /// A new cartridge, formatted and put in the selected drive. Writable: a
    /// cartridge somebody has just formatted is one they mean to write to.
    pub fn create_blank_cartridge(&mut self) {
        let dialog = rfd::FileDialog::new().set_file_name("cartridge.mdr");
        let dialog = match self.prefs.dir_for(FileKind::Cartridge) {
            Some(dir) if dir.is_dir() => dialog.set_directory(dir),
            _ => dialog,
        };
        let Some(path) = dialog.save_file() else {
            return;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "cartridge".into());
        // A real cartridge holds about 180 sectors; the tape is as long as it
        // is, and 254 is only the most the format can address.
        let cartridge = Cartridge::blank(&name, 180);
        if let Err(e) = std::fs::write(&path, cartridge.to_bytes()) {
            return self.set_status(format!("Could not write {}: {e}", path.display()), true);
        }
        self.prefs.remember_file(FileKind::Cartridge, &path);
        let what = cartridge.describe();
        let drive = self.selected_drive;
        if let Some(if1) = self.spec.bus.if1.as_mut() {
            if let Some(slot) = if1.drives.get_mut(drive) {
                *slot = Drive::loaded(cartridge, Some(path.clone()), false);
            }
        }
        self.show_microdrive = true;
        self.set_status(format!("{} — {what}, writable", path.display()), false);
    }

    /// Write the cartridge back if it has changed, and take it out.
    pub fn eject_cartridge(&mut self, drive: usize) {
        self.save_cartridge(drive);
        if let Some(if1) = self.spec.bus.if1.as_mut() {
            if let Some(slot) = if1.drives.get_mut(drive) {
                *slot = Drive::empty();
            }
        }
        self.set_status(format!("Drive {} empty", drive + 1), false);
    }

    /// Write a cartridge back to wherever its writes go.
    pub fn save_cartridge(&mut self, drive: usize) -> bool {
        let Some(if1) = self.spec.bus.if1.as_mut() else {
            return false;
        };
        let Some(slot) = if1.drives.get_mut(drive) else {
            return false;
        };
        if slot.read_only {
            return false;
        }
        let Some(cartridge) = slot.cartridge.as_mut() else {
            return false;
        };
        if !cartridge.dirty {
            return false;
        }
        let Some(path) = slot.path.clone() else {
            return false;
        };
        let bytes = cartridge.to_bytes();
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                cartridge.dirty = false;
                self.set_status(format!("Wrote {}", path.display()), false);
                true
            }
            Err(e) => {
                self.set_status(format!("Could not write {}: {e}", path.display()), true);
                false
            }
        }
    }

    /// Write every cartridge back that has changed — on the way out, and when
    /// the interface is unplugged.
    pub fn save_cartridges(&mut self) {
        let drives = self.spec.bus.if1.as_ref().map_or(0, |i| i.drive_count());
        for drive in 0..drives {
            self.save_cartridge(drive);
        }
    }

    /// The question a waiting cartridge asks.
    pub fn cartridge_prompt(&mut self, ctx: &egui::Context) {
        let Some((pending, drive)) = &self.pending_cartridge else {
            return;
        };
        let name = pending.source.name();
        let in_archive = !pending.source.writable_in_place();
        let what = pending.cartridge.describe();
        let protected = pending.cartridge.write_protected;
        let suggested = pending.source.copy_name_with("mdr");
        let copy_label = suggested
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "a copy".into());
        let drive = *drive;

        let mut chosen: Option<(Mounted, Option<std::path::PathBuf>)> = None;
        let mut cancelled = false;
        egui::Window::new("Insert cartridge")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{name} — {what}, into drive {}", drive + 1));
                ui.add_space(6.0);
                if protected {
                    ui.label(
                        egui::RichText::new(
                            "Its write-protect tab is broken off, so the machine cannot \
                             write to it whatever is chosen here.",
                        )
                        .small()
                        .color(theme::DIM),
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "A program writes to the cartridge it loaded from. How should \
                         writes be treated?",
                    )
                    .small()
                    .color(theme::DIM),
                );
                ui.add_space(8.0);
                if ui.button("Read-only").clicked() {
                    chosen = Some((Mounted::ReadOnly, None));
                }
                if ui
                    .button(format!("Write to a copy — {copy_label}"))
                    .clicked()
                {
                    chosen = Some((Mounted::Copy, Some(suggested.clone())));
                }
                if in_archive {
                    ui.label(
                        egui::RichText::new(
                            "It came out of a zip, so there is nowhere to write it back to: \
                             a copy, or nothing.",
                        )
                        .small()
                        .color(theme::DIM),
                    );
                } else if ui.button("Write to this file").clicked() {
                    chosen = Some((Mounted::InPlace, None));
                }
                ui.add_space(4.0);
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        if cancelled {
            self.pending_cartridge = None;
            self.set_status("No cartridge inserted".into(), false);
            return;
        }
        if let Some((how, to)) = chosen {
            self.mount_pending_cartridge(how, to);
        }
    }
}
