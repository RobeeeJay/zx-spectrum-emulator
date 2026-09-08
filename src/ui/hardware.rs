//! The Hardware window: what is plugged into the back of the machine.
//!
//! Each peripheral says what it is and how far it is emulated. The ones that
//! do nothing yet say what is missing, in the window rather than only in a
//! document, because a switch that turns on nothing looks like a thing that is
//! working.

use eframe::egui;

use crate::hardware::{Emulated, Peripheral};
use crate::if1::{If1, MAX_DRIVES};
use crate::ui::theme;
use crate::ui::App;

impl App {
    /// Plug something in, or take it out.
    ///
    /// The Interface 1 is the one with anything behind it: fitting it builds
    /// the interface and looks for its ROM, and taking it out takes the
    /// cartridges with it.
    pub fn fit(&mut self, what: Peripheral, yes: bool) {
        self.spec.bus.hardware.fit(what, yes);
        match what {
            Peripheral::Interface1 if yes => {
                let drives = self.spec.bus.hardware.if1_drives;
                let mut if1 = If1::new(drives);
                if1.rom = self.interface_1_rom();
                if if1.rom.is_none() {
                    self.set_status(
                        "Interface 1 fitted, but there is no roms/if1.rom to page in: \
                         everything the microdrives do is done by that ROM."
                            .into(),
                        true,
                    );
                }
                self.spec.bus.if1 = Some(if1);
            }
            Peripheral::Interface1 => {
                self.spec.bus.if1 = None;
                self.show_microdrive = false;
            }
            // The sound add-ons that are emulated bring their own chip.
            Peripheral::Fuller if yes => {
                self.spec.bus.audio.extra_ay = Some(Default::default());
            }
            Peripheral::Fuller => self.spec.bus.audio.extra_ay = None,
            Peripheral::SpecDrum if !yes => self.spec.bus.audio.dac = 0.0,
            _ => {}
        }
    }

    /// The Interface 1's ROM, from wherever the machine's ROMs are kept.
    fn interface_1_rom(&self) -> Option<Vec<u8>> {
        let dirs = crate::resources::search_dirs();
        crate::resources::find_file(&dirs, &["if1.rom", "interface1.rom", "if1-2.rom"], 8192)
            .map(|(_, data)| data)
    }

    /// How many microdrives are on the chain.
    pub fn set_microdrives(&mut self, count: usize) {
        let count = count.clamp(1, MAX_DRIVES);
        self.spec.bus.hardware.if1_drives = count;
        if let Some(if1) = self.spec.bus.if1.as_mut() {
            if1.set_drive_count(count);
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        theme::group_label(ui, "Machine");
        ui.label(
            egui::RichText::new(app.machine_key())
                .small()
                .color(theme::DIM),
        );
        ui.label(
            egui::RichText::new("— what is plugged into it is remembered with it")
                .small()
                .color(theme::DIM),
        );
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for what in Peripheral::ALL {
                peripheral(app, ui, what);
                ui.add_space(6.0);
            }
        });
}

fn peripheral(app: &mut App, ui: &mut egui::Ui, what: Peripheral) {
    let fitted = app.spec.bus.hardware.fitted(what);
    theme::slab().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let mut on = fitted;
            if theme::toggle(ui, &mut on, what.name()).clicked() {
                app.fit(what, on);
            }
            match what.emulated() {
                Emulated::Yes => {}
                Emulated::NeedsRom(rom) => {
                    let have = app.spec.bus.if1.as_ref().is_some_and(|i| i.rom.is_some());
                    ui.label(
                        egui::RichText::new(if have { "ROM found" } else { "needs a ROM" })
                            .small()
                            .color(if have { theme::GREEN } else { theme::AMBER }),
                    )
                    .on_hover_text(rom);
                }
                Emulated::No(_) => {
                    ui.label(
                        egui::RichText::new("not emulated")
                            .small()
                            .color(theme::AMBER),
                    );
                }
            }
        });
        ui.label(egui::RichText::new(what.what()).small().color(theme::DIM));
        if let Emulated::No(why) = what.emulated() {
            ui.label(egui::RichText::new(why).small().color(theme::DIM));
        }
        if let Emulated::NeedsRom(rom) = what.emulated() {
            if !app.spec.bus.if1.as_ref().is_some_and(|i| i.rom.is_some()) {
                ui.label(egui::RichText::new(rom).small().color(theme::DIM));
            }
        }

        // The Interface 1 is the one with anything to set.
        if what == Peripheral::Interface1 && fitted {
            ui.horizontal_wrapped(|ui| {
                theme::group_label(ui, "Microdrives");
                let mut drives = app.spec.bus.hardware.if1_drives;
                if theme::slider(
                    ui,
                    egui::Slider::new(&mut drives, 1..=MAX_DRIVES).text("on the chain"),
                )
                .changed()
                {
                    app.set_microdrives(drives);
                }
                if theme::selectable(ui, app.show_microdrive, "Microdrive window").clicked() {
                    app.show_microdrive = !app.show_microdrive;
                }
            });
        }
    });
}
