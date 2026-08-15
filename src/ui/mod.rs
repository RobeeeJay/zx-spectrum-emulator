//! egui front end: main window plus the detachable debug viewports.

pub mod back_buffer;
pub mod callflow;
pub mod cassette;
pub mod debugger;
pub mod profiler;
pub mod ram_map;
pub mod sprites;
pub mod tape;
pub mod theme;

use eframe::egui;
use egui::{ColorImage, TextureHandle, TextureOptions, ViewportBuilder, ViewportId};

use crate::audio_out::AudioOut;
use crate::machine::{self, Model, Spectrum, Stop};
use crate::prefs::{FileKind, Prefs, WindowRect};
use crate::screen;
use crate::snapshot;
use crate::zx81::{self, Zx81};

/// T-state of the pixel at (`px`, `py`), clamped into the frame.
fn beam_at(bus: &crate::machine::SpectrumBus, view: screen::View, px: usize, py: usize) -> u32 {
    let t = screen::t_at_pixel(view, bus.first_pixel_t(), bus.model.t_per_line(), px, py);
    t.clamp(0, bus.frame_t() as i64 - 1) as u32
}

/// Draw the ULA's beam over the picture: a line along what it is painting now
/// and a bright point where it has got to.
///
/// The line rather than only the point, because a point moving two pixels a
/// T-state is a speck nobody can follow; the line says which raster line the
/// machine is working on, which is what somebody watching a screen being
/// drawn wants to know.
fn draw_beam(
    painter: &egui::Painter,
    bus: &crate::machine::SpectrumBus,
    view: screen::View,
    picture: egui::Rect,
    scale: f32,
) {
    let (x, y) = screen::pixel_at_t(
        view,
        bus.first_pixel_t(),
        bus.model.t_per_line(),
        bus.tstates,
    );
    let (width, height) = (
        (view.border_x * 2 + 256) as i64,
        (view.border_top + 192 + view.border_bottom) as i64,
    );
    let at = |px: f32, py: f32| picture.min + egui::vec2(px * scale, py * scale);

    // Where it is, in the machine's own terms, so it can be read rather than
    // judged by eye. The line is counted from the first line of the display,
    // so the top border is negative — which is where the beam is when the
    // frame interrupt goes off.
    let line = y - view.border_top as i64;
    painter.text(
        picture.min + egui::vec2(4.0, 4.0),
        egui::Align2::LEFT_TOP,
        format!("T {} · line {line}", bus.tstates),
        egui::FontId::monospace(11.0),
        egui::Color32::from_rgba_unmultiplied(255, 240, 180, 200),
    );

    // Off the picture is where the beam really is during the flyback and the
    // top border a cropped view does not show, and drawing it at the edge
    // would say it was somewhere it is not.
    if y < 0 || y >= height {
        return;
    }
    let row = egui::Rect::from_min_max(at(0.0, y as f32), at(width as f32, y as f32 + 1.0));
    painter.rect_filled(
        row,
        0.0,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 36),
    );

    if (0..width).contains(&x) {
        // Two pixels wide: that is how much of the line goes out in one
        // T-state, so it is as narrow as the beam can honestly be drawn.
        let spot =
            egui::Rect::from_min_max(at(x as f32, y as f32), at(x as f32 + 2.0, y as f32 + 1.0));
        painter.rect_filled(
            spot,
            0.0,
            egui::Color32::from_rgba_unmultiplied(255, 240, 180, 220),
        );
    }
}

/// ROM images found at startup, used when switching machines.
#[derive(Default, Clone)]
pub struct Roms {
    pub rom48: Option<Vec<u8>>,
    pub rom128: Option<Vec<u8>>,
    /// The 64K four-ROM image shared by the +2A and +3.
    pub rom_plus3: Option<Vec<u8>>,
    /// The ZX81's ROM, which is a different machine entirely.
    pub rom_zx81: Option<Vec<u8>>,
}

impl Roms {
    pub fn for_model(&self, model: Model) -> Option<&Vec<u8>> {
        match model {
            Model::Spectrum48 => self.rom48.as_ref(),
            Model::Spectrum128 => self.rom128.as_ref(),
            Model::Plus2A | Model::Plus3 => self.rom_plus3.as_ref(),
        }
    }

    /// Which machine a ROM image is for, judged by its size: 16K is a 48K
    /// ROM, 32K a 128K/+2 one, 64K the four-ROM +2A/+3 image.
    pub fn model_for_rom_size(len: usize) -> Option<Model> {
        match len {
            0x4000 => Some(Model::Spectrum48),
            0x8000 => Some(Model::Spectrum128),
            0x10000 => Some(Model::Plus3),
            _ => None,
        }
    }

    pub fn set_for_model(&mut self, model: Model, data: Vec<u8>) {
        match model {
            Model::Spectrum48 => self.rom48 = Some(data),
            Model::Spectrum128 => self.rom128 = Some(data),
            Model::Plus2A | Model::Plus3 => self.rom_plus3 = Some(data),
        }
    }

    /// ROM images in a directory, recognised by size. Where several fit the
    /// same machine, one whose name mentions it wins (`128.rom` beats
    /// `something.rom`), otherwise the first in alphabetical order.
    pub fn scan_directory(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Model)> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                matches!(
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase())
                        .as_deref(),
                    Some("rom") | Some("bin")
                )
            })
            .collect();
        files.sort();

        let mut best: Vec<(std::path::PathBuf, Model)> = Vec::new();
        for path in files {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let Some(model) = Roms::model_for_rom_size(meta.len() as usize) else {
                continue;
            };
            let named = Roms::name_suggests(&path, model);
            match best.iter().position(|(_, m)| *m == model) {
                Some(i) => {
                    if named && !Roms::name_suggests(&best[i].0, model) {
                        best[i] = (path, model);
                    }
                }
                None => best.push((path, model)),
            }
        }
        best
    }

    /// Whether a file name mentions the machine its size implies.
    fn name_suggests(path: &std::path::Path, model: Model) -> bool {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match model {
            Model::Spectrum48 => name.contains("48"),
            Model::Spectrum128 => name.contains("128") || name.contains("plus2"),
            Model::Plus2A | Model::Plus3 => {
                name.contains("plus3") || name.contains("+3") || name.contains("plus2a")
            }
        }
    }

    /// File name to suggest when the ROM for a model is missing.
    pub fn expected_file(model: Model) -> &'static str {
        match model {
            Model::Spectrum48 => "roms/48.rom",
            Model::Spectrum128 => "roms/128.rom",
            Model::Plus2A | Model::Plus3 => "roms/plus3.rom",
        }
    }
}

/// How fast to run relative to a real Spectrum.
/// How a zoom factor is written on the dropdown.
pub fn zoom_label(scale: f32) -> String {
    if scale.fract() == 0.0 {
        format!("{scale:.0}x")
    } else {
        format!("{scale}x")
    }
}

/// The speed presets, as a dropdown. The slider beside it still takes any
/// value; the list is for the ones worth a single click.
pub fn speed_dropdown(speed: &mut f32, ui: &mut egui::Ui) {
    let selected = SPEED_PRESETS
        .iter()
        .find(|(_, mult)| (*speed - mult).abs() < f32::EPSILON)
        .map(|(name, _)| (*name).to_string())
        // Racing runs at four thousandths of speed, which rounds to "0%".
        .unwrap_or_else(|| {
            if (*speed - RACE_SPEED).abs() < f32::EPSILON {
                "5s/frame".to_string()
            } else if *speed < 0.01 {
                format!("{:.1}%", *speed * 100.0)
            } else {
                format!("{:.0}%", *speed * 100.0)
            }
        });
    theme::dropdown(ui, 74.0, selected, |ui| {
        for (name, mult) in SPEED_PRESETS {
            if ui
                .selectable_label((*speed - mult).abs() < f32::EPSILON, name)
                .clicked()
            {
                *speed = mult;
            }
        }
    });
}

/// The display sizes, as a dropdown.
pub fn zoom_dropdown(scale: &mut f32, ui: &mut egui::Ui) {
    theme::dropdown(ui, 74.0, zoom_label(*scale), |ui| {
        for size in screen::SCALES {
            if ui
                .selectable_label((*scale - size).abs() < f32::EPSILON, zoom_label(size))
                .clicked()
            {
                *scale = size;
            }
        }
    });
}

/// What the emulator is called: the ZX Spectrum by way of the language it is
/// written in.
pub const APP_NAME: &str = "ZX-Rustrum";

/// The most frames of a recording to play in one go, so a long pause or a
/// slow host frame does not run half the tape past in one step.
const MAX_RECORDED_FRAMES: f32 = 24.0;

/// How many places a recording is remembered as having reached. Enough to
/// document a program, not enough to grow without limit over a long recording.
const VISITED_CAP: usize = 4096;

/// How many hand-stepped instructions can be stepped back over. Enough to see
/// how the machine got where it is, which is what stepping back is for; a
/// hundred would be a recording, and there is one of those already.
pub const REWIND: usize = 20;

pub const SPEED_PRESETS: [(&str, f32); 8] = [
    ("1%", 0.01),
    ("5%", 0.05),
    ("10%", 0.1),
    ("25%", 0.25),
    ("50%", 0.5),
    ("100%", 1.0),
    ("200%", 2.0),
    ("Max", MAX_SPEED),
];

/// How long a write stays marked before it has blended into the colour it
/// should be, in seconds of the user's time. Long enough to see where a write
/// landed while the frame it landed in is still on screen.
pub const TINT_SECONDS: f32 = 2.0;

/// Five seconds to a frame: 0.02s of machine time in 5s of ours. Slow enough
/// that a frame's drawing can be followed by eye, which is the only speed at
/// which watching the beam tells anybody anything.
pub const RACE_SPEED: f32 = 0.02 / 5.0;

/// As fast as the emulator will go. The work per host frame is capped as well,
/// so this is a ceiling rather than a promise.
pub const MAX_SPEED: f32 = 20.0;

/// Putting a window back where it was left takes a few frames, and sometimes
/// does not take at all.
struct Placement {
    asked_at: std::time::Instant,
    settled: bool,
}

/// What the machine dropdown was asked for, decided after the list is closed
/// so the borrow of the app inside it has ended.
enum Machine {
    Spectrum(Model),
    Zx81(zx81::Ram),
}

/// A recording being played back, and where it has got to.
pub struct RzxPlayback {
    pub recording: crate::rzx::Recording,
    pub path: std::path::PathBuf,
    /// Which frame is next.
    pub frame: usize,
    /// What is left of the current frame, when a breakpoint stopped it
    /// part-way through.
    pub remaining: u32,
    /// Fractional frames carried over, so playback runs at the speed the
    /// emulator is set to rather than at whatever the host frame rate is.
    pub owed: f32,
    /// Addresses the recording has actually reached: what AutoDoc reads on
    /// top of what it can work out from the code alone.
    pub visited: std::collections::BTreeSet<u16>,
    /// Run it as fast as the host will go, rather than at the speed it was
    /// played. A recording is often twenty minutes long and the interesting
    /// part is rarely at the start.
    pub max_speed: bool,
}

impl RzxPlayback {
    /// How far through, for the status line.
    pub fn progress(&self) -> f32 {
        if self.recording.is_empty() {
            return 0.0;
        }
        self.frame as f32 / self.recording.len() as f32
    }

    pub fn finished(&self) -> bool {
        self.frame >= self.recording.len()
    }
}

pub struct App {
    pub spec: Spectrum,
    /// Labels and comments for the listing, and the file they are kept in.
    pub notes: crate::notes::Notes,
    /// The tape in the deck, if it came from a file. The notes belong beside
    /// whatever is being disassembled, which is the tape when there is one.
    pub tape_path: Option<std::path::PathBuf>,
    /// The ROM the current machine booted from, for the same reason.
    pub rom_path: Option<std::path::PathBuf>,
    /// When the notes last changed, so they are written a moment later.
    notes_changed_at: Option<std::time::Instant>,
    /// The recording being played back, if there is one.
    pub rzx: Option<RzxPlayback>,
    pub running: bool,
    pub speed: f32,
    pub status: String,

    pub show_ram_map: bool,
    pub show_debugger: bool,
    pub show_back_buffer: bool,
    /// The program's loop, drawn.
    pub show_callflow: bool,
    pub callflow: callflow::CallFlowState,
    /// Memory read as graphics.
    pub show_sprites: bool,
    pub sprites: sprites::SpriteView,
    pub show_tape: bool,
    pub show_profiler: bool,

