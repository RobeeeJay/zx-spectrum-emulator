//! The disk window: the drive as it looks from the front, what it has been
//! reading, and what is on the disk.
//!
//! Three parts under the controls, the same shape as the tape window: the
//! drive itself, then a map of the disk with what has been read and written
//! lit up over it, then the catalogue where the tape window lists blocks.

use eframe::egui;

use crate::disk::{Disk, Format};
use crate::fdc::Speed;
use crate::ui::theme;
use crate::ui::App;

/// How tall the picture of the drive is.
const DRIVE_H: f32 = 96.0;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    ui.add_space(6.0);
    drive(app, ui);
    ui.add_space(8.0);
    platter(app, ui);
    ui.add_space(8.0);
    catalogue(app, ui);
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Disk");
        if ui
            .button("Load…")
            .on_hover_text("Put a .dsk in drive A:")
            .clicked()
        {
            if let Some(path) = app.pick_file(Some(crate::prefs::FileKind::Disk)) {
                app.open_disk(&path);
            }
        }
        if ui
            .button("Blank…")
            .on_hover_text("Make a blank disk and put it in")
            .clicked()
        {
            app.create_blank_disk();
        }
        let has_disk = app.spec.bus.fdc.drives[0].is_some();
        if ui
            .add_enabled(has_disk, egui::Button::new("Eject"))
            .on_hover_text("Write it back if it has changed, and take it out")
            .clicked()
        {
            app.eject_disk();
        }
    });
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Speed");
        let speed = app.spec.bus.fdc.speed;
        if theme::selectable(ui, speed == Speed::Normal, "Normal")
            .on_hover_text(
                "The waits a real drive makes the program sit through: the motor coming \
                 up to speed, the head stepping, and the sector coming round under it.",
            )
            .clicked()
        {
            app.spec.bus.fdc.speed = Speed::Normal;
        }
        if theme::selectable(ui, speed == Speed::Fastload, "Fastload")
            .on_hover_text(
                "No waits at all: every answer is ready the moment it is asked for, so a \
                 disk load takes as long as the ROM's own code takes to run.",
            )
            .clicked()
        {
            app.spec.bus.fdc.speed = Speed::Fastload;
        }
    });
}

/// The front of the drive: the slot, the eject button, and the light.
///
/// The light is the one on a real +3 — green, above the slot — and it means
/// what that one means: on while the drive is being read or written, and off
/// the rest of the time. Whether there is a disk in is the slot, not the light.
fn drive(app: &mut App, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, DRIVE_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // The case.
    painter.rect_filled(rect, 6.0, theme::CASE_LIGHT);
    let face = rect.shrink(6.0);
    painter.rect_filled(face, 4.0, theme::CASE);

    // The slot, with a disk in it or without.
    let slot = egui::Rect::from_min_size(
        face.min + egui::vec2(18.0, 26.0),
        egui::vec2(face.width() - 100.0, 30.0),
    );
    painter.rect_filled(slot, 2.0, theme::CASE_DARK);
    let has_disk = app.spec.bus.fdc.drives[0].is_some();
    if has_disk {
        // The disk itself, sticking out of the slot a little, with its shutter
        // to one side.
        let disk = egui::Rect::from_min_size(
            slot.min + egui::vec2(3.0, 4.0),
            egui::vec2(slot.width() - 6.0, slot.height() - 8.0),
        );
        painter.rect_filled(disk, 2.0, theme::CONTROL_HOVER);
        let shutter = egui::Rect::from_min_size(
            disk.min + egui::vec2(disk.width() - 34.0, 3.0),
            egui::vec2(26.0, disk.height() - 6.0),
        );
        painter.rect_filled(shutter, 1.0, theme::DIM);
        let label = app.spec.bus.fdc.drives[0]
            .as_ref()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "disk".into());
        painter.text(
            disk.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            theme::CASE_DARK,
        );
    }

    // The eject button, to the right of the slot.
    let eject = egui::Rect::from_min_size(
        egui::pos2(slot.right() + 14.0, slot.center().y - 7.0),
        egui::vec2(30.0, 14.0),
    );
    painter.rect_filled(eject, 2.0, theme::CASE_LIGHT);

    // The light. Lit while the drive is working, and dark otherwise; a drive
    // with the motor on but nothing to do shows a dim glow, as one does.
    let lit = app.spec.bus.fdc.light();
    let motor = app.spec.bus.fdc.motor;
    let colour = if lit {
        theme::GREEN
    } else if motor {
        egui::Color32::from_rgb(0x0a, 0x4a, 0x22)
    } else {
        egui::Color32::from_rgb(0x10, 0x22, 0x16)
    };
    let led = egui::pos2(slot.left() + 8.0, face.top() + 12.0);
    painter.circle_filled(led, 5.0, colour);
    if lit {
        // A little bloom, so a flash is visible out of the corner of the eye.
        painter.circle_filled(led, 9.0, colour.gamma_multiply(0.25));
    }
    painter.text(
        led + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        match (has_disk, motor) {
            (false, _) => "no disk".to_string(),
            (true, false) => "disk in, motor off".to_string(),
            (true, true) => "reading".to_string(),
        },
        egui::FontId::proportional(11.0),
        theme::DIM,
    );

    // What the disk is, and how it went in.
    if let (Some(drive), Some(how)) = (&app.spec.bus.fdc.drives[0], app.disk_mounted) {
        let dirty = if drive.disk.dirty { " • changed" } else { "" };
        painter.text(
            egui::pos2(face.left() + 8.0, face.bottom() - 10.0),
            egui::Align2::LEFT_CENTER,
            format!("{} — {}{dirty}", drive.disk.describe(), how.label()),
            egui::FontId::proportional(11.0),
            theme::DIM,
        );
    }
}

