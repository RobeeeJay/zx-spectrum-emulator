//! A first guess at what the code being disassembled is doing.
//!
//! This is inference, not knowledge. It follows the code from where the
//! machine is, notes every routine that gets called, and reads each one for
//! the marks a Spectrum program leaves when it does a particular job: the
//! ports it touches, the parts of memory it writes to, the ROM routines it
//! calls, the block instructions it uses. Rules are tried in order and the
//! first that fits names the routine.
//!
//! What comes out is a starting point for reading a program, not a
//! decompilation. It is kept apart from what the user types and is never
//! written to their notes file: a guess should not end up in somebody's own
//! annotations, and the toggle that produces it can be turned off again.

use std::collections::{BTreeMap, BTreeSet};

use crate::disasm;

/// The display file, and the attributes after it.
const SCREEN: std::ops::Range<u16> = 0x4000..0x5800;
const ATTRS: std::ops::Range<u16> = 0x5800..0x5B00;
/// The last two character rows, where a game usually keeps its score panel.
const PANEL: std::ops::Range<u16> = 0x5000..0x5800;
/// The ROM's character set.
const FONT: u16 = 0x3D00;

/// How much code to read before giving up, so a run through uninitialised
/// memory cannot take the debugger with it.
const MAX_INSTRUCTIONS: usize = 20_000;
/// How far a single routine is followed.
const MAX_ROUTINE: usize = 400;

/// Routines in the 48K ROM worth recognising by address. These are the ones a
/// program actually calls; the rest of the ROM is BASIC's own business.
pub const ROM_ROUTINES: &[(u16, &str, &str)] = &[
    (0x0000, "rom_start", "ROM: reset"),
    (0x0008, "rom_error", "ROM: report an error"),
    (0x0010, "rom_print_a", "ROM: print the character in A"),
    (0x0018, "rom_get_char", "ROM: collect a character"),
    (0x0020, "rom_next_char", "ROM: collect the next character"),
    (0x0028, "rom_calculator", "ROM: floating point calculator"),
    (0x0038, "rom_mask_int", "ROM: the frame interrupt handler"),
    (0x028E, "rom_key_scan", "ROM: scan the keyboard"),
    (
        0x02BF,
        "rom_keyboard",
        "ROM: read the keyboard into the buffer",
    ),
    (0x0333, "rom_k_decode", "ROM: turn a key into a character"),
    (0x03B5, "rom_beeper", "ROM: sound a note (BEEP)"),
    (0x03F8, "rom_beep", "ROM: BEEP command"),
    (0x04C2, "rom_sa_bytes", "ROM: save a block to tape"),
    (0x0556, "rom_ld_bytes", "ROM: load a block from tape"),
    (0x0605, "rom_save_etc", "ROM: SAVE/LOAD/VERIFY/MERGE"),
    (0x07CB, "rom_ld_block", "ROM: read a tape block"),
    (0x0808, "rom_ld_edge", "ROM: wait for a tape edge"),
    (0x0949, "rom_copy", "ROM: COPY the screen to a printer"),
    (0x09F4, "rom_pr_string", "ROM: print a string"),
    (0x0B24, "rom_po_any", "ROM: print any character"),
    (0x0B7F, "rom_pr_all", "ROM: print a character cell"),
    (0x0BDB, "rom_po_attr", "ROM: set the attribute of a cell"),
    (0x0C0A, "rom_po_msg", "ROM: print a message from a table"),
    (0x0D4D, "rom_temps", "ROM: set temporary colours"),
    (0x0D6B, "rom_cls", "ROM: clear the screen"),
    (0x0DAF, "rom_cl_all", "ROM: clear the whole display"),
    (0x0DFE, "rom_cl_scroll", "ROM: scroll the display"),
    (0x0E44, "rom_cl_line", "ROM: clear lines of the display"),
    (0x0E9B, "rom_cl_addr", "ROM: work out a screen address"),
    (0x0EAC, "rom_copy_buff", "ROM: printer buffer"),
    (0x1601, "rom_chan_open", "ROM: open a channel (stream)"),
    (0x15F2, "rom_print_out", "ROM: print out a character"),
    (0x1605, "rom_chan_flag", "ROM: set the channel flags"),
    (0x16B0, "rom_set_min", "ROM: reset the work areas"),
    (0x1B17, "rom_line_run", "ROM: run a BASIC line"),
    (0x1C8C, "rom_expt_exp", "ROM: evaluate an expression"),
    (0x203C, "rom_str_data", "ROM: fetch a string variable"),
    (0x229B, "rom_plot", "ROM: PLOT a pixel"),
    (0x2307, "rom_draw", "ROM: DRAW a line"),
    (0x2477, "rom_circle", "ROM: CIRCLE"),
    (
        0x2AB6,
        "rom_stk_store",
        "ROM: put a value on the calculator stack",
    ),
    (
        0x2BF1,
        "rom_stk_fetch",
        "ROM: take a value off the calculator stack",
    ),
    (0x2D28, "rom_stack_a", "ROM: stack the value in A"),
    (0x2DA2, "rom_fp_to_bc", "ROM: floating point to BC"),
    (0x3D00, "rom_char_set", "ROM: the character set"),
];

