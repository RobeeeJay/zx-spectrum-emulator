//! Live map of memory: one pixel per byte, green for reads, red for writes,
//! fading every frame.
//!
//! Two views. *Address space* is the 64K the CPU sees right now. *All memory*
//! lays out every RAM bank and ROM page the machine has, so on a 128K you can
//! watch banks that are not currently paged in, with overlays showing which
//! address each one is mapped to.

use eframe::egui;
use egui::{Color32, ColorImage, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::machine::Slot;
use crate::tracker::{ram_phys, rom_phys, Region, BANK_SIZE, SCREEN_END, SCREEN_START};
use crate::ui::{theme, App};

/// Rows of 256 bytes in one 16K bank.
pub const ROWS_PER_BANK: usize = BANK_SIZE / 256;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum View {
    /// The 64K the CPU currently sees.
    AddressSpace,
    /// Every bank and ROM page, whether paged in or not.
    AllMemory,
}

/// One block of the physical view: a RAM bank or a ROM page.
#[derive(Clone)]
pub struct Chunk {
    /// First index into the tracker's physical arrays.
    pub phys: usize,
    /// Row this chunk starts at in the image.
    pub row: usize,
    /// Which 16K slot it is paged into, if any.
    pub slot: Option<usize>,
    pub label: String,
    pub is_rom: bool,
    /// RAM bank number, or ROM page number.
    pub number: usize,
}

pub struct RamMapState {
    pixels: Vec<u8>,
    tex: Option<TextureHandle>,
    pub view: View,
    /// How big a byte was drawn last time, in points. A record of what the
    /// window did rather than a setting: the map is drawn to the width it has.
    pub drawn_zoom: f32,
    pub show_read: bool,
    pub show_write: bool,
    pub show_exec: bool,
    pub show_overlays: bool,
    /// Brightness multiplier applied to the heat values.
    pub gain: f32,
    pub hover: Option<Hover>,
    rows: usize,
}

impl RamMapState {
    /// The image last drawn, as RGBA. One pixel per byte.
    pub fn image(&self) -> &[u8] {
        &self.pixels
    }
}

/// What the cursor is over.
#[derive(Clone)]
pub struct Hover {
    pub phys: usize,
    /// Address it answers to, if it is paged in at all.
    pub addr: Option<u16>,
    pub what: String,
    pub offset: u16,
}

impl Default for RamMapState {
    fn default() -> Self {
        RamMapState {
            pixels: vec![0; 256 * 256 * 4],
            tex: None,
            view: View::AddressSpace,
            drawn_zoom: MIN_ZOOM,
            show_read: true,
            show_write: true,
            show_exec: true,
            show_overlays: true,
            gain: 1.0,
            hover: None,
            rows: 256,
        }
    }
}

/// The blocks making up the physical view, in the order they are drawn.
pub fn chunks(app: &App) -> Vec<Chunk> {
    if app.on_zx81() {
        // Nothing is paged: the ROM and the RAM are always where they are.
        return vec![
            Chunk {
                phys: rom_phys(0, 0),
                row: 0,
                slot: Some(0),
                label: "ROM".into(),
                is_rom: true,
                number: 0,
            },
            Chunk {
                phys: ram_phys(0, 0),
                row: ROWS_PER_BANK,
                slot: Some(1),
                label: "RAM".into(),
                is_rom: false,
                number: 0,
            },
        ];
    }
    let bus = &app.spec.bus;
    let mut out = Vec::new();
    let mut row = 0;
    for page in 0..bus.rom_pages() {
        out.push(Chunk {
            phys: rom_phys(page, 0),
            row,
            slot: bus.rom_page_slot(page),
            label: format!("ROM{page}"),
            is_rom: true,
            number: page,
        });
        row += ROWS_PER_BANK;
    }
    for bank in bus.visible_banks() {
        out.push(Chunk {
            phys: ram_phys(bank, 0),
            row,
            slot: bus.ram_bank_slot(bank),
            label: format!("RAM{bank}"),
            is_rom: false,
            number: bank,
        });
        row += ROWS_PER_BANK;
    }
    out
}

fn colour(app: &App, phys: usize, floor: u8) -> [u8; 3] {
    let t = app.tracker();
    let gain = app.ram.gain;
    let r = if app.ram.show_write {
        (t.write_heat[phys] as f32 * gain).min(255.0) as u8
    } else {
        0
    };
    let g = if app.ram.show_read {
        (t.read_heat[phys] as f32 * gain).min(255.0) as u8
    } else {
        0
    };
    let b = if app.ram.show_exec {
        (t.exec_heat[phys] as f32 * gain * 0.9).min(255.0) as u8
    } else {
        0
    };
    [r.max(floor), g.max(floor), b.max(floor)]
}

fn build_image(app: &mut App) {
    let rows = match app.ram.view {
        View::AddressSpace => 256,
        View::AllMemory => chunks(app).last().map_or(256, |c| c.row + ROWS_PER_BANK),
    };
    if app.ram.rows != rows || app.ram.pixels.len() != rows * 256 * 4 {
        app.ram.rows = rows;
        app.ram.pixels = vec![0; rows * 256 * 4];
        app.ram.tex = None;
    }

    match app.ram.view {
        View::AddressSpace => {
            for addr in 0..65536usize {
                // A dim floor keeps the ROM/RAM split and untouched memory
                // visible.
                let floor = if app.is_rom(addr as u16) { 22 } else { 10 };
                let phys = app.phys_index(addr as u16);
                let c = colour(app, phys, floor);
                let i = addr * 4;
                app.ram.pixels[i] = c[0];
                app.ram.pixels[i + 1] = c[1];
                app.ram.pixels[i + 2] = c[2];
                app.ram.pixels[i + 3] = 255;
            }
        }
        View::AllMemory => {
            for chunk in chunks(app) {
                // Paged-in blocks sit on a slightly brighter floor.
                let floor = match (chunk.is_rom, chunk.slot.is_some()) {
                    (true, true) => 26,
                    (true, false) => 16,
                    (false, true) => 14,
                    (false, false) => 6,
                };
                for offset in 0..BANK_SIZE {
                    let c = colour(app, chunk.phys + offset, floor);
                    let i = (chunk.row * 256 + offset) * 4;
                    app.ram.pixels[i] = c[0];
                    app.ram.pixels[i + 1] = c[1];
                    app.ram.pixels[i + 2] = c[2];
                    app.ram.pixels[i + 3] = 255;
                }
            }
        }
    }
}

/// How big a byte is drawn, in points.
///
/// The narrowest a byte is drawn, in points.
///
/// The map is drawn to whatever width the window is — it is 256 bytes across
/// and the window is a fixed width, so the two are the same thing — and this
/// is only the floor for a window somebody has made very narrow, below which
/// the map scrolls instead.
const MIN_ZOOM: f32 = 1.0;

/// How big a byte is drawn, given the width the map has to draw in.
///
/// The map is 256 bytes across whatever the view, and the window is a fixed
/// width, so filling the width is a division. Below one point a byte the map
/// scrolls instead of shrinking further, since a map nobody can see the
/// individual bytes of says nothing.
pub fn zoom_for(room: f32) -> f32 {
    (room / 256.0).max(MIN_ZOOM)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.label("View:");
        // The theme's own selectable, not egui's: egui leaves the frame off an
        // unselected button until the pointer arrives, and the stroke it then
        // draws is a point on each side, so the button grows by two and shoves
        // everything after it along the row.
        if theme::selectable(ui, app.ram.view == View::AddressSpace, "Address space").clicked() {
            app.ram.view = View::AddressSpace;
        }
        if theme::selectable(ui, app.ram.view == View::AllMemory, "All memory")
            .on_hover_text("Every RAM bank and ROM page, paged in or not")
            .clicked()
        {
            app.ram.view = View::AllMemory;
        }
        ui.separator();
        theme::toggle(ui, &mut app.ram.show_overlays, "Overlays");
    });
    // Each channel with its own fade beside it: the switch, its colour, and
    // how long its marks take to fade. The fades used to be three sliders on a
    // row of their own, where which was which had to be read off their labels.
    let (mut read, mut write, mut exec) = {
        let t = app.tracker();
        (t.fade_read, t.fade_write, t.fade_exec)
    };
    let channel =
        |ui: &mut egui::Ui, on: &mut bool, name: &str, colour: egui::Color32, fade: &mut u8| {
            theme::toggle(ui, on, name);
            ui.colored_label(colour, "■");
            ui.add_enabled_ui(*on, |ui| {
                theme::slider(ui, egui::Slider::new(fade, 1..=64).text("fade"));
            });
        };
    ui.horizontal(|ui| {
        ui.set_min_height(theme::ROW_H);
        ui.spacing_mut().slider_width = 76.0;
        channel(ui, &mut app.ram.show_read, "Read", theme::GREEN, &mut read);
        ui.separator();
        channel(ui, &mut app.ram.show_write, "Write", theme::RED, &mut write);
    });
    ui.horizontal(|ui| {
        ui.set_min_height(theme::ROW_H);
        ui.spacing_mut().slider_width = 76.0;
        channel(
            ui,
            &mut app.ram.show_exec,
            "Execute",
            theme::BLUE,
            &mut exec,
        );
        ui.separator();
        theme::slider(
            ui,
            egui::Slider::new(&mut app.ram.gain, 0.25..=4.0).text("gain"),
        );
    });
    {
        let t = app.tracker_mut();
        t.fade_read = read;
        t.fade_write = write;
        t.fade_exec = exec;
    }
    ui.separator();

    build_image(app);
    let rows = app.ram.rows;
    let img = ColorImage::from_rgba_unmultiplied([256, rows], &app.ram.pixels);
    match &mut app.ram.tex {
        Some(t) => t.set(img, TextureOptions::NEAREST),
        None => {
            app.ram.tex = Some(
                ui.ctx()
                    .load_texture("ram-map", img, TextureOptions::NEAREST),
            )
        }
    }

    // As wide as the window, since the window is a fixed width and the map is
    // always 256 bytes across: a byte is drawn as a square of whatever that
    // divides into. The vertical scrollbar's width comes off first, or the
    // map would be a hair too wide for its own scroll area and gain a
    // horizontal scrollbar it has no use for.
    let room = ui.available_width() - ui.spacing().scroll.bar_width - 2.0;
    let scale = zoom_for(room);
    app.ram.drawn_zoom = scale;
    let size = Vec2::new(256.0 * scale, rows as f32 * scale);
    // Scrollable, so the map is still fully reachable at high zoom.
    let (rect, response) = egui::ScrollArea::both()
        .id_salt("ram-map-scroll")
        .max_height((ui.available_height() - 60.0).max(120.0))
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
        match app.ram.view {
            View::AddressSpace => address_space_overlays(app, &painter, rect, scale),
            View::AllMemory => all_memory_overlays(app, &painter, rect, scale),
        }
    }

    app.ram.hover = response.hover_pos().and_then(|p| {
        let x = ((p.x - rect.left()) / scale).clamp(0.0, 255.0) as u16;
        let y = ((p.y - rect.top()) / scale).max(0.0) as usize;
        hover_at(app, x, y)
    });

    hover_readout(app, ui, &response);

    ui.separator();
    ui.small(match app.ram.view {
        View::AddressSpace => "Each pixel is one byte of the address space; each row is 256 bytes.",
        View::AllMemory => {
            "Each block is a 16K bank or ROM page (64 rows of 256 bytes). \
             Bright outlines mark what is paged in where."
        }
    });
}

