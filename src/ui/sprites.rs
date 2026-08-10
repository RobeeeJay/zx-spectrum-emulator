//! Memory read as graphics.
//!
//! A sprite on this machine is nothing but bytes: eight to a character cell,
//! most significant bit on the left, and cells stored one after another. What
//! nobody can tell from the bytes is how wide the sprite is meant to be, so
//! that is the one thing this asks for. Get the width right and a sheet of
//! sprites appears; get it wrong and it shears, which is itself the clue.

use egui::{Color32, RichText};

use crate::ui::{theme, App};

/// How the memory is being read as pictures.
pub struct SpriteView {
    /// Where the sheet starts.
    pub addr: u16,
    /// The address box, so it can be typed into without jumping about.
    pub addr_text: String,
    /// How many character cells wide one sprite is.
    pub cells_across: usize,
    /// And how many down.
    pub cells_down: usize,
    /// How many sprites to show side by side.
    pub columns: usize,
    /// Pixels per Spectrum pixel.
    pub zoom: f32,
    /// Draw the attribute file's colours over it, or plain white on black.
    pub inverted: bool,
}

impl Default for SpriteView {
    fn default() -> Self {
        SpriteView {
            addr: 0x8000,
            addr_text: "8000".into(),
            cells_across: 2,
            cells_down: 2,
            columns: 8,
            zoom: 3.0,
            inverted: false,
        }
    }
}

impl SpriteView {
    /// Bytes in one sprite.
    pub fn stride(&self) -> u16 {
        (self.cells_across * self.cells_down * 8) as u16
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    ui.separator();
    sheet(app, ui);
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "At");
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.sprites.addr_text)
                .desired_width(64.0)
                .font(egui::TextStyle::Monospace)
                .hint_text("8000"),
        );
        if response.changed() {
            if let Ok(addr) =
                u16::from_str_radix(app.sprites.addr_text.trim().trim_start_matches('$'), 16)
            {
                app.sprites.addr = addr;
            }
        }

        // Stepping by one sprite is how a sheet is brought into line when it
        // starts a byte or two before or after where you guessed.
        let stride = app.sprites.stride();
        for (label, delta) in [
            ("⏪", -(stride as i32)),
            ("◀", -1),
            ("▶", 1),
            ("⏩", stride as i32),
        ] {
            if ui.small_button(label).clicked() {
                app.sprites.addr = (app.sprites.addr as i32 + delta) as u16;
                app.sprites.addr_text = format!("{:04X}", app.sprites.addr);
            }
        }

        theme::divider(ui);
        theme::group_label(ui, "Sprite");
        size_picker(ui, &mut app.sprites.cells_across, "wide");
        size_picker(ui, &mut app.sprites.cells_down, "tall");

        theme::divider(ui);
        theme::group_label(ui, "Sheet");
        size_picker(ui, &mut app.sprites.columns, "across");
        theme::slider(
            ui,
            egui::Slider::new(&mut app.sprites.zoom, 1.0..=8.0)
                .step_by(1.0)
                .show_value(false)
                .text("zoom"),
        );
        ui.toggle_value(&mut app.sprites.inverted, "Invert")
            .on_hover_text("Some sheets are stored as masks, which read inside out");
    });

    ui.label(
        RichText::new(format!(
            "{} bytes a sprite, {} for the sheet on show",
            app.sprites.stride(),
            app.sprites.stride() as usize * app.sprites.columns * rows_shown(app)
        ))
        .small()
        .color(theme::DIM),
    );
}

/// A number of cells, chosen from a small list: sprite sizes on this machine
/// are nearly always one, two, three or four cells.
fn size_picker(ui: &mut egui::Ui, value: &mut usize, what: &str) {
    theme::dropdown(ui, 58.0, format!("{value} {what}"), |ui| {
        for n in 1..=8usize {
            if ui
                .selectable_label(*value == n, format!("{n} {what}"))
                .clicked()
            {
                *value = n;
            }
        }
    });
}

/// How many rows of sprites are drawn.
fn rows_shown(app: &App) -> usize {
    let per_sprite = app.sprites.cells_down * 8;
    let height = (app.sprites.zoom * per_sprite as f32).max(1.0);
    // Enough to fill a sensible window without walking the whole 64K.
    ((520.0 / height).floor() as usize).clamp(1, 32)
}

fn sheet(app: &mut App, ui: &mut egui::Ui) {
    let view = &app.sprites;
    let (across, down) = (view.cells_across, view.cells_down);
    let (columns, rows) = (view.columns.max(1), rows_shown(app));
    let zoom = app.sprites.zoom;
    let gap = 4.0;

    let sprite = egui::vec2(across as f32 * 8.0 * zoom, down as f32 * 8.0 * zoom);
    let size = egui::vec2(
        columns as f32 * (sprite.x + gap) + gap,
        rows as f32 * (sprite.y + gap) + gap,
    );

    egui::ScrollArea::both()
        .id_salt("sprite-sheet")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::LCD_BG);

            let stride = app.sprites.stride();
            let mut at = app.sprites.addr;
            let mut under_pointer = None;

            for row in 0..rows {
                for column in 0..columns {
                    let corner = rect.min
                        + egui::vec2(
                            gap + column as f32 * (sprite.x + gap),
                            gap + row as f32 * (sprite.y + gap),
                        );
                    let cell = egui::Rect::from_min_size(corner, sprite);
                    painter.rect_filled(cell, 0.0, Color32::BLACK);
                    draw_sprite(app, &painter, cell, at, across, down, zoom);

                    if response.hover_pos().is_some_and(|p| cell.contains(p)) {
                        under_pointer = Some(at);
                        painter.rect_stroke(
                            cell,
                            0.0,
                            egui::Stroke::new(1.0, theme::AMBER),
                            egui::StrokeKind::Outside,
                        );
                    }
                    at = at.wrapping_add(stride);
                }
            }

            // Clicking a sprite takes the debugger's memory dump to it, so the
            // bytes can be read beside the picture.
            if let Some(addr) = under_pointer {
                response.clone().on_hover_text(format!("${addr:04X}"));
                if response.clicked() {
                    app.dbg.mem_addr = addr;
                    app.dbg.mem_text = format!("{addr:04X}");
                    app.show_debugger = true;
                }
            }
        });
}

/// One sprite: cells in reading order, eight bytes each, high bit on the left.
fn draw_sprite(
    app: &App,
    painter: &egui::Painter,
    rect: egui::Rect,
    at: u16,
    across: usize,
    down: usize,
    zoom: f32,
) {
    let mut byte_at = at;
    for cell_y in 0..down {
        for cell_x in 0..across {
            for row in 0..8 {
                let byte = app.peek(byte_at);
                byte_at = byte_at.wrapping_add(1);
                for bit in 0..8 {
                    let set = byte & (0x80 >> bit) != 0;
                    if set == app.sprites.inverted {
                        continue;
                    }
                    let x = rect.left() + ((cell_x * 8 + bit) as f32) * zoom;
                    let y = rect.top() + ((cell_y * 8 + row) as f32) * zoom;
                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(zoom, zoom)),
                        0.0,
                        Color32::WHITE,
                    );
                }
            }
        }
    }
}
