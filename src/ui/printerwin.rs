//! The printer window: the paper, as it comes out of the printer.
//!
//! The newest line is at the bottom, where the paper leaves the printer, and
//! the printout grows upwards from there — the view stays on the bottom while
//! it prints, unless it has been scrolled up by hand to read something.

use eframe::egui;

use crate::hardware::Peripheral;
use crate::printer::{self, Paper, DOTS, LINE_BYTES};
use crate::ui::{theme, App};

/// Lines to a texture. The printout is uploaded in pieces so that a new line
/// re-renders the last piece rather than everything printed so far.
const CHUNK: usize = 256;
/// The printer's mouth under the paper.
const SLOT_H: f32 = 28.0;
/// A ZX Printer's paper is 4 inches wide and its 256 dots are square.
const MM_PER_LINE: f32 = 101.6 / DOTS as f32;

/// What has been uploaded, so a frame with nothing new uploads nothing.
#[derive(Default)]
pub struct View {
    chunks: Vec<(egui::TextureHandle, usize)>,
    paper: Option<Paper>,
}

impl View {
    fn refresh(&mut self, ctx: &egui::Context, lines: &[[u8; LINE_BYTES]], paper: Paper) {
        let uploaded: usize = self.chunks.iter().map(|(_, n)| n).sum();
        // A different paper redraws everything; so does a printout that has
        // got shorter, which is one that was torn off.
        if self.paper != Some(paper) || uploaded > lines.len() {
            self.chunks.clear();
            self.paper = Some(paper);
        }
        // Heat blurs a thermal print a little; a spark does not.
        let options = match paper {
            Paper::Metallised => egui::TextureOptions::NEAREST,
            Paper::Thermal => egui::TextureOptions::LINEAR,
        };
        for (k, part) in lines.chunks(CHUNK).enumerate() {
            let image = || {
                egui::ColorImage::from_rgba_unmultiplied(
                    [DOTS, part.len()],
                    &printer::render(part, k * CHUNK, paper),
                )
            };
            match self.chunks.get_mut(k) {
                Some((_, n)) if *n == part.len() => {}
                Some((tex, n)) => {
                    tex.set(image(), options);
                    *n = part.len();
                }
                None => self.chunks.push((
                    ctx.load_texture(format!("printout-{k}"), image(), options),
                    part.len(),
                )),
            }
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(printer) = app.spec.bus.printer.as_ref() else {
        ui.label(
            "No printer is fitted: fit a ZX Printer or an Alphacom 32 in the Hardware window.",
        );
        return;
    };
    let lines = printer.lines.len();
    let running = printer.running();
    let mut paper = printer.paper;
    let alphacom = app.spec.bus.hardware.fitted(Peripheral::Alphacom32);

    ui.horizontal(|ui| {
        theme::group_label(ui, "Printout");
        if ui
            .add_enabled(lines > 0, egui::Button::new("Save PNG…"))
            .on_hover_text("Write the printout to a PNG, a pixel a dot, on the paper shown")
            .clicked()
        {
            save(app);
        }
        if ui
            .add_enabled(lines > 0, egui::Button::new("Tear off"))
            .on_hover_text("Tear the printout off and start again on fresh paper")
            .clicked()
        {
            if let Some(printer) = app.spec.bus.printer.as_mut() {
                printer.lines.clear();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.horizontal(|ui| paper_switch(ui, &mut paper));
        });
    });
    if let Some(printer) = app.spec.bus.printer.as_mut() {
        printer.paper = paper;
    }
    ui.label(
        egui::RichText::new(format!(
            "{lines} line{}, {:.1} cm of paper{}",
            if lines == 1 { "" } else { "s" },
            lines as f32 * MM_PER_LINE / 10.0,
            if running { " — printing" } else { "" }
        ))
        .small()
        .color(theme::DIM),
    );

    let Some(printer) = app.spec.bus.printer.as_ref() else {
        return;
    };
    app.printer_view.refresh(ui.ctx(), &printer.lines, paper);

    let view_h = (ui.available_height() - SLOT_H).max(40.0);
    egui::ScrollArea::vertical()
        .max_height(view_h)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let scale = ((ui.available_width() - 24.0) / DOTS as f32)
                .floor()
                .max(1.0);
            let paper_w = DOTS as f32 * scale;
            let paper_h = lines as f32 * scale;
            // Short of a window's worth, the paper hangs from the printer
            // rather than starting at the top.
            ui.add_space((view_h - paper_h).max(0.0));
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), paper_h.max(1.0)),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            let left = rect.center().x - paper_w / 2.0;
            if lines == 0 {
                return;
            }
            let sheet = egui::Rect::from_min_size(
                egui::pos2(left, rect.top()),
                egui::vec2(paper_w, paper_h),
            );
            painter.rect_filled(
                sheet.translate(egui::vec2(3.0, 0.0)),
                0.0,
                egui::Color32::from_black_alpha(90),
            );
            for (k, (tex, n)) in app.printer_view.chunks.iter().enumerate() {
                let top = rect.top() + (k * CHUNK) as f32 * scale;
                let piece = egui::Rect::from_min_size(
                    egui::pos2(left, top),
                    egui::vec2(paper_w, *n as f32 * scale),
                );
                painter.image(
                    tex.id(),
                    piece,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        });

