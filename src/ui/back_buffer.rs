//! Back-buffer detection and preview: watch a screen being built somewhere
//! other than video RAM before it is blitted across.

use eframe::egui;
use egui::{ColorImage, RichText, TextureHandle, TextureOptions};

use crate::screen;
use crate::tracker::Region;
use crate::ui::theme;
use crate::ui::App;

pub struct BackBufferState {
    pixels: Vec<u8>,
    tex: Option<TextureHandle>,
    pub manual_text: String,
    pub scale: f32,
}

impl Default for BackBufferState {
    fn default() -> Self {
        BackBufferState {
            pixels: vec![0; screen::View::OVERSCAN.buffer_len()],
            tex: None,
            manual_text: "8000".into(),
            scale: 1.5,
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.toggle_value(
        &mut app.spec.bus.tracker.detect_enabled,
        "Detect back buffers automatically",
    )
    .on_hover_text(
        "Looks for a long run of heavily written pages outside video RAM, \
         and scores them higher when their contents are later copied into the screen.",
    );

    let detected = app.spec.bus.tracker.detected;
    let confidence = app.spec.bus.tracker.detected_confidence;
    match detected {
        Some(r) => ui.label(
            RichText::new(format!(
                "Detected: ${:04X}–${:04X}  ({} bytes, confidence {:.0}%)",
                r.start,
                r.start as u32 + r.len as u32 - 1,
                r.len,
                confidence * 100.0
            ))
            .monospace(),
        ),
        None => ui.label(RichText::new("Detected: none yet").monospace()),
    };

    ui.horizontal(|ui| {
        ui.label("Manual override:");
        ui.add(
            egui::TextEdit::singleline(&mut app.back.manual_text)
                .desired_width(70.0)
                .hint_text("8000"),
        );
        if ui.button("Set").clicked() {
            if let Ok(a) =
                u16::from_str_radix(app.back.manual_text.trim().trim_start_matches('$'), 16)
            {
                app.spec.bus.tracker.manual = Some(Region {
                    start: a,
                    len: 6912,
                });
            }
        }
        if ui.button("Use detected").clicked() {
            app.spec.bus.tracker.manual = None;
        }
        if ui.button("Clear").clicked() {
            app.spec.bus.tracker.manual = None;
            app.spec.bus.tracker.detected = None;
            app.spec.bus.tracker.detected_confidence = 0.0;
        }
    });

    let region = app.spec.bus.tracker.back_buffer();
    ui.separator();

    ui.horizontal(|ui| {
        ui.toggle_value(&mut app.spec.bus.slow.enabled, "Slow draw");
        ui.toggle_value(
            &mut app.spec.bus.slow.watch_back_buffer,
            "watch back buffer",
        );
        ui.toggle_value(&mut app.spec.bus.slow.watch_screen, "watch video RAM");
        theme::slider(
            ui,
            egui::Slider::new(&mut app.spec.bus.slow.writes_per_slice, 1..=4096)
                .logarithmic(true)
                .text("writes/frame"),
        );
    });
    ui.small(
        "With slow draw on, the CPU is parked once it has written this many bytes \
         to the watched area, so you see the picture assemble one host frame at a time.",
    );

    ui.separator();
    match region {
        Some(r) => {
            ui.label(
                RichText::new(format!("Previewing ${:04X} as a 6912-byte screen", r.start))
                    .monospace(),
            );
            let view = app.view();
            if app.back.pixels.len() != view.buffer_len() {
                app.back.pixels = vec![0; view.buffer_len()];
                app.back.tex = None;
            }
            screen::render_from(
                &app.spec.bus,
                view,
                r.start,
                &mut app.back.pixels,
                (app.spec.bus.frame / 16) % 2 == 1,
                false,
            );
            let img =
                ColorImage::from_rgba_unmultiplied([view.width(), view.height()], &app.back.pixels);
            match &mut app.back.tex {
                Some(t) => t.set(img, TextureOptions::NEAREST),
                None => {
                    app.back.tex = Some(ui.ctx().load_texture(
                        "back-buffer",
                        img,
                        TextureOptions::NEAREST,
                    ))
                }
            }
            theme::slider(
                ui,
                egui::Slider::new(&mut app.back.scale, 0.5..=3.0).text("zoom"),
            );
            if let Some(tex) = &app.back.tex {
                let size = egui::vec2(
                    view.width() as f32 * app.back.scale,
                    view.height() as f32 * app.back.scale,
                );
                ui.image(egui::ImageSource::Texture(egui::load::SizedTexture::new(
                    tex.id(),
                    size,
                )));
            }
        }
        None => {
            ui.label(
                "No back buffer yet. Run a program that builds a screen in ordinary RAM \
                 and copies it into $4000, or set an address manually above.",
            );
        }
    }
}
