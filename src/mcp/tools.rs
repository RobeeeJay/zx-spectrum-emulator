//! The session the tools work on, and the tools that drive the machine.
//!
//! Everything a tool returns is text, because the thing reading it is a
//! language model: a table it can quote back is worth more than a structure it
//! has to re-serialise. Addresses are written the way the debugger writes
//! them, `$8000`, and accepted as either a number or a string.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::machine::{Model, Spectrum, Stop, FRAME_T};
use crate::mcp::json::Json;
use crate::notes::Notes;
use crate::rzx::Recording;
use crate::snapshot;
use crate::tape::Tape;

/// What the client is told the server is for, at the handshake.
pub const INSTRUCTIONS: &str = "\
Drives a ZX Spectrum emulator so a program can be taken apart and understood: \
load a tape, snapshot or RZX recording, run it under control, and read back \
what the machine did. Addresses may be given as numbers (decimal) or as \
strings ($8000, 0x8000). Start with machine_info, then load_tape or \
load_snapshot. watch_routines turns on the observer that routines, call_graph \
and autodoc read.";

/// How much memory one read will hand back. Enough for a screen third; more
/// than that in one reply is a wall of hex nobody reads.
pub const MAX_READ: usize = 4096;

/// How long a run tool will let the machine go before saying so, in frames.
/// Twenty seconds of emulated time: enough to load a level, short enough that
/// a program stuck in a loop comes back rather than hanging the conversation.
const RUN_LIMIT_FRAMES: u32 = 1000;

/// What a tool gives back: text, or a picture with something to read beside
/// it. MCP carries an image as base64 in a content block of its own, which is
/// how a model with eyes gets to see the screen rather than being told about
/// it.
pub enum Reply {
    Text(String),
    Picture { png: Vec<u8>, text: String },
}

impl Reply {
    pub fn content(&self) -> Json {
        match self {
            Reply::Text(text) => crate::mcp::content(text, false),
            Reply::Picture { png, text } => Json::obj([
                (
                    "content",
                    Json::arr(vec![
                        Json::obj([
                            ("type", Json::str("image")),
                            ("data", Json::str(crate::mcp::picture::base64(png))),
                            ("mimeType", Json::str("image/png")),
                        ]),
                        Json::obj([("type", Json::str("text")), ("text", Json::str(text))]),
                    ]),
                ),
                ("isError", Json::Bool(false)),
            ]),
        }
    }

    /// The words of it, which is all a test usually wants.
    pub fn text(&self) -> &str {
        match self {
            Reply::Text(text) => text,
            Reply::Picture { text, .. } => text,
        }
    }
}

impl From<String> for Reply {
    fn from(text: String) -> Reply {
        Reply::Text(text)
    }
}

pub struct Session {
    pub spec: Spectrum,
    /// Labels and comments. Attached to a file when one has been loaded, so
    /// they are written beside it as `<name>.zxrs.txt`.
    pub notes: Notes,
    /// Snapshots taken by save_state, by the name they were given.
    pub states: BTreeMap<String, Vec<u8>>,
    /// What was in memory when each of those was taken, as the CPU saw it, so
    /// "what changed since" can be answered without unpacking a snapshot.
    pub memories: BTreeMap<String, Vec<u8>>,
    /// The recording being played, if there is one.
    pub rzx: Option<Playing>,
    /// Where the ROMs are looked for.
    pub rom_dirs: Vec<PathBuf>,
    /// Names for addresses that came from a file rather than from this
    /// session: ROM symbols, and a game's disassembly if somebody has one.
    pub symbols: crate::autodoc::Symbols,
    /// Fingerprints of known routines, so a ROM routine copied into RAM is
    /// still recognised where it lands.
    pub signatures: crate::autodoc::Signatures,
    /// What is loaded, for machine_info to say.
    pub loaded: Option<String>,
    /// Whether a real ROM has been put in. A machine that has just been made
    /// has $FF everywhere the ROM should be, which is RST $38 forty thousand
    /// times over: it runs, it draws rubbish, and it looks from the outside
    /// like a game that has loaded and gone wrong.
    pub rom_loaded: bool,
}

