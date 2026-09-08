//! The Microdrive window: the drives on the Interface 1's chain.
//!
//! Built like the tape and disk windows and fixed to the same width. The
//! cartridge is drawn as a cartridge with its lamp, the tape loop under it
//! shows which sector is under the head and what has been read or written, and
//! where the tape window lists blocks this one lists the files on the
//! cartridge.

use eframe::egui;

use crate::hardware::Peripheral;
use crate::microdrive::Cartridge;
use crate::prefs::FileKind;
use crate::ui::theme;
use crate::ui::App;

/// How tall the picture of a cartridge is.
const CARTRIDGE_H: f32 = 74.0;

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if !app.spec.bus.hardware.fitted(Peripheral::Interface1) {
        ui.label(
            egui::RichText::new(
                "No Interface 1 is fitted. The microdrives hang off it, and everything they \
                 do is done by its ROM — the Hardware window is where it is plugged in.",
            )
            .small()
            .color(theme::DIM),
        );
        return;
    }
    controls(app, ui);
    ui.add_space(6.0);

    let drives = app.spec.bus.if1.as_ref().map_or(0, |i| i.drive_count());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for drive in 0..drives {
                cartridge(app, ui, drive);
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            catalogue(app, ui);
        });
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Cartridge");
        if ui
            .button("Load…")
            .on_hover_text("Put an .mdr — or a .zip with one inside — in a drive")
            .clicked()
        {
            if let Some(path) = app.pick_file(Some(FileKind::Cartridge)) {
                app.open_cartridge(&path, app.selected_drive);
            }
        }
        if ui
            .button("Blank…")
            .on_hover_text("Format a new cartridge and put it in")
            .clicked()
        {
            app.create_blank_cartridge();
        }
        let loaded = app
            .spec
            .bus
            .if1
            .as_ref()
            .and_then(|i| i.drives.get(app.selected_drive))
            .is_some_and(|d| d.cartridge.is_some());
        if ui
            .add_enabled(loaded, egui::Button::new("Eject"))
            .on_hover_text("Write it back if it has changed, and take it out")
            .clicked()
        {
            app.eject_cartridge(app.selected_drive);
        }
    });
    let drives = app.spec.bus.if1.as_ref().map_or(1, |i| i.drive_count());
    if drives > 1 {
        ui.horizontal_wrapped(|ui| {
            theme::group_label(ui, "Drive");
            for drive in 0..drives {
                if theme::selectable(ui, app.selected_drive == drive, &format!("{}", drive + 1))
                    .on_hover_text("Which drive Load, Blank and Eject act on")
                    .clicked()
                {
                    app.selected_drive = drive;
                }
            }
        });
    }
}

