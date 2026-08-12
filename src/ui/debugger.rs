//! Disassembly, registers and execution controls.

use eframe::egui;
use egui::{Color32, RichText};

use crate::disasm;
use crate::machine::{Slot, Stop, FRAME_T};
use crate::ui::{theme, App};

/// How wide the debugger window is, and stays: the disassembly and the
/// registers beside it are laid out in columns, and a window narrow enough to
/// wrap them is no use for reading either.
pub const WINDOW_W: f32 = 1000.0;

/// How wide each column of the listing is. Fixed rather than taken from the
/// space left over: a column that measures itself against what is available
/// changes width as the scrollbar comes and goes, which egui then lays out
/// again, and the listing shivers from frame to frame.
const GUTTER_W: f32 = 12.0;
const LABEL_W: f32 = 84.0;
const ADDR_W: f32 = 44.0;
const VALUE_W: f32 = 92.0;
const INSN_W: f32 = 140.0;
const COMMENT_W: f32 = 200.0;
/// The gap between one column and the next.
const COLUMN_GAP: f32 = 6.0;
/// A row of the memory dump: address, eight bytes, eight characters. Fixed
/// for the same reason the listing's columns are.
const DUMP_W: f32 = 330.0;

/// How wide the register panel is allowed to be, so what sits beside it has
/// somewhere to be.
const REGISTERS_W: f32 = 350.0;

/// How many words of the stack are shown, and how wide that column is.
const STACK_DEPTH: u16 = 12;
const STACK_W: f32 = 124.0;

/// How tall the listing is, and so how much of memory is on show.
pub const LISTING_H: f32 = 460.0;

/// Instructions moved for each row of wheel travel. Three is about what a
/// document scrolls by, and an instruction is shorter than a line of prose.
const LINES_PER_ROW: f32 = 3.0;

/// How far above the address on show the listing starts, so there is
/// something to scroll back through. Bytes rather than lines, since how many
/// instructions that is depends on what they are — and not so far that the
/// line you came to look at falls off the bottom of what is visible.
const BEFORE: u16 = 32;

/// The list of names, and how far it runs before it scrolls.
const LABELS_W: f32 = 132.0;
/// The list of data blocks found.
const DATA_W: f32 = 132.0;
const LABELS_H: f32 = 150.0;

/// The picture beside the registers: wide enough to make out what is being
/// drawn, with a margin of case around it.
const VIDEO_W: f32 = 150.0;
const VIDEO_BORDER: f32 = 5.0;

/// One register, and the address it would take the dump to.
fn pair(name: &str, value: u16) -> (String, Option<u16>) {
    (format!("{name:<3} {value:04X}"), Some(value))
}

/// How much of the window the listing takes; the registers, breakpoints and
/// memory dump have the rest.
const LISTING_SHARE: f32 = 0.62;

/// How wide the row of panels above the listing comes out: the panels
/// themselves, the frame around each, and the gaps between them.
///
/// The window is a fixed width with no horizontal scrolling, so a row that
/// adds up to more than it simply loses its right-hand end — which is how the
/// stack and the memory dump disappeared once before.
pub fn top_row_width() -> f32 {
    const PANELS: f32 = REGISTERS_W + STACK_W + LABELS_W + DATA_W + VIDEO_W;
    /// Each panel sits in a frame with a margin either side, and there is a
    /// gap between one panel and the next.
    const FURNITURE: f32 = 5.0 * 14.0 + 4.0 * 8.0;
    PANELS + FURNITURE
}

/// One row of either pane. Both are laid out to the same height, so the
/// listing and the memory dump read as one instrument rather than two.
fn row_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y
}

pub struct DebuggerState {
    pub follow_pc: bool,
    /// Set when the machine stops at a breakpoint. The window asks to be
    /// raised on the next frame it draws, and clears it.
    pub raise: bool,
    /// Whether the "clear everything" button is waiting to be confirmed.
    pub confirm_clear: bool,
    pub view_addr: u16,
    /// The row to mark in the listing: what was last asked to be shown, from
    /// the labels list or the call flow window. Marked rather than only
    /// scrolled to, because a listing scrolled to an address leaves the reader
    /// counting rows to find which one was meant.
    pub marked: Option<u16>,
    /// Whether the debugger has asked for the program to be watched so it can
    /// work out where the routines and the data are.
    pub watching_blocks: bool,
    /// Wheel travel that has not yet added up to a whole instruction. A
    /// trackpad delivers a few points at a time, and rounding each of those to
    /// the nearest line throws every one of them away.
    pub scroll_debt: f32,
    pub lines: usize,
    pub goto_text: String,
    pub bp_text: String,
    pub mem_addr: u16,
    pub mem_text: String,
}

impl Default for DebuggerState {
    fn default() -> Self {
        DebuggerState {
            follow_pc: true,
            raise: false,
            confirm_clear: false,
            view_addr: 0,
            marked: None,
            watching_blocks: false,
            scroll_debt: 0.0,
            lines: 96,
            goto_text: String::new(),
            bp_text: String::new(),
            mem_addr: 0x4000,
            mem_text: String::new(),
        }
    }
}

impl App {
    pub fn step_into(&mut self) {
        self.running = false;
        self.step_machine();
        self.dbg.follow_pc = true;
        self.last_stop = Some(Stop::Stepped);
        self.status = format!("Stepped to ${:04X}", self.cpu().pc);
    }