/// A recording, and how far through it is.
pub struct Playing {
    pub recording: Recording,
    pub frame: usize,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Session {
        Session {
            spec: Spectrum::new(),
            notes: Notes::unattached(),
            states: BTreeMap::new(),
            memories: BTreeMap::new(),
            rzx: None,
            symbols: crate::autodoc::Symbols::default(),
            signatures: crate::autodoc::Signatures::empty(),
            rom_dirs: crate::resources::search_dirs(),
            loaded: None,
            rom_loaded: false,
        }
    }

    /// Run one tool. The error is what the model is shown, so it says what to
    /// do rather than only what went wrong.
    pub fn call(&mut self, name: &str, args: &Json) -> Result<Reply, String> {
        // Tools that give back a picture are called out; everything else
        // returns text and is wrapped here.
        match name {
            "screen" => return crate::mcp::looking::screen(self, args),
            "graphics" => return crate::mcp::looking::graphics(self, args),
            _ => {}
        }
        self.text_call(name, args).map(Reply::Text)
    }

    fn text_call(&mut self, name: &str, args: &Json) -> Result<String, String> {
        match name {
            "machine_info" => self.machine_info(),
            "set_machine" => self.set_machine(args),
            "reset" => {
                self.spec.reset();
                Ok(format!("Reset. PC ${:04X}", self.spec.cpu.pc))
            }
            "load_tape" => self.load_tape(args),
            "load_snapshot" => self.load_snapshot(args),
            "load_recording" => self.load_recording(args),
            "play_recording" => self.play_recording(args),
            "tape_blocks" => crate::mcp::deck::tape_blocks(self, args),
            "loader" => crate::mcp::deck::loader(self, args),
            "step" => self.step(args),
            "run_frames" => self.run_frames(args),
            "run_tstates" => self.run_tstates(args),
            "run_until" => self.run_until(args),
            "watch_events" => self.watch_events(args),
            "press_keys" => crate::mcp::input::press_keys(self, args),
            "type_text" => crate::mcp::input::type_text(self, args),
            "registers" => Ok(self.registers()),
            "read_memory" => crate::mcp::memory::read_memory(self, args),
            "write_memory" => crate::mcp::memory::write_memory(self, args),
            "find_bytes" => crate::mcp::memory::find_bytes(self, args),
            "changed_since" => crate::mcp::memory::changed_since(self, args),
            "save_state" => self.save_state(args),
            "restore_state" => self.restore_state(args),
            "disassemble" => crate::mcp::analysis::disassemble(self, args),
            "watch_routines" => crate::mcp::analysis::watch_routines(self, args),
            "routines" => crate::mcp::analysis::routines(self, args),
            "routine" => crate::mcp::analysis::routine(self, args),
            "call_graph" => crate::mcp::analysis::call_graph(self, args),
            "code_map" => crate::mcp::analysis::code_map(self, args),
            "xrefs" => crate::mcp::analysis::xrefs(self, args),
            "memory_activity" => crate::mcp::activity::memory_activity(self, args),
            "frame_timing" => crate::mcp::analysis::frame_timing(self, args),
            "sound_state" => crate::mcp::sound::sound_state(self, args),
            "load_symbols" => crate::mcp::names::load_symbols(self, args),
            "symbols" => crate::mcp::names::list_symbols(self, args),
            "identify" => crate::mcp::names::identify(self, args),
            "autodoc" => crate::mcp::analysis::autodoc(self, args),
            "set_comment" => crate::mcp::analysis::set_comment(self, args),
            "comments" => crate::mcp::analysis::comments(self, args),
            "save_comments" => crate::mcp::analysis::save_comments(self, args),
            "export_listing" => crate::mcp::analysis::export_listing(self, args),
            other => Err(format!("no tool called {other:?}; try tools/list")),
        }
    }