/// What is under the cursor, in whichever view is showing.
pub fn hover_at(app: &App, x: u16, y: usize) -> Option<Hover> {
    match app.ram.view {
        View::AddressSpace => {
            let addr = (y.min(255) as u16) * 256 + x;
            let what = app.slot_label(addr);
            Some(Hover {
                phys: app.phys_index(addr),
                addr: Some(addr),
                what,
                offset: addr & 0x3fff,
            })
        }
        View::AllMemory => {
            let chunk = chunks(app)
                .into_iter()
                .find(|c| y >= c.row && y < c.row + ROWS_PER_BANK)?;
            let offset = ((y - chunk.row) * 256 + x as usize) as u16;
            Some(Hover {
                phys: chunk.phys + offset as usize,
                addr: chunk
                    .slot
                    .map(|slot| ((slot as u16) << 14) | (offset & 0x3fff)),
                what: chunk.label,
                offset,
            })
        }
    }
}

/// What is under the pointer, on one line.
///
/// Truncated rather than allowed to run on. The window is a fixed width and
/// the line is not: a reading longer than the window made the whole thing
/// wider than its own viewport, which brought up a horizontal scrollbar,
/// which took height from the map, which changed the map's size — and the
/// controls above it moved as the pointer went over the map. That is what
/// "the buttons jump about on hover" was.
fn hover_readout(app: &mut App, ui: &mut egui::Ui, response: &egui::Response) {
    let line = |ui: &mut egui::Ui, text: String| {
        ui.add(egui::Label::new(egui::RichText::new(text).monospace()).truncate());
    };
    let Some(hover) = app.ram.hover.clone() else {
        line(
            ui,
            "hover for details; click to show the address in the debugger".into(),
        );
        return;
    };
    let value = match hover.addr {
        Some(addr) => app.peek(addr),
        // Not paged in: read it straight out of the bank.
        None => app.spec.bus.bank_byte(hover.phys / BANK_SIZE, hover.offset),
    };
    let where_ = match hover.addr {
        Some(addr) => format!("@ ${addr:04X}"),
        None => "not paged in".to_string(),
    };
    let t = app.tracker();
    let text = format!(
        "{} +${:04X} {}  = ${value:02X}   reads {}   writes {}",
        hover.what, hover.offset, where_, t.read_count[hover.phys], t.write_count[hover.phys],
    );
    line(ui, text);
    if response.clicked() {
        if let Some(addr) = hover.addr {
            app.dbg.follow_pc = false;
            app.dbg.view_addr = addr;
            app.show_debugger = true;
        }
    }
}

