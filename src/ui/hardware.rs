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
use crate::printer::{Paper, ZxPrinter};
use crate::sp0256::Sp0256;
use crate::ui::theme;
use crate::ui::App;
use crate::uspeech::Uspeech;

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
            // Both printers are the same device to the machine, on the same
            // port, so fitting one takes the other off.
            Peripheral::ZxPrinter | Peripheral::Alphacom32 if yes => {
                let (other, paper) = if what == Peripheral::ZxPrinter {
                    (Peripheral::Alphacom32, Paper::Metallised)
                } else {
                    (Peripheral::ZxPrinter, Paper::Thermal)
                };
                self.spec.bus.hardware.fit(other, false);
                self.spec.bus.printer = Some(ZxPrinter::new(paper));
            }
            Peripheral::ZxPrinter | Peripheral::Alphacom32 => {
                self.spec.bus.printer = None;
                self.show_printer = false;
            }
            Peripheral::Fuller if yes => {
                self.spec.bus.audio.extra_ay = Some(Default::default());
            }
            Peripheral::Fuller => self.spec.bus.audio.extra_ay = None,
            Peripheral::SpecDrum if !yes => self.spec.bus.audio.dac = 0.0,
            // The µSpeech is its ROM: without one there is nothing to page in
            // at the interrupt, and the box does nothing at all.
            Peripheral::Uspeech if yes => {
                let mut uspeech = Uspeech::new();
                uspeech.rom = self.uspeech_rom();
                if uspeech.rom.is_none() {
                    self.set_status(
                        "µSpeech fitted, but there is no roms/uspeech.rom: everything the \
                         interface does is done by that ROM."
                            .into(),
                        true,
                    );
                }
                self.spec.bus.uspeech = Some(uspeech);
                // The speech chip is the other half, and its allophones are
                // inside its own ROM: without that the interface works and
                // nothing is audible.
                self.spec.bus.audio.speech = self.speech_chip_rom().map(|rom| Sp0256::new(&rom));
                if self.spec.bus.audio.speech.is_none() {
                    self.set_status(
                        "µSpeech fitted, but there is no roms/sp0256-al2.rom: the interface \
                         works and nothing is audible."
                            .into(),
                        true,
                    );
                }
            }
            Peripheral::Uspeech => {
                self.spec.bus.uspeech = None;
                self.spec.bus.audio.speech = None;
            }
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

    /// The µSpeech's ROM, from wherever the machine's ROMs are kept.
    fn uspeech_rom(&self) -> Option<Vec<u8>> {
        let dirs = crate::resources::search_dirs();
        crate::resources::find_file(
            &dirs,
            &["uspeech.rom", "currah.rom", "microspeech.rom"],
            crate::uspeech::ROM_LEN,
        )
        .map(|(_, data)| data)
    }

    /// The SP0256-AL2's own 2K, which is where the allophones live.
    fn speech_chip_rom(&self) -> Option<Vec<u8>> {
        let dirs = crate::resources::search_dirs();
        crate::resources::find_file(&dirs, &["sp0256-al2.rom", "sp0256-al2.bin"], 2048)
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
        if what == Peripheral::Uspeech {
            // Both halves: the interface's ROM and the chip's own. With only
            // the first it works but says nothing, which is not "found".
            return self.spec.bus.uspeech.as_ref().is_some_and(|u| u.ready())
                && self.spec.bus.audio.speech.is_some();
        }
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

        // The red button itself is on the main window, under Buttons: it is
        // pressed while something else is running, and going to find a window
        // first is not that.

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