    // ---- the machine itself ------------------------------------------------

    fn machine_info(&self) -> Result<String, String> {
        let bus = &self.spec.bus;
        let mut out = String::new();
        out.push_str(&format!(
            "{} — {:.2}MHz, {} T-states a frame, {} a line\n",
            bus.model.name(),
            bus.model.cpu_hz() / 1e6,
            bus.model.frame_t(),
            bus.model.t_per_line()
        ));
        out.push_str(&format!(
            "frame {}, T {}, border {}\n",
            bus.frame, bus.tstates, bus.border
        ));
        out.push_str(&format!(
            "ROM {} bytes loaded, paging {}\n",
            bus.rom.len(),
            if bus.model.has_paging() { "yes" } else { "no" }
        ));
        if bus.model.has_paging() {
            out.push_str(&format!(
                "page register ${:02X}, ROM {} in use, banks visible {:?}\n",
                bus.page_reg,
                bus.rom_in_use(),
                bus.visible_banks()
            ));
        }
        match &self.loaded {
            Some(what) => out.push_str(&format!("loaded: {what}\n")),
            None => out.push_str("loaded: nothing yet\n"),
        }
        if let Some(rzx) = &self.rzx {
            out.push_str(&format!(
                "recording: frame {} of {}\n",
                rzx.frame,
                rzx.recording.len()
            ));
        }
        out.push_str(&format!(
            "observer: {}, {} routines seen\n",
            if bus.observer.enabled { "on" } else { "off" },
            bus.observer.routines.len()
        ));
        Ok(out)
    }

    fn set_machine(&mut self, args: &Json) -> Result<String, String> {
        let name = text(args, "model")?;
        let model = match name.to_ascii_lowercase().replace([' ', '-'], "").as_str() {
            "48" | "48k" | "spectrum48" => Model::Spectrum48,
            "128" | "128k" | "spectrum128" => Model::Spectrum128,
            "+2a" | "plus2a" => Model::Plus2A,
            "+3" | "plus3" => Model::Plus3,
            other => return Err(format!("no model {other:?}: try 48k, 128k, +2a, +3")),
        };
        let rom = self.rom_for(model)?;
        self.spec.set_model(model, &rom);
        self.spec.reset();
        self.rom_loaded = true;
        Ok(format!("{} it is, and reset", model.name()))
    }

