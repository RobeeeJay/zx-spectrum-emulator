use zx_spectrum_emulator::{
    audio_out::AudioOut, demo_rom, machine::Model, machine::Spectrum, prefs::Prefs, snapshot,
    tape::Tape, ui,
};

/// Look for ROM images in ./roms. The 48K one is needed to boot anything real;
/// without it the built-in demo ROM runs instead.
fn find_roms(prefs: &Prefs) -> ui::Roms {
    // Spectrum ROMs are 16K or more; the ZX81's is 8K, so it needs its own
    // floor rather than being quietly rejected as too small.
    let read_min = |names: &[&str], min: usize| -> Option<Vec<u8>> {
        for n in names {
            if let Ok(d) = std::fs::read(n) {
                if d.len() >= min {
                    return Some(d);
                }
            }
        }
        None
    };
    let read = |names: &[&str]| read_min(names, 0x4000);
    let mut roms = ui::Roms {
        rom48: read(&["roms/48.rom", "roms/48k.rom", "48.rom"]),
        rom128: read(&["roms/128.rom", "roms/128k.rom", "128.rom"]),
        rom_plus3: read(&["roms/plus3.rom", "roms/plus2a.rom", "plus3.rom"]),
        rom_zx81: read_min(&["roms/zx81.rom", "zx81.rom"], 0x2000),
    };

    // Then whatever is in the directory the last ROM was opened from, so the
    // machines stay available between sessions.
    if let Some(dir) = &prefs.rom_dir {
        for (path, model) in ui::Roms::scan_directory(dir) {
            if roms.for_model(model).is_none() {
                if let Ok(data) = std::fs::read(&path) {
                    roms.set_for_model(model, data);
                }
            }
        }
    }
    roms
}

/// Files named on the command line are dispatched by extension:
/// `.rom` replaces the ROM, `.tzx`/`.tap` load a tape, `.sna`/`.z80` a snapshot.
/// `--128`, `--48`, `--zx81` and `--zx81-1k` choose the machine.
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
    // Created on first launch, so there is always a file to look at.
    let prefs = Prefs::load_or_create();
    let mut roms = find_roms(&prefs);
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
    let zx81_ram = std::env::args().find_map(|a| match a.as_str() {
        "--zx81" | "--zx81-16k" => Some(zx_spectrum_emulator::zx81::Ram::K16),
        "--zx81-1k" => Some(zx_spectrum_emulator::zx81::Ram::K1),
        _ => None,
    });

    // Put the main window back where it was last time.
    let mut viewport = eframe::egui::ViewportBuilder::default().with_title("ZX Spectrum");
    viewport = match prefs.window("main") {
        Some(r) => viewport
            .with_position([r.x, r.y])
            .with_inner_size([r.w, r.h]),
        None => viewport.with_inner_size([720.0, 660.0]),
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "zx-spectrum-emulator",
        options,
        Box::new(move |_cc| {
            let mut app = ui::App::with_roms(spec, status, roms, audio_out);
            app.prefs = prefs;
            app.apply_prefs();
            if let Some(ram) = zx81_ram {
                app.switch_to_zx81(ram);
            }
            app.show_tape = opened_tape;
            app.audio_error = audio_error;
            Ok(Box::new(app))
        }),
    )
}
