//! Memory read as graphics.
//!
//! A graphic on this machine is nothing but bytes: eight to a character cell,
//! most significant bit on the left, and cells stored one after another. What
//! nobody can tell from the bytes is how wide it is meant to be, so that is
//! the one thing this asks for. Get the width right and a sheet of graphics
//! appears; get it wrong and it shears, which is itself the clue.
//!
//! Graphics are kept two ways. In cells — eight bytes a character, cells one
//! after another — which is how the ROM keeps its font; or in rows — each row
//! of pixels as many bytes as the sprite is wide, one row after another —
//! which is how most games keep their sprites. The Find button picks a block
//! off the screen and looks for it both ways (`crate::gfxfind`).
//!
//! Nor is the data always packed. A format may carry a mask byte, an attribute
//! or a byte of padding after each row of cells, which shears the picture in
//! the same way and is cured by stepping over them.

use egui::{Color32, RichText};

use crate::ui::{theme, App};

/// How a graphic's bytes are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// Eight bytes a character cell, cells one after another.
    Cells,
    /// A row of pixels at a time, as many bytes as the graphic is wide.
    Rows,
}

/// How the memory is being read as pictures.
pub struct SpriteView {
    pub layout: Layout,
    /// Draw each byte the other way round, for a sprite kept facing left.
    pub mirrored: bool,
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
    /// Bytes to step over after each row of cells within one graphic. Some
    /// formats keep a mask, an attribute or a byte of padding there, and
    /// reading straight past it shears the picture exactly as a wrong width
    /// does.
    pub skip_after_row: u16,
    /// Draw the attribute file's colours over it, or plain white on black.
    pub inverted: bool,
    /// Set when something sends the viewer somewhere: the window asks to be
    /// brought forward on the next frame it draws, and clears it.
    pub raise: bool,
    /// How long the block it was sent to is, so the sheet can be sized to show
    /// the whole of it rather than an arbitrary window on to it.
    pub block_length: Option<u16>,
}

impl Default for SpriteView {
    fn default() -> Self {
        SpriteView {
            layout: Layout::Cells,
            mirrored: false,
            addr: 0x8000,
            addr_text: "8000".into(),
            cells_across: 2,
            cells_down: 2,
            columns: 8,
            skip_after_row: 0,
            zoom: 3.0,
            inverted: false,
            raise: false,
            block_length: None,
        }
    }
}

impl SpriteView {
    /// Bytes in one graphic, the skipped ones included: the next graphic
    /// starts after the padding of the last row, not before it.
    pub fn stride(&self) -> u16 {
        match self.layout {
            Layout::Cells => {
                ((self.cells_across * 8) as u16 + self.skip_after_row) * self.cells_down as u16
            }
            Layout::Rows => {
                (self.cells_across as u16 + self.skip_after_row) * (self.cells_down * 8) as u16
            }
        }
    }

    /// Bytes in one row of cells, before whatever is skipped after it.
    pub fn row_bytes(&self) -> u16 {
        (self.cells_across * 8) as u16
    }

    /// Where one pixel row of one cell lives, counting from the start of a
    /// graphic. Cells are stored one after another within a row of them, eight
    /// bytes each, and whatever the format keeps between rows is stepped over.
    pub fn byte_of(&self, at: u16, cell_y: usize, cell_x: usize, row: usize) -> u16 {
        match self.layout {
            Layout::Cells => {
                let row_start = at.wrapping_add(
                    (self.row_bytes() + self.skip_after_row).wrapping_mul(cell_y as u16),
                );
                row_start.wrapping_add((cell_x * 8 + row) as u16)
            }
            // A row of pixels is a byte for each cell across, then whatever is
            // skipped — a mask, padding — before the next row.
            Layout::Rows => {
                let pitch = self.cells_across as u16 + self.skip_after_row;
                at.wrapping_add(pitch.wrapping_mul((cell_y * 8 + row) as u16))
                    .wrapping_add(cell_x as u16)
            }
        }
    }

    /// Point the viewer at something the search found, laid out as it was
    /// found. Only the column that matched is known, so a row-major sprite is
    /// shown as wide as its rows are apart, starting at that column: the
    /// arrows move it along if the sprite starts further left.
    pub fn show(&mut self, found: &crate::gfxfind::Match) {
        self.addr = found.addr;
        self.addr_text = format!("{:04X}", found.addr);
        self.mirrored = found.mirrored;
        self.inverted = found.inverted;
        self.block_length = None;
        self.raise = true;
        if found.pitch == 1 {
            self.layout = Layout::Cells;
            self.cells_across = 1;
            self.cells_down = 1;
            self.skip_after_row = 0;
        } else {
            self.layout = Layout::Rows;
            self.cells_across = (found.pitch as usize).min(8);
            self.cells_down = 2;
            self.skip_after_row = found.pitch - self.cells_across as u16;
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    found(app, ui);
    if let Some(length) = app.sprites.block_length {
        ui.label(
            RichText::new(format!(
                "Showing the {length}-byte block at ${:04X}. Nothing in the bytes says \
                 how wide a sprite is, so try the widths until the shapes line up.",
                app.sprites.addr
            ))
            .small()
            .color(theme::DIM),
        );
    }
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
        theme::group_label(ui, "Graphic");
        size_picker(ui, &mut app.sprites.cells_across, "wide");
        size_picker(ui, &mut app.sprites.cells_down, "tall");
        skip_picker(ui, &mut app.sprites.skip_after_row);
        let layout = match app.sprites.layout {
            Layout::Cells => "in cells",
            Layout::Rows => "in rows",
        };
        theme::dropdown(ui, 80.0, layout, |ui| {
            if ui
                .selectable_label(app.sprites.layout == Layout::Cells, "in cells")
                .on_hover_text("Eight bytes a character, cells one after another, as the font")
                .clicked()
            {
                app.sprites.layout = Layout::Cells;
            }
            if ui
                .selectable_label(app.sprites.layout == Layout::Rows, "in rows")
                .on_hover_text("A row of pixels at a time, as most games keep their sprites")
                .clicked()
            {
                app.sprites.layout = Layout::Rows;
            }
        });

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
        theme::toggle(ui, &mut app.sprites.inverted, "Invert")
            .on_hover_text("Some sheets are stored as masks, which read inside out");
        theme::toggle(ui, &mut app.sprites.mirrored, "Mirror")
            .on_hover_text("A sprite kept facing the other way reads backwards");

        theme::divider(ui);
        let find = ui
            .add_enabled(!app.on_zx81(), egui::Button::new("Find…"))
            .on_hover_text(
                "Pause the machine, then click an 8x8 block on the screen to look for it \
                 in memory: as a character, as part of a wider sprite, mirrored or inverted",
            );
        if find.clicked() {
            app.start_graphics_find();
        }
    });