    /// The ROM image for a model, from wherever the emulator keeps them.
    fn rom_for(&self, model: Model) -> Result<Vec<u8>, String> {
        let names: &[&str] = match model {
            Model::Spectrum48 => &["48.rom", "spectrum48.rom", "48k.rom"],
            Model::Spectrum128 => &["128.rom", "spectrum128.rom"],
            Model::Plus2A | Model::Plus3 => &["plus3.rom", "+3.rom"],
        };
        crate::resources::find_file(&self.rom_dirs, names, model.rom_size())
            .map(|(_, data)| data)
            .ok_or_else(|| {
                format!(
                    "no ROM for the {}: looked for {} in {}",
                    model.name(),
                    names.join(" or "),
                    self.rom_dirs
                        .iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// Make sure a real ROM is in, since a machine with none runs nothing —
    /// and, worse, looks as though it is running something.
    fn ensure_rom(&mut self) -> Result<(), String> {
        if self.rom_loaded {
            return Ok(());
        }
        let rom = self.rom_for(self.spec.bus.model)?;
        self.spec.load_rom(&rom);
        self.spec.reset();
        self.rom_loaded = true;
        Ok(())
    }

    // ---- loading -----------------------------------------------------------

    fn load_tape(&mut self, args: &Json) -> Result<String, String> {
        let path = PathBuf::from(text(args, "path")?);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (inner, bytes) = if path.extension().is_some_and(|e| e == "zip") {
            crate::zip::first_with_extension(&bytes, &["tzx", "tap"])
                .ok_or_else(|| format!("{}: no tape inside the zip", path.display()))?
        } else {
            (path.display().to_string(), bytes)
        };
        let tape = Tape::from_bytes(&inner, &bytes)?;
        let blocks = tape.blocks.len();
        self.ensure_rom()?;
        self.notes = Notes::for_file(&path);
        self.spec.bus.tape = Some(tape);
        self.loaded = Some(format!("tape {}", path.display()));

        if flag(args, "autoload", true) {
            let report = self.autoload()?;
            return Ok(format!(
                "Loaded {} ({blocks} blocks) and started it. {report}",
                path.display()
            ));
        }
        Ok(format!(
            "{} is in the deck: {blocks} blocks. Nothing is playing yet.",
            path.display()
        ))
    }

    /// Type LOAD "" and let the tape run until it stops, with the ROM's own
    /// loading handed over where it can be — a tape plays at 1,500 baud
    /// however fast the machine is run, and waiting for it in real time is
    /// forty minutes of nothing.
    fn autoload(&mut self) -> Result<String, String> {
        self.spec.reset();
        self.spec.bus.tape_flash = true;
        self.spec.bus.tape_boost = true;
        for _ in 0..120 {
            self.spec.run(FRAME_T);
        }
        // J, then symbol-shift P twice, then ENTER: LOAD "" as the 48K ROM
        // wants it typed.
        for keys in [
            &[(6usize, 3u8)][..],
            &[(7, 1), (5, 0)][..],
            &[(7, 1), (5, 0)][..],
            &[(6, 0)][..],
        ] {
            for (row, bit) in keys {
                self.spec.bus.keys[*row] &= !(1 << bit);
            }
            for _ in 0..4 {
                self.spec.run(FRAME_T);
            }
            for (row, bit) in keys {
                self.spec.bus.keys[*row] |= 1 << bit;
            }
            for _ in 0..4 {
                self.spec.run(FRAME_T);
            }
        }
        let now = self.spec.bus.total_t();
        if let Some(tape) = self.spec.bus.tape.as_mut() {
            tape.play(now);
        }
        let mut frames = 0u32;
        while self.spec.bus.tape_playing() && frames < 60 * 60 * 10 {
            self.spec.run(FRAME_T);
            frames += 1;
        }
        // A moment more, so a loader that starts the game as the tape ends has
        // somewhere to start it.
        for _ in 0..200 {
            self.spec.run(FRAME_T);
        }
        Ok(format!(
            "The tape ran for {frames} frames; PC is now ${:04X}",
            self.spec.cpu.pc
        ))
    }

    fn load_snapshot(&mut self, args: &Json) -> Result<String, String> {
        let path = PathBuf::from(text(args, "path")?);
        let model = snapshot::probe_model(&path)?;
        let rom = self.rom_for(model)?;
        self.spec.set_model(model, &rom);
        snapshot::load(&mut self.spec, &path)?;
        self.rom_loaded = true;
        self.notes = Notes::for_file(&path);
        self.loaded = Some(format!("snapshot {}", path.display()));
        Ok(format!(
            "{} loaded into a {}. PC ${:04X}, SP ${:04X}",
            path.display(),
            model.name(),
            self.spec.cpu.pc,
            self.spec.cpu.sp
        ))
    }

    fn load_recording(&mut self, args: &Json) -> Result<String, String> {
        let path = PathBuf::from(text(args, "path")?);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let recording = crate::rzx::parse(&bytes)?;
        let snap = recording
            .snapshot
            .clone()
            .ok_or("the recording carries no snapshot to start from")?;
        let model = snapshot::probe_model_bytes(&snap.extension, &snap.data)?;
        let rom = self.rom_for(model)?;
        self.spec.set_model(model, &rom);
        match snap.extension.as_str() {
            "sna" => snapshot::load_sna(&mut self.spec, &snap.data)?,
            "z80" => snapshot::load_z80(&mut self.spec, &snap.data)?,
            other => return Err(format!("unsupported snapshot in the recording: .{other}")),
        }
        self.rom_loaded = true;
        self.spec.bus.playback = Some(crate::machine::Playback::default());
        let frames = recording.len();
        self.notes = Notes::for_file(&path);
        self.rzx = Some(Playing {
            recording,
            frame: 0,
        });
        self.loaded = Some(format!("recording {}", path.display()));
        Ok(format!(
            "{} loaded: {frames} frames on a {}. play_recording runs it.",
            path.display(),
            model.name()
        ))
    }

    /// Play a recording forward. A frame of a recording is a number of opcode
    /// fetches, not a number of T-states, and the frame interrupt comes at the
    /// recording's boundary rather than on the clock — see `docs/timing.md`.
    fn play_recording(&mut self, args: &Json) -> Result<String, String> {
        let want = count(args, "frames", 1)?;
        let mut ran = 0u32;
        let mut short = 0u32;
        let mut stopped = None;
        while ran < want {
            let Some(rzx) = &mut self.rzx else {
                return Err("no recording is loaded".into());
            };
            if rzx.frame >= rzx.recording.len() {
                break;
            }
            let frame = &rzx.recording.frames[rzx.frame];
            let fetches = frame.fetches.max(1) as u32;
            let inputs = frame.inputs.clone();
            rzx.frame += 1;
            if let Some(playback) = &mut self.spec.bus.playback {
                playback.inputs = inputs;
                playback.cursor = 0;
            }
            let (stop, _) = self.spec.run_fetches(fetches);
            self.spec.bus.end_frame_here();
            self.spec.bus.raise_interrupt();
            ran += 1;
            short = self.spec.bus.playback.as_ref().map_or(0, |p| p.short);
            if !matches!(stop, Stop::Budget) {
                stopped = Some(stop);
                break;
            }
        }
        let rzx = self.rzx.as_ref().ok_or("no recording is loaded")?;
        let mut out = format!(
            "Played {ran} frames; at frame {} of {}. PC ${:04X}",
            rzx.frame,
            rzx.recording.len(),
            self.spec.cpu.pc
        );
        if let Some(stop) = stopped {
            out.push_str(&format!("\nStopped: {}", describe_stop(stop)));
        }
        if short > 0 {
            out.push_str(&format!(
                "\n{short} frames asked for more input than the recording holds: it has come adrift."
            ));
        }
        Ok(out)
    }

    // ---- running -----------------------------------------------------------

    fn step(&mut self, args: &Json) -> Result<String, String> {
        let count = count(args, "count", 1)?.min(10_000);
        let over = flag(args, "over", false);
        let mut lines = Vec::new();
        for _ in 0..count {
            let pc = self.spec.cpu.pc;
            let insn = crate::disasm::disasm(&|a| self.spec.bus.peek_raw(a), pc);
            if over && self.spec.is_step_over_target(pc) {
                let after = pc.wrapping_add(insn.len as u16);
                let stop = self.run_to(&[after], RUN_LIMIT_FRAMES)?;
                lines.push(format!("${pc:04X}  {:<20} (over)", insn.text));
                if !matches!(stop, Stop::Breakpoint(_)) {
                    lines.push(format!("  did not come back: {}", describe_stop(stop)));
                    break;
                }
            } else {
                lines.push(format!("${pc:04X}  {}", insn.text));
                self.spec.step_instruction();
            }
        }
        lines.push(self.registers());
        Ok(lines.join("\n"))
    }

    fn run_frames(&mut self, args: &Json) -> Result<String, String> {
        let frames = count(args, "frames", 1)?.min(RUN_LIMIT_FRAMES * 10);
        let before = self.spec.bus.frame;
        let mut stopped = None;
        for _ in 0..frames {
            let stop = self.spec.run(FRAME_T);
            if !matches!(stop, Stop::Budget) {
                stopped = Some(stop);
                break;
            }
        }
        Ok(self.after_running(self.spec.bus.frame - before, stopped))
    }

    fn run_tstates(&mut self, args: &Json) -> Result<String, String> {
        let want = count(args, "tstates", 1)?;
        let before = self.spec.bus.total_t();
        let mut left = want;
        let mut stopped = None;
        while left > 0 {
            let slice = left.min(FRAME_T);
            let stop = self.spec.run(slice);
            let done = (self.spec.bus.total_t() - before) as u32;
            if !matches!(stop, Stop::Budget) {
                stopped = Some(stop);
                break;
            }
            left = want.saturating_sub(done);
        }
        let ran = self.spec.bus.total_t() - before;
        let mut out = format!("Ran {ran} T-states. {}", self.registers());
        if let Some(stop) = stopped {
            out.push_str(&format!("\nStopped: {}", describe_stop(stop)));
        }
        Ok(out)
    }

    fn run_until(&mut self, args: &Json) -> Result<String, String> {
        let mut wanted = Vec::new();
        if let Some(one) = args.get("address") {
            wanted.push(addr_of(one)?);
        }
        if let Some(list) = args.get("addresses").and_then(|a| a.as_array()) {
            for item in list {
                wanted.push(addr_of(item)?);
            }
        }
        if wanted.is_empty() {
            return Err("run_until needs an address, or addresses".into());
        }
        let limit = count(args, "max_frames", RUN_LIMIT_FRAMES)?;
        let before = self.spec.bus.frame;
        let stop = self.run_to(&wanted, limit)?;
        Ok(self.after_running(self.spec.bus.frame - before, Some(stop)))
    }

    /// Run with temporary breakpoints on `wanted`, putting back whatever
    /// breakpoints were there before.
    fn run_to(&mut self, wanted: &[u16], limit_frames: u32) -> Result<Stop, String> {
        let kept = std::mem::take(&mut self.spec.breakpoints);
        self.spec.breakpoints = wanted.to_vec();
        let mut stop = Stop::Budget;
        for _ in 0..limit_frames {
            stop = self.spec.run(FRAME_T);
            if !matches!(stop, Stop::Budget) {
                break;
            }
        }
        self.spec.breakpoints = kept;
        Ok(stop)
    }

    fn watch_events(&mut self, args: &Json) -> Result<String, String> {
        let breaks = &mut self.spec.bus.breaks;
        for (key, field) in [
            ("screen", 0),
            ("beeper", 1),
            ("ay", 2),
            ("interrupt", 3),
            ("rom", 4),
            ("port_in", 5),
            ("port_out", 6),
        ] {
            if let Some(on) = args.get(key).and_then(|v| v.as_bool()) {
                match field {
                    0 => breaks.screen = on,
                    1 => breaks.beeper = on,
                    2 => breaks.ay = on,
                    3 => breaks.interrupt = on,
                    4 => breaks.rom = on,
                    5 => breaks.port_in = on,
                    _ => breaks.port_out = on,
                }
            }
        }
        // A write to an address, or to a range of them. The one watch that is
        // about a place rather than a kind of thing.
        if let Some(value) = args.get("write_to") {
            if value.is_null() {
                self.spec.bus.breaks.write_range = None;
            } else {
                let low = addr_of(value)?;
                let high = match args.get("write_to_end") {
                    Some(end) => addr_of(end)?,
                    None => low,
                };
                self.spec.bus.breaks.write_range = Some((low.min(high), low.max(high)));
            }
        }

        let breaks = self.spec.bus.breaks;
        let mut on: Vec<String> = Vec::new();
        if let Some((low, high)) = breaks.write_range {
            on.push(if low == high {
                format!("writes to ${low:04X}")
            } else {
                format!("writes to ${low:04X}-${high:04X}")
            });
        }
        for (name, set) in [
            ("screen", breaks.screen),
            ("beeper", breaks.beeper),
            ("ay", breaks.ay),
            ("interrupt", breaks.interrupt),
            ("rom", breaks.rom),
            ("port_in", breaks.port_in),
            ("port_out", breaks.port_out),
        ] {
            if set {
                on.push(name.to_string());
            }
        }
        let watching = if on.is_empty() {
            "nothing".to_string()
        } else {
            on.join(", ")
        };
        if flag(args, "run", false) {
            let frames = count(args, "max_frames", RUN_LIMIT_FRAMES)?;
            let mut stop = Stop::Budget;
            for _ in 0..frames {
                stop = self.spec.run(FRAME_T);
                if !matches!(stop, Stop::Budget) {
                    break;
                }
            }
            return Ok(format!(
                "Watching {watching}. Stopped: {}\n{}",
                describe_stop(stop),
                self.registers()
            ));
        }
        Ok(format!(
            "Watching {watching}. Add run: true to run until one of them happens."
        ))
    }

    fn after_running(&self, frames: u64, stop: Option<Stop>) -> String {
        let mut out = format!("Ran {frames} frames. {}", self.registers());
        if let Some(stop) = stop {
            out.push_str(&format!("\nStopped: {}", describe_stop(stop)));
        }
        out
    }

    // ---- state -------------------------------------------------------------

    pub fn registers(&self) -> String {
        let cpu = &self.spec.cpu;
        let bus = &self.spec.bus;
        format!(
            "PC=${:04X} SP=${:04X} AF=${:02X}{:02X} BC=${:02X}{:02X} DE=${:02X}{:02X} \
             HL=${:02X}{:02X} IX=${:04X} IY=${:04X}\n\
             AF'=${:02X}{:02X} BC'=${:02X}{:02X} DE'=${:02X}{:02X} HL'=${:02X}{:02X} \
             I=${:02X} R=${:02X} IM{} IFF1={} IFF2={}{}\n\
             flags {} | frame {} T {} border {}",
            cpu.pc,
            cpu.sp,
            cpu.a,
            cpu.f,
            cpu.b,
            cpu.c,
            cpu.d,
            cpu.e,
            cpu.h,
            cpu.l,
            cpu.ix,
            cpu.iy,
            cpu.a_,
            cpu.f_,
            cpu.b_,
            cpu.c_,
            cpu.d_,
            cpu.e_,
            cpu.h_,
            cpu.l_,
            cpu.i,
            cpu.r,
            cpu.im,
            cpu.iff1 as u8,
            cpu.iff2 as u8,
            if cpu.halted { " HALTED" } else { "" },
            flags(cpu.f),
            bus.frame,
            bus.tstates,
            bus.border,
        )
    }

    fn save_state(&mut self, args: &Json) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("last")
            .to_string();
        let data = snapshot::save_sna(&self.spec);
        let mut out = format!(
            "Saved the machine as {name:?}: {} bytes, PC ${:04X}",
            data.len(),
            self.spec.cpu.pc
        );
        if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
            std::fs::write(path, &data).map_err(|e| format!("{path}: {e}"))?;
            out.push_str(&format!(", and written to {path}"));
        }
        self.states.insert(name.clone(), data);
        // And the memory as the CPU sees it, for changed_since. 48K of it,
        // which is nothing beside what a snapshot costs.
        self.memories.insert(
            name,
            (0x4000..=0xFFFFu32)
                .map(|a| self.spec.bus.peek_raw(a as u16))
                .collect(),
        );
        Ok(out)
    }

    fn restore_state(&mut self, args: &Json) -> Result<String, String> {
        if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
            let path = Path::new(path);
            let model = snapshot::probe_model(path)?;
            let rom = self.rom_for(model)?;
            self.spec.set_model(model, &rom);
            snapshot::load(&mut self.spec, path)?;
            self.rom_loaded = true;
            return Ok(format!(
                "Restored from {}. PC ${:04X}",
                path.display(),
                self.spec.cpu.pc
            ));
        }
        let name = args.get("name").and_then(|n| n.as_str()).unwrap_or("last");
        let data = self
            .states
            .get(name)
            .ok_or_else(|| {
                format!(
                    "no state called {name:?}; there is {}",
                    if self.states.is_empty() {
                        "nothing saved".to_string()
                    } else {
                        self.states.keys().cloned().collect::<Vec<_>>().join(", ")
                    }
                )
            })?
            .clone();
        snapshot::load_sna(&mut self.spec, &data)?;
        Ok(format!(
            "Restored {name:?}. PC ${:04X}, SP ${:04X}",
            self.spec.cpu.pc, self.spec.cpu.sp
        ))
    }
}

// ---- shared helpers --------------------------------------------------------

/// A required string argument.
pub fn text(args: &Json, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("{key} is needed, as a string"))
}