/// What the analysis produced: a name for a routine, and a note on the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doc {
    pub labels: BTreeMap<u16, String>,
    pub comments: BTreeMap<u16, String>,
}

impl Doc {
    pub fn label(&self, addr: u16) -> &str {
        self.labels.get(&addr).map_or("", |s| s.as_str())
    }

    pub fn comment(&self, addr: u16) -> &str {
        self.comments.get(&addr).map_or("", |s| s.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty() && self.comments.is_empty()
    }
}

/// What a routine was seen to do. Rules are written against this rather than
/// against the instructions, so the reasoning is in one place and readable.
#[derive(Clone, Debug, Default)]
pub struct Features {
    /// How many instructions were read before the routine ended.
    pub length: usize,
    /// Mnemonics, in order, as the disassembler prints them.
    pub text: Vec<String>,
    /// Addresses this routine calls, ROM and otherwise.
    pub calls: BTreeSet<u16>,
    /// Sixteen-bit constants loaded into registers: where a routine is aimed.
    pub constants: Vec<u16>,
    /// Ports read from and written to, as far as they can be told statically.
    pub ports_in: BTreeSet<u16>,
    pub ports_out: BTreeSet<u16>,
    /// Block copies and fills.
    pub ldir: bool,
    pub lddr: bool,
    /// Bit shuffling of the kind a decompressor does.
    pub shifts: usize,
    /// Writes through a register pair rather than to a fixed address.
    pub indirect_writes: usize,
    /// XOR or OR against memory: how a sprite is drawn.
    pub masked_writes: usize,
    /// Comparisons, which is what checking and colliding look like.
    pub compares: usize,
    /// Decimal adjust, which in practice means a score.
    pub daa: bool,
    /// The routine reads the refresh register, or the ROM's own bytes.
    pub reads_r: bool,
    pub reads_rom: bool,
}

impl Features {
    fn touches(&self, range: &std::ops::Range<u16>) -> bool {
        self.constants.iter().any(|a| range.contains(a))
    }

    fn calls_rom(&self, addr: u16) -> bool {
        self.calls.contains(&addr)
    }

