//! Disassembly, registers and execution controls.

use eframe::egui;
use egui::{Color32, RichText};

use crate::disasm;
use crate::machine::{Slot, Stop, FRAME_T};
use crate::ui::{theme, App};

pub struct DebuggerState {
    pub follow_pc: bool,
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

    ui.columns(2, |cols| {
        disassembly(app, &mut cols[0]);
        right_column(app, &mut cols[1]);
    });
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(if app.running { "⏸ Pause" } else { "▶ Run" })
            .clicked()
        {
            app.running = !app.running;
        }
        if ui.button("⤓ Step into").on_hover_text("F7").clicked() {
            app.step_into();
        }
        if ui.button("⤼ Step over").on_hover_text("F8").clicked() {
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
    });

    if ui.input(|i| i.key_pressed(egui::Key::F7)) {
        app.step_into();
    }
    if ui.input(|i| i.key_pressed(egui::Key::F8)) {
        app.step_over();
    }
    if ui.input(|i| i.key_pressed(egui::Key::F5)) {
        app.running = !app.running;
    }

    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Speed");
        crate::ui::speed_dropdown(&mut app.speed, ui);
    });
}

fn registers(app: &mut App, ui: &mut egui::Ui) {
    theme::lcd().show(ui, |ui| registers_lcd(app, ui));
}

fn registers_lcd(app: &mut App, ui: &mut egui::Ui) {
    let c = app.cpu();
    let mono =
        |ui: &mut egui::Ui, s: String| ui.label(RichText::new(s).monospace().color(theme::LCD_FG));

    egui::Grid::new("regs").num_columns(4).show(ui, |ui| {
        mono(ui, format!("AF  {:04X}", c.af()));
        mono(ui, format!("AF' {:04X}", (c.a_ as u16) << 8 | c.f_ as u16));
        mono(ui, format!("IX  {:04X}", c.ix));
        mono(ui, format!("PC  {:04X}", c.pc));
        ui.end_row();
        mono(ui, format!("BC  {:04X}", c.bc()));
        mono(ui, format!("BC' {:04X}", (c.b_ as u16) << 8 | c.c_ as u16));
        mono(ui, format!("IY  {:04X}", c.iy));
        mono(ui, format!("SP  {:04X}", c.sp));
        ui.end_row();
        mono(ui, format!("DE  {:04X}", c.de()));
        mono(ui, format!("DE' {:04X}", (c.d_ as u16) << 8 | c.e_ as u16));
        mono(ui, format!("I   {:02X}", c.i));
        mono(ui, format!("WZ  {:04X}", c.wz));
        ui.end_row();
        mono(ui, format!("HL  {:04X}", c.hl()));
        mono(ui, format!("HL' {:04X}", (c.h_ as u16) << 8 | c.l_ as u16));
        mono(ui, format!("R   {:02X}", c.r_full()));
        mono(ui, format!("IM  {}", c.im));
        ui.end_row();
    });

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

    egui::ScrollArea::vertical()
        .id_salt("disasm")
        .max_height(360.0)
        .show(ui, |ui| {
            let mut clicked: Option<u16> = None;
            for _ in 0..app.dbg.lines {
                let insn = disasm::disasm(&peek, addr);
                let is_pc = addr == pc;
                let has_bp = app.breakpoints().contains(&addr);
                let bytes: String = insn
                    .bytes
                    .iter()
                    .map(|b| format!("{b:02X} "))
                    .collect::<String>();
                let text = format!(
                    "{}{:04X}  {:<12}{}",
                    if has_bp { "●" } else { " " },
                    addr,
                    bytes,
                    insn.text
                );
                let mut rich = RichText::new(text).monospace();
                if is_pc {
                    rich = rich.color(Color32::BLACK).background_color(theme::AMBER);
                } else if has_bp {
                    rich = rich.color(theme::RED);
                }
                if ui
                    .add(egui::Label::new(rich).sense(egui::Sense::click()))
                    .clicked()
                {
                    clicked = Some(addr);
                }
                addr = addr.wrapping_add(insn.len.max(1) as u16);
            }
            if let Some(a) = clicked {
                if let Some(i) = app.breakpoints().iter().position(|&b| b == a) {
                    app.breakpoints_mut().remove(i);
                } else {
                    app.breakpoints_mut().push(a);
                }
            }
        });
    ui.small("Click a line to toggle a breakpoint.");
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
        if ui.small_button("HL").clicked() {
            app.dbg.mem_addr = app.cpu().hl();
        }
        if ui.small_button("SP").clicked() {
            app.dbg.mem_addr = app.cpu().sp;
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("memdump")
        .max_height(220.0)
        .show(ui, |ui| {
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
                ui.monospace(line);
            }
        });
}