/// A count, with a default when it is not given.
pub fn count(args: &Json, key: &str, default: u32) -> Result<u32, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(value) => value
            .as_i64()
            .filter(|v| *v >= 0)
            .map(|v| v as u32)
            .ok_or_else(|| format!("{key} should be a whole number, not {value}")),
    }
}

/// A count with a floor and a ceiling, since a tile view forty across is not
/// a tile view.
pub fn count_in(args: &Json, key: &str, default: u32, low: u32, high: u32) -> Result<u32, String> {
    let value = count(args, key, default)?;
    if !(low..=high).contains(&value) {
        return Err(format!(
            "{key} should be between {low} and {high}, not {value}"
        ));
    }
    Ok(value)
}

pub fn flag(args: &Json, key: &str, default: bool) -> bool {
    args.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// An address argument. A number is decimal, as JSON numbers are; a string may
/// be `$8000`, `0x8000` or `8000h`, since that is how everything else in this
/// emulator writes them. A bare string of digits is decimal, deliberately: a
/// disassembly that read decimal addresses as hex found nothing at all and
/// said so quietly, which is a mistake worth only making once.
pub fn addr(args: &Json, key: &str) -> Result<u16, String> {
    let value = args
        .get(key)
        .ok_or_else(|| format!("{key} is needed: a number, or a string like \"$8000\""))?;
    addr_of(value)
}

pub fn addr_of(value: &Json) -> Result<u16, String> {
    match value {
        Json::Num(n) if n.fract() == 0.0 && (0.0..=65535.0).contains(n) => Ok(*n as u16),
        Json::Str(text) => {
            let t = text.trim();
            let (digits, radix) = if let Some(rest) = t.strip_prefix('$') {
                (rest, 16)
            } else if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                (rest, 16)
            } else if let Some(rest) = t.strip_suffix('h').or_else(|| t.strip_suffix('H')) {
                (rest, 16)
            } else {
                (t, 10)
            };
            u32::from_str_radix(digits, radix)
                .ok()
                .filter(|v| *v <= 0xFFFF)
                .map(|v| v as u16)
                .ok_or_else(|| format!("{text:?} is not an address in 0..=$FFFF"))
        }
        other => Err(format!("{other} is not an address")),
    }
}

/// The flags register, spelled out. SZ5H3PNC is the order the bits are in.
pub fn flags(f: u8) -> String {
    let names = ['S', 'Z', '5', 'H', '3', 'P', 'N', 'C'];
    names
        .iter()
        .enumerate()
        .map(|(i, name)| if f & (0x80 >> i) != 0 { *name } else { '.' })
        .collect()
}

pub fn describe_stop(stop: Stop) -> String {
    match stop {
        Stop::Budget => "the time asked for ran out".to_string(),
        Stop::SlowDraw => "the slow-draw write allowance ran out".to_string(),
        Stop::Stepped => "one instruction was stepped".to_string(),
        Stop::Breakpoint(at) => format!("breakpoint at ${at:04X}"),
        // The machine's own words for it, which say what happened rather than
        // naming the variant.
        Stop::Watched(event, at) => format!("{} — at ${at:04X}", event.describe()),
    }
}