fn outline(
    painter: &egui::Painter,
    r: Rect,
    color: Color32,
    text: &str,
    align: egui::Align2,
    at: egui::Pos2,
) {
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    painter.text(at, align, text, egui::FontId::monospace(10.0), color);
}

fn address_space_overlays(app: &App, painter: &egui::Painter, rect: Rect, scale: f32) {
    let row_y = |addr: u32| rect.top() + (addr as f32 / 256.0) * scale;
    let band = |start: u32, end: u32| {
        Rect::from_min_max(
            egui::pos2(rect.left(), row_y(start)),
            egui::pos2(rect.right(), row_y(end)),
        )
    };

    if app.on_zx81() {
        zx81_overlays(app, painter, &band, &outline);
        program_counter(app, painter, rect, row_y(app.cpu().pc as u32));
        return;
    }

    // What is paged into each 16K slot.
    for slot in 0..4u32 {
        let base = slot * 0x4000;
        let name = match app.spec.bus.slot_of(base as u16) {
            Slot::Rom(p) => format!("${base:04X}  ROM{p}"),
            Slot::Ram(b) => {
                let mut s = format!("${base:04X}  RAM{b}");
                if b == app.spec.bus.screen_bank() {
                    s.push_str("  (screen)");
                }
                s
            }
        };
        let r = band(base, base + 0x4000);
        outline(
            painter,
            r,
            theme::DIM,
            &name,
            egui::Align2::RIGHT_TOP,
            r.right_top() + Vec2::new(-3.0, 1.0),
        );
    }

    let r = band(SCREEN_START as u32, SCREEN_END as u32);
    outline(
        painter,
        r,
        theme::YELLOW,
        "video RAM",
        egui::Align2::LEFT_TOP,
        r.left_top() + Vec2::new(3.0, 1.0),
    );

    if let Some(bb) = app.spec.bus.tracker.back_buffer() {
        let r = band(bb.start as u32, bb.start as u32 + bb.len as u32);
        outline(
            painter,
            r,
            theme::CYAN,
            "back buffer",
            egui::Align2::LEFT_TOP,
            r.left_top() + Vec2::new(3.0, 1.0),
        );
    }

    program_counter(app, painter, rect, row_y(app.cpu().pc as u32));
}