/// One drive: the cartridge in it, its lamp, and the loop of tape.
fn cartridge(app: &mut App, ui: &mut egui::Ui, drive: usize) {
    let Some(if1) = app.spec.bus.if1.as_ref() else {
        return;
    };
    let Some(state) = if1.drives.get(drive) else {
        return;
    };
    let running = if1.motor_on() && if1.selected == drive + 1;
    let position = state.sector();
    let held = state.cartridge.as_ref();
    let name = held.map(|c| c.name()).unwrap_or_default();
    let what = held.map(|c| c.describe()).unwrap_or_default();
    let sectors = held.map(|c| c.sectors.len()).unwrap_or(0);
    let writable = state.writable();
    let file = state
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    let changed = held.is_some_and(|c| c.dirty);

    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, CARTRIDGE_H), egui::Sense::click());
    let painter = ui.painter_at(rect);

    // The drive, and the cartridge sticking out of it.
    painter.rect_filled(rect, 5.0, theme::CASE_LIGHT);
    let face = rect.shrink(5.0);
    painter.rect_filled(face, 3.0, theme::CASE);

    // The lamp: on while this drive's tape is running.
    let lamp = egui::pos2(face.left() + 14.0, face.top() + 13.0);
    painter.circle_filled(
        lamp,
        5.0,
        if running {
            theme::RED
        } else {
            egui::Color32::from_rgb(0x3a, 0x14, 0x14)
        },
    );
    if running {
        painter.circle_filled(lamp, 9.0, theme::RED.gamma_multiply(0.25));
    }
    painter.text(
        lamp + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!(
            "Drive {}{}",
            drive + 1,
            match (held.is_some(), running) {
                (false, _) => " — empty".to_string(),
                (true, true) => format!(" — running, sector {position}"),
                (true, false) => String::new(),
            }
        ),
        egui::FontId::proportional(11.0),
        theme::INK,
    );

    if let Some(cartridge) = held {
        // The cartridge's own label, written the way the tape's is.
        let label = egui::Rect::from_min_size(
            egui::pos2(face.left() + 10.0, face.top() + 26.0),
            egui::vec2(face.width() - 20.0, 20.0),
        );
        painter.rect_filled(label, 2.0, theme::CASE_DARK);
        crate::ui::cassette::written_on(
            &painter,
            egui::pos2(label.center().x, label.bottom() - 4.0),
            label.width() - 12.0,
            16.0,
            if name.is_empty() { "unnamed" } else { &name },
            theme::WHITE,
        );

        // The loop of tape: a mark per sector, with the one under the head
        // lit. A cartridge is a loop, so it is drawn as one long strip that
        // wraps rather than as a reel.
        let loop_rect = egui::Rect::from_min_size(
            egui::pos2(face.left() + 10.0, label.bottom() + 5.0),
            egui::vec2(face.width() - 20.0, 8.0),
        );
        painter.rect_filled(loop_rect, 1.0, theme::LCD_BG);
        let step = loop_rect.width() / sectors.max(1) as f32;
        for (i, sector) in cartridge.sectors.iter().enumerate() {
            let at = egui::Rect::from_min_size(
                egui::pos2(loop_rect.left() + i as f32 * step, loop_rect.top()),
                egui::vec2((step - 0.5).max(0.5), loop_rect.height()),
            );
            let colour = if i == position && running {
                theme::AMBER
            } else if sector.in_use() {
                theme::LCD_FG.gamma_multiply(0.7)
            } else {
                theme::LCD_GRID
            };
            painter.rect_filled(at, 0.0, colour);
        }

        painter.text(
            egui::pos2(face.left() + 10.0, face.bottom() - 8.0),
            egui::Align2::LEFT_CENTER,
            format!(
                "{what}{}{}",
                if writable { "" } else { ", read-only" },
                if changed { " • changed" } else { "" }
            ),
            egui::FontId::proportional(10.0),
            theme::DIM,
        );
        if let Some(file) = file {
            painter.text(
                egui::pos2(face.right() - 10.0, face.bottom() - 8.0),
                egui::Align2::RIGHT_CENTER,
                file,
                egui::FontId::proportional(10.0),
                theme::DIM,
            );
        }
    }

    if response.clicked() {
        app.selected_drive = drive;
    }
    response.on_hover_text(
        "The lamp is on while this drive's tape is running. Under the label is the loop of \
         tape, a mark per sector: lit where a file is, amber where the head is.",
    );
}

/// What is on the cartridge in the selected drive.
fn catalogue(app: &mut App, ui: &mut egui::Ui) {
    let Some(cartridge) = app
        .spec
        .bus
        .if1
        .as_ref()
        .and_then(|i| i.drives.get(app.selected_drive))
        .and_then(|d| d.cartridge.as_ref())
    else {
        return;
    };
    theme::group_label(ui, "Catalogue");
    let files = cartridge.catalogue();
    if files.is_empty() {
        ui.label(
            egui::RichText::new("Nothing on it")
                .small()
                .color(theme::DIM),
        );
    }
    for file in &files {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&file.name).monospace());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{:>4} sectors  {:>6} bytes",
                        file.sectors, file.bytes
                    ))
                    .monospace()
                    .color(theme::DIM),
                );
            });
        });
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!("{} sectors free", cartridge.free_sectors()))
            .small()
            .color(theme::DIM),
    );
    // A cartridge that has been sitting in a drawer for forty years may well
    // have a sector that does not add up, and that is worth saying.
    let bad = cartridge.bad_checksums();
    if !bad.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "{} sector{} do not add up — the tape's own wear, most likely",
                bad.len(),
                if bad.len() == 1 { "" } else { "s" }
            ))
            .small()
            .color(theme::AMBER),
        );
    }
}

/// The cartridge in a drive, for the tests.
pub fn mounted(app: &App, drive: usize) -> Option<&Cartridge> {
    app.spec
        .bus
        .if1
        .as_ref()
        .and_then(|i| i.drives.get(drive))
        .and_then(|d| d.cartridge.as_ref())
}