    screen_pixels: Vec<u8>,
    screen_tex: Option<TextureHandle>,
    pub scale: f32,
    /// Show the whole overscan area, or crop the border to television size.
    pub overscan: bool,
    /// Follow the raster with the mouse: the frame is replayed from its
    /// interrupt to wherever the cursor is.
    pub cursor_beam: bool,
    /// Beam position under the cursor last frame, in T-states.
    pub beam_t: Option<u32>,
    /// Run at five seconds a frame with the picture fading behind the beam,
    /// so a frame can be watched being drawn.
    pub racing: bool,
    /// How bright a pixel is left by the time the beam comes round to it
    /// again, as a fraction of how bright it was drawn.
    pub fade_floor: f32,
    /// The speed to go back to when racing is switched off.
    speed_before_race: f32,
    /// The frame being raced: a copy of the machine taken at the interrupt,
    /// run forward to wherever the cursor is. Only ever set while the machine
    /// is stopped.
    pub race: Option<crate::race::Race>,

    pub ram: ram_map::RamMapState,
    pub dbg: debugger::DebuggerState,
    pub back: back_buffer::BackBufferState,
    pub tape: tape::TapeWindowState,
    pub profiler: profiler::ProfilerWindowState,

    pub last_stop: Option<Stop>,
    /// The last few instructions stepped by hand, newest last, so they can be
    /// stepped back over. What each one holds is what it changed rather than a
    /// copy of the machine.
    pub rewind: std::collections::VecDeque<crate::machine::Undo>,
    /// The machine as it was when recording started, waiting to be written
    /// into the file when it stops.
    pub recording_from: Option<Vec<u8>>,
    /// Emulated T-states left over from the previous host frame.
    leftover: f32,

    /// True when `status` is a failure the user should notice.
    pub status_is_error: bool,
    /// Model shown in the window title, so a switch is always visible.
    title_shown: String,

    /// How far ahead of the sound device to stay, in seconds.
    pub audio_latency_target: f32,

    /// The ZX81, when that is the machine in use.
    pub zx81: Option<Zx81>,
    /// Which RAM the ZX81 has fitted, remembered across switches.
    pub zx81_ram: zx81::Ram,

    /// Remembered settings, including the directories files were opened from.
    pub prefs: Prefs,
    /// The layout as last written, and when: the file is rewritten a moment
    /// after things settle, so a crash or a kill does not lose it.
    last_saved: Option<String>,
    last_save_at: Option<std::time::Instant>,

    pub roms: Roms,
    /// Kept alive for as long as the app runs; dropping it stops the sound.
    pub audio_out: Option<AudioOut>,
    /// Whether the theme has been applied to the context yet.
    styled: bool,
    /// The cassette artwork, rasterised from the SVGs.
    pub art: cassette::Art,
    /// How each debug window's placement is going.
    placed: std::collections::HashMap<&'static str, Placement>,
    pub audio_error: Option<String>,
}

impl App {
    pub fn new(spec: Spectrum, status: String) -> Self {
        Self::with_roms(spec, status, Roms::default(), None)
    }

    pub fn with_roms(
        spec: Spectrum,
        status: String,
        roms: Roms,
        audio_out: Option<AudioOut>,
    ) -> Self {
        App {
            spec,
            notes: crate::notes::Notes::unattached(),
            tape_path: None,
            rom_path: None,
            notes_changed_at: None,
            rzx: None,
            running: true,
            speed: 1.0,
            status,
            show_ram_map: true,
            show_debugger: true,
            show_back_buffer: false,
            show_callflow: false,
            callflow: callflow::CallFlowState::default(),
            show_sprites: false,
            sprites: sprites::SpriteView::default(),
            show_tape: false,
            show_profiler: false,
            screen_pixels: vec![0; screen::View::OVERSCAN.buffer_len()],
            screen_tex: None,
            scale: 2.0,
            overscan: true,
            cursor_beam: false,
            racing: false,
            fade_floor: 0.5,
            speed_before_race: 1.0,
            race: None,
            beam_t: None,
            ram: ram_map::RamMapState::default(),
            dbg: debugger::DebuggerState::default(),
            back: back_buffer::BackBufferState::default(),
            tape: tape::TapeWindowState::default(),
            profiler: profiler::ProfilerWindowState::default(),
            last_stop: None,
            rewind: std::collections::VecDeque::new(),
            recording_from: None,
            leftover: 0.0,
            status_is_error: false,
            title_shown: String::new(),
            audio_latency_target: 0.06,
            zx81: None,
            zx81_ram: zx81::Ram::K16,
            prefs: Prefs::default(),
            last_saved: None,
            last_save_at: None,
            roms,
            audio_out,
            styled: false,
            art: cassette::Art::default(),
            placed: std::collections::HashMap::new(),
            audio_error: None,
        }
    }

    /// Switch between the 48K and 128K machines, which needs the matching ROM.
    pub fn switch_model(&mut self, model: Model) {
        if self.zx81.take().is_some() {
            // Coming back from the ZX81; the Spectrum is still as it was.
            self.running = true;
            self.set_status(format!("Switched to {}", model.name()), false);
            if self.spec.bus.model == model {
                return;
            }
        }
        if self.spec.bus.model == model {
            self.set_status(format!("Already running as {}", model.name()), false);
            return;
        }
        let Some(rom) = self.roms.for_model(model).cloned() else {
            self.set_status(
                format!(
                    "Cannot switch to {}: no ROM. Put a {} ROM at {}, or use File ▸ Load ROM…",
                    model.name(),
                    model.name(),
                    Roms::expected_file(model)
                ),
                true,
            );
            return;
        };
        self.spec.set_model(model, &rom);
        self.leftover = 0.0;
        self.running = true;
        self.set_status(format!("Switched to {}", model.name()), false);
    }

    /// Load a ROM image, switching to the machine its size implies. A 32K
    /// image is a 128K ROM, so loading one on a 48K has to change machine or
    /// the image would simply be truncated.
    pub fn load_rom_image(&mut self, path: &std::path::Path, data: Vec<u8>) {
        let Some(model) = Roms::model_for_rom_size(data.len()) else {
            let len = data.len();
            self.spec.load_rom(&data);
            self.spec.reset();
            self.set_status(
                format!(
                    "{}: {len} bytes is not a 16K, 32K or 64K ROM. Loaded it into the {} as-is.",
                    path.display(),
                    self.spec.bus.model.name()
                ),
                true,
            );
            return;
        };

        // Remember it, so the toolbar offers that machine from now on.
        self.roms.set_for_model(model, data.clone());
        self.rom_path = Some(path.to_path_buf());
        self.reload_notes();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        if self.spec.bus.model == model {
            self.spec.load_rom(&data);
            self.spec.reset();
        } else {
            self.spec.set_model(model, &data);
            self.leftover = 0.0;
        }
        self.running = true;

        self.prefs.remember_file(FileKind::Rom, path);
        // ROMs tend to arrive in sets, so pick up the machines this one's
        // neighbours can provide.
        let also = match path.parent() {
            Some(dir) => self.adopt_roms_from(dir),
            None => Vec::new(),
        };
        let extra = if also.is_empty() {
            String::new()
        } else {
            format!("; also found {}", also.join(", "))
        };
        self.set_status(format!("Loaded {name} as a {}{extra}", model.name()), false);
    }

    /// Fill in ROMs for machines that have none from the images in `dir`.
    /// Returns what was adopted, for the status line.
    pub fn adopt_roms_from(&mut self, dir: &std::path::Path) -> Vec<String> {
        let mut adopted = Vec::new();
        for (path, model) in Roms::scan_directory(dir) {
            if self.roms.for_model(model).is_some() {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            self.roms.set_for_model(model, data);
            adopted.push(format!(
                "{} ({})",
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                model.name()
            ));
        }
        adopted
    }

    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status = text;
        self.status_is_error = is_error;
    }