/// Where the CPU is, as a line across the row it is executing in.
///
/// Labelled, like everything else drawn over the map. It was a bare white line
/// that moved up and down as the machine ran, which reads as something wrong
/// with the window rather than as the one part of the picture that is alive.
fn program_counter(app: &App, painter: &egui::Painter, rect: Rect, y: f32) {
    painter.line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        Stroke::new(1.0, Color32::WHITE),
    );
    painter.text(
        egui::pos2(rect.right() - 3.0, y + 1.0),
        egui::Align2::RIGHT_TOP,
        format!("PC ${:04X}", app.cpu().pc),
        egui::FontId::monospace(9.0),
        Color32::WHITE,
    );
}

fn all_memory_overlays(app: &App, painter: &egui::Painter, rect: Rect, scale: f32) {
    let pc_phys = app.phys_index(app.cpu().pc);
    // A ZX81 has no screen bank: the display file lives wherever the ROM put
    // it, inside the one bank of RAM.
    let screen_bank = if app.on_zx81() {
        usize::MAX
    } else {
        app.spec.bus.screen_bank()
    };

    for chunk in chunks(app) {
        let top = rect.top() + chunk.row as f32 * scale;
        let bottom = top + ROWS_PER_BANK as f32 * scale;
        let r = Rect::from_min_max(
            egui::pos2(rect.left(), top),
            egui::pos2(rect.right(), bottom),
        );
        let shows_screen = !chunk.is_rom && chunk.number == screen_bank;

        // Paged-in blocks get a bright outline naming the address they answer
        // to; the rest are dimmed.
        let (color, mut text) = match chunk.slot {
            Some(slot) => (
                Color32::from_rgb(120, 210, 255),
                format!("{} → ${:04X}", chunk.label, (slot as u32) << 14),
            ),
            None => (
                Color32::from_gray(110),
                format!("{} (paged out)", chunk.label),
            ),
        };
        if shows_screen {
            text.push_str("  (screen)");
        }
        outline(
            painter,
            r,
            color,
            &text,
            egui::Align2::LEFT_TOP,
            r.left_top() + Vec2::new(3.0, 1.0),
        );

        // The display file inside whichever bank the ULA is showing.
        if shows_screen {
            let sr = Rect::from_min_max(
                egui::pos2(rect.left(), top),
                egui::pos2(
                    rect.right(),
                    top + (crate::tracker::SCREEN_LEN as f32 / 256.0) * scale,
                ),
            );
            painter.rect_stroke(
                sr,
                0.0,
                Stroke::new(1.0, theme::YELLOW),
                egui::StrokeKind::Inside,
            );
        }

        // A back buffer found in the address space belongs to whichever bank
        // is paged there.
        if let Some((from, to)) = back_buffer_in(app, &chunk) {
            let br = Rect::from_min_max(
                egui::pos2(rect.left(), top + (from as f32 / 256.0) * scale),
                egui::pos2(rect.right(), top + (to as f32 / 256.0) * scale),
            );
            painter.rect_stroke(
                br,
                0.0,
                Stroke::new(1.0, theme::CYAN),
                egui::StrokeKind::Inside,
            );
        }

        // Where the CPU is executing.
        if pc_phys >= chunk.phys && pc_phys < chunk.phys + BANK_SIZE {
            let py = top + ((pc_phys - chunk.phys) as f32 / 256.0) * scale;
            painter.line_segment(
                [egui::pos2(rect.left(), py), egui::pos2(rect.right(), py)],
                Stroke::new(1.0, Color32::WHITE),
            );
        }
    }
}

