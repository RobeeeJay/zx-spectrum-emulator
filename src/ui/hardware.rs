//! The Hardware window: what is plugged into the back of the machine.
//!
//! Each peripheral says what it is and how far it is emulated. The ones that
//! do nothing yet say what is missing, in the window rather than only in a
//! document, because a switch that turns on nothing looks like a thing that is
//! working.

use eframe::egui;

use crate::hardware::{Emulated, Peripheral};
use crate::if1::{If1, MAX_DRIVES};
use crate::multiface::{Model as MfModel, Multiface};
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
            // A Multiface is its ROM and 8K of RAM. Taking one out throws the
            // RAM away, which is what unplugging the box does.
            _ if multiface_model(what).is_some() => {
                let model = multiface_model(what).unwrap();
                self.spec.bus.multifaces.retain(|mf| mf.model != model);
                if yes {
                    let mut mf = Multiface::new(model);
                    mf.rom = self.multiface_rom(model);
                    if mf.rom.is_none() {
                        self.set_status(
                            format!(
                                "{} fitted, but there is no {} to run: the red button \
                                 has nothing behind it.",
                                model.name(),
                                model.rom_names()[0]
                            ),
                            true,
                        );
                    }
                    self.spec.bus.multifaces.push(mf);
                    // In the order the models came out, so one button reaching
                    // the last one on the back reaches the newest.
                    self.spec.bus.multifaces.sort_by_key(|mf| mf.model as u8);
                }
            }
            _ => {}
        }
    }

    /// The Interface 1's ROM, from wherever the machine's ROMs are kept.
    fn interface_1_rom(&self) -> Option<Vec<u8>> {
        let dirs = crate::resources::search_dirs();
        crate::resources::find_file(&dirs, &["if1.rom", "interface1.rom", "if1-2.rom"], 8192)
            .map(|(_, data)| data)
    }

    /// A Multiface's ROM, from wherever the machine's ROMs are kept.
    fn multiface_rom(&self, model: MfModel) -> Option<Vec<u8>> {
        let dirs = crate::resources::search_dirs();
        crate::resources::find_file(&dirs, model.rom_names(), crate::multiface::ROM_LEN)
            .map(|(_, data)| data)
    }

    /// Whether a peripheral that wants a ROM has found one.
    pub fn has_rom_for(&self, what: Peripheral) -> bool {
        match multiface_model(what) {
            Some(model) => self
                .spec
                .bus
                .multifaces
                .iter()
                .any(|mf| mf.model == model && mf.ready()),
            None => self.spec.bus.if1.as_ref().is_some_and(|i| i.rom.is_some()),
        }
    }

    /// The red button, and what it did. One button serves every Multiface on
    /// the back, as on the hardware.
    pub fn press_red_button(&mut self) {
        if self.spec.bus.press_red_button() {
            self.running = true;
            self.set_status("Red button: the Multiface has the machine.".into(), false);
        } else {
            self.set_status(
                "The red button did nothing: no Multiface with a ROM in it, or its menu \
                 has not finished with the last press."
                    .into(),
                true,
            );
        }
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
                    let have = app.has_rom_for(what);
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
            if !app.has_rom_for(what) {
                ui.label(egui::RichText::new(rom).small().color(theme::DIM));
            }
        }

        // The red button is the whole of a Multiface's front panel.
        if multiface_model(what).is_some() && fitted && app.has_rom_for(what) {
            ui.horizontal_wrapped(|ui| {
                if theme::selectable(ui, false, "Red button").clicked() {
                    app.press_red_button();
                }
                ui.label(
                    egui::RichText::new("stops the machine wherever it is and brings up its menu")
                        .small()
                        .color(theme::DIM),
                );
            });
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

/// Which Multiface a peripheral is, if it is one.
fn multiface_model(what: Peripheral) -> Option<MfModel> {
    match what {
        Peripheral::MultifaceOne => Some(MfModel::One),
        Peripheral::Multiface128 => Some(MfModel::OneTwentyEight),
        Peripheral::Multiface3 => Some(MfModel::Three),
        _ => None,
    }
}