    if app.sprites.skip_after_row > 0 {
        ui.label(
            RichText::new(format!(
                "Stepping over {} byte{} after each row of {}: {} of the {} bytes \
                 a graphic takes are not drawn.",
                app.sprites.skip_after_row,
                if app.sprites.skip_after_row == 1 {
                    ""
                } else {
                    "s"
                },
                match app.sprites.layout {
                    Layout::Cells => "cells",
                    Layout::Rows => "pixels",
                },
                app.sprites.skip_after_row as usize
                    * app.sprites.cells_down
                    * match app.sprites.layout {
                        Layout::Cells => 1,
                        Layout::Rows => 8,
                    },
                app.sprites.stride()
            ))
            .small()
            .color(theme::DIM),
        );
    }

    ui.label(
        RichText::new(format!(
            "{} bytes a graphic, {} for the sheet on show",
            app.sprites.stride(),
            app.sprites.stride() as usize * app.sprites.columns * rows_shown(app)
        ))
        .small()
        .color(theme::DIM),
    );
}

/// How many bytes to step over after each row of cells. Kept to a short list:
/// a format carries a mask byte, an attribute, or a few bytes of padding, and
/// anything longer than a cell is a different question about the data.
fn skip_picker(ui: &mut egui::Ui, value: &mut u16) {
    // The explanation goes on the group label rather than the button: the
    // dropdown hands back what its menu returned, not the button's response.
    theme::dropdown(ui, 74.0, format!("skip {value}"), |ui| {
        for n in 0..=16u16 {
            if ui
                .selectable_label(*value == n, format!("skip {n}"))
                .clicked()
            {
                *value = n;
            }
        }
    });
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
    for cell_y in 0..down {
        for cell_x in 0..across {
            for row in 0..8 {
                let byte = app.peek(app.sprites.byte_of(at, cell_y, cell_x, row));
                for bit in 0..8 {
                    let mask = if app.sprites.mirrored {
                        0x01 << bit
                    } else {
                        0x80 >> bit
                    };
                    let set = byte & mask != 0;
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

/// What the last Find turned up, with a way to show each.
fn found(app: &mut App, ui: &mut egui::Ui) {
    if app.gfx_picking {
        ui.label(
            RichText::new("Click an 8x8 block on the main window's screen; Esc to give up.")
                .small()
                .color(theme::AMBER),
        );
    }
    let Some(picked) = app.gfx_found.clone() else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Found");
        let (x, y) = picked.cell;
        let bytes: Vec<String> = picked.pattern.iter().map(|b| format!("{b:02X}")).collect();
        ui.label(
            RichText::new(format!(
                "the block at column {x}, row {y}: {}",
                bytes.join(" ")
            ))
            .small()
            .monospace()
            .color(theme::DIM),
        );
        if ui
            .small_button("×")
            .on_hover_text("Put the finds away")
            .clicked()
        {
            app.gfx_found = None;
        }
    });
    match &picked.result {
        Err(why) => {
            ui.label(RichText::new(why).small().color(theme::AMBER));
        }
        Ok(search) if search.matches.is_empty() => {
            ui.label(
                RichText::new(
                    "Nowhere in memory as it stands. It may be drawn shifted by a few pixels, \
                     built up from something else, kept upside down, or in a bank not paged in.",
                )
                .small()
                .color(theme::DIM),
            );
        }
        Ok(search) => {
            egui::ScrollArea::vertical()
                .id_salt("found-graphics")
                .max_height(150.0)
                .show(ui, |ui| {
                    for m in &search.matches {
                        ui.horizontal(|ui| {
                            if ui.small_button("Show").clicked() {
                                app.sprites.show(m);
                            }
                            ui.label(RichText::new(m.describe()).small().monospace());
                        });
                    }
                });
            if search.more {
                ui.label(
                    RichText::new(format!(
                        "and more: only the first {} are listed",
                        crate::gfxfind::MAX_FOUND
                    ))
                    .small()
                    .color(theme::DIM),
                );
            }
        }
    }
    ui.add_space(4.0);
}