/// The part of a detected back buffer that falls inside this block, as offsets
/// within the block.
fn back_buffer_in(app: &App, chunk: &Chunk) -> Option<(u16, u16)> {
    let bb: Region = app.spec.bus.tracker.back_buffer()?;
    let slot = chunk.slot?;
    let slot_start = (slot as u32) << 14;
    let slot_end = slot_start + BANK_SIZE as u32;
    let start = (bb.start as u32).max(slot_start);
    let end = (bb.start as u32 + bb.len as u32).min(slot_end);
    if start >= end {
        return None;
    }
    Some(((start - slot_start) as u16, (end - slot_start) as u16))
}

/// The ZX81's address space: a ROM page, a RAM page, and the mirror of both
/// above $8000 that the display routine executes through. The display file
/// moves as the program grows, so it is read out of D_FILE rather than fixed.
type Outline<'a> = &'a dyn Fn(&egui::Painter, Rect, Color32, &str, egui::Align2, egui::Pos2);

fn zx81_overlays(
    app: &App,
    painter: &egui::Painter,
    band: &dyn Fn(u32, u32) -> Rect,
    outline: Outline<'_>,
) {
    let grey = theme::DIM;
    for (start, end, name) in [
        (0x0000u32, 0x4000u32, "$0000  ROM"),
        (0x4000, 0x8000, "$4000  RAM"),
        (0x8000, 0x10000, "$8000  mirror of $0000-$7FFF"),
    ] {
        let r = band(start, end);
        outline(
            painter,
            r,
            grey,
            name,
            egui::Align2::RIGHT_TOP,
            r.right_top() + Vec2::new(-3.0, 1.0),
        );
    }

    // D_FILE at $400C points at the display file; VARS at $4010 is the first
    // thing after it.
    let word = |addr: u16| u16::from_le_bytes([app.peek(addr), app.peek(addr + 1)]);
    let (d_file, vars) = (word(0x400c), word(0x4010));
    if d_file >= 0x4000 && vars > d_file {
        let r = band(d_file as u32, vars as u32);
        outline(
            painter,
            r,
            theme::YELLOW,
            "display file",
            egui::Align2::LEFT_TOP,
            r.left_top() + Vec2::new(3.0, 1.0),
        );
    }
}
