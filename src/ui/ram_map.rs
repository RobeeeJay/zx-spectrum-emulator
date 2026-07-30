//! Live map of the whole 64K address space: one pixel per byte, green for
//! reads, red for writes, fading every frame.

use eframe::egui;
use egui::{Color32, ColorImage, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::tracker::{Region, SCREEN_END, SCREEN_START};
use crate::ui::App;

pub struct RamMapState {
    pixels: Vec<u8>,
    tex: Option<TextureHandle>,
    pub scale: f32,
    pub show_read: bool,
    pub show_write: bool,
    pub show_exec: bool,
    pub show_overlays: bool,
    /// Brightness multiplier applied to the heat values.
    pub gain: f32,
    pub hover_addr: Option<u16>,
}

impl Default for RamMapState {
    fn default() -> Self {
        RamMapState {
            pixels: vec![0; 256 * 256 * 4],
            tex: None,
            scale: 2.0,
            show_read: true,
            show_write: true,
            show_exec: true,
            show_overlays: true,
            gain: 1.0,
            hover_addr: None,
        }
    }
}

fn build_image(app: &mut App) {
    let t = &app.spec.bus.tracker;
    let gain = app.ram.gain;
    let (show_read, show_write, show_exec) =
        (app.ram.show_read, app.ram.show_write, app.ram.show_exec);
    for addr in 0..65536usize {
        let r = if show_write {
            (t.write_heat[addr] as f32 * gain).min(255.0) as u8
        } else {
            0
        };
        let g = if show_read {
            (t.read_heat[addr] as f32 * gain).min(255.0) as u8
        } else {
            0
        };
        let b = if show_exec {
            (t.exec_heat[addr] as f32 * gain * 0.9).min(255.0) as u8
        } else {
            0
        };
        // A dim floor keeps the ROM/RAM split and untouched memory visible.
        let floor = if addr < 0x4000 { 22 } else { 10 };
        let i = addr * 4;
        app.ram.pixels[i] = r.max(floor);
        app.ram.pixels[i + 1] = g.max(floor);
        app.ram.pixels[i + 2] = b.max(floor);
        app.ram.pixels[i + 3] = 255;
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Show:");
        ui.checkbox(&mut app.ram.show_read, "Read");
        ui.colored_label(Color32::from_rgb(0, 255, 0), "■");
        ui.checkbox(&mut app.ram.show_write, "Write");
        ui.colored_label(Color32::from_rgb(255, 0, 0), "■");
        ui.checkbox(&mut app.ram.show_exec, "Execute");
        ui.colored_label(Color32::from_rgb(80, 80, 255), "■");
        ui.separator();
        ui.checkbox(&mut app.ram.show_overlays, "Overlays");
    });
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut app.ram.scale, 1.0..=4.0).text("zoom"));
        ui.add(egui::Slider::new(&mut app.ram.gain, 0.25..=4.0).text("gain"));
    });
    ui.horizontal_wrapped(|ui| {
        let t = &mut app.spec.bus.tracker;
        ui.add(egui::Slider::new(&mut t.fade_read, 1..=64).text("read fade"));
        ui.add(egui::Slider::new(&mut t.fade_write, 1..=64).text("write fade"));
        ui.add(egui::Slider::new(&mut t.fade_exec, 1..=64).text("exec fade"));
    });
    ui.separator();

    build_image(app);
    let img = ColorImage::from_rgba_unmultiplied([256, 256], &app.ram.pixels);
    match &mut app.ram.tex {
        Some(t) => t.set(img, TextureOptions::NEAREST),
        None => {
            app.ram.tex = Some(ui.ctx().load_texture("ram-map", img, TextureOptions::NEAREST))
        }
    }

    let scale = app.ram.scale;
    let size = Vec2::splat(256.0 * scale);
    // Scrollable, so the map is still fully reachable at high zoom.
    let (rect, response) = egui::ScrollArea::both()
        .id_salt("ram-map-scroll")
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| ui.allocate_exact_size(size, Sense::click()))
        .inner;
    let painter = ui.painter_at(rect);
    if let Some(tex) = &app.ram.tex {
        painter.image(
            tex.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    if app.ram.show_overlays {
        let outline = |region: Region, color: Color32, label: &str| {
            let y0 = rect.top() + (region.start as f32 / 256.0) * scale;
            let y1 = rect.top()
                + ((region.start as u32 + region.len as u32) as f32 / 256.0) * scale;
            let r = Rect::from_min_max(
                egui::pos2(rect.left(), y0),
                egui::pos2(rect.right(), y1),
            );
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
            painter.text(
                r.left_top() + Vec2::new(3.0, 1.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::monospace(10.0),
                color,
            );
        };
        outline(
            Region {
                start: SCREEN_START,
                len: SCREEN_END - SCREEN_START,
            },
            Color32::from_rgb(255, 220, 0),
            "video RAM",
        );
        if let Some(bb) = app.spec.bus.tracker.back_buffer() {
            outline(bb, Color32::from_rgb(0, 220, 255), "back buffer");
        }
        // Where the CPU is right now.
        let pc = app.spec.cpu.pc;
        let py = rect.top() + (pc as f32 / 256.0) * scale;
        painter.line_segment(
            [egui::pos2(rect.left(), py), egui::pos2(rect.right(), py)],
            Stroke::new(1.0, Color32::from_rgb(255, 255, 255)),
        );
    }

    // Hovering reports the byte under the cursor.
    app.ram.hover_addr = response.hover_pos().map(|p| {
        let x = ((p.x - rect.left()) / scale).clamp(0.0, 255.0) as u16;
        let y = ((p.y - rect.top()) / scale).clamp(0.0, 255.0) as u16;
        y * 256 + x
    });

    if let Some(addr) = app.ram.hover_addr {
        let t = &app.spec.bus.tracker;
        ui.monospace(format!(
            "${addr:04X}  = ${:02X}   reads {}   writes {}",
            app.spec.bus.peek_raw(addr),
            t.read_count[addr as usize],
            t.write_count[addr as usize],
        ));
        if response.clicked() {
            app.dbg.follow_pc = false;
            app.dbg.view_addr = addr;
            app.show_debugger = true;
        }
    } else {
        ui.monospace("hover a pixel for the address; click to show it in the debugger");
    }

    ui.separator();
    ui.small("Each pixel is one byte; each row is 256 bytes. Row 0 is $0000, row 255 is $FF00.");
}
