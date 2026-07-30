use zx_spectrum_emulator::{
    audio_out::AudioOut, demo_rom, machine::Model, machine::Spectrum, snapshot, tape::Tape, ui,
};

/// Look for ROM images in ./roms. The 48K one is needed to boot anything real;
/// without it the built-in demo ROM runs instead.
fn find_roms() -> ui::Roms {
    let read = |names: &[&str]| -> Option<Vec<u8>> {
        for n in names {
            if let Ok(d) = std::fs::read(n) {
                if d.len() >= 0x4000 {
                    return Some(d);
                }
            }
        }
        None
    };
    ui::Roms {
        rom48: read(&["roms/48.rom", "roms/48k.rom", "48.rom"]),
        rom128: read(&["roms/128.rom", "roms/128k.rom", "128.rom"]),
        rom_plus3: read(&["roms/plus3.rom", "roms/plus2a.rom", "plus3.rom"]),
    }
}

/// Files named on the command line are dispatched by extension:
/// `.rom` replaces the ROM, `.tzx`/`.tap` load a tape, `.sna`/`.z80` a snapshot.
/// `--128` / `--48` choose the machine.
fn load_cli_files(spec: &mut Spectrum, roms: &mut ui::Roms, status: &mut String) -> bool {
    let mut opened_tape = false;
    for arg in std::env::args().skip(1) {
        if arg.starts_with("--") {
            continue; // handled before the machine was built
        }
        let path = std::path::PathBuf::from(&arg);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "tzx" | "tap" => match Tape::load(&path) {
                Ok(t) => {
                    // Loaded stopped: the tape waits for Play, like a real one.
                    *status = format!(
                        "Tape: {} ({} blocks) — press Play",
                        t.name,
                        t.blocks.len()
                    );
                    spec.bus.tape = Some(t);
                    opened_tape = true;
                }
                Err(e) => *status = format!("Tape load failed: {e}"),
            },
            "sna" | "z80" => {
                if let Ok(model) = snapshot::probe_model(&path) {
                    if model != spec.bus.model {
                        if let Some(rom) = roms.for_model(model) {
                            spec.set_model(model, rom);
                        }
                    }
                }
                match snapshot::load(spec, &path) {
                    Ok(()) => *status = format!("Loaded {}", path.display()),
                    Err(e) => *status = format!("Snapshot load failed: {e}"),
                }
            }
            "rom" | "bin" => match std::fs::read(&path) {
                Ok(data) => {
                    // The size says which machine the image belongs to, so a
                    // 32K ROM on the command line selects a 128K.
                    match ui::Roms::model_for_rom_size(data.len()) {
                        Some(model) => {
                            roms.set_for_model(model, data.clone());
                            if spec.bus.model == model {
                                spec.load_rom(&data);
                                spec.reset();
                            } else {
                                spec.set_model(model, &data);
                            }
                            *status = format!("ROM: {} ({})", path.display(), model.name());
                        }
                        None => {
                            spec.load_rom(&data);
                            spec.reset();
                            *status = format!(
                                "ROM: {} ({} bytes is an odd size)",
                                path.display(),
                                data.len()
                            );
                        }
                    }
                }
                Err(e) => *status = format!("ROM load failed: {e}"),
            },
            _ => *status = format!("Ignoring {arg}: unknown file type"),
        }
    }
    opened_tape
}

fn main() -> eframe::Result<()> {
    let mut roms = find_roms();
    let model = std::env::args()
        .find_map(|a| match a.as_str() {
            "--48" => Some(Model::Spectrum48),
            "--128" => Some(Model::Spectrum128),
            "--plus2a" | "--+2a" => Some(Model::Plus2A),
            "--plus3" | "--+3" => Some(Model::Plus3),
            _ => None,
        })
        .unwrap_or(Model::Spectrum48);

    let mut spec = Spectrum::with_model(model);
    let mut status = match roms.for_model(model) {
        Some(rom) => {
            spec.load_rom(rom);
            format!("{} ROM loaded", model.name())
        }
        None => {
            spec.load_rom(&demo_rom::DEMO_ROM);
            format!(
                "No {} ROM found (File ▸ Load ROM…). Running the built-in back-buffer demo.",
                model.name()
            )
        }
    };

    // Sound is optional: if no device opens, the emulator still runs silently.
    let (audio_out, audio_error) = match AudioOut::start() {
        Ok(out) => {
            spec.bus.audio.attach(out.queue.clone(), out.sample_rate);
            (Some(out), None)
        }
        Err(e) => (None, Some(e)),
    };

    let opened_tape = load_cli_files(&mut spec, &mut roms, &mut status);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("ZX Spectrum")
            .with_inner_size([720.0, 660.0]),
        ..Default::default()
    };
    eframe::run_native(
        "zx-spectrum-emulator",
        options,
        Box::new(move |_cc| {
            let mut app = ui::App::with_roms(spec, status, roms, audio_out);
            app.show_tape = opened_tape;
            app.audio_error = audio_error;
            Ok(Box::new(app))
        }),
    )
}