    /// Keep the window title in step with the machine, so switching is
    /// visible even if the screen happens to look similar.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let machine = if self.zx81.is_some() {
            self.zx81_ram.name().to_string()
        } else {
            format!("ZX Spectrum {}", self.spec.bus.model.name())
        };
        let want = format!("{APP_NAME} — {machine}");
        if self.title_shown != want {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(want.clone()));
            self.title_shown = want;
        }
    }

    /// Every machine the emulator can be, in one list. Those whose ROM is
    /// missing stay in it, saying so, rather than disappearing — otherwise
    /// there is nothing to click to find out what is wanted.
    fn machine_dropdown(&mut self, ui: &mut egui::Ui) {
        let selected = if self.on_zx81() {
            self.zx81_ram.name().to_string()
        } else {
            self.spec.bus.model.name().to_string()
        };
        let mut chosen: Option<Machine> = None;
        theme::dropdown(ui, 120.0, selected, |ui| {
            for model in [
                Model::Spectrum48,
                Model::Spectrum128,
                Model::Plus2A,
                Model::Plus3,
            ] {
                let current = !self.on_zx81() && self.spec.bus.model == model;
                let have_rom = self.roms.for_model(model).is_some();
                let label = if have_rom {
                    model.name().to_string()
                } else {
                    format!("{} (no ROM)", model.name())
                };
                if ui
                    .selectable_label(current, label)
                    .on_hover_text(if have_rom {
                        format!("Switch to the {} and reset", model.name())
                    } else {
                        format!("Needs {}", Roms::expected_file(model))
                    })
                    .clicked()
                {
                    chosen = Some(Machine::Spectrum(model));
                }
            }
            for ram in [zx81::Ram::K1, zx81::Ram::K16] {
                let current = self.on_zx81() && self.zx81_ram == ram;
                let have_rom = self.roms.rom_zx81.is_some();
                let label = if have_rom {
                    ram.name().to_string()
                } else {
                    format!("{} (no ROM)", ram.name())
                };
                if ui
                    .selectable_label(current, label)
                    .on_hover_text(if have_rom {
                        format!("Switch to a {} and reset", ram.name())
                    } else {
                        "Needs roms/zx81.rom".to_string()
                    })
                    .clicked()
                {
                    chosen = Some(Machine::Zx81(ram));
                }
            }
        });
        match chosen {
            Some(Machine::Spectrum(model)) => self.switch_model(model),
            Some(Machine::Zx81(ram)) => self.switch_to_zx81(ram),
            None => {}
        }
    }

    /// The second toolbar row: reset, and which windows are open. The machine,
    /// speed, zoom and video controls are on the row above.
    fn machine_row(&mut self, ui: &mut egui::Ui) {
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 8.0);
        let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
        ui.allocate_ui_with_layout(row, layout, |ui| {
            theme::group_label(ui, "Windows");
            theme::toggle(ui, &mut self.show_ram_map, "RAM map");
            theme::toggle(ui, &mut self.show_debugger, "Debugger");
            theme::toggle(ui, &mut self.show_tape, "Tape");
            theme::toggle(ui, &mut self.show_back_buffer, "Back buffer");
            theme::toggle(ui, &mut self.show_sprites, "Graphics");
            theme::toggle(ui, &mut self.show_callflow, "Call flow");
            theme::toggle(ui, &mut self.show_profiler, "Profiler");
        });
    }

    /// Start recording what the machine does, from where it is now.
    ///
    /// The snapshot is taken first: a recording is a machine to start from and
    /// then every byte read from a port after it, and one without the first is
    /// a list of numbers.
    pub fn start_recording(&mut self) {
        if self.rzx.is_some() {
            self.set_status("A recording is already playing".to_string(), true);
            return;
        }
        self.recording_from = Some(snapshot::save_sna(&self.spec));
        self.spec.bus.capture = Some(crate::rzx::Capture {
            start_t: self.spec.bus.tstates,
            mark: self.spec.bus.fetches,
            ..Default::default()
        });
        self.set_status("Recording".to_string(), false);
    }

    /// How many frames have been recorded so far, if anything is being.
    pub fn recorded_frames(&self) -> Option<usize> {
        self.spec
            .bus
            .capture
            .as_ref()
            .map(|capture| capture.frames.len())
    }

    /// Stop recording and hand back the file's bytes, or nothing if there was
    /// nothing to record.
    pub fn stop_recording(&mut self) -> Option<Vec<u8>> {
        let capture = self.spec.bus.capture.take()?;
        let snapshot = self.recording_from.take();
        if capture.frames.is_empty() {
            self.set_status("Nothing was recorded".to_string(), true);
            return None;
        }
        let recording = crate::rzx::Recording {
            creator: APP_NAME.to_string(),
            snapshot: snapshot.map(|data| crate::rzx::Snapshot {
                extension: "sna".to_string(),
                data,
            }),
            frames: capture.frames,
            start_t: capture.start_t,
        };
        Some(crate::rzx::write(&recording))
    }

    /// Where a recording should go by default: beside the tape that is in the
    /// deck, under the same name. A machine with nothing loaded has nothing to
    /// be named after, so it gets a plain one.
    pub fn recording_path(&self) -> std::path::PathBuf {
        match &self.tape_path {
            Some(path) => path.with_extension("rzx"),
            None => std::path::PathBuf::from("recording.rzx"),
        }
    }

    /// Where a snapshot should go by default: beside the tape in the deck,
    /// under the same name. A machine with nothing loaded has nothing to be
    /// named after, so it gets a plain one.
    pub fn snapshot_path(&self) -> std::path::PathBuf {
        match &self.tape_path {
            Some(path) => path.with_extension("sna"),
            None => std::path::PathBuf::from("snapshot.sna"),
        }
    }

    /// Write the machine as it stands out as a `.sna`.
    ///
    /// A snapshot is the machine's registers and its RAM, and nothing about
    /// where the tape had reached or what the sound was doing: those are not
    /// in the format, and a snapshot that claimed to hold them would be
    /// lying about what comes back.
    pub fn save_snapshot(&mut self) {
        if self.on_zx81() {
            self.set_status("The ZX81 has no snapshot format here".to_string(), true);
            return;
        }
        let bytes = snapshot::save_sna(&self.spec);
        let suggested = self.snapshot_path();
        let dialog = rfd::FileDialog::new()
            .add_filter("Snapshot", &["sna"])
            .set_file_name(
                suggested
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "snapshot.sna".to_string()),
            );
        let directory = suggested
            .parent()
            .filter(|parent| parent.is_dir())
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.prefs.dir_for(FileKind::Snapshot).cloned())
            .filter(|dir| dir.is_dir());
        let dialog = match directory {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        };
        let Some(path) = dialog.save_file() else {
            self.set_status("Snapshot not saved".to_string(), false);
            return;
        };
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                self.prefs.remember_file(FileKind::Snapshot, &path);
                self.set_status(
                    format!("Wrote {} ({} bytes)", path.display(), bytes.len()),
                    false,
                );
            }
            Err(e) => self.set_status(format!("Could not write {}: {e}", path.display()), true),
        }
    }

    /// Stop recording and ask where to put it.
    ///
    /// The dialog opens beside the tape in the deck, under the same name with
    /// an `.rzx` on it, which is where somebody would go looking for a
    /// recording of that game.
    pub fn save_recording(&mut self) {
        let Some(bytes) = self.stop_recording() else {
            return;
        };
        let suggested = self.recording_path();
        let dialog = rfd::FileDialog::new().set_file_name(
            suggested
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "recording.rzx".to_string()),
        );
        // Beside the tape if there is one, and otherwise wherever the last
        // recording went.
        let directory = suggested
            .parent()
            .filter(|parent| parent.is_dir())
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.prefs.dir_for(FileKind::Recording).cloned())
            .filter(|dir| dir.is_dir());
        let dialog = match directory {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        };
        let Some(path) = dialog.save_file() else {
            self.set_status("Recording thrown away".to_string(), true);
            return;
        };
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                // Where recordings go, for the next time one is opened or
                // written.
                self.prefs.remember_file(FileKind::Recording, &path);
                self.set_status(
                    format!("Wrote {} ({} bytes)", path.display(), bytes.len()),
                    false,
                );
            }
            Err(e) => self.set_status(format!("Could not write {}: {e}", path.display()), true),
        }
    }

    /// A file picker that opens where the last file of that kind came from.
    pub fn pick_file(&self, kind: Option<FileKind>) -> Option<std::path::PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        dialog = match kind {
            Some(FileKind::Rom) => dialog.add_filter("ROM image", &["rom", "bin"]),
            Some(FileKind::Tape) => {
                dialog.add_filter("Tape", &["tzx", "tap", "p", "81", "p81", "zip"])
            }
            Some(FileKind::Snapshot) => dialog.add_filter("Snapshot", &["sna", "z80"]),
            // No filter, deliberately. rfd's macOS backend sets the panel's
            // allowed types from the extension list through an API that wants
            // types the system knows, and nothing on the machine claims
            // `.rzx`: the recordings end up greyed out and unselectable. The
            // file is checked when it is opened instead.
            Some(FileKind::Recording) => dialog,
            None => dialog
                .add_filter(
                    "Tape, snapshot or ROM",
                    &[
                        "tzx", "tap", "p", "81", "p81", "sna", "z80", "rom", "bin", "rzx", "zip",
                    ],
                )
                .add_filter("Tape", &["tzx", "tap", "p", "81", "p81"])
                .add_filter("Snapshot", &["sna", "z80"])
                .add_filter("Archive", &["zip"])
                .add_filter("ROM image", &["rom", "bin"]),
        };
        // For the catch-all picker, start wherever the most recent file of any
        // kind came from.
        let start = match kind {
            Some(k) => self.prefs.dir_for(k).cloned(),
            None => self
                .prefs
                .tape_dir
                .clone()
                .or_else(|| self.prefs.snapshot_dir.clone())
                .or_else(|| self.prefs.rom_dir.clone()),
        };
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file()
    }

    /// One file picker for every supported type, dispatched by extension.
    pub fn load_any_file(&mut self) {
        if let Some(path) = self.pick_file(None) {
            self.load_path(&path);
        }
    }

    /// Put a tape in the deck of whichever machine is running.
    /// Load whatever is worth loading out of an archive.
    ///
    /// The first file of a kind this can read, which is how a download of one
    /// game is usually shaped: the tape, a scan of the inlay and a text file
    /// about the cracking group. An archive with nothing loadable in it does
    /// nothing, rather than guessing at the readme.
    fn load_zip(&mut self, path: &std::path::Path) {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(e) => {
                return self.set_status(format!("Could not read {}: {e}", path.display()), true)
            }
        };
        let wanted = ["tzx", "tap", "p", "81", "p81", "rzx", "sna", "z80"];
        let Some((name, bytes)) = crate::zip::first_with_extension(&data, &wanted) else {
            return self.set_status(
                format!(
                    "{} holds nothing this can load",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                true,
            );
        };

        let inner = std::path::Path::new(&name);
        let ext = inner
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rzx" => self.load_recording_bytes(path, &bytes),
            "sna" | "z80" => match snapshot::probe_model_bytes(&ext, &bytes) {
                Ok(model) => {
                    self.switch_model(model);
                    let loaded = if ext == "sna" {
                        snapshot::load_sna(&mut self.spec, &bytes)
                    } else {
                        snapshot::load_z80(&mut self.spec, &bytes)
                    };
                    match loaded {
                        Ok(()) => {
                            self.set_status(format!("Loaded {name} from {}", path.display()), false)
                        }
                        Err(e) => self.set_status(format!("{name}: {e}"), true),
                    }
                }
                Err(e) => self.set_status(format!("{name}: {e}"), true),
            },
            _ => {
                // A ZX81 program only plays into a ZX81, the same as one that
                // arrived on its own.
                if matches!(ext.as_str(), "p" | "81" | "p81") && !self.on_zx81() {
                    self.switch_to_zx81(self.zx81_ram);
                }
                self.insert_tape_bytes(path, &name, &bytes);
            }
        }
    }

    fn insert_tape(&mut self, path: &std::path::Path) {
        match crate::tape::Tape::load(path) {
            Ok(t) => self.accept_tape(path, t),
            Err(e) => self.set_status(format!("Tape load failed: {e}"), true),
        }
    }

    /// The same, for a tape that came out of an archive. The notes still go
    /// beside the archive: that is the file the user has, and unpacking it
    /// somewhere temporary to hold the annotations would lose them.
    fn insert_tape_bytes(&mut self, path: &std::path::Path, name: &str, data: &[u8]) {
        match crate::tape::Tape::from_bytes(name, data) {
            Ok(t) => self.accept_tape(path, t),
            Err(e) => self.set_status(format!("Tape load failed: {e}"), true),
        }
    }

    fn accept_tape(&mut self, path: &std::path::Path, t: crate::tape::Tape) {
        let blocks = t.blocks.len();
        let zx81 = matches!(t.blocks.first(), Some(crate::tape::Block::Zx81 { .. }));
        let name = t.name.clone();
        self.set_tape(Some(t));
        self.show_tape = true;
        self.tape.scroll_to_current = true;
        self.tape.last_block = None;
        self.prefs.remember_file(FileKind::Tape, path);
        self.tape_path = Some(path.to_path_buf());
        self.reload_notes();
        // A tape waits for Play, as a real one does; a ZX81 also needs
        // LOAD "" typed at it first, which is easy to forget.
        let hint = if zx81 {
            " — type LOAD \"\" then press Play"
        } else {
            " — press Play"
        };
        let plural = if blocks == 1 { "block" } else { "blocks" };
        self.set_status(format!("Tape: {name} ({blocks} {plural}){hint}"), false);
    }

    /// Write the notes out a little after they last changed.
    ///
    /// AutoDoc writes into them without anybody touching a field, so waiting
    /// for one to lose focus would leave a session's worth of guesses unsaved.
    /// The delay keeps it to one write rather than one per frame while the
    /// listing is being scrolled about.
    fn save_notes_if_due(&mut self) {
        const AFTER: std::time::Duration = std::time::Duration::from_secs(3);
        if !self.notes.is_dirty() {
            self.notes_changed_at = None;
            return;
        }
        let since = *self
            .notes_changed_at
            .get_or_insert_with(std::time::Instant::now);
        if since.elapsed() < AFTER {
            return;
        }
        self.notes_changed_at = None;
        if let Err(e) = self.notes.save_if_dirty() {
            self.set_status(format!("Could not save notes: {e}"), true);
        }
    }

    /// Load a recording and start it playing.
    ///
    /// The snapshot inside it says which machine it was made on, so that one
    /// is brought up first; everything after that is the recording's doing.
    fn load_recording(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                return self.set_status(format!("Could not read {}: {e}", path.display()), true)
            }
        };
        self.load_recording_bytes(path, &bytes);
        // Where recordings are kept, for the next time one is opened. Only
        // for a file that is really a recording on disk: an archive is
        // remembered as whatever it was opened as.
        self.prefs.remember_file(FileKind::Recording, path);
    }

    /// A recording from bytes rather than from a file, so one that arrived
    /// inside an archive can be played without being written out first. The
    /// path is still the archive's, which is where the notes go.
    fn load_recording_bytes(&mut self, path: &std::path::Path, bytes: &[u8]) {
        let recording = match crate::rzx::parse(bytes) {
            Ok(recording) => recording,
            Err(e) => return self.set_status(format!("{}: {e}", path.display()), true),
        };
        let Some(snapshot) = recording.snapshot.clone() else {
            return self.set_status(
                "The recording carries no snapshot to start from".into(),
                true,
            );
        };

        match snapshot::probe_model_bytes(&snapshot.extension, &snapshot.data) {
            Ok(model) => self.switch_model(model),
            Err(e) => return self.set_status(format!("{}: {e}", path.display()), true),
        }
        let loaded = match snapshot.extension.as_str() {
            "sna" => snapshot::load_sna(&mut self.spec, &snapshot.data),
            "z80" => snapshot::load_z80(&mut self.spec, &snapshot.data),
            other => Err(format!(
                "unsupported snapshot type in the recording: .{other}"
            )),
        };
        if let Err(e) = loaded {
            return self.set_status(format!("{}: {e}", path.display()), true);
        }

        let frames = recording.len();
        let by = if recording.creator.is_empty() {
            String::new()
        } else {
            format!(" by {}", recording.creator)
        };
        self.rzx = Some(RzxPlayback {
            recording,
            path: path.to_path_buf(),
            frame: 0,
            remaining: 0,
            owed: 0.0,
            visited: Default::default(),
            max_speed: false,
        });
        self.spec.bus.playback = Some(crate::machine::Playback::default());
        // A machine that was paused stays paused: somebody who stopped it to
        // look at something has not asked for a recording to start running the
        // moment it is loaded, and the first frame of one is worth looking at.
        let waiting = !self.running;
        // The notes belong beside the recording now: it is what is being read.
        self.reload_notes();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        self.set_status(
            if waiting {
                format!("Loaded {name}{by}: {frames} frames, paused. Press Run to play it.")
            } else {
                format!(
                    "Playing {name}{by}: {frames} frames. The keyboard is the \
                     recording's, not yours."
                )
            },
            false,
        );
    }

    /// Stop playing, and give the machine back to the user.
    pub fn stop_playback(&mut self) {
        self.rzx = None;
        self.spec.bus.playback = None;
        self.reload_notes();
    }

    /// Play the recording forward by however much time has passed.
    fn advance_recording(&mut self, dt: f32, boost: f32) {
        let Some(rzx) = &self.rzx else { return };
        if rzx.finished() {
            let done = rzx.recording.len();
            self.stop_playback();
            self.running = false;
            self.set_status(format!("The recording ended after {done} frames"), false);
            return;
        }

        // A recording is measured in frames rather than T-states, so the speed
        // control counts frames. At maximum speed it runs as many as the cap
        // allows every host frame instead of counting at all.
        //
        // The budget is kept as a fraction of a frame rather than a whole
        // number of them: below one frame per host frame — which is where
        // Race the Beam lives, at five seconds a frame — a whole number is
        // zero nearly always and one now and then, so the picture jumps a
        // frame at a time instead of the beam crawling down it.
        let mut budget = if rzx.max_speed {
            MAX_RECORDED_FRAMES
        } else {
            let rate = crate::machine::CPU_HZ as f32 / self.spec.bus.model.frame_t() as f32;
            (rzx.owed + dt * rate * self.speed * boost).min(MAX_RECORDED_FRAMES)
        };

        // In slow motion the frame is run in pieces, so the picture moves
        // under the beam instead of jumping a frame at a time; at normal
        // speed and above, whole frames, with the fraction carried over.
        //
        // From the speed the user asked for, not from the budget: a frame of
        // a 128K recording is a little longer than the fiftieth of a second a
        // host frame takes, so at full speed the budget is 0.99 frames a call
        // and every frame would be split for no reason.
        let crawling = self.speed < 1.0 && !rzx.max_speed;
        while budget > 0.0 {
            let Some(rzx) = &mut self.rzx else { return };
            if rzx.finished() {
                return;
            }
            let whole_frame = rzx.recording.frames[rzx.frame].fetches.max(1) as u32;
            // What the budget will pay for, of the frame the recording is on.
            // A budget too small to buy a single fetch buys nothing and is
            // carried over: rounding it up to one would run a sliver of the
            // next frame after every whole one, and leave the machine a few
            // dozen instructions into a frame it has not been asked to begin.
            let started = rzx.remaining > 0;
            let ask = if budget >= 1.0 {
                if started {
                    rzx.remaining
                } else {
                    whole_frame
                }
            } else if crawling || started {
                let piece = (budget * whole_frame as f32) as u32;
                if started {
                    rzx.remaining.min(piece)
                } else {
                    piece
                }
            } else {
                0
            };
            if ask == 0 {
                break;
            }
            // A frame that was interrupted part-way through is picked up where
            // it stopped, with the input it had already been handed. A frame
            // is not begun — and its input not put in front of the machine —
            // until there is budget to run some of it.
            if !started {
                let frame = &rzx.recording.frames[rzx.frame];
                let fetches = frame.fetches as u32;
                let inputs = frame.inputs.clone();
                if let Some(rzx) = &mut self.rzx {
                    rzx.remaining = fetches;
                }
                if let Some(playback) = &mut self.spec.bus.playback {
                    playback.inputs = inputs;
                    playback.cursor = 0;
                }
            }
            let (stop, ran) = self.spec.run_fetches(ask);
            budget -= ran as f32 / whole_frame as f32;
            let pc = self.spec.cpu.pc;

            let mut ended = false;
            if let Some(rzx) = &mut self.rzx {
                // A prefixed instruction is two fetches or more, so the last
                // one can carry past what was asked for. Saturating rather
                // than wrapping: the frame is over either way, and taking the
                // difference off an unsigned count leaves it enormous, which
                // ran the frame to the end of the recording and read input
                // that was never recorded.
                rzx.remaining = rzx.remaining.saturating_sub(ran);
                if rzx.remaining == 0 {
                    rzx.frame += 1;
                    // Where the recording actually got to: AutoDoc reads this
                    // on top of what it can work out from the code alone.
                    if rzx.visited.len() < VISITED_CAP {
                        rzx.visited.insert(pc);
                    }
                    ended = true;
                }
            }
            if ended {
                // The frame boundary is the recording's, so the video frame
                // ends here and the interrupt is raised here — not wherever
                // the T-state count happens to have reached. On the machine
                // the recording was made on these were the same moment, and
                // letting them drift apart puts every screen effect at the
                // wrong height.
                self.spec.bus.end_frame_here();
                self.spec.bus.raise_interrupt();
            }
            self.last_stop = Some(stop);
            if !matches!(stop, Stop::Budget) {
                self.handle_stop(stop);
                return;
            }
        }
        // What is left over is owed to the next host frame, so a pace of a
        // fifth of a frame a second adds up to a frame every five seconds.
        if let Some(rzx) = &mut self.rzx {
            rzx.owed = budget.max(0.0);
        }
    }

    /// The file the notes and symbols belong to: the recording if one is
    /// playing, otherwise the tape, otherwise the ROM.
    pub fn notes_source(&self) -> Option<std::path::PathBuf> {
        self.rzx
            .as_ref()
            .map(|rzx| rzx.path.clone())
            .or_else(|| self.tape_path.clone())
            .or_else(|| self.rom_path.clone())
    }

    /// Where names supplied by hand are read from: one file beside whatever is
    /// being disassembled, and one shared file in the preferences directory.
    ///
    /// The shared one is where a ROM disassembly goes. It describes the
    /// machine rather than any one game, so tying it to a tape would mean
    /// copying it beside every tape.
    pub fn symbol_files(&self) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        if let Some(dir) = crate::prefs::config_dir() {
            // Per machine as well as shared: the ZX81 and the Spectrum have
            // entirely different routines at the same addresses, so one file
            // for both would name each after the other.
            let machine = if self.on_zx81() {
                "zx81"
            } else {
                match self.spec.bus.model {
                    Model::Spectrum48 => "48",
                    Model::Spectrum128 => "128",
                    Model::Plus2A => "plus2a",
                    Model::Plus3 => "plus3",
                }
            };
            files.push(dir.join("symbols.txt"));
            files.push(dir.join(format!("symbols-{machine}.txt")));
            // The 128K's ROM is two 16K ROMs and the +3's is four, each
            // addressed $0000-$3FFF in its own right. A name only means
            // anything alongside which one is paged in, so they have a file
            // each and only the one in use is read.
            if !self.on_zx81() && self.spec.bus.rom_pages() > 1 {
                let rom = self.spec.bus.rom_in_use();
                files.push(dir.join(format!("symbols-{machine}-rom{rom}.txt")));
            }
        }
        if let Some(source) = self.notes_source() {
            // Named after the file itself: `manic.tap` has `manic.symbols.txt`
            // beside it, not `manic.zxrs.symbols.txt`.
            files.push(source.with_extension("symbols.txt"));
        }
        files
    }

    /// Point the notes at whatever is being disassembled: the tape in the
    /// deck if there is one, otherwise the ROM the machine booted from.
    /// Anything unsaved is written out first, so switching tapes does not
    /// throw away what was just typed.
    pub fn reload_notes(&mut self) {
        if let Err(e) = self.notes.save_if_dirty() {
            self.set_status(format!("Could not save notes: {e}"), true);
        }
        // A recording is what is being read when one is playing, so its notes
        // go beside it rather than beside the tape or the ROM.
        let source = self.notes_source();
        self.notes = match source {
            Some(path) => crate::notes::Notes::for_file(&path),
            None => crate::notes::Notes::unattached(),
        };
    }

    pub fn load_path(&mut self, path: &std::path::Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "tzx" | "tap" => self.insert_tape(path),
            // A ZX81 program only plays into a ZX81, so bring one up first.
            "p" | "81" | "p81" => {
                if !self.on_zx81() {
                    self.switch_to_zx81(self.zx81_ram);
                }
                if self.on_zx81() {
                    self.insert_tape(path);
                }
            }
            "rzx" => self.load_recording(path),
            "sna" | "z80" => match snapshot::probe_model(path) {
                Ok(model) => {
                    self.switch_model(model);
                    match snapshot::load(&mut self.spec, path) {
                        Ok(()) => {
                            self.prefs.remember_file(FileKind::Snapshot, path);
                            self.set_status(format!("Loaded {}", path.display()), false)
                        }
                        Err(e) => self.set_status(format!("Snapshot load failed: {e}"), true),
                    }
                }
                Err(e) => self.set_status(format!("Snapshot load failed: {e}"), true),
            },
            "rom" | "bin" => match std::fs::read(path) {
                Ok(data) => self.load_rom_image(path, data),
                Err(e) => self.set_status(format!("ROM load failed: {e}"), true),
            },
            "zip" => self.load_zip(path),
            other => self.set_status(format!("Unsupported file type: .{other}"), true),
        }
    }

    /// Note where a window is now, so it can be put back next time.
    fn remember_window(&mut self, name: &str, ctx: &egui::Context) {
        let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
        // Position comes from the outer rectangle and size from the inner one,
        // to match what the viewport builder takes: mixing them would grow
        // every window by the height of its title bar on each launch.
        if let (Some(outer), Some(inner)) = (outer, inner) {
            // The tape window's width is not up for negotiation, so it is not
            // recorded either: only its height and where it sits.
            let w = if name == "tape" {
                cassette::WINDOW_W
            } else {
                inner.width()
            };
            self.prefs.set_window(
                name,
                WindowRect {
                    x: outer.min.x,
                    y: outer.min.y,
                    w,
                    h: inner.height(),
                },
            );
        }
    }

    /// Whether a debug window has been put back where it was left. Closing one
    /// clears this, so reopening places it again.
    pub fn window_is_placed(&self, name: &str) -> bool {
        self.placed.contains_key(name)
    }

    /// Fix a window's width at the window manager's level, so a sideways drag
    /// is refused rather than allowed and then undone.
    ///
    /// Sent as commands rather than left to the viewport builder: the builder
    /// is only diffed against the frame before, so a window that is rebuilt —
    /// which happens whenever it has not been drawn for a while — comes back
    /// without the constraint.
    fn fix_width(&self, ctx: &egui::Context, width: f32) {
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize([width, 320.0].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize([width, 8000.0].into()));
    }

    /// Put a debug window back where it was left.
    ///
    /// The geometry in the viewport builder is not reliably honoured when the
    /// window is created — the window manager may centre a default-sized one
    /// instead — so it is sent again from inside the viewport, every frame,
    /// until the window reports that it has arrived. Recording where the
    /// window is only starts then, or the position it is being moved away
    /// from would be saved over the real one.
    ///
    /// Returns whether the window has settled, and can be recorded.
    fn place_window(
        &mut self,
        name: &'static str,
        ctx: &egui::Context,
        default_pos: [f32; 2],
        default_size: [f32; 2],
    ) -> bool {
        /// How long to keep asking before letting the window be where it is.
        const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(2);
        /// A window landing within a couple of points is close enough.
        const NEAR: f32 = 2.0;

        let (pos, mut size) = match self.prefs.window(name) {
            Some(r) => ([r.x, r.y], [r.w, r.h]),
            None => (default_pos, default_size),
        };
        // The tape window is built around the cassette, so its width is not
        // the user's to choose; the height still is.
        if name == "tape" {
            size[0] = cassette::WINDOW_W;
        }
        let placement = self.placed.entry(name).or_insert(Placement {
            asked_at: std::time::Instant::now(),
            settled: false,
        });
        if placement.settled {
            return true;
        }
        let arrived = ctx
            .input(|i| i.viewport().outer_rect)
            .is_some_and(|r| (r.min.x - pos[0]).abs() <= NEAR && (r.min.y - pos[1]).abs() <= NEAR);
        if arrived || placement.asked_at.elapsed() > GIVE_UP {
            placement.settled = true;
            return arrived;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos.into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size.into()));
        false
    }

    /// Apply the saved geometry for a window, once.
    fn restore_window(
        &mut self,
        name: &'static str,
        builder: ViewportBuilder,
        default_pos: [f32; 2],
        default_size: [f32; 2],
    ) -> ViewportBuilder {
        // The geometry goes in every frame, not just the first. A window that
        // is not drawn for a while — while the application is in the
        // background, say — is retired, and the next frame builds it again; a
        // builder without a size gets the window system's default, which is
        // how alt-tabbing away and back used to resize everything to 800x600.
        // Asking for what the window already is costs nothing.
        let (pos, mut size) = match self.prefs.window(name) {
            Some(r) => ([r.x, r.y], [r.w, r.h]),
            None => (default_pos, default_size),
        };
        if name == "tape" {
            size[0] = cassette::WINDOW_W;
        }
        builder.with_position(pos).with_inner_size(size)
    }

    /// Write the window layout and display settings out. Called on close.
    pub fn save_window_state(&mut self) {
        self.prefs.display_scale = Some(self.scale);
        self.prefs.overscan = Some(self.overscan);
        self.prefs.open_windows = Some(self.open_windows());
        self.prefs.save();
        self.last_saved = Some(self.prefs.to_text());
        self.last_save_at = Some(std::time::Instant::now());
    }

    /// Save the layout if it has changed and has been still for a moment.
    fn save_window_state_if_settled(&mut self) {
        const SETTLE: std::time::Duration = std::time::Duration::from_secs(2);
        if self.last_save_at.is_some_and(|at| at.elapsed() < SETTLE) {
            return;
        }
        self.prefs.display_scale = Some(self.scale);
        self.prefs.overscan = Some(self.overscan);
        self.prefs.open_windows = Some(self.open_windows());
        let text = self.prefs.to_text();
        if self.last_saved.as_deref() == Some(text.as_str()) {
            self.last_save_at = Some(std::time::Instant::now());
            return;
        }
        self.save_window_state();
    }

    /// Take the display settings from the preferences file, if it has any.
    pub fn apply_prefs(&mut self) {
        if let Some(scale) = self.prefs.display_scale.filter(|s| *s > 0.0) {
            self.scale = scale;
        }
        if let Some(overscan) = self.prefs.overscan {
            self.overscan = overscan;
        }
        if let Some(open) = self.prefs.open_windows.clone() {
            let is_open = |name: &str| open.iter().any(|n| n == name);
            self.show_ram_map = is_open("ram_map");
            self.show_debugger = is_open("debugger");
            self.show_tape = is_open("tape");
            self.show_back_buffer = is_open("back_buffer");
            self.show_sprites = is_open("sprites");
            self.show_callflow = is_open("callflow");
            self.show_profiler = is_open("profiler");
        }
    }

    /// Which debug windows are open, by the names their geometry is saved
    /// under, so they can be opened again with the emulator.
    fn open_windows(&self) -> Vec<String> {
        [
            ("ram_map", self.show_ram_map),
            ("debugger", self.show_debugger),
            ("tape", self.show_tape),
            ("back_buffer", self.show_back_buffer),
            ("sprites", self.show_sprites),
            ("callflow", self.show_callflow),
            ("profiler", self.show_profiler),
        ]
        .into_iter()
        .filter(|(_, open)| *open)
        .map(|(name, _)| name.to_string())
        .collect()
    }

    /// True when the ZX81 is the machine in use.
    pub fn on_zx81(&self) -> bool {
        self.zx81.is_some()
    }

    // ---- the tape deck, wherever it currently is ---------------------------
    //
    // Both machines have their own deck, because both have their own clock and
    // a tape is timed in T-states. These pick out whichever one is running so
    // the tape window does not have to care.

    pub fn tape_ref(&self) -> Option<&crate::tape::Tape> {
        match &self.zx81 {
            Some(zx) => zx.bus.tape.as_ref(),
            None => self.spec.bus.tape.as_ref(),
        }
    }

    pub fn tape_mut(&mut self) -> Option<&mut crate::tape::Tape> {
        match &mut self.zx81 {
            Some(zx) => zx.bus.tape.as_mut(),
            None => self.spec.bus.tape.as_mut(),
        }
    }

    pub fn set_tape(&mut self, tape: Option<crate::tape::Tape>) {
        match &mut self.zx81 {
            Some(zx) => zx.bus.tape = tape,
            None => self.spec.bus.tape = tape,
        }
    }

    // ---- the machine that is running ---------------------------------------
    //
    // Both machines are a Z80 with memory and breakpoints, so the debugger and
    // the RAM map work through these rather than reaching for the Spectrum.

    pub fn cpu(&self) -> &crate::z80::Z80 {
        match &self.zx81 {
            Some(zx) => &zx.cpu,
            None => &self.spec.cpu,
        }
    }

    /// Show an address in the disassembly and mark the row, so what was asked
    /// for can be picked out of the twenty-odd lines around it.
    pub fn show_in_listing(&mut self, addr: u16) {
        self.dbg.view_addr = addr;
        self.dbg.follow_pc = false;
        self.dbg.marked = Some(addr);
        // Put in the middle of the listing rather than at whatever line it
        // happens to fall on, so what was asked for is where the eye goes.
        self.dbg.centre = true;
    }

    /// Read a block of memory as pictures: open the graphics viewer on it,
    /// bring it forward, and start with a sprite size the block divides into.
    ///
    /// Nothing in the bytes says how wide a sprite is meant to be — that is
    /// the one thing the viewer asks for — but a block's length usually
    /// divides by the size of the sprites in it, which is a better place to
    /// start guessing from than whatever was set last.
    pub fn show_as_graphics(&mut self, addr: u16, length: u16) {
        self.sprites.addr = addr;
        self.sprites.addr_text = format!("{addr:04X}");
        self.sprites.block_length = Some(length);
        self.show_sprites = true;
        self.sprites.raise = true;

        // The largest square sprite the block divides into, up to four cells:
        // a sheet of 16x16 sprites is 32 bytes each and a sheet of 8x8 is 8,
        // and either divides the block exactly.
        for cells in (1..=4usize).rev() {
            let stride = (cells * cells * 8) as u16;
            if length >= stride && length.is_multiple_of(stride) {
                self.sprites.cells_across = cells;
                self.sprites.cells_down = cells;
                break;
            }
        }
        // And enough across to show the whole block without scrolling, when
        // that is a sensible number.
        let stride = self.sprites.stride().max(1);
        let sprites = (length / stride) as usize;
        self.sprites.columns = sprites.clamp(1, 8);
    }

    /// The same, from another window: open the debugger, bring it to the
    /// front and give it the focus. Sending somebody to a listing in a window
    /// that is behind the one they are looking at is sending them nowhere.
    pub fn show_in_debugger(&mut self, addr: u16) {
        self.show_in_listing(addr);
        self.show_debugger = true;
        self.dbg.raise = true;
    }

    /// Write a byte into the machine that is running, as the debugger does
    /// when somebody types over one. The ROM ignores it, as the hardware does.
    /// Run to the end of the frame the machine is part-way through.
    ///
    /// What is wanted is a picture the ULA has finished painting: stopping
    /// wherever the machine happens to be leaves half a frame drawn and the
    /// rest of it left over from before. So it runs to the frame boundary,
    /// which is where the beam has finished the screen and gone back to the
    /// top, and stops there.
    pub fn next_frame(&mut self) {
        self.running = false;
        // At full speed, whatever else is set: the point is to arrive at the
        // frame boundary. Slow draw parks the CPU after a few writes and its
        // allowance is only refilled while the machine is running, so leaving
        // it on meant this worked once — on whatever allowance was left — and
        // then did nothing at all.
        let slow = self.spec.bus.slow.enabled;
        self.spec.bus.slow.enabled = false;
        self.spec.bus.slow.begin_slice();

        let was = self.frame_count();
        for _ in 0..32 {
            match &mut self.zx81 {
                Some(zx) => {
                    let frame = zx.frame_t();
                    zx.run(frame);
                }
                None => {
                    self.spec.run(self.spec.bus.frame_t());
                }
            }
            if self.frame_count() != was {
                break;
            }
        }
        self.spec.bus.slow.enabled = slow;
        self.spec.bus.slow.begin_slice();
        self.dbg.follow_pc = true;
        self.status = format!("Frame {}", self.frame_count());
    }

    /// Frames the machine has finished, whichever machine is running.
    pub fn frame_count(&self) -> u64 {
        match &self.zx81 {
            Some(zx) => zx.bus.frame,
            None => self.spec.bus.frame,
        }
    }

    /// Show the byte behind a pixel of the picture in the memory dump.
    ///
    /// The coordinates are of the whole picture, border and all, so the border
    /// is taken off before working out which cell of the display file the
    /// pixel belongs to. A click on the border itself has no byte behind it
    /// and is left alone.
    pub fn show_pixel_in_memory(&mut self, px: usize, py: usize) {
        // Only while the machine is stopped and the debugger is open to show
        // the answer. A click on a running picture is somebody playing a game,
        // not somebody asking where a byte is.
        if self.running || !self.show_debugger {
            return;
        }
        let view = self.view();
        let (Some(x), Some(y)) = (
            px.checked_sub(view.border_x),
            py.checked_sub(view.border_top),
        ) else {
            return;
        };
        if x >= 256 || y >= 192 {
            return;
        }
        let addr = crate::machine::screen_bitmap_addr(y as u16, (x / 8) as u16);
        self.dbg.mem_addr = addr;
        self.dbg.mem_text = format!("{addr:04X}");
        self.dbg.selected = Some((addr, crate::ui::debugger::Column::Hex));
        self.dbg.half_typed = None;
        self.show_debugger = true;
        self.dbg.raise = true;
    }

    pub fn poke_byte(&mut self, addr: u16, value: u8) {
        match &mut self.zx81 {
            Some(zx) => zx.bus.poke(addr, value),
            None => self.spec.bus.poke(addr, value),
        }
    }

    pub fn peek(&self, addr: u16) -> u8 {
        match &self.zx81 {
            Some(zx) => zx.bus.peek_raw(addr),
            None => self.spec.bus.peek_raw(addr),
        }
    }

    pub fn breakpoints(&self) -> &Vec<u16> {
        match &self.zx81 {
            Some(zx) => &zx.breakpoints,
            None => &self.spec.breakpoints,
        }
    }

    pub fn breakpoints_mut(&mut self) -> &mut Vec<u16> {
        match &mut self.zx81 {
            Some(zx) => &mut zx.breakpoints,
            None => &mut self.spec.breakpoints,
        }
    }

    pub fn tracker(&self) -> &crate::tracker::Tracker {
        match &self.zx81 {
            Some(zx) => &zx.bus.tracker,
            None => &self.spec.bus.tracker,
        }
    }

    pub fn tracker_mut(&mut self) -> &mut crate::tracker::Tracker {
        match &mut self.zx81 {
            Some(zx) => &mut zx.bus.tracker,
            None => &mut self.spec.bus.tracker,
        }
    }

    pub fn phys_index(&self, addr: u16) -> usize {
        match &self.zx81 {
            Some(zx) => zx.bus.phys_index(addr),
            None => self.spec.bus.phys_index(addr),
        }
    }

    /// What sits at an address: a bank number on a Spectrum, ROM or RAM on a
    /// ZX81, which has nothing to page.
    pub fn slot_label(&self, addr: u16) -> String {
        match &self.zx81 {
            Some(zx) => if zx.bus.is_rom(addr) { "ROM" } else { "RAM" }.to_string(),
            None => match self.spec.bus.slot_of(addr) {
                crate::machine::Slot::Rom(p) => format!("ROM{p}"),
                crate::machine::Slot::Ram(b) => format!("RAM{b}"),
            },
        }
    }

    pub fn is_rom(&self, addr: u16) -> bool {
        match &self.zx81 {
            Some(zx) => zx.bus.is_rom(addr),
            None => matches!(self.spec.bus.slot_of(addr), crate::machine::Slot::Rom(_)),
        }
    }

    /// Frame number, T-states into the frame, and T-states in a whole frame.
    pub fn machine_clock(&self) -> (u64, u32, u32) {
        match &self.zx81 {
            Some(zx) => (
                zx.bus.frame,
                (zx.bus.tstates % zx.frame_t()) as u32,
                zx.frame_t() as u32,
            ),
            None => (
                self.spec.bus.frame,
                self.spec.bus.tstates,
                self.spec.bus.frame_t(),
            ),
        }
    }

    pub fn reset_machine(&mut self) {
        match &mut self.zx81 {
            Some(zx) => zx.reset(),
            None => self.spec.reset(),
        }
    }

    pub fn step_machine(&mut self) {
        match &mut self.zx81 {
            Some(zx) => zx.step_instruction(),
            None => {
                // Kept so it can be stepped back over. Only while stepping by
                // hand: a running machine writes millions of bytes a second
                // and none of them is going to be walked back through.
                let undo = self.spec.step_recording();
                if self.rewind.len() == REWIND {
                    self.rewind.pop_front();
                }
                self.rewind.push_back(undo);
            }
        }
    }

    /// How many instructions can be stepped back over.
    ///
    /// Enough to see how the machine got where it is, which is what stepping
    /// back is for; a hundred would be a recording, and there is one of those
    /// already.
    pub fn can_step_back(&self) -> bool {
        !self.rewind.is_empty()
    }

    /// Let the machine run, and throw away what could have been stepped back
    /// over.
    ///
    /// Running writes what an undo cannot put back: a step over a CALL is
    /// thousands of instructions, and the entries kept from before it describe
    /// a machine that no longer exists. Stepping back into that would put the
    /// registers somewhere plausible and leave the memory wrong, which is
    /// worse than not offering it.
    pub fn forget_rewind(&mut self) {
        self.rewind.clear();
    }

    /// Put the last instruction back the way it was.
    pub fn step_back(&mut self) {
        let Some(undo) = self.rewind.pop_back() else {
            self.set_status("Nothing to step back to".to_string(), false);
            return;
        };
        self.running = false;
        self.spec.undo_step(&undo);
        self.dbg.follow_pc = true;
        self.dbg.centre = true;
        self.last_stop = Some(crate::machine::Stop::Stepped);
        self.status = format!(
            "Stepped back to ${:04X}, {} left",
            self.cpu().pc,
            self.rewind.len()
        );
    }

    /// The mixer of the machine that is running.
    pub fn audio(&mut self) -> &mut crate::audio::Audio {
        match &mut self.zx81 {
            Some(zx) => &mut zx.bus.audio,
            None => &mut self.spec.bus.audio,
        }
    }

    pub fn tape_boost(&self) -> bool {
        match &self.zx81 {
            Some(zx) => zx.bus.tape_boost,
            None => self.spec.bus.tape_boost,
        }
    }

    pub fn tape_boost_mut(&mut self) -> &mut bool {
        match &mut self.zx81 {
            Some(zx) => &mut zx.bus.tape_boost,
            None => &mut self.spec.bus.tape_boost,
        }
    }

    pub fn tape_is_playing(&self) -> bool {
        self.tape_ref().is_some_and(|t| t.playing)
    }

    /// Whether the tape is actually loading something, as against sitting in
    /// the silence the program is meant to be watched through.
    ///
    /// Only the last pause of a tape, or one before a block that stops the
    /// tape, is worth coming back to normal speed for: that is where the
    /// loading ends and the program takes over. The silences between the
    /// blocks of a multi-load are the loader getting ready for the next one,
    /// and dropping to normal speed through every one of them makes a hurried
    /// tape barely quicker than an unhurried one.
    pub fn tape_is_loading(&self) -> bool {
        self.tape_ref()
            .is_some_and(|t| t.playing && !t.pause_ends_the_tape())
    }

    /// Clock of the running machine: tape times are in its T-states, and the
    /// ZX81's is not the Spectrum's.
    pub fn cpu_hz(&self) -> f64 {
        if self.on_zx81() {
            zx81::CPU_HZ
        } else {
            crate::machine::CPU_HZ
        }
    }

    /// T-states since power-on for the running machine, which is the timebase
    /// a tape is played against.
    pub fn machine_t(&self) -> u64 {
        match &self.zx81 {
            Some(zx) => zx.bus.tstates,
            None => self.spec.bus.total_t(),
        }
    }

    /// Switch to a ZX81 with the given memory, or back to the Spectrum.
    pub fn switch_to_zx81(&mut self, ram: zx81::Ram) {
        let Some(rom) = self.roms.rom_zx81.clone() else {
            self.set_status(
                "Cannot switch to a ZX81: no ROM. Put an 8K ZX81 ROM at roms/zx81.rom".into(),
                true,
            );
            return;
        };
        if self.zx81.is_some() && self.zx81_ram == ram {
            self.set_status(format!("Already running as a {}", ram.name()), false);
            return;
        }
        let mut machine = Zx81::new(ram);
        machine.load_rom(&rom);
        machine.reset();
        // The ZX81 has its own mixer, on its own clock, so it needs its own
        // connection to the sound device — and the volume the user last set.
        if let Some(out) = &self.audio_out {
            machine.bus.audio.attach(out.queue.clone(), out.sample_rate);
        }
        machine.bus.audio.enabled = self.spec.bus.audio.enabled;
        machine.bus.audio.volume = self.spec.bus.audio.volume;
        self.zx81 = Some(machine);
        self.zx81_ram = ram;
        self.running = true;
        self.set_status(format!("Switched to a {}", ram.name()), false);
    }

    /// The picture as it was last drawn: RGBA, the size of [`App::view`].
    pub fn picture(&self) -> &[u8] {
        &self.screen_pixels
    }

    /// The machine as it stood `t` T-states into the frame being raced.
    ///
    /// The snapshot is taken the first time it is asked for, and again
    /// whenever the machine has moved since — stepping an instruction makes
    /// the frame that was being raced somebody else's.
    fn raced_to(&mut self, t: u32) -> &Spectrum {
        let stale = self
            .race
            .as_ref()
            .is_none_or(|race| !race.is_of(&self.spec));
        if stale {
            self.race = Some(crate::race::Race::start(&self.spec));
        }
        self.race.as_mut().expect("just made one").at(t)
    }

    /// Is the machine going slowly enough to watch the picture being drawn?
    ///
    /// The beam is shown and the picture is drawn as it is painted under the
    /// same condition, so the two always agree: a beam crawling over a picture
    /// that only changes when the frame ends would be telling a lie about
    /// where the machine is.
    pub fn crawling(&self) -> bool {
        self.speed < 1.0 || self.spec.bus.slow.enabled
    }

    /// How much border to draw.
    pub fn view(&self) -> screen::View {
        if self.overscan {
            screen::View::OVERSCAN
        } else {
            screen::View::CROPPED
        }
    }

    fn flash_on(&self) -> bool {
        (self.spec.bus.frame / 16) % 2 == 1
    }

    /// Advance the emulation by however much wall-clock time has passed.
    /// Run the machine for a slice of wall-clock time, as a frame of the UI
    /// would. Public so tests can stop it at a breakpoint without a window.
    pub fn advance(&mut self, dt: f32) {
        if !self.running {
            return;
        }
        // Racing the beam is a way of looking at a stopped machine: it replays
        // one frame from its interrupt, and a machine that is running has
        // moved on to another frame before the cursor has been read.
        self.cursor_beam = false;
        self.race = None;
        if self.zx81.is_some() {
            // A ZX81 loads at about fifty bytes a second, so the boost matters
            // even more here than it does on a Spectrum.
            let boost = if self.tape_boost() && self.tape_is_loading() {
                MAX_SPEED
            } else {
                1.0
            };
            // Keep the sound buffer near its target depth, as for the
            // Spectrum, and mute when the speed is too far from normal.
            let target = self.audio_latency_target as f64;
            let pace = if self.speed == 1.0 && boost == 1.0 && self.audio().enabled {
                self.audio().pace(target)
            } else {
                1.0
            };
            let effective = self.speed * boost;
            self.audio().speed_ok = (0.85..=1.2).contains(&effective);
            let zx = self.zx81.as_mut().expect("just checked");
            let dt = dt.clamp(0.0, 0.1);
            let want = zx81::CPU_HZ as f32 * dt * self.speed * boost * pace + self.leftover;
            let budget = want.max(0.0) as u64;
            self.leftover = want - budget as f32;
            // Cap the work per host frame, as for the Spectrum.
            let budget = budget.min(zx.frame_t() * 24);
            if let Some(pc) = zx.run(budget) {
                self.running = false;
                self.set_status(format!("Breakpoint at ${pc:04X}"), false);
            }
            return;
        }
        self.spec.bus.slow.begin_slice();

        let dt = dt.clamp(0.0, 0.1);
        // A recording is measured in frames of instructions rather than in
        // T-states, so it does its own running.
        if self.rzx.is_some() {
            let flat_out = self.rzx.as_ref().is_some_and(|rzx| rzx.max_speed);
            self.spec.bus.audio.speed_ok = !flat_out && (0.85..=1.2).contains(&self.speed);
            self.advance_recording(dt, 1.0);
            self.spec.bus.audio_sync();
            self.spec.bus.audio.flush();
            return;
        }
        // Loading a real tape takes minutes; run as fast as the emulator will
        // go while it moves.
        let boost = if self.tape_boost() && self.tape_is_loading() {
            MAX_SPEED
        } else {
            1.0
        };
        // Nudge the amount of work to keep the sound buffer near its target
        // depth, so it neither runs dry nor backs up.
        let pace = if self.speed == 1.0 && self.spec.bus.audio.enabled {
            self.spec.bus.audio.pace(self.audio_latency_target as f64)
        } else {
            1.0
        };
        let want =
            self.spec.bus.model.cpu_hz() as f32 * dt * self.speed * boost * pace + self.leftover;
        let budget = want.max(0.0) as u32;
        self.leftover = want - budget as f32;

        // Sound only makes sense near real time; muting keeps fast-forward
        // from shrieking.
        let effective = self.speed * boost;
        self.spec.bus.audio.speed_ok = (0.85..=1.2).contains(&effective);

        // Cap the work per host frame so "Max" speed cannot lock up the UI.
        let budget = budget.min(self.spec.bus.frame_t() * 24);
        self.rewind.clear();
        let stop = self.spec.run(budget);
        self.last_stop = Some(stop);
        self.handle_stop(stop);
        self.spec.bus.audio_sync();
        self.spec.bus.audio.flush();
    }

    /// What to do about the machine having stopped.
    fn handle_stop(&mut self, stop: Stop) {
        match stop {
            Stop::Breakpoint(pc) => {
                self.running = false;
                self.status = format!("Breakpoint at ${pc:04X}");
                self.dbg.follow_pc = true;
                // Stopping somewhere is only useful if you can see where: the
                // debugger is opened if it was closed and asks for the front
                // on the next frame it draws.
                self.show_debugger = true;
                self.dbg.raise = true;
            }
            Stop::Watched(event, at) => {
                self.running = false;
                self.status = format!("{} (PC ${at:04X})", event.describe());
                // The listing is put on the instruction that did it rather
                // than left following PC, which by then is the one after.
                self.dbg.follow_pc = false;
                self.dbg.view_addr = at;
                self.show_debugger = true;
                self.dbg.raise = true;
            }
            Stop::SlowDraw => {
                self.leftover = 0.0;
            }
            _ => {}
        }
    }

    /// The picture as it stands, for anything that wants to show it: the
    /// debugger keeps a small copy beside the registers.
    pub fn screen_texture(&self) -> Option<TextureHandle> {
        self.screen_tex.clone()
    }

    fn draw_screen_texture(&mut self, ctx: &egui::Context) {
        if self.zx81.is_some() {
            let view = if self.overscan {
                zx81::View::OVERSCAN
            } else {
                zx81::View::CROPPED
            };
            if self.screen_pixels.len() != view.buffer_len() {
                self.screen_pixels = vec![0; view.buffer_len()];
                self.screen_tex = None;
            }
            if let Some(zx) = self.zx81.as_ref() {
                zx.bus.render(view, &mut self.screen_pixels);
            }
            let img = ColorImage::from_rgba_unmultiplied([view.w, view.h], &self.screen_pixels);
            match &mut self.screen_tex {
                Some(t) => t.set(img, TextureOptions::NEAREST),
                None => {
                    self.screen_tex =
                        Some(ctx.load_texture("zx81-screen", img, TextureOptions::NEAREST))
                }
            }
            return;
        }
        let view = self.view();
        let flash = self.flash_on();
        if self.screen_pixels.len() != view.buffer_len() {
            self.screen_pixels = vec![0; view.buffer_len()];
            self.screen_tex = None; // the texture has to be remade at the new size
        }
        // Bring the painted frame up to where the beam has got to. It is
        // otherwise only caught up when the program writes to the screen, so a
        // machine stopped between two writes would show the last of them as
        // the beam's position rather than where the beam actually is.
        self.spec.bus.catch_up_painting();
        // Racing replays a frame rather than reading the machine, so the
        // pixels are borrowed out of the way of the copy being run.
        let mut pixels = std::mem::take(&mut self.screen_pixels);
        match self.beam_t.filter(|_| self.cursor_beam && !self.running) {
            Some(beam) => {
                let raced = self.raced_to(beam);
                screen::render_racing(&raced.bus, view, &mut pixels, flash, beam);
            }
            // While the machine is crawling, the picture is the one being
            // painted, so that it builds under the beam rather than sitting
            // still until the frame ends.
            // Racing: the frame being painted, fading behind the beam, with
            // the writes since marked by which side of the beam they landed.
            None if self.racing => {
                let fade = screen::Fade {
                    now: self.spec.bus.tstates,
                    floor: self.fade_floor,
                };
                // The user is given the blend in seconds of their own time;
                // how much of the machine's time that is depends on how
                // slowly it is being run.
                let over = (TINT_SECONDS * self.speed * self.spec.bus.model.cpu_hz() as f32) as u64;
                let tint = self.spec.bus.tints.as_ref().map(|tints| screen::Tinting {
                    tints,
                    now: self.spec.bus.total_t(),
                    over,
                });
                screen::render_fading(&self.spec.bus, view, &mut pixels, flash, fade, tint);
            }
            None if self.crawling() => {
                screen::render_painting(&self.spec.bus, view, &mut pixels, flash)
            }
            None => screen::render(&self.spec.bus, view, &mut pixels, flash),
        }
        self.screen_pixels = pixels;
        let img =
            ColorImage::from_rgba_unmultiplied([view.width(), view.height()], &self.screen_pixels);
        match &mut self.screen_tex {
            Some(t) => t.set(img, TextureOptions::NEAREST),
            None => {
                self.screen_tex =
                    Some(ctx.load_texture("spectrum-screen", img, TextureOptions::NEAREST))
            }
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        // A wrapping row rather than a menu bar: a bar does not wrap, and with
        // only one menu left on it the rest of the row would be clipped off
        // the edge of a narrow window.
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 8.0);
        let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
        ui.allocate_ui_with_layout(row, layout, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Load ROM…").clicked() {
                    if let Some(path) = self.pick_file(Some(FileKind::Rom)) {
                        self.load_path(&path);
                    }
                    ui.close();
                }
                if ui.button("Load tape…").clicked() {
                    if let Some(path) = self.pick_file(Some(FileKind::Tape)) {
                        self.load_path(&path);
                    }
                    ui.close();
                }
                if ui.button("Load recording…").clicked() {
                    if let Some(path) = self.pick_file(Some(FileKind::Recording)) {
                        self.load_path(&path);
                    }
                    ui.close();
                }
                if ui.button("Load snapshot…").clicked() {
                    if let Some(path) = self.pick_file(Some(FileKind::Snapshot)) {
                        match snapshot::probe_model(&path) {
                            Ok(model) => {
                                self.switch_model(model);
                                match snapshot::load(&mut self.spec, &path) {
                                    Ok(()) => self.status = format!("Loaded {}", path.display()),
                                    Err(e) => {
                                        self.set_status(format!("Snapshot load failed: {e}"), true)
                                    }
                                }
                            }
                            Err(e) => self.set_status(format!("Snapshot load failed: {e}"), true),
                        }
                    }
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(!self.on_zx81(), egui::Button::new("Save snapshot…"))
                    .on_hover_text(
                        "Write the machine as it stands to a .sna, to be loaded \
                         back later or carried to another emulator.",
                    )
                    .clicked()
                {
                    self.save_snapshot();
                    ui.close();
                }
                ui.separator();
                if ui.button("Reset").clicked() {
                    self.spec.reset();
                    self.status = "Reset".into();
                    ui.close();
                }
            });
            theme::divider(ui);

            if theme::run_pause_button(ui, self.running).clicked() {
                self.running = !self.running;
            }

            // Recording what the machine does, so a run of a game can be read
            // back later instruction by instruction. Not offered while a
            // recording is playing: what would be captured is the recording.
            let recording = self.recorded_frames();
            match recording {
                None => {
                    if ui
                        .add_enabled(self.rzx.is_none(), egui::Button::new("⏺ Record"))
                        .on_hover_text(
                            "Record everything the machine reads from now on, so it \
                             can be played back and read instruction by instruction. \
                             It is kept in memory until you stop.",
                        )
                        .clicked()
                    {
                        self.start_recording();
                    }
                }
                Some(frames) => {
                    if ui
                        .button("⏹ Stop")
                        .on_hover_text("Stop recording and write it out")
                        .clicked()
                    {
                        self.save_recording();
                    }
                    ui.label(
                        egui::RichText::new(format!("● {frames} frames"))
                            .color(theme::RED)
                            .monospace(),
                    );
                }
            }

            theme::divider(ui);
            theme::group_label(ui, "Speed");
            // Choosing a speed by hand is choosing not to race.
            speed_dropdown(&mut self.speed, ui);
            if self.racing && self.speed != RACE_SPEED {
                self.racing = false;
            }

            theme::divider(ui);
            theme::group_label(ui, "Machine");
            self.machine_dropdown(ui);
            self.late_timing(ui);
            if ui.button("Reset").clicked() {
                match &mut self.zx81 {
                    Some(zx) => {
                        zx.reset();
                        let name = self.zx81_ram.name();
                        self.set_status(format!("Reset ({name})"), false);
                    }
                    None => {
                        self.spec.reset();
                        self.set_status(format!("Reset ({})", self.spec.bus.model.name()), false);
                    }
                }
            }
        });
    }

    /// Zoom and the ways of watching the picture, on a line of their own: the
    /// machine row is long enough without them, and these belong together.
    fn video_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            // Claim the height of the tallest thing in the row before
            // anything is placed. egui centres each control in the row as it
            // stands when the control is added, so a slider half way along an
            // otherwise short row leaves everything before it sitting three
            // pixels higher than everything after it.
            ui.set_min_height(theme::ROW_H);
            theme::group_label(ui, "Zoom");
            zoom_dropdown(&mut self.scale, ui);

            theme::divider(ui);
            theme::group_label(ui, "Video");

            // Five seconds a frame, with the picture fading behind the beam:
            // one frame's drawing, slowed down until it can be read.
            if theme::toggle(ui, &mut self.racing, "Race the Beam")
                .on_hover_text(
                    "Run at five seconds a frame and watch the beam go down the \
                     screen. The picture fades behind it, so how long ago each \
                     part was drawn is visible at a glance.",
                )
                .changed()
            {
                self.set_racing(self.racing);
            }
            ui.add_enabled_ui(self.racing, |ui| {
                theme::slider(
                    ui,
                    egui::Slider::new(&mut self.fade_floor, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .text("fades to"),
                )
                .on_hover_text(
                    "How much of its brightness a pixel is left with by the \
                     time the beam comes round to draw it again.",
                );
            });

            theme::divider(ui);
            // Only while stopped: the frame is replayed from its interrupt,
            // which means being able to hold the machine still and run a copy
            // of it instead.
            ui.add_enabled_ui(!self.running, |ui| {
                theme::toggle(ui, &mut self.cursor_beam, "Cursor Beam").on_hover_text(
                    "Replay the next frame from its interrupt. Hover the picture and \
                     everything above the cursor is the screen as the machine had it \
                     by the time the beam reached that point — every instruction up to \
                     there executed, and no more. Below it is what the display file \
                     holds at that moment, dimmed, since the ULA has not put it out \
                     yet. Stopped machines only.",
                );
            });
            theme::toggle(ui, &mut self.overscan, "Overscan").on_hover_text(
                "Show the whole border the ULA draws, not just a television's worth.",
            );

            // One whole picture at a time, for watching a game draw itself.
            // Only while it is stopped: a running machine is already doing
            // this fifty times a second.
            if ui
                .add_enabled(!self.running, egui::Button::new("Next frame"))
                .on_hover_text(
                    "Run to the end of the frame the machine is in the middle \
                     of, so the picture on screen is one the ULA has finished \
                     painting. Only while it is paused.",
                )
                .clicked()
            {
                self.next_frame();
            }
        });
    }

    /// Switch the slowed-down, fading picture on or off.
    ///
    /// Switching it on takes the speed down to five seconds a frame and starts
    /// the machine — there is nothing to watch otherwise — and switching it
    /// off puts the speed back where it was.
    pub fn set_racing(&mut self, on: bool) {
        // Marking every write costs a branch on the busiest path in the
        // emulator, so it is only done while somebody is looking at the marks.
        self.spec.bus.tints = on.then(machine::Tints::default);
        if on {
            self.speed_before_race = self.speed;
            self.speed = RACE_SPEED;
            self.running = true;
        } else {
            self.speed = self.speed_before_race;
        }
        self.racing = on;
    }

    /// The 48K's two timings, which only it has.
    fn late_timing(&mut self, ui: &mut egui::Ui) {
        if self.on_zx81() || self.spec.bus.model != Model::Spectrum48 {
            return;
        }
        let mut late = self.spec.bus.late_timing;
        if theme::toggle(ui, &mut late, "Late timing")
            .on_hover_text(
                "Later 48K machines run the display one T-state later \
                 relative to the interrupt. HALT2INT tells them apart.",
            )
            .changed()
        {
            self.spec.bus.set_late_timing(late);
            self.set_status(
                format!("48K {} timing", if late { "late" } else { "early" }),
                false,
            );
        }
    }

    fn controls_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            theme::toggle(ui, &mut self.spec.bus.slow.enabled, "Slow draw")
                .on_hover_text(
                    "Park the CPU after a set number of writes to watched video memory \
                     so the picture visibly builds up over several host frames.",
                );
            ui.add_enabled_ui(self.spec.bus.slow.enabled, |ui| {
                theme::slider(
                    ui,
                    egui::Slider::new(&mut self.spec.bus.slow.writes_per_slice, 1..=4096)
                        .logarithmic(true)
                        .text("writes/frame"),
                );
                theme::toggle(ui, &mut self.spec.bus.slow.watch_screen, "video RAM");
                theme::toggle(ui, &mut self.spec.bus.slow.watch_back_buffer, "back buffer");
            });
            ui.separator();
            if let Some(rzx) = &self.rzx {
                let (frame, total) = (rzx.frame, rzx.recording.len());
                let mut max_speed = rzx.max_speed;
                let short = self.spec.bus.playback.as_ref().map_or(0, |p| p.short);
                let percent = if total == 0 {
                    0.0
                } else {
                    frame as f32 * 100.0 / total as f32
                };

                // Said plainly: the machine is not taking orders from the
                // keyboard at the moment, and it should be obvious why.
                ui.label(
                    egui::RichText::new("⏵ REPLAY")
                        .strong()
                        .color(theme::AMBER),
                )
                .on_hover_text("Playing back a recording. The keyboard is the recording's.");
                ui.label(
                    egui::RichText::new(format!("{frame}/{total}  {percent:.0}%"))
                        .monospace()
                        .color(theme::LCD_FG),
                );

                if ui
                    .toggle_value(&mut max_speed, "Max speed")
                    .on_hover_text(
                        "Run the recording as fast as this machine can rather \
                         than at the speed it was played. Twenty minutes of \
                         play is a long wait for the part you want to see.",
                    )
                    .changed()
                {
                    if let Some(rzx) = &mut self.rzx {
                        rzx.max_speed = max_speed;
                        rzx.owed = 0.0;
                    }
                }

                // A recording that asks for more input than was recorded has
                // come adrift from the machine: what is on screen after that
                // is the emulator's guess, not what was played, and saying so
                // is better than letting it look authentic.
                if short > 0 {
                    ui.label(egui::RichText::new(format!("out of step ({short})")).color(theme::RED))
                        .on_hover_text(
                            "The program has read more from the ports than the \
                             recording holds, so it is no longer following the \
                             path it was recorded taking.",
                        );
                }
                if ui.button("Stop").clicked() {
                    self.stop_playback();
                    self.set_status("Stopped the recording".into(), false);
                }
                theme::divider(ui);
            }
            if self.tape_ref().is_some() {
                let playing = self.tape_is_playing();
                if ui
                    .button(if playing { "⏸ Tape" } else { "▶ Tape" })
                    .clicked()
                {
                    let now = self.machine_t();
                    let t = self.tape_mut().unwrap();
                    if playing {
                        t.stop();
                    } else {
                        t.play(now);
                    }
                }
                ui.separator();
            }
            // Everything about the sound lives here: what the top menu used to
            // hold as well, since it was the same two controls twice.
            let sound = match (&self.audio_out, &self.audio_error) {
                (Some(out), _) => format!(
                    "{} @ {} Hz\nbuffer {} samples ({:.0} ms), {} dropped",
                    out.device_name,
                    out.sample_rate,
                    self.spec.bus.audio.queue_len(),
                    self.spec.bus.audio.latency() * 1000.0,
                    self.spec.bus.audio.dropped
                ),
                (None, Some(e)) => e.clone(),
                (None, None) => "no audio device".to_string(),
            };
            let failed = self.audio_out.is_none();
            theme::toggle(ui, &mut self.audio().enabled, if failed { "🔇" } else { "🔊" })
                .on_hover_text(&sound);
            theme::slider(
                ui,
                egui::Slider::new(&mut self.audio().volume, 0.0..=1.0)
                    .show_value(false)
                    .text("vol"),
            );
            theme::toggle(ui, &mut self.spec.bus.audio.mute_off_speed, "auto-mute")
                .on_hover_text("Silence the sound unless the machine is running at about normal speed, so fast-forwarding does not shriek.");
            theme::slider(
                ui,
                egui::Slider::new(&mut self.audio_latency_target, 0.02..=0.25)
                    .show_value(false)
                    .text("buffer"),
            )
            .on_hover_text(
                "How far ahead of the sound card to stay. Raise it if you hear \
                 crackling, lower it for a more immediate beeper.",
            );
        });
    }
}