/// The disk as a disk: rings of tracks with the bits on them, and what the
/// machine has been reading and writing lit over the top.
///
/// Track 0 is the outermost ring, where it is on a real disk — which is why an
/// empty disk has a bright band at the edge, the directory, and nothing behind
/// it. Reads light green and writes amber over the sector they touched, and
/// both fade, so a load draws itself round the disk as it happens.
fn platter(app: &mut App, ui: &mut egui::Ui) {
    let Some(drive) = app.spec.bus.fdc.drives[0].as_ref() else {
        ui.label(
            egui::RichText::new("No disk in the drive.")
                .small()
                .color(theme::DIM),
        );
        return;
    };
    let tracks = drive.disk.tracks_per_side.max(1) as usize;
    let sectors = drive
        .disk
        .tracks
        .iter()
        .map(|t| t.sectors.len())
        .max()
        .unwrap_or(9)
        .max(1);

    let side = ui.available_width().min(360.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), side), egui::Sense::hover());
    let square = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));

    // The bits, rasterised once and kept until the disk changes.
    let pixels = (side * ui.ctx().pixels_per_point()).round().max(64.0) as usize;
    let texture = {
        let disk = &app.spec.bus.fdc.drives[0]
            .as_ref()
            .expect("just checked")
            .disk;
        app.platter
            .texture(ui.ctx(), disk, pixels.min(1024))
            .clone()
    };
    let painter = ui.painter_at(rect);
    painter.image(
        texture.id(),
        square,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    let centre = square.center();
    let half = square.width() / 2.0;
    // A wedge of the disk, drawn as a filled polygon: the two radii and the
    // arc between them, which at this size is a handful of points.
    let wedge = |track: usize, sector: usize, colour: egui::Color32| {
        let (angles, radii) = crate::ui::diskface::sector_wedge(tracks, track, sector, sectors);
        let steps = 6;
        let mut points = Vec::with_capacity(steps * 2 + 2);
        for i in 0..=steps {
            let a = angles.start + (angles.end - angles.start) * i as f32 / steps as f32;
            points.push(centre + egui::vec2(a.cos(), a.sin()) * radii.end * half);
        }
        for i in (0..=steps).rev() {
            let a = angles.start + (angles.end - angles.start) * i as f32 / steps as f32;
            points.push(centre + egui::vec2(a.cos(), a.sin()) * radii.start * half);
        }
        painter.add(egui::Shape::convex_polygon(
            points,
            colour,
            egui::Stroke::NONE,
        ));
    };

    // What has been touched lately. A write counts for more than a read: it
    // changed the disk.
    let fdc = &app.spec.bus.fdc;
    let sector_index = |track: u8, r: u8| -> Option<usize> {
        fdc.drives[0]
            .as_ref()?
            .disk
            .track(track, 0)?
            .sectors
            .iter()
            .position(|s| s.r == r)
    };
    let mut lit: Vec<(usize, usize, egui::Color32)> = Vec::new();
    for ((track, side_no, r), heat) in &fdc.reads {
        if *side_no != 0 {
            continue;
        }
        if let Some(index) = sector_index(*track, *r) {
            lit.push((
                *track as usize,
                index,
                theme::LCD_FG.gamma_multiply(*heat as f32 / 255.0 * 0.85),
            ));
        }
    }
    for ((track, side_no, r), heat) in &fdc.writes {
        if *side_no != 0 {
            continue;
        }
        if let Some(index) = sector_index(*track, *r) {
            lit.push((
                *track as usize,
                index,
                theme::AMBER.gamma_multiply(*heat as f32 / 255.0 * 0.9),
            ));
        }
    }
    for (track, sector, colour) in lit {
        wedge(track, sector, colour);
    }

    // Where the head is: the track it is over, all the way round.
    let head = app.spec.bus.fdc.head_at(0) as usize;
    if head < tracks {
        let (_, radii) = crate::ui::diskface::sector_wedge(tracks, head, 0, sectors);
        let middle = (radii.start + radii.end) / 2.0 * half;
        painter.circle_stroke(
            centre,
            middle,
            egui::Stroke::new(
                1.0,
                theme::AMBER.gamma_multiply(if app.spec.bus.fdc.motor { 0.55 } else { 0.3 }),
            ),
        );
    }

    response.on_hover_text(
        "The disk as it is written: a ring per track with track 0 outermost, and the bits \
         of each track round it — white for a one, black for a nought, one bit sampled per \
         step round the ring rather than all 36,864 of them. Green is a sector just read, \
         amber one just written, and the ring is where the head is.",
    );
}