    /// Run a CALL/RST/LDIR to completion; otherwise behave like step into.
    pub fn step_over(&mut self) {
        let pc = self.cpu().pc;
        if !self.spec.is_step_over_target(pc) {
            self.step_into();
            return;
        }
        let peek = |a: u16| self.peek(a);
        let len = disasm::disasm(&peek, pc).len.max(1) as u16;
        let target = pc.wrapping_add(len);
        if self.on_zx81() {
            self.step_to(target);
            return;
        }
        self.spec.temp_bp = Some(target);

        let was_slow = self.spec.bus.slow.enabled;
        self.spec.bus.slow.enabled = false;
        let mut spent = 0u32;
        let mut hit = false;
        while spent < FRAME_T * 200 {
            match self.spec.run(FRAME_T) {
                Stop::Breakpoint(a) if a == target => {
                    hit = true;
                    break;
                }
                Stop::Breakpoint(a) => {
                    self.status = format!("Breakpoint at ${a:04X} inside the call");
                    self.spec.temp_bp = None;
                    self.dbg.follow_pc = true;
                    self.spec.bus.slow.enabled = was_slow;
                    return;
                }
                _ => {}
            }
            spent += FRAME_T;
        }
        self.spec.bus.slow.enabled = was_slow;
        self.spec.temp_bp = None;
        self.running = false;
        self.dbg.follow_pc = true;
        self.status = if hit {
            format!("Stepped over to ${target:04X}")
        } else {
            "Step over gave up after 200 frames".into()
        };
    }

    /// Run instructions until the target address is reached, for a machine
    /// with no temporary breakpoint of its own.
    fn step_to(&mut self, target: u16) {
        let mut guard = 0u32;
        while guard < 20_000_000 {
            self.step_machine();
            guard += 1;
            let pc = self.cpu().pc;
            if pc == target {
                break;
            }
            if self.breakpoints().contains(&pc) {
                self.status = format!("Breakpoint at ${pc:04X} inside the call");
                self.running = false;
                self.dbg.follow_pc = true;
                return;
            }
        }
        self.running = false;
        self.dbg.follow_pc = true;
        self.status = format!("Stepped over to ${:04X}", self.cpu().pc);
    }

    /// Run until the current subroutine returns (SP back above where it is now).
    pub fn step_out(&mut self) {
        let sp0 = self.cpu().sp;
        if self.on_zx81() {
            let mut guard = 0u32;
            while guard < 20_000_000 {
                self.step_machine();
                guard += 1;
                if self.cpu().sp > sp0 {
                    break;
                }
            }
            self.running = false;
            self.dbg.follow_pc = true;
            self.status = format!("Returned to ${:04X}", self.cpu().pc);
            return;
        }
        let was_slow = self.spec.bus.slow.enabled;
        self.spec.bus.slow.enabled = false;
        let mut guard = 0u32;
        while guard < 20_000_000 {
            self.spec.step_instruction();
            guard += 1;
            if self.spec.cpu.sp > sp0 {
                break;
            }
        }
        self.spec.bus.slow.enabled = was_slow;
        self.running = false;
        self.dbg.follow_pc = true;
        self.status = format!("Returned to ${:04X}", self.spec.cpu.pc);
    }
}

fn flag_chip(ui: &mut egui::Ui, name: &str, on: bool) {
    let color = if on {
        theme::RED
    } else {
        Color32::from_rgb(0x4a, 0x46, 0x3f)
    };
    ui.label(RichText::new(name).monospace().color(color));
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    controls(app, ui);
    ui.separator();
    registers(app, ui);
    ui.separator();

    // Not `columns`, which splits evenly: the listing now carries five
    // columns of its own and needs the larger share.
    let full = ui.available_width();
    ui.horizontal_top(|ui| {
        let listing = (full * LISTING_SHARE).floor();
        ui.allocate_ui_with_layout(
            egui::vec2(listing, ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| disassembly(app, ui),
        );
        ui.separator();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.available_height()),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| right_column(app, ui),
        );
    });
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if theme::run_pause_button(ui, app.running).clicked() {
            app.running = !app.running;
        }
        // The icons are picked from what the bundled fonts actually have: the
        // arrows that were here before were in no font at all and drew as
        // empty boxes. Down into the call, past it, back out of it.
        if ui.button("⤵ Step into").on_hover_text("F7").clicked() {
            app.step_into();
        }
        if ui.button("⏭ Step over").on_hover_text("F8").clicked() {
            app.step_over();
        }
        if ui.button("⤴ Step out").clicked() {
            app.step_out();
        }
        if ui.button("↺ Reset").clicked() {
            app.reset_machine();
            app.status = "Reset".into();
        }
        ui.separator();
        theme::toggle(ui, &mut app.dbg.follow_pc, "Follow PC");
    });

    // Stopping on what a program does rather than on where it is: the things
    // that are awkward to find by address. The ZX81's bus is a different one
    // and none of these are wired to it, so they are not offered there — and
    // a machine with no sound chip is not offered the sound chip.
    if !app.on_zx81() {
        let has_ay = app.spec.bus.model.has_ay();
        ui.horizontal_wrapped(|ui| {
            theme::group_label(ui, "Break");
            let breaks = &mut app.spec.bus.breaks;
            theme::toggle(ui, &mut breaks.screen, "Screen")
                .on_hover_text("Stop on a write anywhere in the display file");
            theme::toggle(ui, &mut breaks.beeper, "Beeper")
                .on_hover_text("Stop when the speaker or MIC bit of port $FE changes");
            if has_ay {
                theme::toggle(ui, &mut breaks.ay, "AY")
                    .on_hover_text("Stop on any access to the sound chip");
            }
            theme::toggle(ui, &mut breaks.interrupt, "Interrupt")
                .on_hover_text("Stop when the CPU accepts the frame interrupt");
            theme::toggle(ui, &mut breaks.port_in, "In")
                .on_hover_text("Stop on any IN: the program reading any port at all");
            theme::toggle(ui, &mut breaks.port_out, "Out")
                .on_hover_text("Stop on any OUT: the program writing to any port at all");
            theme::toggle(ui, &mut breaks.rom, "ROM").on_hover_text(
                "Stop when the program goes into the ROM from outside it. \
                 Moving about within the ROM does not count, so a ROM routine \
                 calling another one is left alone.",
            );
        });
    }

    if ui.input(|i| i.key_pressed(egui::Key::F7)) {
        app.step_into();
    }
    if ui.input(|i| i.key_pressed(egui::Key::F8)) {
        app.step_over();
    }
    if ui.input(|i| i.key_pressed(egui::Key::F5)) {
        app.running = !app.running;
    }
}

