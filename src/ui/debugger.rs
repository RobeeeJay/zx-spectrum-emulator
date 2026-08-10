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
const REGISTERS_W: f32 = 420.0;

/// How many words of the stack are shown, and how wide that column is.
const STACK_DEPTH: u16 = 12;
const STACK_W: f32 = 130.0;

/// The list of names, and how far it runs before it scrolls.
const LABELS_W: f32 = 150.0;
const LABELS_H: f32 = 150.0;

/// The picture beside the registers: wide enough to make out what is being
/// drawn, with a margin of case around it.
const VIDEO_W: f32 = 176.0;
const VIDEO_BORDER: f32 = 5.0;

/// One register, and the address it would take the dump to.
fn pair(name: &str, value: u16) -> (String, Option<u16>) {
    (format!("{name:<3} {value:04X}"), Some(value))
}

/// How much of the window the listing takes; the registers, breakpoints and
/// memory dump have the rest.
const LISTING_SHARE: f32 = 0.62;

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
    /// Whether to guess at what the code is doing, and the last guess made.
    pub autodoc: bool,
    pub doc: crate::autodoc::Doc,
    /// What the guess was made from, so it is not made again every frame.
    doc_from: Option<(u16, u16)>,
    pub view_addr: u16,
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
            autodoc: false,
            doc: crate::autodoc::Doc::default(),
            doc_from: None,
            view_addr: 0,
            lines: 24,
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
        ui.toggle_value(&mut app.dbg.follow_pc, "Follow PC");
        let was = app.dbg.autodoc;
        ui.toggle_value(&mut app.dbg.autodoc, "AutoDoc")
            .on_hover_text(
                "Guess at what the routines being called are for, and note it \
             against them. Guesses are shown in place of an empty label or \
             comment and are never written to your notes file.",
            );
        if app.dbg.autodoc != was {
            // Turned on or off: the guess is stale either way.
            app.dbg.doc = crate::autodoc::Doc::default();
            app.dbg.doc_from = None;
        }
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
            ui.toggle_value(&mut breaks.screen, "Screen")
                .on_hover_text("Stop on a write anywhere in the display file");
            ui.toggle_value(&mut breaks.beeper, "Beeper")
                .on_hover_text("Stop when the speaker or MIC bit of port $FE changes");
            if has_ay {
                ui.toggle_value(&mut breaks.ay, "AY")
                    .on_hover_text("Stop on any access to the sound chip");
            }
            ui.toggle_value(&mut breaks.interrupt, "Interrupt")
                .on_hover_text("Stop when the CPU accepts the frame interrupt");
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
                registers_lcd(app, ui);
            });
        });
        stack(app, ui);
        labels(app, ui);
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
            if entries.is_empty() {
                ui.label(RichText::new("none yet").monospace().color(theme::DIM));
                return;
            }
            let mut go_to = None;
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
            if let Some(addr) = go_to {
                app.dbg.view_addr = addr;
                app.dbg.follow_pc = false;
            }
        });
    });
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
    egui::Frame::new()
        .fill(theme::CASE_DARK)
        .inner_margin(egui::Margin::same(VIDEO_BORDER as i8))
        .show(ui, |ui| {
            ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
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
    refresh_autodoc(app);

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

    let peek = |a: u16| app.peek(a);
    // Start a little above the anchor, aligned to a real opcode boundary.
    let mut addr = disasm::sync_start(&peek, app.dbg.view_addr, 12);

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

    egui::ScrollArea::vertical()
        .id_salt("disasm")
        .max_height(360.0)
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
                    }
                    rich
                };

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
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut comment)
                            .id_salt(("note-comment", addr))
                            .desired_width(COMMENT_W)
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
}

/// Make the guess again, if what it was made from has changed.
///
/// Reading a few hundred instructions is cheap, but not cheap enough to do
/// sixty times a second for no reason: it is redone when the listing moves or
/// the machine stops somewhere new.
fn refresh_autodoc(app: &mut App) {
    if !app.dbg.autodoc {
        if !app.dbg.doc.is_empty() {
            app.dbg.doc = crate::autodoc::Doc::default();
        }
        return;
    }
    let from = (app.dbg.view_addr, app.cpu().pc);
    if app.dbg.doc_from == Some(from) {
        return;
    }
    app.dbg.doc_from = Some(from);
    // Both the code on screen and the code being run are worth following: the
    // listing may be somewhere the machine has not reached yet.
    //
    // A recording adds the best evidence there is. Static reading has to guess
    // which bytes are code; a recording says where the program actually went,
    // past the loader and the protection and into the game itself.
    let mut entries = vec![from.0, from.1];
    if let Some(rzx) = &app.rzx {
        entries.extend(rzx.visited.iter().copied());
    }
    let peek = |a: u16| app.peek(a);
    let doc = crate::autodoc::analyse(&peek, &entries);

    // Into the notes, where they are kept with the rest. A guess replaces an
    // earlier guess but never a line the user wrote.
    for (addr, label) in &doc.labels {
        app.notes.suggest(*addr, label, doc.comment(*addr));
    }
    for (addr, comment) in &doc.comments {
        if !doc.labels.contains_key(addr) {
            app.notes.suggest(*addr, "", comment);
        }
    }
    app.dbg.doc = doc;
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
