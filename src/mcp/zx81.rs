//! The ZX81, which is a different machine and not a Spectrum with less in it.
//!
//! There is no ULA drawing the screen for it: the CPU walks the display file
//! and the chip watches the bus, so an opcode fetched above $8000 with bit 6
//! clear is fed to the CPU as a NOP and turned into eight pixels on the way
//! past. That is why so much of what the Spectrum tools measure has no meaning
//! here — there is no display file to watch being written, and no observer
//! attached to this machine's bus.
//!
//! What is supported is what a ZX81 program can be taken apart with: loading,
//! running, registers, memory, the disassembly and the notes.

use crate::mcp::json::Json;
use crate::mcp::tools::{addr, count, flag, text, Reply, Session};
use crate::zx81::{Ram, Zx81};

/// Start a ZX81. The Spectrum is put away rather than thrown away, so a
/// session can go back to it.
pub fn start(session: &mut Session, ram: Ram) -> Result<String, String> {
    let names: &[&str] = &["zx81.rom", "ZX81.rom", "zx81.bin"];
    let (_, rom) = crate::resources::find_file(&session.rom_dirs, names, 8192)
        .ok_or("no ZX81 ROM: looked for zx81.rom")?;
    let mut machine = Zx81::new(ram);
    machine.load_rom(&rom);
    machine.reset();
    session.zx81 = Some(machine);
    Ok(format!(
        "A ZX81 with {}. The screen is drawn by the CPU walking the display file, so the \
         tools that watch a ULA — the observer, the frame timing, the sound chip — have \
         nothing to report here.",
        ram.name()
    ))
}

/// Whether the ZX81 is the machine in use.
pub fn in_use(session: &Session) -> bool {
    session.zx81.is_some()
}

/// A .p or .81 file: the ZX81's own snapshot, which is its memory from $4009
/// up and nothing else.
pub fn load_program(session: &mut Session, args: &Json) -> Result<String, String> {
    let path = text(args, "path")?;
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    if session.zx81.is_none() {
        start(session, Ram::K16)?;
    }
    let machine = session.zx81.as_mut().expect("just started");
    machine.load_p(&data)?;
    session.loaded = Some(format!("ZX81 program {path}"));
    session.notes = crate::notes::Notes::for_file(std::path::Path::new(&path));
    Ok(format!(
        "{path} loaded. PC ${:04X}. Run it and it picks up where the program was saved.",
        machine.cpu.pc
    ))
}

pub fn run_frames(session: &mut Session, args: &Json) -> Result<String, String> {
    let want = count(args, "frames", 1)?.min(10_000);
    let machine = session.zx81.as_mut().ok_or("no ZX81 is running")?;
    let before = machine.bus.frame;
    let mut stopped = None;
    for _ in 0..want {
        let frame_t = machine.frame_t();
        if let Some(at) = machine.run(frame_t) {
            stopped = Some(at);
            break;
        }
    }
    let ran = machine.bus.frame - before;
    let mut out = format!("Ran {ran} frames. {}", registers(session));
    if let Some(at) = stopped {
        out.push_str(&format!("\nStopped at a breakpoint: ${at:04X}"));
    }
    Ok(out)
}

pub fn step(session: &mut Session, args: &Json) -> Result<String, String> {
    let count = count(args, "count", 1)?.min(10_000);
    let mut lines = Vec::new();
    for _ in 0..count {
        let machine = session.zx81.as_mut().ok_or("no ZX81 is running")?;
        let pc = machine.cpu.pc;
        let insn = crate::disasm::disasm(&|a| machine.bus.peek_raw(a), pc);
        lines.push(format!("${pc:04X}  {}", insn.text));
        machine.step_instruction();
    }
    lines.push(registers(session));
    Ok(lines.join("\n"))
}

pub fn registers(session: &Session) -> String {
    let Some(machine) = session.zx81.as_ref() else {
        return "no ZX81 is running".into();
    };
    let cpu = &machine.cpu;
    format!(
        "PC=${:04X} SP=${:04X} AF=${:02X}{:02X} BC=${:02X}{:02X} DE=${:02X}{:02X} \
         HL=${:02X}{:02X} IX=${:04X} IY=${:04X}\n\
         I=${:02X} R=${:02X} IM{} IFF1={}{} | flags {} | frame {} T {} line {}",
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
        cpu.i,
        cpu.r,
        cpu.im,
        cpu.iff1 as u8,
        if cpu.halted { " HALTED" } else { "" },
        crate::mcp::tools::flags(cpu.f),
        machine.bus.frame,
        machine.bus.tstates,
        machine.bus.line,
    )
}