fn registers(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_top(|ui| {
        theme::lcd().show(ui, |ui| {
            // Explicitly downwards: a frame inherits the layout of the `Ui` it
            // is shown in, and this one is shown in a row, so the flags ended
            // up beside the register grid rather than under it — and past the
            // width the panel is meant to keep to.
            ui.vertical(|ui| {
                ui.set_max_width(REGISTERS_W);
                ui.label(RichText::new("Registers").small().color(theme::DIM));
                registers_lcd(app, ui);
            });
        });
        stack(app, ui);
        labels(app, ui);
        data_blocks(app, ui);
        video(app, ui);
    });
    // The clock and the memory map are lines rather than columns, and putting
    // them in the panel above made it 300 points wider than the window, which
    // pushed the stack off the right-hand edge entirely.
    machine_state(app, ui);
}

/// Where the machine is in the frame, and what is paged in where.
fn machine_state(app: &mut App, ui: &mut egui::Ui) {
    let (frame, t, frame_t) = app.machine_clock();
    ui.label(
        RichText::new(format!(
            "frame {frame}   T {t:5}/{frame_t}   instructions {}",
            app.cpu().instructions
        ))
        .monospace(),
    );
    memory_map(app, ui);
    if !app.on_zx81() && app.spec.bus.model.has_ay() {
        ay_registers(app, ui);
    }
}

/// What is on the stack, from the stack pointer up.
///
/// Return addresses and saved registers are the fastest way to work out where
/// a routine came from, so clicking one takes the dump to it — the same as
/// clicking a register.
fn stack(app: &mut App, ui: &mut egui::Ui) {
    let sp = app.cpu().sp;
    let entries: Vec<(u16, u16)> = (0..STACK_DEPTH)
        .map(|i| {
            let at = sp.wrapping_add(i * 2);
            let word = u16::from_le_bytes([app.peek(at), app.peek(at.wrapping_add(1))]);
            (at, word)
        })
        .collect();

    theme::lcd().show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_min_width(STACK_W);
            ui.label(RichText::new("Stack").small().color(theme::DIM));
            let mut go_to = None;
            for (i, (at, word)) in entries.iter().enumerate() {
                // The top of the stack is what SP points at; the rest is what
                // is under it, counted in words as a Z80 programmer would.
                let text = format!("+{:<2} {at:04X}  {word:04X}", i * 2);
                let cell = ui.add(
                    egui::Label::new(RichText::new(text).monospace().color(theme::LCD_FG))
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .sense(egui::Sense::click()),
                );
                if cell.clicked() {
                    go_to = Some(*word);
                }
                cell.on_hover_text(format!("Show ${word:04X} in the memory dump"));
            }
            if let Some(addr) = go_to {
                show_in_dump(app, addr);
            }
        });
    });
}

/// Everything that has a name, as a way of getting to it.
///
/// The list is the labels from the notes — the user's own and AutoDoc's, the
/// guesses in the dim colour — because a name is the thing somebody remembers
/// a place in a program by.
fn labels(app: &mut App, ui: &mut egui::Ui) {
    let entries: Vec<(u16, String, bool)> = app
        .notes
        .labelled()
        .map(|(addr, label, auto)| (addr, label.to_string(), auto))
        .collect();

    theme::lcd().show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_min_width(LABELS_W);
            ui.set_max_width(LABELS_W);
            ui.label(RichText::new("Labels").small().color(theme::DIM));
            // The list is what is empty, not the panel: what follows it works
            // out the shape of the program, and returning early here left a
            // program with no labels yet — which is every program to begin
            // with — without the button that starts the work.
            let mut go_to = None;
            if entries.is_empty() {
                ui.label(RichText::new("none yet").monospace().color(theme::DIM));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("labels")
                    .max_height(LABELS_H)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (addr, label, auto) in &entries {
                            let colour = if *auto { theme::DIM } else { theme::LCD_FG };
                            let text = format!("{addr:04X} {label}");
                            let row = ui.add(
                                egui::Label::new(RichText::new(text).monospace().color(colour))
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .sense(egui::Sense::click()),
                            );
                            if row.clicked() {
                                go_to = Some(*addr);
                            }
                            row.on_hover_text(format!("Show the listing at ${addr:04X}"));
                        }
                    });
            }
            if let Some(addr) = go_to {
                app.show_in_listing(addr);
            }

            // Working out the shape of the program: what is a routine and
            // what is a table. It has to watch the program to know, so the
            // first press starts watching and the second reads off what was
            // seen. Running a whole recording between the two is what fills
            // in a game.
            ui.separator();
            let watching = app.spec.bus.observer.enabled;
            let known = app.notes.blocks().len();
            ui.horizontal(|ui| {
                if ui
                    .button(if watching {
                        "Find blocks"
                    } else {
                        "Watch for blocks"
                    })
                    .on_hover_text(
                        "Work out which runs of memory are routines and which \
                         are data, from what the program has been seen doing, \
                         and keep them in the notes file. Press it again after \
                         running more of the program and what it finds is added \
                         to what was known.",
                    )
                    .clicked()
                {
                    find_blocks(app);
                }
                ui.label(
                    RichText::new(if known > 0 {
                        format!("{known} blocks")
                    } else if watching {
                        "watching".to_string()
                    } else {
                        "none yet".to_string()
                    })
                    .small()
                    .color(theme::DIM),
                );
            });

            // Throwing the lot away takes the file with it, so it is asked
            // about rather than done on one click.
            ui.separator();
            let kept = app.notes.len();
            if app.dbg.confirm_clear {
                ui.label(
                    RichText::new(format!("Delete all {kept}?"))
                        .small()
                        .color(theme::RED),
                );
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        app.notes.clear();
                        app.dbg.confirm_clear = false;
                        if let Err(e) = app.notes.save_if_dirty() {
                            app.set_status(format!("Could not save notes: {e}"), true);
                        } else {
                            app.set_status(format!("Deleted {kept} labels and comments"), false);
                        }
                    }
                    if ui.button("Keep").clicked() {
                        app.dbg.confirm_clear = false;
                    }
                });
            } else if ui
                .add_enabled(kept > 0, egui::Button::new("Clear all…"))
                .on_hover_text(
                    "Delete every label and comment, the ones you wrote as \
                     well as AutoDoc's, and the file they are kept in.",
                )
                .clicked()
            {
                app.dbg.confirm_clear = true;
            }
        });
    });
}

