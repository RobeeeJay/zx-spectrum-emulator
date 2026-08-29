//! Putting a disk in the +3, and what happens to it afterwards.
//!
//! Three ways in, and the difference between them is what a write does:
//!
//! - **read-only**, where the machine is told the disk is write-protected and
//!   the file is never touched;
//! - **a copy**, where the image is written to a new file and the original is
//!   left as it was — which is what you want for a game you would rather not
//!   damage;
//! - **the file itself**, for a work disk somebody means to change.
//!
//! Writable is not the default for a disk that came off the disc: a game
//! writes its high scores to the disk it loaded from, and the first time that
//! happens should not be a surprise. The choice is asked for, in the window,
//! rather than guessed at.

use eframe::egui;

use crate::disk::Disk;
use crate::fdc::Drive;
use crate::prefs::FileKind;
use crate::ui::theme;
use crate::ui::App;

/// What a disk waiting to go in is waiting for.
pub struct Pending {
    pub path: std::path::PathBuf,
    pub disk: Disk,
}

/// How a disk was put in, which is what the window says under the drive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mounted {
    ReadOnly,
    /// Writes go to a copy; the original is untouched.
    Copy,
    /// Writes go to the file it came from.
    InPlace,
}

impl Mounted {
    pub fn label(&self) -> &'static str {
        match self {
            Mounted::ReadOnly => "read-only",
            Mounted::Copy => "writing to a copy",
            Mounted::InPlace => "writable",
        }
    }
}