    /// Whether one of the screen sizes turns up as a constant: 6144 bytes of
    /// pixels, 768 of attributes, or the 6912 of both.
    fn screen_sized(&self) -> bool {
        self.constants
            .iter()
            .any(|c| matches!(c, 0x1800 | 0x0300 | 0x1B00))
    }
}

/// Read the code from `entries` and describe what it finds.
///
/// `entries` are only where the reading starts — where the listing is pointed
/// and where the machine is — and are not routines in themselves: the middle
/// of a loop is a perfectly ordinary place for the machine to be stopped, and
/// naming it would put a label on something that is not an entry point. Only
/// the addresses something calls or jumps to are named.
pub fn analyse<F: Fn(u16) -> u8>(peek: &F, entries: &[u16]) -> Doc {
    let mut doc = Doc::default();
    let routines = walk(peek, entries);

    for entry in &routines {
        // A call into the ROM is named from the table rather than guessed at.
        if let Some((_, label, comment)) = ROM_ROUTINES.iter().find(|(a, _, _)| a == entry) {
            doc.labels.insert(*entry, (*label).to_string());
            doc.comments.insert(*entry, (*comment).to_string());
            continue;
        }
        let features = read_routine(peek, *entry);
        let (label, comment) = describe(&features);
        doc.labels.insert(*entry, format!("{label}_{entry:04X}"));
        doc.comments.insert(*entry, comment);
    }

    // Lines worth a note of their own, wherever they turn up — including
    // along the code being looked at, which has no label of its own.
    for addr in routines.iter().chain(entries) {
        annotate_lines(peek, *addr, &mut doc);
    }
    doc
}

/// Follow the code from the entry points, collecting the addresses that get
/// called. Flow is followed through jumps so a routine reached only by a jump
/// table is still read.
fn walk<F: Fn(u16) -> u8>(peek: &F, entries: &[u16]) -> BTreeSet<u16> {
    let mut seen: BTreeSet<u16> = BTreeSet::new();
    let mut routines: BTreeSet<u16> = BTreeSet::new();
    let mut queue: Vec<u16> = entries.to_vec();
    let mut read = 0usize;

    while let Some(start) = queue.pop() {
        let mut addr = start;
        let mut steps = 0;
        while steps < MAX_ROUTINE && read < MAX_INSTRUCTIONS {
            if !seen.insert(addr) {
                break;
            }
            steps += 1;
            read += 1;
            let insn = disasm::disasm(peek, addr);
            let text = insn.text.as_str();

            if let Some(target) = call_target(text) {
                if routines.insert(target) {
                    queue.push(target);
                }
            }
            // A JP is how a routine hands over to another one — a tail call,
            // or a jump table — so its target is an entry point too. A JR is
            // not: two bytes of reach makes it a loop within a routine
            // nine times out of ten, and labelling those would bury the
            // entry points in noise.
            if let Some(target) = jump_target(text) {
                if text.starts_with("JP ") {
                    routines.insert(target);
                }
                if !seen.contains(&target) {
                    queue.push(target);
                }
            }
            // The end of a routine: a return, or a jump that never comes back.
            if text.starts_with("RET") && !text.contains(',') {
                break;
            }
            if text.starts_with("JP $") || text.starts_with("JR $") {
                break;
            }
            addr = addr.wrapping_add(insn.len.max(1) as u16);
        }
        if read >= MAX_INSTRUCTIONS {
            break;
        }
    }
    routines
}

/// Read one routine and total up what it does.
pub fn read_routine<F: Fn(u16) -> u8>(peek: &F, entry: u16) -> Features {
    let mut f = Features::default();
    let mut addr = entry;

    while f.length < MAX_ROUTINE {
        let insn = disasm::disasm(peek, addr);
        let text = insn.text.clone();
        f.length += 1;

        if let Some(target) = call_target(&text) {
            f.calls.insert(target);
        }
        if let Some(value) = loaded_constant(&text) {
            f.constants.push(value);
        }
        if let Some(port) = port_of(&text, "IN") {
            f.ports_in.insert(port);
        }
        if let Some(port) = port_of(&text, "OUT") {
            f.ports_out.insert(port);
        }
        match () {
            _ if text == "LDIR" || text == "LDI" => f.ldir = true,
            _ if text == "LDDR" || text == "LDD" => f.lddr = true,
            _ if text == "DAA" => f.daa = true,
            _ if text.starts_with("SRL")
                || text.starts_with("RR")
                || text.starts_with("RL")
                || text.starts_with("SLA") =>
            {
                f.shifts += 1
            }
            _ if text.starts_with("CP") || text.starts_with("SUB") => f.compares += 1,
            _ if text == "LD A,R" => f.reads_r = true,
            _ => {}
        }
        if text.starts_with("LD (HL),") || text.starts_with("LD (DE),") || text.contains("(IX+") {
            f.indirect_writes += 1;
        }
        if (text.starts_with("XOR (HL)") || text.starts_with("OR (HL)"))
            || (text.starts_with("LD (HL),A") && f.masked_writes > 0)
        {
            f.masked_writes += 1;
        }
        if text.starts_with("XOR (HL)") || text.starts_with("OR (HL)") {
            f.masked_writes += 1;
        }

        f.text.push(text.clone());

        if (text.starts_with("RET") && !text.contains(',')) || text.starts_with("JP $") {
            break;
        }
        addr = addr.wrapping_add(insn.len.max(1) as u16);
    }

    f.reads_rom = f.constants.iter().any(|c| *c < 0x4000 && *c >= 0x0100);
    f
}

/// The rules, in the order they are tried. The first that fits wins, so the
/// specific ones come before the general ones.
pub fn describe(f: &Features) -> (String, String) {
    // Tape, sound and input are named by the hardware they touch, which is the
    // firmest evidence there is.
    if f.calls_rom(0x0556) || f.calls_rom(0x07CB) || f.calls_rom(0x0808) {
        return (
            "load_from_tape".into(),
            "Loads from tape through the ROM's loader".into(),
        );
    }
    if f.calls_rom(0x04C2) {
        return ("save_to_tape".into(), "Saves to tape".into());
    }
    if f.ports_in.contains(&0x1F) {
        return (
            "read_joystick".into(),
            "Reads the Kempston joystick on port $1F".into(),
        );
    }
    if f.ports_out.iter().any(|p| *p == 0xFFFD || *p == 0xBFFD) {
        return ("play_sound".into(), "Writes to the AY sound chip".into());
    }
    if f.ports_out.contains(&0xFE) && f.text.iter().any(|t| t.contains("DJNZ")) {
        return (
            "play_sound".into(),
            "Toggles the beeper in a timed loop".into(),
        );
    }
    if f.calls_rom(0x03B5) || f.calls_rom(0x03F8) {
        return ("play_sound".into(), "Sounds a note through the ROM".into());
    }
    if f.ports_in.contains(&0xFE) || f.calls_rom(0x028E) || f.calls_rom(0x02BF) {
        // A joystick wired to the keyboard reads the same port; the rows tell
        // them apart, and only sometimes, so both are offered.
        return (
            "read_keys".into(),
            "Reads the keyboard (or a joystick wired to it) on port $FE".into(),
        );
    }

    // The screen: what is written, where, and how much of it.
    if f.calls_rom(0x0D6B) || f.calls_rom(0x0DAF) {
        return (
            "clear_screen".into(),
            "Clears the screen via the ROM".into(),
        );
    }
    // A fill and a copy both end in LDIR over the same number of bytes. What
    // tells them apart is the value written before it: a clear writes one
    // byte and lets LDIR smear it along, a copy has a source to read from.
    let smears_one_byte = f.text.iter().any(|t| t.starts_with("LD (HL),$"));
    if f.ldir && f.touches(&SCREEN) && f.screen_sized() && smears_one_byte {
        return (
            "clear_screen".into(),
            "Fills the display file with one value: a screen clear".into(),
        );
    }
    if f.ldir && f.touches(&SCREEN) && f.screen_sized() {
        return (
            "blit_screen".into(),
            "Copies a whole screen's worth of bytes into the display file: a back buffer being shown".into(),
        );
    }
    if f.ldir && f.touches(&SCREEN) {
        return (
            "copy_to_screen".into(),
            "Copies a block of bytes into the display file".into(),
        );
    }
    if f.touches(&ATTRS) && (f.ldir || f.indirect_writes > 0) {
        return (
            "set_colours".into(),
            "Writes to the attribute file: colouring the display".into(),
        );
    }
    if f.constants.contains(&FONT) || f.calls_rom(0x0010) || f.calls_rom(0x09F4) {
        return (
            "print_text".into(),
            "Prints text, using the ROM's character set".into(),
        );
    }
    if f.touches(&PANEL) && f.indirect_writes > 0 {
        return (
            "draw_panel".into(),
            "Writes to the bottom of the screen: a score panel or status line".into(),
        );
    }
    if f.masked_writes > 0 && f.touches(&SCREEN) {
        return (
            "draw_sprite".into(),
            "Merges bytes into the display file with XOR or OR: drawing a sprite".into(),
        );
    }
    if f.masked_writes > 0 {
        return (
            "draw_sprite".into(),
            "Merges bytes into memory with XOR or OR, as a sprite routine does".into(),
        );
    }

    // Data being unpacked: a control word being shifted a bit at a time, with
    // a copy loop for the back-references.
    if f.shifts > 3 && (f.ldir || f.lddr) {
        return (
            "decompress".into(),
            "Shifts a control word bit by bit and copies runs: unpacking compressed data".into(),
        );
    }
    if f.shifts > 3 && f.compares > 2 {
        return (
            "pack_data".into(),
            "Shifts bits and compares runs: packing or unpacking data".into(),
        );
    }

    // Guesses, plainly hedged.
    if f.reads_r || (f.reads_rom && f.compares > 2) {
        return (
            "maybe_protection".into(),
            "Reads the refresh register or the ROM's own bytes and compares them: possibly a protection check".into(),
        );
    }
    if f.daa {
        return (
            "update_score".into(),
            "Decimal arithmetic, which on this machine usually means a score".into(),
        );
    }
    if f.compares > 3 && f.indirect_writes > 0 && f.length < 80 {
        return (
            "maybe_collision".into(),
            "Compares several values and writes back: possibly collision detection".into(),
        );
    }
    if f.calls.len() > 3 {
        return (
            "game_logic".into(),
            "Calls several other routines in turn: the shape of a main loop or a game turn".into(),
        );
    }
    if f.ldir || f.lddr {
        return ("copy_block".into(), "Copies a block of memory".into());
    }
    ("routine".into(), String::new())
}

/// Notes against particular lines, where a single instruction says something
/// on its own.
fn annotate_lines<F: Fn(u16) -> u8>(peek: &F, entry: u16, doc: &mut Doc) {
    let mut addr = entry;
    for _ in 0..MAX_ROUTINE {
        let insn = disasm::disasm(peek, addr);
        let text = insn.text.as_str();

        let note = if let Some(target) = call_target(text) {
            ROM_ROUTINES
                .iter()
                .find(|(a, _, _)| *a == target)
                .map(|(_, _, comment)| (*comment).to_string())
        } else if text.contains("($1F)") {
            Some("Kempston joystick".into())
        } else if text.starts_with("OUT ($FE)") {
            Some("Border, beeper and MIC".into())
        } else if text.starts_with("IN A,($FE)")
            || text.contains("($FE),A") && text.starts_with("IN")
        {
            Some("Keyboard row, and the tape's EAR bit".into())
        } else if text.contains("($FFFD)") {
            Some("AY register select".into())
        } else if text.contains("($BFFD)") {
            Some("AY register data".into())
        } else if text.contains("($7FFD)") {
            Some("Memory paging".into())
        } else if text == "HALT" {
            Some("Waits for the frame interrupt".into())
        } else {
            None
        };

        if let Some(note) = note {
            doc.comments.entry(addr).or_insert(note);
        }

        if (text.starts_with("RET") && !text.contains(',')) || text.starts_with("JP $") {
            break;
        }
        addr = addr.wrapping_add(insn.len.max(1) as u16);
    }
}

/// The address a CALL or RST goes to, if it is a fixed one.
fn call_target(text: &str) -> Option<u16> {
    if let Some(rest) = text.strip_prefix("RST ") {
        return parse_hex(rest);
    }
    let rest = text.strip_prefix("CALL ")?;
    // CALL cc,nn as well as CALL nn.
    let target = rest.rsplit(',').next()?;
    parse_hex(target)
}

/// The address a jump goes to, if it is a fixed one.
fn jump_target(text: &str) -> Option<u16> {
    let rest = text
        .strip_prefix("JP ")
        .or_else(|| text.strip_prefix("JR "))?;
    parse_hex(rest.rsplit(',').next()?)
}

/// A sixteen-bit constant being loaded into a register pair.
fn loaded_constant(text: &str) -> Option<u16> {
    let rest = text.strip_prefix("LD ")?;
    let (target, value) = rest.split_once(',')?;
    let wide = matches!(target, "BC" | "DE" | "HL" | "SP" | "IX" | "IY");
    if !wide {
        return None;
    }
    parse_hex(value)
}

/// The port an IN or OUT uses, when it is written out rather than held in C.
fn port_of(text: &str, kind: &str) -> Option<u16> {
    if !text.starts_with(kind) {
        return None;
    }
    let open = text.find('(')?;
    let close = text[open..].find(')')? + open;
    parse_hex(&text[open + 1..close])
}

fn parse_hex(text: &str) -> Option<u16> {
    let text = text.trim();
    let digits = text.strip_prefix('$')?;
    u16::from_str_radix(digits, 16).ok()
}