/// Work out where the routines and the data are, and keep it.
///
/// Nothing can be worked out until the program has been watched, so the first
/// press turns the watching on and says so rather than reporting that it found
/// nothing. What is found is added to what was known: one run of a game sees
/// its title screen, and the next sees a level.
fn find_blocks(app: &mut App) {
    if !app.spec.bus.observer.enabled {
        app.dbg.watching_blocks = true;
        app.spec.bus.observer.enabled = true;
        app.set_status(
            "Watching. Run the program — a whole recording if you have one — \
             then press Find blocks."
                .to_string(),
            false,
        );
        return;
    }

    let found = crate::blocks::work_out(&app.spec.bus.observer);
    let merged = crate::blocks::merge(app.notes.blocks(), &found);
    let was = app.notes.blocks().len();
    let bytes: u32 = merged.iter().map(|block| block.length()).sum();
    app.notes.set_blocks(merged);
    let now = app.notes.blocks().len();
    match app.notes.save_if_dirty() {
        Err(e) => app.set_status(format!("Could not save notes: {e}"), true),
        Ok(_) => app.set_status(
            format!("{now} blocks over {bytes} bytes ({} known before)", was),
            false,
        ),
    }
}

/// The blocks of memory that were read but never run: data, with a guess at
/// what kind and — for the graphics — a picture of it.
///
/// A picture settles it. A block that draws as recognisable sprites is sprite
/// data whatever any rule says, and one that draws as noise is not.
fn data_blocks(app: &mut App, ui: &mut egui::Ui) {
    let blocks = app.spec.bus.observer.blocks(64);
    theme::lcd().show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_min_width(DATA_W);
            ui.set_max_width(DATA_W);
            ui.label(RichText::new("Data").small().color(theme::DIM));
            if blocks.is_empty() {
                ui.label(
                    RichText::new(if app.spec.bus.observer.enabled {
                        "nothing read yet"
                    } else {
                        "nothing is being watched"
                    })
                    .monospace()
                    .color(theme::DIM),
                );
                return;
            }
            let mut go_to = None;
            egui::ScrollArea::vertical()
                .id_salt("datablocks")
                .max_height(LABELS_H)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for block in blocks.iter().take(24) {
                        let text =
                            format!("{:04X} {:5} {}", block.at, block.length, block.kind.label());
                        let row = ui.add(
                            egui::Label::new(RichText::new(text).monospace().color(theme::LCD_FG))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .sense(egui::Sense::click()),
                        );
                        if row.clicked() {
                            go_to = Some(block.at);
                        }
                        // Who reads it, and who calls them: a graphics block
                        // plus its reader plus its reader's caller is most of
                        // "this is the sprite table for the guardians".
                        let read_by = match block.readers.first() {
                            Some(entry) => {
                                let name = app.notes.label(*entry);
                                let named = if name.is_empty() {
                                    format!("${entry:04X}")
                                } else {
                                    format!("{name} (${entry:04X})")
                                };
                                let caller = app
                                    .spec
                                    .bus
                                    .observer
                                    .edges
                                    .keys()
                                    .find(|(_, to)| to == entry)
                                    .map(|(from, _)| *from);
                                match caller {
                                    Some(from) => {
                                        format!("Read by {named}, which is called from ${from:04X}")
                                    }
                                    None => format!("Read by {named}"),
                                }
                            }
                            None => "Nothing has read it".to_string(),
                        };
                        row.on_hover_ui(|ui| {
                            ui.label(read_by);
                            if block.kind == crate::observe::DataKind::Graphics {
                                sprites(app, ui, block.at, block.length);
                            }
                        });
                    }
                });
            if let Some(addr) = go_to {
                show_in_dump(app, addr);
            }
        });
    });
}

/// Draw a block of memory as the Spectrum would if it were graphics: eight
/// bytes to a character cell, most significant bit on the left.
fn sprites(app: &App, ui: &mut egui::Ui, at: u16, length: u16) {
    const CELL: usize = 8;
    const ACROSS: usize = 16;
    let cells = (length as usize / CELL).min(ACROSS * 8);
    if cells == 0 {
        return;
    }
    let rows = cells.div_ceil(ACROSS);
    let scale = 2.0;
    let size = egui::vec2(
        ACROSS as f32 * CELL as f32 * scale,
        rows as f32 * CELL as f32 * scale,
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    for cell in 0..cells {
        let (cx, cy) = (cell % ACROSS, cell / ACROSS);
        for row in 0..CELL {
            let byte = app.peek(at.wrapping_add((cell * CELL + row) as u16));
            for bit in 0..8 {
                if byte & (0x80 >> bit) == 0 {
                    continue;
                }
                let x = rect.left() + (cx * CELL + bit) as f32 * scale;
                let y = rect.top() + (cy * CELL + row) as f32 * scale;
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(scale, scale)),
                    0.0,
                    Color32::WHITE,
                );
            }
        }
    }
}