impl App {
    /// Read a disk image and ask what should be done with it.
    ///
    /// Nothing is mounted here: what the machine gets depends on the answer,
    /// and asking afterwards would mean a write could have happened first.
    pub fn open_disk(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return self.set_status(format!("{}: {e}", path.display()), true),
        };
        match Disk::parse(&bytes) {
            Ok(disk) => {
                self.prefs.remember_file(FileKind::Disk, path);
                self.set_status(format!("{} — {}", path.display(), disk.describe()), false);
                self.pending_disk = Some(Pending {
                    path: path.to_path_buf(),
                    disk,
                });
            }
            Err(e) => self.set_status(format!("{}: {e}", path.display()), true),
        }
    }

    /// Put the waiting disk in, one of the three ways.
    pub fn mount_pending(&mut self, how: Mounted, copy_to: Option<std::path::PathBuf>) {
        let Some(pending) = self.pending_disk.take() else {
            return;
        };
        let (path, protected) = match how {
            Mounted::ReadOnly => (Some(pending.path.clone()), true),
            Mounted::InPlace => (Some(pending.path.clone()), false),
            Mounted::Copy => {
                let to = match copy_to {
                    Some(to) => to,
                    None => copy_name(&pending.path),
                };
                // Written now rather than at the first write: a copy that does
                // not exist until something changes is a copy somebody cannot
                // find.
                if let Err(e) = std::fs::write(&to, pending.disk.to_bytes()) {
                    self.set_status(format!("Could not write {}: {e}", to.display()), true);
                    return;
                }
                self.prefs.remember_file(FileKind::Disk, &to);
                (Some(to), false)
            }
        };
        let name = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.spec.bus.fdc.drives[0] = Some(Drive::new(pending.disk, path, protected));
        self.disk_mounted = Some(how);
        // The window that shows the drive, since a disk going in is the moment
        // somebody wants to watch it.
        self.show_disk = true;
        self.set_status(format!("{name} — {}", how.label()), false);
    }

    /// A new disk, formatted as the machine formats one, written to a file the
    /// user names. Writable: a disk somebody has just made is one they mean to
    /// write to.
    pub fn new_disk(&mut self, path: &std::path::Path) {
        let disk = Disk::blank("zx-rustrum");
        if let Err(e) = std::fs::write(path, disk.to_bytes()) {
            return self.set_status(format!("Could not write {}: {e}", path.display()), true);
        }
        self.prefs.remember_file(FileKind::Disk, path);
        let what = disk.describe();
        self.spec.bus.fdc.drives[0] = Some(Drive::new(disk, Some(path.to_path_buf()), false));
        self.disk_mounted = Some(Mounted::InPlace);
        self.show_disk = true;
        self.pending_disk = None;
        self.set_status(
            format!("{} — a blank disk, {what}, writable", path.display()),
            false,
        );
    }

    /// Write the disk out if anything has changed, and take it out of the
    /// drive.
    pub fn eject_disk(&mut self) {
        self.save_disk();
        self.spec.bus.fdc.drives[0] = None;
        self.disk_mounted = None;
        self.set_status("Disk ejected".into(), false);
    }

    /// Write the disk back to wherever its writes go, if anything has changed.
    ///
    /// A read-only disk has nowhere to go and is left alone; the machine was
    /// told it was write-protected, so nothing should have changed anyway.
    pub fn save_disk(&mut self) -> bool {
        let Some(drive) = self.spec.bus.fdc.drives[0].as_mut() else {
            return false;
        };
        if !drive.disk.dirty || drive.write_protected {
            return false;
        }
        let Some(path) = drive.path.clone() else {
            return false;
        };
        let bytes = drive.disk.to_bytes();
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                drive.disk.dirty = false;
                self.set_status(format!("Wrote {}", path.display()), false);
                true
            }
            Err(e) => {
                self.set_status(format!("Could not write {}: {e}", path.display()), true);
                false
            }
        }
    }

    /// Make a blank disk, from the File menu: ask where it goes, bring up a
    /// machine that has a drive if the one running has not, and put it in.
    pub fn create_blank_disk(&mut self) {
        let Some(path) = self.pick_new_disk() else {
            return;
        };
        if self.on_zx81() || !self.spec.bus.model.has_disk() {
            self.switch_model(crate::machine::Model::Plus3);
        }
        if !self.spec.bus.model.has_disk() {
            return;
        }
        self.new_disk(&path);
        self.show_disk = true;
    }

    /// Where a new blank disk should go.
    pub fn pick_new_disk(&self) -> Option<std::path::PathBuf> {
        let dialog = rfd::FileDialog::new().set_file_name("new disk.dsk");
        let dialog = match self.prefs.dir_for(FileKind::Disk) {
            Some(dir) if dir.is_dir() => dialog.set_directory(dir),
            _ => dialog,
        };
        dialog.save_file()
    }

    /// The question a disk waiting to go in asks: how should writes be
    /// treated? Drawn over the window, because it has to be answered before
    /// the machine can have the disk.
    pub fn disk_prompt(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_disk else {
            return;
        };
        let name = pending
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| pending.path.display().to_string());
        let what = pending.disk.describe();
        let suggested = copy_name(&pending.path);
        let copy_label = suggested
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "a copy".into());

        let mut chosen: Option<(Mounted, Option<std::path::PathBuf>)> = None;
        let mut cancelled = false;
        egui::Window::new("Insert disk")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{name} — {what}"));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "A game writes its high scores to the disk it loaded from. \
                         How should writes be treated?",
                    )
                    .small()
                    .color(theme::DIM),
                );
                ui.add_space(8.0);
                if ui
                    .button("Read-only")
                    .on_hover_text(
                        "The machine is told the disk is write-protected. The file is \
                         never touched.",
                    )
                    .clicked()
                {
                    chosen = Some((Mounted::ReadOnly, None));
                }
                if ui
                    .button(format!("Write to a copy — {copy_label}"))
                    .on_hover_text(
                        "The image is copied now and writes go to the copy. The \
                         original is left as it is.",
                    )
                    .clicked()
                {
                    chosen = Some((Mounted::Copy, Some(suggested.clone())));
                }
                if ui
                    .button("Write to a copy…")
                    .on_hover_text("The same, with somewhere else to put the copy")
                    .clicked()
                {
                    let dialog = rfd::FileDialog::new().set_file_name(copy_label.clone());
                    let dialog = match suggested.parent().filter(|d| d.is_dir()) {
                        Some(dir) => dialog.set_directory(dir),
                        None => dialog,
                    };
                    if let Some(to) = dialog.save_file() {
                        chosen = Some((Mounted::Copy, Some(to)));
                    }
                }
                if ui
                    .button("Write to this file")
                    .on_hover_text("Writes go to the disk image itself, as they would to a disk")
                    .clicked()
                {
                    chosen = Some((Mounted::InPlace, None));
                }
                ui.add_space(4.0);
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        if cancelled {
            self.pending_disk = None;
            self.set_status("No disk inserted".into(), false);
            return;
        }
        if let Some((how, to)) = chosen {
            self.mount_pending(how, to);
        }
    }
}

/// What a copy of a disk is called: the same name with " (copy)" on it, beside
/// the original, and a number after that if one is already there.
pub fn copy_name(path: &std::path::Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "disk".into());
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut candidate = dir.join(format!("{stem} (copy).dsk"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} (copy {n}).dsk"));
        n += 1;
    }
    candidate
}