    // The printer's mouth, which the paper comes out of.
    let (slot, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SLOT_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(slot);
    painter.rect_filled(slot, 3.0, theme::CASE_DARK);
    let lip = egui::Rect::from_min_size(
        egui::pos2(slot.center().x - DOTS as f32 * 1.1, slot.top() + 4.0),
        egui::vec2(DOTS as f32 * 2.2, 5.0),
    );
    painter.rect_filled(lip, 2.0, theme::EDGE);
    painter.text(
        egui::pos2(slot.right() - 8.0, slot.bottom() - 5.0),
        egui::Align2::RIGHT_BOTTOM,
        if alphacom {
            "ALPHACOM 32"
        } else {
            "ZX PRINTER"
        },
        egui::FontId::proportional(10.0),
        theme::DIM,
    );
    if lines == 0 {
        painter.text(
            egui::pos2(slot.left() + 8.0, slot.bottom() - 5.0),
            egui::Align2::LEFT_BOTTOM,
            "Nothing printed yet: COPY, LPRINT and LLIST print here",
            egui::FontId::proportional(10.0),
            theme::DIM,
        );
    }
}

/// A two-way switch with a knob that slides: silver paper one way, thermal the
/// other. It looks like a switch because it is one — the paper is a physical
/// thing the printer was loaded with.
fn paper_switch(ui: &mut egui::Ui, paper: &mut Paper) -> egui::Response {
    let thermal = *paper == Paper::Thermal;
    let label = |ui: &mut egui::Ui, text: &str, lit: bool| {
        ui.label(egui::RichText::new(text).small().color(if lit {
            theme::INK
        } else {
            theme::DIM
        }));
    };
    label(ui, "Silver", !thermal);
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(48.0, 22.0), egui::Sense::click());
    label(ui, "Thermal", thermal);
    if response.clicked() {
        *paper = if thermal {
            Paper::Metallised
        } else {
            Paper::Thermal
        };
        response.mark_changed();
    }
    let on = *paper == Paper::Thermal;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, "Thermal paper")
    });
    let response = response.on_hover_text(
        "The paper in the printer: the ZX Printer's silver metallised roll, or the \
         thermal paper an Alphacom 32 took, faded and uneven the way it went",
    );

    let t = ui.ctx().animate_bool_responsive(response.id, on);
    let painter = ui.painter();
    // The bezel and the track the knob runs in.
    painter.rect_filled(rect, 5.0, theme::EDGE);
    let track = rect.shrink(2.0);
    painter.rect_filled(track, 4.0, theme::CASE_DARK);
    painter.line_segment(
        [
            track.left_top() + egui::vec2(4.0, 1.0),
            track.right_top() + egui::vec2(-4.0, 1.0),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(170)),
    );
    // A lamp at the thermal end, which the knob uncovers.
    let lamp = egui::pos2(track.left() + 7.0, track.center().y);
    let lit = theme::AMBER.gamma_multiply(0.25 + 0.75 * t);
    painter.circle_filled(lamp, 3.0, lit);
    if t > 0.5 {
        painter.circle_filled(lamp, 6.0, theme::AMBER.gamma_multiply(0.18 * t));
    }
    // The knob: shadow, body, a lit top edge and three grip ridges.
    let knob_w = track.width() * 0.55;
    let x = egui::lerp(track.left()..=track.right() - knob_w, t);
    let knob = egui::Rect::from_min_size(
        egui::pos2(x, track.top() + 1.0),
        egui::vec2(knob_w, track.height() - 2.0),
    );
    painter.rect_filled(
        knob.translate(egui::vec2(0.0, 1.5)),
        3.0,
        egui::Color32::from_black_alpha(140),
    );
    painter.rect_filled(knob, 3.0, egui::Color32::from_rgb(0x55, 0x50, 0x49));
    let upper = egui::Rect::from_min_max(
        knob.min + egui::vec2(1.0, 1.0),
        egui::pos2(knob.max.x - 1.0, knob.center().y),
    );
    painter.rect_filled(upper, 2.0, egui::Color32::from_rgb(0x72, 0x6c, 0x63));
    for i in -1..=1 {
        let cx = knob.center().x + i as f32 * 4.0;
        painter.line_segment(
            [
                egui::pos2(cx, knob.top() + 4.0),
                egui::pos2(cx, knob.bottom() - 4.0),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(0x36, 0x32, 0x2d)),
        );
    }
    painter.line_segment(
        [
            knob.left_top() + egui::vec2(2.0, 0.5),
            knob.right_top() + egui::vec2(-2.0, 0.5),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(70)),
    );
    response
}

/// Write the printout to a PNG, a pixel a dot, on the paper it is shown on.
fn save(app: &mut App) {
    let Some(printer) = app.spec.bus.printer.as_ref() else {
        return;
    };
    let (w, h) = (DOTS, printer.lines.len());
    let rgba = printer::render(&printer.lines, 0, printer.paper);
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .set_file_name("printout.png")
        .save_file()
    else {
        app.set_status("Printout not saved".into(), false);
        return;
    };
    let written = crate::mcp::picture::encode(&rgba, w, h)
        .and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()));
    match written {
        Ok(()) => app.set_status(format!("Wrote {} ({w}×{h})", path.display()), false),
        Err(e) => app.set_status(format!("Could not write {}: {e}", path.display()), true),
    }
}