impl eframe::App for App {
    /// Run the machine and put the debug windows up.
    ///
    /// This is deliberately not part of `ui`: eframe skips `ui` while the main
    /// window is not visible — which on macOS includes the moment the
    /// application is switched away from — and then prunes every viewport that
    /// frame did not declare, destroying the windows. They came back on the
    /// next switch as new windows, in creation order, which is why they
    /// vanished, reappeared and shuffled themselves about. `logic` runs
    /// whether the window is visible or not, so the windows stay put.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw_main(ui);
    }

    /// Keep the layout for next time.
    fn on_exit(&mut self) {
        self.save_window_state();
        let _ = self.notes.save_if_dirty();
    }
}

impl App {
    /// The whole user interface for one frame. Separate from the `eframe::App`
    /// impl so tests can drive it without a real window.
    /// Everything for one frame: used by the tests, which drive the interface
    /// through a single `Ui` rather than through eframe's split.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.frame_logic(&ctx);
        self.draw_main(ui);
    }

    /// The machine, the windows around it, and the repaint that keeps both
    /// going. Runs every frame, visible or not.
    pub fn frame_logic(&mut self, ctx: &egui::Context) {
        if !self.styled {
            theme::apply(ctx);
            self.styled = true;
        }
        let ctx = ctx.clone();
        let dt = ctx.input(|i| i.stable_dt);
        self.read_keyboard(&ctx);
        self.advance(dt);
        self.sync_title(&ctx);
        self.remember_window("main", &ctx);
        match &mut self.zx81 {
            Some(zx) => zx.bus.tape_tick(),
            None => self.spec.bus.tape_tick(),
        }
        match &mut self.zx81 {
            // The back-buffer detector watches the Spectrum's display file, so
            // there is nothing for it to do here; the heat maps still have to
            // fade, or every byte the ROM has ever touched stays lit.
            Some(zx) => zx.bus.tracker.fade(),
            None => self.spec.bus.frame_visuals(),
        }
        self.draw_screen_texture(&ctx);

        // The debug windows are rendered before this viewport's own panels:
        // an immediate viewport runs a nested pass over the same Context, and
        // doing that after a menu popup has been opened discards the popup.
        self.debug_viewports(&ctx);

        // Watching runs while somebody is looking for something, and is
        // switched on from the window doing the looking rather than from a
        // toggle somewhere else. Set here because the window need not be open
        // for the machine to be running.
        // Two windows can want it: the call flow looking for something, and
        // the debugger working out where the routines are. Either is enough,
        // and neither switches the other off — this used to be a plain
        // assignment from the call flow, which quietly undid the debugger's
        // request on the next frame.
        self.spec.bus.observer.enabled = self.callflow.looking || self.dbg.watching_blocks;

        self.save_window_state_if_settled();
        self.save_notes_if_due();
        ctx.request_repaint();
    }

    /// The main window itself: its toolbars, the picture and the status line.
    fn draw_main(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            self.menu(ui);
            self.machine_row(ui);
            self.video_row(ui);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            self.controls_row(ui);
            let status = self.status.clone();
            ui.horizontal(|ui| {
                if self.status_is_error {
                    ui.colored_label(theme::RED, status);
                } else {
                    ui.colored_label(theme::DIM, status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::rainbow(ui);
                });
            });
        });
        let mut beam = None;
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(tex) = &self.screen_tex {
                let (w, h) = match &self.zx81 {
                    Some(_) if self.overscan => (zx81::View::OVERSCAN.w, zx81::View::OVERSCAN.h),
                    Some(_) => (zx81::View::CROPPED.w, zx81::View::CROPPED.h),
                    None => {
                        let v = self.view();
                        (v.width(), v.height())
                    }
                };
                let size = egui::vec2(w as f32 * self.scale, h as f32 * self.scale);
                // Take the whole panel and put the picture in the middle of it,
                // so the space around the display is equal on all four sides.
                let (area, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
                let painter = ui.painter_at(area);
                // The picture sits in a bevelled surround rather than on bare
                // black, as a set does in its case.
                painter.rect_filled(area, 0.0, theme::CASE_DARK);
                let picture = screen::centred(area, size);
                let bezel = picture.expand(10.0);
                painter.rect_filled(bezel, 6.0, egui::Color32::from_rgb(0x1a, 0x17, 0x13));
                painter.rect_stroke(
                    bezel,
                    6.0,
                    egui::Stroke::new(1.0, egui::Color32::BLACK),
                    egui::StrokeKind::Inside,
                );
                painter.rect_stroke(
                    picture.expand(1.0),
                    2.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(0x3a, 0x35, 0x2d)),
                    egui::StrokeKind::Outside,
                );
                painter.image(
                    tex.id(),
                    picture,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                // Clicking a pixel picks out the byte behind it in the
                // debugger's dump: "what draws this?" starts with knowing
                // which byte it is, and counting rows and thirds by hand to
                // work out a display address is a job nobody should be doing.
                if response.clicked() {
                    if let Some(at) = response.interact_pointer_pos() {
                        if picture.contains(at) {
                            let px = ((at.x - picture.left()) / self.scale) as usize;
                            let py = ((at.y - picture.top()) / self.scale) as usize;
                            self.show_pixel_in_memory(px, py);
                        }
                    }
                }

                // The ULA's own beam, drawn where it has actually reached.
                //
                // Only worth showing while the machine is going slowly enough
                // to see it: at full speed a frame of work is done between one
                // repaint and the next, so the beam would sit at the top of
                // the frame looking broken. The ZX81 has no ULA drawing a
                // picture — the CPU does it — so there is no beam to show.
                if self.crawling() && self.zx81.is_none() {
                    draw_beam(&painter, &self.spec.bus, self.view(), picture, self.scale);
                }

                // Where is the beam? Wherever the cursor is over the picture.
                // The ZX81 draws with the CPU, so there is no beam to race.
                let spectrum_view = self.view();
                beam = (self.zx81.is_none())
                    .then(|| {
                        response
                            .hover_pos()
                            .filter(|p| picture.contains(*p))
                            .map(|p| {
                                let px = ((p.x - picture.left()) / self.scale) as usize;
                                let py = ((p.y - picture.top()) / self.scale) as usize;
                                beam_at(&self.spec.bus, spectrum_view, px, py)
                            })
                    })
                    .flatten();
            }
        });
        self.beam_t = beam;
    }

    fn debug_viewports(&mut self, ctx: &egui::Context) {
        // A window that has been closed loses its viewport, and the next one
        // opened under the same name is a new window that the system will
        // place where it likes. Forget that it was ever positioned, so it is
        // put back where it was left rather than wherever it reappears.
        for (name, shown) in [
            ("ram_map", self.show_ram_map),
            ("debugger", self.show_debugger),
            ("profiler", self.show_profiler),
            ("tape", self.show_tape),
            ("back_buffer", self.show_back_buffer),
            ("sprites", self.show_sprites),
            ("callflow", self.show_callflow),
        ] {
            if !shown {
                self.placed.remove(name);
            }
        }

        if self.show_ram_map {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("ram-map"),
                self.restore_window(
                    "ram_map",
                    ViewportBuilder::default().with_title("RAM access map"),
                    [1120.0, 40.0],
                    [560.0, 700.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    if self.place_window("ram_map", &ctx, [1120.0, 40.0], [560.0, 700.0]) {
                        self.remember_window("ram_map", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| ram_map::ui(self, ui));
                },
            );
            self.show_ram_map = open;
        }

        if self.show_debugger {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("debugger"),
                self.restore_window(
                    "debugger",
                    ViewportBuilder::default().with_title("Debugger"),
                    [20.0, 40.0],
                    [debugger::WINDOW_W, 780.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    self.fix_width(&ctx, debugger::WINDOW_W);
                    if self.dbg.raise {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        self.dbg.raise = false;
                    }
                    if self.place_window(
                        "debugger",
                        &ctx,
                        [20.0, 40.0],
                        [debugger::WINDOW_W, 780.0],
                    ) {
                        self.remember_window("debugger", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| debugger::ui(self, ui));
                },
            );
            self.show_debugger = open;
        }

        if self.show_profiler {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("profiler"),
                self.restore_window(
                    "profiler",
                    ViewportBuilder::default().with_title("Profiler"),
                    [160.0, 320.0],
                    [900.0, 620.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    if self.place_window("profiler", &ctx, [220.0, 120.0], [900.0, 620.0]) {
                        self.remember_window("profiler", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| profiler::ui(self, ui));
                },
            );
            self.show_profiler = open;
        }

        if self.show_tape {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("tape"),
                self.restore_window(
                    "tape",
                    ViewportBuilder::default().with_title("Tape"),
                    [300.0, 120.0],
                    [720.0, 780.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    self.fix_width(&ctx, cassette::WINDOW_W);
                    if self.place_window("tape", &ctx, [260.0, 120.0], [cassette::WINDOW_W, 760.0])
                    {
                        // Built around the cassette: the height is the user's
                        // to drag, the width is not.
                        self.remember_window("tape", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| tape::ui(self, ui));
                },
            );
            self.show_tape = open;
        }

        if self.show_callflow {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("callflow"),
                self.restore_window(
                    "callflow",
                    ViewportBuilder::default().with_title("Call flow"),
                    [360.0, 180.0],
                    [520.0, 700.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    if self.place_window("callflow", &ctx, [360.0, 180.0], [520.0, 700.0]) {
                        self.remember_window("callflow", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| callflow::ui(self, ui));
                },
            );
            self.show_callflow = open;
        }

        if self.show_sprites {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("sprites"),
                self.restore_window(
                    "sprites",
                    ViewportBuilder::default().with_title("Graphics"),
                    [340.0, 200.0],
                    [720.0, 640.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    if self.sprites.raise {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        self.sprites.raise = false;
                    }
                    if self.place_window("sprites", &ctx, [340.0, 200.0], [720.0, 640.0]) {
                        self.remember_window("sprites", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| sprites::ui(self, ui));
                },
            );
            self.show_sprites = open;
        }

        if self.show_back_buffer {
            let mut open = true;
            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("back-buffer"),
                self.restore_window(
                    "back_buffer",
                    ViewportBuilder::default().with_title("Back buffer"),
                    [420.0, 240.0],
                    [680.0, 700.0],
                ),
                |ui, _class| {
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    let ctx = ui.ctx().clone();
                    if self.place_window("back_buffer", &ctx, [300.0, 160.0], [680.0, 700.0]) {
                        self.remember_window("back_buffer", &ctx);
                    }
                    egui::CentralPanel::default().show(ui, |ui| back_buffer::ui(self, ui));
                },
            );
            self.show_back_buffer = open;
        }
    }

    /// Map host keys onto the 8x5 Spectrum keyboard matrix.
    fn read_keyboard(&mut self, ctx: &egui::Context) {
        use egui::Key;
        // A recording supplies every byte the machine reads from a port,
        // including the keyboard: typing at it would do nothing, and letting
        // the host keys through would only be confusing.
        if self.rzx.is_some() {
            return;
        }
        let mut matrix = [0xffu8; 8];
        let mut press = |row: usize, bit: u8| matrix[row] &= !(1 << bit);

        ctx.input(|i| {
            const MAP: &[(Key, usize, u8)] = &[
                (Key::Z, 0, 1),
                (Key::X, 0, 2),
                (Key::C, 0, 3),
                (Key::V, 0, 4),
                (Key::A, 1, 0),
                (Key::S, 1, 1),
                (Key::D, 1, 2),
                (Key::F, 1, 3),
                (Key::G, 1, 4),
                (Key::Q, 2, 0),
                (Key::W, 2, 1),
                (Key::E, 2, 2),
                (Key::R, 2, 3),
                (Key::T, 2, 4),
                (Key::Num1, 3, 0),
                (Key::Num2, 3, 1),
                (Key::Num3, 3, 2),
                (Key::Num4, 3, 3),
                (Key::Num5, 3, 4),
                (Key::Num0, 4, 0),
                (Key::Num9, 4, 1),
                (Key::Num8, 4, 2),
                (Key::Num7, 4, 3),
                (Key::Num6, 4, 4),
                (Key::P, 5, 0),
                (Key::O, 5, 1),
                (Key::I, 5, 2),
                (Key::U, 5, 3),
                (Key::Y, 5, 4),
                (Key::Enter, 6, 0),
                (Key::L, 6, 1),
                (Key::K, 6, 2),
                (Key::J, 6, 3),
                (Key::H, 6, 4),
                (Key::Space, 7, 0),
                (Key::M, 7, 2),
                (Key::N, 7, 3),
                (Key::B, 7, 4),
            ];
            for &(key, row, bit) in MAP {
                if i.key_down(key) {
                    press(row, bit);
                }
            }
            if i.modifiers.shift {
                press(0, 0); // CAPS SHIFT
            }
            if i.modifiers.alt || i.modifiers.ctrl {
                press(7, 1); // SYMBOL SHIFT
            }
            // Convenience keys that need CAPS SHIFT on real hardware.
            if i.key_down(Key::Backspace) {
                press(0, 0);
                press(4, 0);
            }
            if i.key_down(Key::ArrowLeft) {
                press(0, 0);
                press(3, 4);
            }
            if i.key_down(Key::ArrowDown) {
                press(0, 0);
                press(4, 4);
            }
            if i.key_down(Key::ArrowUp) {
                press(0, 0);
                press(4, 3);
            }
            if i.key_down(Key::ArrowRight) {
                press(0, 0);
                press(4, 2);
            }
        });
        match &mut self.zx81 {
            // The ZX81's matrix is wired the same way, minus the bottom row's
            // shift keys.
            Some(zx) => zx.bus.keys = matrix,
            None => self.spec.bus.keys = matrix,
        }
    }
}