/// A small picture of what the machine is putting out, so it is clear what the
/// code being stepped through is drawing without going back to the main
/// window. The border around it is the machine's own, in its current colour.
fn video(app: &mut App, ui: &mut egui::Ui) {
    let Some(texture) = app.screen_texture() else {
        return;
    };
    let size = texture.size_vec2();
    if size.x <= 0.0 {
        return;
    }
    let scale = VIDEO_W / size.x;
    ui.vertical(|ui| {
        ui.label(RichText::new("Screen").small().color(theme::DIM));
        let picture = egui::Frame::new()
            .fill(theme::CASE_DARK)
            .inner_margin(egui::Margin::same(VIDEO_BORDER as i8))
            .show(ui, |ui| {
                ui.add(
                    egui::Image::new(&texture)
                        .fit_to_exact_size(size * scale)
                        .sense(egui::Sense::click()),
                )
            });

        // Pointing at something on the picture and being told what drew it.
        // The bus watched it happen, so this is not a guess: it is the routine
        // that last wrote those bytes.
        let response = picture.inner;
        let Some(at) = response.hover_pos() else {
            return;
        };
        let rect = response.rect;
        let (column, row) = (
            ((at.x - rect.left()) / rect.width() * 32.0) as usize,
            ((at.y - rect.top()) / rect.height() * 24.0) as usize,
        );
        if column >= 32 || row >= 24 {
            return;
        }
        let drew = app.spec.bus.observer.drew_cell(column, row);
        let mut go_to = None;
        let clicked = response.clicked();
        response.on_hover_ui(|ui| {
            ui.label(format!("Character cell {column},{row}"));
            if drew.is_empty() {
                ui.label(
                    RichText::new(if app.spec.bus.observer.enabled {
                        "nothing has written here since watching began"
                    } else {
                        "nothing is being watched"
                    })
                    .color(theme::DIM),
                );
                return;
            }
            for (entry, bytes) in drew.iter().take(4) {
                let name = app.notes.label(*entry);
                let named = if name.is_empty() {
                    format!("${entry:04X}")
                } else {
                    format!("{name} (${entry:04X})")
                };
                ui.label(format!("{bytes} of its bytes written by {named}"));
            }
        });
        if clicked {
            if let Some((entry, _)) = drew.first() {
                go_to = Some(*entry);
            }
        }
        if let Some(entry) = go_to {
            app.show_in_listing(entry);
        }
    });
}

/// Point the memory dump at an address, and say so in its own box.
fn show_in_dump(app: &mut App, addr: u16) {
    app.dbg.mem_addr = addr;
    app.dbg.mem_text = format!("{addr:04X}");
}

fn registers_lcd(app: &mut App, ui: &mut egui::Ui) {
    let c = app.cpu();
    // Read out first: every sixteen-bit register is a pointer as far as the
    // debugger is concerned, and clicking one takes the dump there. The
    // eight-bit ones and the interrupt mode are not addresses, so they are
    // shown but not offered.
    let grid: [[(String, Option<u16>); 4]; 4] = [
        [
            pair("AF", c.af()),
            pair("AF'", (c.a_ as u16) << 8 | c.f_ as u16),
            pair("IX", c.ix),
            pair("PC", c.pc),
        ],
        [
            pair("BC", c.bc()),
            pair("BC'", (c.b_ as u16) << 8 | c.c_ as u16),
            pair("IY", c.iy),
            pair("SP", c.sp),
        ],
        [
            pair("DE", c.de()),
            pair("DE'", (c.d_ as u16) << 8 | c.e_ as u16),
            (format!("{:<3} {:02X}", "I", c.i), None),
            pair("WZ", c.wz),
        ],
        [
            pair("HL", c.hl()),
            pair("HL'", (c.h_ as u16) << 8 | c.l_ as u16),
            (format!("{:<3} {:02X}", "R", c.r_full()), None),
            (format!("{:<3} {}", "IM", c.im), None),
        ],
    ];

    let mut go_to = None;
    egui::Grid::new("regs").num_columns(4).show(ui, |ui| {
        for row in &grid {
            for (text, addr) in row {
                let rich = RichText::new(text).monospace().color(theme::LCD_FG);
                match addr {
                    Some(addr) => {
                        let cell = ui.add(
                            egui::Label::new(rich)
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .sense(egui::Sense::click()),
                        );
                        if cell.clicked() {
                            go_to = Some(*addr);
                        }
                        cell.on_hover_text(format!("Show ${addr:04X} in the memory dump"));
                    }
                    None => {
                        ui.label(rich);
                    }
                }
            }
            ui.end_row();
        }
    });
    if let Some(addr) = go_to {
        show_in_dump(app, addr);
    }
    let c = app.cpu();

    ui.horizontal(|ui| {
        ui.label("Flags:");
        let f = c.f;
        flag_chip(ui, "S", f & 0x80 != 0);
        flag_chip(ui, "Z", f & 0x40 != 0);
        flag_chip(ui, "5", f & 0x20 != 0);
        flag_chip(ui, "H", f & 0x10 != 0);
        flag_chip(ui, "3", f & 0x08 != 0);
        flag_chip(ui, "P", f & 0x04 != 0);
        flag_chip(ui, "N", f & 0x02 != 0);
        flag_chip(ui, "C", f & 0x01 != 0);
        ui.separator();
        flag_chip(ui, "IFF1", c.iff1);
        flag_chip(ui, "IFF2", c.iff2);
        if c.halted {
            ui.label(RichText::new("HALTED").color(theme::AMBER).monospace());
        }
    });
}