pub fn read_memory(session: &mut Session, args: &Json) -> Result<String, String> {
    let start = addr(args, "address")?;
    let length = count(args, "length", 256)?.min(crate::mcp::tools::MAX_READ as u32) as usize;
    let machine = session.zx81.as_ref().ok_or("no ZX81 is running")?;
    let bytes: Vec<u8> = (0..length)
        .map(|i| machine.bus.peek_raw(start.wrapping_add(i as u16)))
        .collect();
    Ok(format!(
        "{length} bytes from ${start:04X}. The ZX81's character set is its own — this is \
         not ASCII, and the printable column will be wrong for text.\n{}",
        crate::mcp::memory::hex_dump(start, &bytes)
    ))
}

pub fn write_memory(session: &mut Session, args: &Json) -> Result<String, String> {
    let start = addr(args, "address")?;
    let bytes = crate::mcp::memory::bytes_argument(args)?;
    let machine = session.zx81.as_mut().ok_or("no ZX81 is running")?;
    for (i, byte) in bytes.iter().enumerate() {
        machine.bus.poke(start.wrapping_add(i as u16), *byte);
    }
    Ok(format!("Wrote {} bytes at ${start:04X}.", bytes.len()))
}

pub fn disassemble(session: &mut Session, args: &Json) -> Result<String, String> {
    let machine = session.zx81.as_ref().ok_or("no ZX81 is running")?;
    let start = match args.get("address") {
        Some(_) => addr(args, "address")?,
        None => machine.cpu.pc,
    };
    let lines = count(args, "count", 32)?.min(512);
    let show_notes = flag(args, "comments", true);
    let mut out = String::new();
    let mut at = start;
    for _ in 0..lines {
        let machine = session.zx81.as_ref().expect("still there");
        let insn = crate::disasm::disasm(&|a| machine.bus.peek_raw(a), at);
        let bytes: String = insn.bytes.iter().map(|b| format!("{b:02X} ")).collect();
        let label = session.notes.label(at);
        let comment = session.notes.comment(at);
        if show_notes && !label.is_empty() {
            out.push_str(&format!("{label}:\n"));
        }
        out.push_str(&format!("${at:04X}  {bytes:<12} {:<20}", insn.text));
        if show_notes && !comment.is_empty() {
            out.push_str(&format!("; {comment}"));
        }
        out.push('\n');
        at = at.wrapping_add(insn.len.max(1) as u16);
    }
    out.push_str(&format!("(next address ${at:04X})\n"));
    Ok(out)
}

/// The screen, as a picture and as characters. The ZX81's display is a file of
/// character codes rather than a bitmap, so what is drawn comes from the frame
/// buffer the machine built as it ran.
pub fn screen(session: &mut Session, args: &Json) -> Result<Reply, String> {
    let machine = session.zx81.as_ref().ok_or("no ZX81 is running")?;
    let view = if flag(args, "border", false) {
        crate::zx81::View::OVERSCAN
    } else {
        crate::zx81::View::CROPPED
    };
    let mut pixels = vec![0u8; view.buffer_len()];
    machine.bus.render(view, &mut pixels);
    let words = format!(
        "The ZX81's screen, frame {}. Black on white, and drawn by the CPU rather than by \
         video hardware.",
        machine.bus.frame
    );
    if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
        let png = crate::mcp::picture::encode(&pixels, view.w, view.h)?;
        std::fs::write(path, &png).map_err(|e| format!("{path}: {e}"))?;
        return Ok(Reply::Text(format!("{words}\nWritten to {path}")));
    }
    let png = crate::mcp::picture::encode(&pixels, view.w, view.h)?;
    Ok(Reply::Picture { png, text: words })
}

/// What a Spectrum tool says when it is asked of a ZX81.
pub fn not_here(tool: &str) -> String {
    format!(
        "{tool} is about hardware the ZX81 does not have. It has no ULA drawing the screen \
         — the CPU does that — no sound chip, no paging, and this build attaches the \
         observer to the Spectrum's bus only. Switch back with set_machine 48k, or use \
         step, run_frames, registers, read_memory, disassemble and the notes, which work \
         here."
    )
}