/// What is on the disk, where the tape window lists its blocks.
fn catalogue(app: &mut App, ui: &mut egui::Ui) {
    let Some(drive) = app.spec.bus.fdc.drives[0].as_ref() else {
        return;
    };
    let format = drive.disk.format();
    match format {
        Format::Other => {
            ui.label(
                egui::RichText::new(
                    "Not a +3 format disk — its sectors are numbered some other way, so \
                     there is no catalogue to read. The sector map above is what there is.",
                )
                .small()
                .color(theme::DIM),
            );
            return;
        }
        Format::System => {
            theme::group_label(ui, "Catalogue (system format)");
        }
        Format::Data => {
            theme::group_label(ui, "Catalogue");
        }
    }
    let Some(files) = drive.disk.catalogue() else {
        return;
    };
    let free = drive.disk.free_kilobytes();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if files.is_empty() {
                ui.label(
                    egui::RichText::new("No files found")
                        .small()
                        .color(theme::DIM),
                );
            }
            for file in &files {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&file.name).monospace());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut marks = String::new();
                        if file.read_only {
                            marks.push_str(" read-only");
                        }
                        if file.system {
                            marks.push_str(" hidden");
                        }
                        ui.label(
                            egui::RichText::new(format!("{}K{marks}", file.kilobytes))
                                .monospace()
                                .color(theme::DIM),
                        );
                    });
                });
            }
            if let Some(free) = free {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("{free}K free"))
                        .small()
                        .color(theme::DIM),
                );
            }
        });
}

/// Whether the window has anything to draw about: a machine with a drive.
pub fn available(app: &App) -> bool {
    app.spec.bus.model.has_disk() && !app.on_zx81()
}

/// The disk in the drive, if there is one, for the tests to look at.
pub fn mounted(app: &App) -> Option<&Disk> {
    app.spec.bus.fdc.drives[0].as_ref().map(|d| &d.disk)
}