/// What each 16K slot currently points at, and the 128K paging latch.
fn memory_map(app: &mut App, ui: &mut egui::Ui) {
    if app.on_zx81() {
        // No paging to speak of: the ROM is in the bottom page and the RAM in
        // the next, and both are mirrored above $8000.
        let ram = app.zx81_ram.name();
        let rom_k = app
            .zx81
            .as_ref()
            .map(|zx| zx.bus.rom.len() / 1024)
            .unwrap_or(8);
        ui.label(
            RichText::new(format!(
                "{ram}  $0000:ROM ({rom_k}K)  $4000:RAM  $8000:mirror of $0000"
            ))
            .monospace(),
        );
        return;
    }
    let bus = &app.spec.bus;
    let slot_name = |slot: Slot| match slot {
        Slot::Rom(p) => format!("ROM{p}"),
        Slot::Ram(b) => format!("RAM{b}"),
    };
    let mut line = format!(
        "{}  $0000:{}  $4000:{}  $8000:{}  $C000:{}",
        bus.model.name(),
        slot_name(bus.slot_of(0x0000)),
        slot_name(bus.slot_of(0x4000)),
        slot_name(bus.slot_of(0x8000)),
        slot_name(bus.slot_of(0xc000)),
    );
    if bus.model.has_paging() {
        line += &format!("   $7FFD={:02X}", bus.page_reg);
        if bus.model.has_plus3_paging() {
            line += &format!(
                " $1FFD={:02X}{}",
                bus.page_reg_1ffd,
                if bus.special_paging() {
                    format!(" all-RAM cfg{}", (bus.page_reg_1ffd >> 1) & 3)
                } else {
                    String::new()
                }
            );
        }
        line += &format!(
            "  screen RAM{}{}",
            bus.screen_bank(),
            if bus.paging_locked { "  LOCKED" } else { "" }
        );
    }
    ui.label(RichText::new(line).monospace());
}

/// AY-3-8912 state: raw registers plus what they mean.
fn ay_registers(app: &mut App, ui: &mut egui::Ui) {
    let ay = &app.spec.bus.audio.ay;
    let regs = ay.regs;
    let clock = app.spec.bus.model.cpu_hz() / 2.0;

    ui.collapsing("AY-3-8912", |ui| {
        let hex: String = regs.iter().map(|r| format!("{r:02X} ")).collect();
        ui.label(RichText::new(format!("R0-15  {hex}")).monospace());
        ui.label(RichText::new(format!("selected R{}", ay.selected)).monospace());

        egui::Grid::new("ay-channels")
            .num_columns(4)
            .show(ui, |ui| {
                ui.label(RichText::new("ch").monospace().strong());
                ui.label(RichText::new("period").monospace().strong());
                ui.label(RichText::new("freq").monospace().strong());
                ui.label(RichText::new("volume").monospace().strong());
                ui.end_row();
                for (i, name) in ["A", "B", "C"].iter().enumerate() {
                    let period = ((regs[i * 2 + 1] as u32 & 0x0f) << 8) | regs[i * 2] as u32;
                    let freq = if period == 0 {
                        0.0
                    } else {
                        clock / (16.0 * period as f64)
                    };
                    let amp = regs[8 + i];
                    let vol = if amp & 0x10 != 0 {
                        "env".to_string()
                    } else {
                        format!("{}", amp & 0x0f)
                    };
                    let mixer = regs[7];
                    let tone = if mixer & (1 << i) == 0 {
                        "tone"
                    } else {
                        "----"
                    };
                    let noise = if mixer & (8 << i) == 0 {
                        "noise"
                    } else {
                        "-----"
                    };
                    ui.label(RichText::new(format!("{name} {tone} {noise}")).monospace());
                    ui.label(RichText::new(format!("{period:4}")).monospace());
                    ui.label(RichText::new(format!("{freq:7.1} Hz")).monospace());
                    ui.label(RichText::new(vol).monospace());
                    ui.end_row();
                }
            });

        let noise_p = regs[6] & 0x1f;
        let env_p = ((regs[12] as u32) << 8) | regs[11] as u32;
        ui.label(
            RichText::new(format!(
                "noise period {noise_p:2}   envelope period {env_p:5} shape ${:X}",
                regs[13] & 0x0f
            ))
            .monospace(),
        );
    });
}

fn disassembly(app: &mut App, ui: &mut egui::Ui) {
    let pc = app.cpu().pc;
    if app.dbg.follow_pc {
        app.dbg.view_addr = pc;
    }

    ui.horizontal(|ui| {
        ui.label("Go to:");
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.dbg.goto_text)
                .desired_width(70.0)
                .hint_text("4000"),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Ok(a) = u16::from_str_radix(app.dbg.goto_text.trim_start_matches('$'), 16) {
                app.dbg.view_addr = a;
                app.dbg.follow_pc = false;
            }
        }
        if ui.small_button("PC").clicked() {
            app.dbg.follow_pc = true;
        }
    });

    // As many lines as the listing has room for, and no more. The rows are a
    // window onto memory rather than a list with ends: any that do not fit
    // would give the area a scrollbar of its own, and the wheel would move
    // within those rows instead of travelling through the address space —
    // which is what "infinite scroll stopped working" was.
    app.dbg.lines = ((LISTING_H / row_height(ui)).floor() as usize).max(8);

    let peek = |a: u16| app.peek(a);
    // Start a little above the anchor, aligned to a real opcode boundary.
    // Well back from where you are, so there is something above the line you
    // came to look at: a listing that starts at the address you asked for can
    // only be scrolled one way.
    let mut addr = disasm::sync_start(&peek, app.dbg.view_addr.wrapping_sub(BEFORE), 12);

    // Take a copy of the bytes on show, so the listing can be drawn without
    // holding a borrow of the machine while the rest of the window is built.
    let base = addr;
    let window: Vec<u8> = (0..app.dbg.lines * 4 + 8)
        .map(|i| app.peek(base.wrapping_add(i as u16)))
        .collect();
    let peek = |a: u16| {
        window
            .get(a.wrapping_sub(base) as usize)
            .copied()
            .unwrap_or(0)
    };

    // Column headings. Every column is left justified and a fixed width, so
    // the heading sits over what it names on every row beneath it.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = COLUMN_GAP;
        ui.add_space(GUTTER_W);
        for (name, width) in [
            ("Label", LABEL_W),
            ("Address", ADDR_W),
            ("Value", VALUE_W),
            ("Instruction", INSN_W),
            ("Comments", COMMENT_W),
        ] {
            heading(ui, name, width);
        }
    });

    // The listing is a window onto the whole address space, not a list with
    // ends: rolling the wheel moves it through memory an instruction at a
    // time, so it can be followed as far as it goes in either direction.
    // Taken once for the frame: the listing asks about every row, and the
    // notes are borrowed mutably inside it.
    let blocks: Vec<crate::blocks::Block> = app.notes.blocks().to_vec();

    let listing_top = ui.cursor().min.y;
    egui::ScrollArea::vertical()
        .id_salt("disasm")
        .max_height(LISTING_H)
        // Never scrolls itself: rolling the wheel over it moves the window
        // through memory instead, which has no ends to stop at.
        .scroll([false, false])
        // A scroll area that shrinks to its contents makes its width depend on
        // what is inside it, which is the other half of the feedback that had
        // the listing shivering.
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            let mut clicked: Option<u16> = None;
            let mut finished_editing = false;
            for _ in 0..app.dbg.lines {
                let insn = disasm::disasm(&peek, addr);
                let is_pc = addr == pc;
                let has_bp = app.breakpoints().contains(&addr);
                let marked = app.dbg.marked == Some(addr);
                let bytes: String = insn
                    .bytes
                    .iter()
                    .map(|b| format!("{b:02X} "))
                    .collect::<String>();

                // The listing's own three columns share one look: the current
                // instruction is on an amber bar, a breakpoint is red.
                let paint = |text: String| {
                    let mut rich = RichText::new(text).monospace();
                    if is_pc {
                        rich = rich.color(Color32::BLACK).background_color(theme::AMBER);
                    } else if has_bp {
                        rich = rich.color(theme::RED);
                    } else if marked {
                        // Where you asked to be taken, on a band of its own so
                        // it can be told from the current instruction.
                        rich = rich.background_color(theme::MARK);
                    }
                    rich
                };

                // Which block this row is in, and so which band it sits on.
                // Neighbouring blocks take different shades, and code and data
                // take different pairs, so the shape of the program reads off
                // the listing without anything having to be labelled.
                let band = crate::blocks::at(&blocks, addr)
                    .map(|index| theme::band(index, blocks[index].kind));

                egui::Frame::NONE
                    .fill(band.unwrap_or(Color32::TRANSPARENT))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = COLUMN_GAP;

                            // The breakpoint marker sits in a gutter of its own, so a
                            // dot appearing does not push the addresses sideways.
                            cell(
                                ui,
                                RichText::new(if has_bp { "●" } else { " " })
                                    .monospace()
                                    .color(theme::RED),
                                GUTTER_W,
                            );

                            // What is written against this address. A guess is shown
                            // in the dim colour, so it reads as a guess; typing over
                            // one makes it the user's own and it goes to full ink.
                            let mut label = app.notes.label(addr).to_string();
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut label)
                                    .id_salt(("note-label", addr))
                                    .desired_width(LABEL_W)
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(if app.notes.label_is_auto(addr) {
                                        theme::DIM
                                    } else {
                                        theme::INK
                                    })
                                    .frame(egui::Frame::NONE),
                            );
                            if resp.changed() {
                                app.notes.set_label(addr, &label);
                            }
                            finished_editing |= resp.lost_focus();

                            let mut listing = cell(ui, paint(format!("{addr:04X}")), ADDR_W);
                            listing |= cell(ui, paint(bytes), VALUE_W);
                            listing |= cell(ui, paint(insn.text.clone()), INSN_W);
                            if listing.clicked() {
                                clicked = Some(addr);
                            }

                            // The semicolon is the listing's, not the file's: it marks
                            // a comment where there is one and stays out of the way
                            // where there is not.
                            let mut comment = app.notes.comment(addr).to_string();
                            // Multi-line, so a comment long enough to say something
                            // useful can be read in full rather than trailing off the
                            // end of a field. A row with nothing in it is still one
                            // line tall, so the listing keeps its pitch.
                            let resp = ui.add(
                                egui::TextEdit::multiline(&mut comment)
                                    .id_salt(("note-comment", addr))
                                    .desired_width(COMMENT_W)
                                    .desired_rows(1)
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(if app.notes.comment_is_auto(addr) {
                                        theme::DIM
                                    } else {
                                        theme::INK
                                    })
                                    .frame(egui::Frame::NONE),
                            );
                            if resp.changed() {
                                app.notes.set_comment(addr, &comment);
                            }
                            finished_editing |= resp.lost_focus();
                        });
                    });
                addr = addr.wrapping_add(insn.len.max(1) as u16);
            }
            if let Some(a) = clicked {
                if let Some(i) = app.breakpoints().iter().position(|&b| b == a) {
                    app.breakpoints_mut().remove(i);
                } else {
                    app.breakpoints_mut().push(a);
                }
            }
            // Written out when a field is left rather than on every keystroke:
            // the file is small, but a disk write per character typed is not
            // something to do to somebody's SSD.
            if finished_editing {
                if let Err(e) = app.notes.save_if_dirty() {
                    app.set_status(format!("Could not save notes: {e}"), true);
                }
            }
        });

    scroll_through_memory(app, ui, listing_top);
}

/// Move the listing through memory as the wheel is rolled over it.
///
/// A disassembly has no length to scroll within: what is wanted is to travel
/// through the address space. Backwards means finding an instruction boundary
/// above the one on show, which is what `sync_start` is for.
fn scroll_through_memory(app: &mut App, ui: &mut egui::Ui, top: f32) {
    let area = egui::Rect::from_min_max(
        egui::pos2(ui.min_rect().left(), top),
        egui::pos2(ui.min_rect().right(), top + LISTING_H),
    );
    let over_it = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| area.contains(p));
    if !over_it {
        return;
    }
    let wheel = ui.input(|i| i.smooth_scroll_delta.y);
    if wheel == 0.0 {
        return;
    }
    // How far a roll of the wheel carries. A row of the listing is a point of
    // wheel travel apiece, which moved the listing at a crawl next to every
    // other window on the desktop, so it goes several lines to the row — and
    // the part that does not add up to a whole line is kept rather than
    // rounded away, which is the whole of a trackpad's output.
    app.dbg.scroll_debt += wheel * LINES_PER_ROW / row_height(ui);
    let lines = app.dbg.scroll_debt.trunc() as i32;
    app.dbg.scroll_debt -= lines as f32;
    if lines == 0 {
        return;
    }
    app.dbg.follow_pc = false;
    let peek = |a: u16| app.peek(a);
    let mut addr = app.dbg.view_addr;
    if lines < 0 {
        for _ in 0..(-lines) {
            let insn = disasm::disasm(&peek, addr);
            addr = addr.wrapping_add(insn.len.max(1) as u16);
        }
    } else {
        for _ in 0..lines {
            // Back one instruction: the boundary above where we are.
            addr = disasm::sync_start(&peek, addr.wrapping_sub(1), 4);
        }
    }
    app.dbg.view_addr = addr;
}

/// A column heading: the same width as the column under it, and left
/// justified like everything in it.
fn heading(ui: &mut egui::Ui, name: &str, width: f32) {
    cell(ui, RichText::new(name).small().color(theme::DIM), width);
}

/// One cell of the listing: a fixed width, its text against the left edge.
///
/// Not `add_sized`, which centres what it is given inside the space it
/// allocates — that is what had the addresses drifting a few points either way
/// as the text beside them changed length.
fn cell(ui: &mut egui::Ui, text: RichText, width: f32) -> egui::Response {
    let height = row_height(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.add(
                egui::Label::new(text)
                    .wrap_mode(egui::TextWrapMode::Extend)
                    .sense(egui::Sense::click()),
            )
        },
    )
    .inner
}

fn right_column(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Breakpoints").strong());
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.dbg.bp_text)
                .desired_width(70.0)
                .hint_text("hex"),
        );
        let add = ui.button("Add").clicked()
            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if add {
            if let Ok(a) = u16::from_str_radix(app.dbg.bp_text.trim().trim_start_matches('$'), 16) {
                if !app.breakpoints().contains(&a) {
                    app.breakpoints_mut().push(a);
                }
                app.dbg.bp_text.clear();
            }
        }
        if ui.button("Clear all").clicked() {
            app.breakpoints_mut().clear();
        }
    });
    let mut remove: Option<usize> = None;
    for (i, bp) in app.breakpoints().clone().iter().enumerate() {
        ui.horizontal(|ui| {
            ui.monospace(format!("${bp:04X}"));
            if ui.small_button("✖").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        app.breakpoints_mut().remove(i);
    }

    ui.separator();
    ui.label(RichText::new("Memory").strong());
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.dbg.mem_text)
                .desired_width(70.0)
                .hint_text("4000"),
        );
        if resp.changed() {
            if let Ok(a) = u16::from_str_radix(app.dbg.mem_text.trim().trim_start_matches('$'), 16)
            {
                app.dbg.mem_addr = a;
            }
        }
        // The pointers a program is most likely to be using, so a look at
        // what one of them is aimed at is one click rather than a retyped
        // address.
        for (name, addr) in [
            ("HL", app.cpu().hl()),
            ("BC", app.cpu().bc()),
            ("DE", app.cpu().de()),
            ("IX", app.cpu().ix),
            ("IY", app.cpu().iy),
            ("SP", app.cpu().sp),
        ] {
            if ui.small_button(name).clicked() {
                app.dbg.mem_addr = addr;
                app.dbg.mem_text = format!("{addr:04X}");
            }
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("memdump")
        .max_height(220.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Laid out row by row to the listing's height rather than left to
            // the label's own, so a line of the dump sits on the same pitch as
            // a line of disassembly.
            let base = app.dbg.mem_addr & !0x7;
            for row in 0..16u16 {
                let addr = base.wrapping_add(row * 8);
                let mut line = format!("{addr:04X}  ");
                for i in 0..8u16 {
                    line.push_str(&format!("{:02X} ", app.peek(addr.wrapping_add(i))));
                }
                line.push(' ');
                for i in 0..8u16 {
                    let b = app.peek(addr.wrapping_add(i));
                    line.push(if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    });
                }
                cell(ui, RichText::new(line).monospace(), DUMP_W);
            }
        });
}
