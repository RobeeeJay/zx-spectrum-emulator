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

/// Recognising a routine by its bytes rather than by what it looks like it
/// does.
///
/// The signatures are not a table written here: they are taken from the ROM
/// the machine is running, which is the one body of Spectrum code that is
/// known for certain. Games copy ROM routines into RAM all the time — the
/// print routine, the keyboard scan — and a copy has the same bytes wherever
/// it ends up. Anything else worth recognising can be added to the table by
/// whoever has the code in front of them to check it against.
pub struct Signatures {
    /// Fingerprint of the first bytes of each known routine, to its name.
    by_hash: BTreeMap<u64, (u16, &'static str, &'static str)>,
}

/// How many bytes of a routine are hashed. Enough to be distinctive, short
/// enough that a routine which has been relocated still matches: absolute
/// addresses inside it would differ, and this stays in front of most of them.
const SIGNATURE_LEN: u16 = 12;

impl Signatures {
    /// Build the table from a ROM image, if there is one to read.
    pub fn from_rom(rom: &[u8]) -> Signatures {
        let mut by_hash = BTreeMap::new();
        for (addr, label, comment) in ROM_ROUTINES {
            let at = *addr as usize;
            if at + SIGNATURE_LEN as usize > rom.len() {
                continue;
            }
            let hash = fingerprint(|i| rom[at + i as usize]);
            by_hash.insert(hash, (*addr, *label, *comment));
        }
        Signatures { by_hash }
    }

    /// Add signatures written out as bytes in a file.
    pub fn add_from_text(&mut self, text: &str) {
        let (_, signatures) = parse_symbols(text);
        for (bytes, name, comment) in signatures {
            if bytes.len() < SIGNATURE_LEN as usize {
                continue;
            }
            let hash = fingerprint(|i| bytes[i as usize]);
            // Leaked deliberately: the table lives as long as the analysis and
            // the alternative is threading a lifetime through every rule for
            // the sake of a few dozen strings.
            let name: &'static str = Box::leak(name.into_boxed_str());
            let comment: &'static str = Box::leak(comment.into_boxed_str());
            self.by_hash.insert(hash, (0, name, comment));
        }
    }

    pub fn empty() -> Signatures {
        Signatures {
            by_hash: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_hash.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }

    /// What the code at this address is, if it is something known.
    pub fn identify<F: Fn(u16) -> u8>(&self, peek: &F, at: u16) -> Option<(&'static str, String)> {
        let hash = fingerprint(|i| peek(at.wrapping_add(i)));
        let (from, label, comment) = self.by_hash.get(&hash)?;
        if at == *from || *from == 0 {
            return Some((label, (*comment).to_string()));
        }
        Some((
            label,
            format!("{comment} — the same code as ${from:04X}, copied here"),
        ))
    }
}

/// Read symbols and signatures out of a file, so anything known can be
/// supplied without changing the emulator.
///
/// Two kinds of line, both optional in any file:
///
/// ```text
/// # a symbol: an address, a name, and what it is
/// 0D6B rom_cls ; clears the screen
/// # a signature: bytes to match anywhere, a name, and what it is
/// bytes 21 00 40 11 01 40 01 FF 17 36 00 ED  zx7_unpack ; ZX7 decompressor
/// ```
///
/// This is where a full ROM disassembly goes: nobody can ship one here, but
/// anybody who has one can turn its symbol list into a file of the first kind
/// and get every ROM call named instead of the four dozen in the table below.
pub fn parse_symbols(text: &str) -> (Vec<Symbol>, Vec<Signature>) {
    let mut symbols = Vec::new();
    let mut signatures = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (body, comment) = match line.split_once(';') {
            Some((body, comment)) => (body.trim(), comment.trim().to_string()),
            None => (line, String::new()),
        };
        if let Some(rest) = body.strip_prefix("bytes ") {
            // Bytes up to the name, which is the first thing that is not a
            // pair of hex digits.
            let mut bytes = Vec::new();
            let mut name = String::new();
            for word in rest.split_whitespace() {
                match u8::from_str_radix(word, 16) {
                    Ok(byte) if word.len() == 2 && name.is_empty() => bytes.push(byte),
                    _ => {
                        name = word.to_string();
                        break;
                    }
                }
            }
            if !bytes.is_empty() && !name.is_empty() {
                signatures.push((bytes, name, comment));
            }
            continue;
        }
        let mut words = body.split_whitespace();
        let (Some(addr), Some(name)) = (words.next(), words.next()) else {
            continue;
        };
        if let Ok(addr) = u16::from_str_radix(addr.trim_start_matches('$'), 16) {
            symbols.push((addr, name.to_string(), comment));
        }
    }
    (symbols, signatures)
}

/// One name for one address, as read out of a file.
pub type Symbol = (u16, String, String);
/// One run of bytes to recognise, with its name.
pub type Signature = (Vec<u8>, String, String);

/// Names for addresses, supplied by whoever has the disassembly.
#[derive(Clone, Debug, Default)]
pub struct Symbols {
    by_address: BTreeMap<u16, (String, String)>,
}

impl Symbols {
    pub fn from_text(text: &str) -> Symbols {
        let (symbols, _) = parse_symbols(text);
        Symbols {
            by_address: symbols
                .into_iter()
                .map(|(addr, name, comment)| (addr, (name, comment)))
                .collect(),
        }
    }

    /// Read the file if it is there. Failure is not an error: most people will
    /// not have one.
    pub fn from_file(path: &std::path::Path) -> Symbols {
        std::fs::read_to_string(path)
            .map(|text| Symbols::from_text(&text))
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.by_address.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_address.is_empty()
    }

    pub fn get(&self, addr: u16) -> Option<(&str, &str)> {
        self.by_address
            .get(&addr)
            .map(|(name, comment)| (name.as_str(), comment.as_str()))
    }
}

/// A hash of the first bytes of a routine. Any stable hash would do; this one
/// is written out so the numbers do not change when the standard library's
/// hasher does, which would silently invalidate anything saved.
fn fingerprint(byte: impl Fn(u16) -> u8) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for i in 0..SIGNATURE_LEN {
        hash ^= byte(i) as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

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
    /// Which of port $FE's four jobs the code appears to be doing. The port
    /// is the border, the beeper, the MIC socket and the keyboard at once, so
    /// the port number says nothing on its own — what the code does with the
    /// byte is the whole of the evidence.
    ///
    /// Bit 6 is the EAR line, which only the tape uses; bit 4 is the speaker
    /// and bit 3 the MIC; the bottom five bits are the keyboard, read with a
    /// half-row mask in the high byte of the address.
    pub ear_bit: bool,
    pub speaker_bit: bool,
    pub key_rows: bool,
    /// Works out where on the screen to write, in one of the two ways the
    /// display file's layout forces on anybody who tries.
    pub next_scanline: bool,
    pub third_crossing: bool,
    pub attribute_address: bool,
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
    analyse_with(peek, entries, &Signatures::empty())
}

/// The same, with a table of known code to check against first.
pub fn analyse_with<F: Fn(u16) -> u8>(peek: &F, entries: &[u16], known: &Signatures) -> Doc {
    let mut doc = Doc::default();
    let routines = walk(peek, entries);

    for entry in &routines {
        // A call into the ROM is named from the table rather than guessed at.
        if let Some((_, label, comment)) = ROM_ROUTINES.iter().find(|(a, _, _)| a == entry) {
            doc.labels.insert(*entry, (*label).to_string());
            doc.comments.insert(*entry, (*comment).to_string());
            continue;
        }
        // Code that matches something known is named from that, which beats
        // any amount of reading: it is the same bytes.
        if let Some((label, comment)) = known.identify(peek, *entry) {
            doc.labels.insert(*entry, format!("{label}_{entry:04X}"));
            doc.comments.insert(*entry, comment);
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
            if ends_routine(text) {
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

        // Which of port $FE's jobs this is. Testing bit 6 is the tape and
        // nothing else: the border, the beeper and the keyboard have no use
        // for the EAR line.
        if text.contains("$40") && (text.starts_with("AND") || text.starts_with("XOR"))
            || text.starts_with("BIT 6,")
        {
            f.ear_bit = true;
        }
        if text.contains("$10") && (text.starts_with("XOR") || text.starts_with("OR ")) {
            f.speaker_bit = true;
        }
        // A keyboard read puts a half-row mask in the high byte of the port
        // address: one bit low out of the top eight.
        if let Some(value) = loaded_constant(&text) {
            let (high, low) = ((value >> 8) as u8, value as u8);
            if low == 0xFE && matches!(high.count_zeros(), 1) {
                f.key_rows = true;
            }
        }

        // The display file's layout is peculiar enough that the arithmetic for
        // getting about it is unmistakable, and is the firmest evidence there
        // is that a routine draws.
        //
        // Down one pixel row is INC H, because the low three bits of H are the
        // row within the character. That overflows every eight rows, and the
        // fix-up — ADD A,$20 on L, and the H correction — is the second half
        // of the idiom. The attribute address for the same place is worked out
        // by folding H down and adding $58.
        if text == "INC H" || text == "DEC H" {
            f.next_scanline = true;
        }
        if text.starts_with("AND $07")
            || text.starts_with("AND $18")
            || text == "ADD A,$20"
            || text == "SUB $20"
        {
            f.third_crossing = true;
        }
        if text.contains("$58") && (text.starts_with("ADD") || text.starts_with("OR ")) {
            f.attribute_address = true;
        }

        f.text.push(text.clone());

        if ends_routine(&text) || text.starts_with("JP $") {
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
    // Port $FE, four ways. Bit 6 is the tape and only the tape.
    if f.ear_bit && (f.ports_in.contains(&0xFE) || f.ports_out.contains(&0xFE)) {
        return (
            "load_from_tape".into(),
            "Reads the EAR line on port $FE: listening to the tape".into(),
        );
    }
    if f.key_rows && f.ports_in.contains(&0xFE) {
        return (
            "read_keys".into(),
            "Reads port $FE with a half-row mask: the keyboard, or a joystick \
             wired to one of its rows"
                .into(),
        );
    }
    if f.speaker_bit && f.ports_out.contains(&0xFE) {
        return (
            "play_sound".into(),
            "Toggles bit 4 of port $FE: the beeper".into(),
        );
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

    if f.calls_rom(0x03B5) || f.calls_rom(0x03F8) {
        return ("play_sound".into(), "Sounds a note through the ROM".into());
    }
    if f.calls_rom(0x028E) || f.calls_rom(0x02BF) {
        return (
            "read_keys".into(),
            "Reads the keyboard through the ROM".into(),
        );
    }
    if f.ports_in.contains(&0xFE) {
        // Nothing about what was done with the byte, so nothing about which of
        // the port's four jobs this was.
        return (
            "reads_port_fe".into(),
            "Reads port $FE — the keyboard, the tape or a joystick wired to \
             one of them; there is nothing here to say which"
                .into(),
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

    // Screen-address arithmetic: a routine that walks the display file the way
    // the display file has to be walked is drawing on it, whatever else it
    // does, and this holds even when the addresses are worked out rather than
    // loaded as constants — which is most of the time.
    if f.next_scanline && f.third_crossing && f.indirect_writes > 0 {
        return (
            "draw_to_screen".into(),
            "Steps down the display file a pixel row at a time, with the fix-up \
             for crossing a character boundary: drawing"
                .into(),
        );
    }
    if f.attribute_address && f.indirect_writes > 0 {
        return (
            "set_colours".into(),
            "Works out an attribute address from a screen one: colouring what \
             something has drawn"
                .into(),
        );
    }
    if f.next_scanline && f.indirect_writes > 0 && f.touches(&SCREEN) {
        return (
            "draw_to_screen".into(),
            "Walks down the display file a pixel row at a time".into(),
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

/// What a routine was measured doing, which beats anything read off its
/// instructions. Returns nothing when the measurements say nothing much, so
/// the static rules still get their turn.
///
/// The numbers are quoted in the comment rather than summarised away: "writes
/// 6144 bytes to the display file, once a frame" is a fact the user can check,
/// where "screen blit" is only a claim.
/// When in the frame a routine runs, said in terms of the picture.
///
/// The frame is border, then picture, then border, and where a routine sits in
/// that says what kind of thing it is: work done above the picture is getting
/// ready for the frame, work done while the beam is on the picture is timed
/// against it, and the rest is thinking.
pub fn beam_phase(
    seen: &crate::observe::Observed,
    first_pixel_t: u32,
    frame_t: u32,
) -> &'static str {
    if seen.calls == 0 || seen.entered_at.high == 0 {
        return "";
    }
    let (low, high) = (seen.entered_at.low as u32, seen.entered_at.high as u32);
    // 192 lines of 224 T-states is the picture on a 48K; near enough on the
    // others for the purpose of saying which third of the frame this is.
    let picture_ends = first_pixel_t + 192 * 224;
    if high < first_pixel_t {
        return ", always before the picture is painted";
    }
    if low >= first_pixel_t && high < picture_ends {
        return ", always while the beam is on the picture";
    }
    if low >= picture_ends && high < frame_t {
        return ", always after the picture";
    }
    ""
}

/// What a routine appears to be handed, from what its registers held on the
/// way in across every call.
pub fn arguments(seen: &crate::observe::Observed) -> String {
    let mut said = Vec::new();
    let hl = seen.entry_hl;
    if !hl.constant() {
        if hl.low >= 0x4000 && hl.high < 0x5B00 {
            said.push("HL is a screen address".to_string());
        } else if hl.high.wrapping_sub(hl.low) > 8 {
            said.push(format!("HL varies ${:04X}..${:04X}", hl.low, hl.high));
        }
    } else if seen.calls > 2 {
        said.push(format!("HL is always ${:04X}", hl.low));
    }
    let bc = seen.entry_bc;
    if !bc.constant() && (bc.high >> 8) <= 23 && (bc.low & 0xFF) <= 31 && seen.calls > 2 {
        said.push("BC looks like character coordinates".to_string());
    }
    if said.is_empty() {
        String::new()
    } else {
        format!(" ({})", said.join("; "))
    }
}

pub fn describe_measured(seen: &crate::observe::Observed, frames: u32) -> Option<(String, String)> {
    let calls = seen.calls.max(1);
    let per_call = |n: u32| n / calls;
    let often = seen.every_frame(frames);
    let rhythm = if often {
        ", every frame"
    } else if seen.calls > 1 {
        ""
    } else {
        ", once"
    };

    // What it mostly does comes first. A routine that touched a port and also
    // wrote a hundred thousand bytes into the display file is drawing; the
    // port rules below are for routines whose work *is* the port. Manic
    // Miner's main loop reads the keys and draws the whole screen, and was
    // being called a keyboard routine on the strength of two IN instructions.
    let mostly_draws = per_call(seen.writes.screen + seen.writes.attrs) > 64;

    // Input and sound are named by the ports they touched, which is not a
    // guess at all.
    if !mostly_draws && seen.ports_in.contains(&0x1F) {
        return Some((
            "read_joystick".into(),
            format!("Reads the Kempston joystick on port $1F{rhythm}"),
        ));
    }
    if !mostly_draws && seen.ports_in.iter().any(|p| p & 0x00FF == 0xFE) {
        return Some((
            "read_keys".into(),
            format!("Reads the keyboard on port $FE{rhythm}"),
        ));
    }
    if seen.ports_out.iter().any(|p| *p == 0xFFFD || *p == 0xBFFD) {
        return Some((
            "play_sound".into(),
            format!("Writes to the AY sound chip{rhythm}"),
        ));
    }
    // The screen, by how much of it was written and where.
    let screen = per_call(seen.writes.screen);
    let attrs = per_call(seen.writes.attrs);

    // Port $FE is the border, the beeper and the MIC socket at once, so the
    // port alone proves nothing. A beeper routine hammers it and writes almost
    // nothing to memory; a routine that sets the border while colouring the
    // screen does the opposite.
    let hammers_fe = seen.ports_out.iter().any(|p| p & 0x00FF == 0xFE)
        && per_call(seen.port_writes) > 30
        && per_call(seen.writes.total()) < 16;
    if hammers_fe {
        return Some((
            "play_sound".into(),
            format!(
                "Writes to port $FE {} times a call and barely touches memory: the beeper{rhythm}",
                per_call(seen.port_writes)
            ),
        ));
    }
    if screen >= 6000 {
        return Some((
            "blit_screen".into(),
            format!("Writes {screen} bytes into the display file per call{rhythm}: a whole screen"),
        ));
    }
    if attrs >= 700 {
        return Some((
            "colour_screen".into(),
            format!("Writes {attrs} bytes into the attribute file per call{rhythm}"),
        ));
    }
    if screen > 0 && attrs > 0 {
        return Some((
            "draw_with_colour".into(),
            format!("Writes {screen} bytes of pixels and {attrs} of attributes per call{rhythm}"),
        ));
    }
    if screen > 0 {
        let rows = seen.longest_loop();
        let shape = match rows {
            7..=9 => " — eight rows, so a character or an eight-pixel sprite",
            15..=17 => " — sixteen rows",
            21..=24 => " — a character row across the screen",
            30..=33 => " — thirty-two across, a full row of cells",
            190..=193 => " — one pass down every pixel row of the screen",
            _ => "",
        };
        return Some((
            "draw_to_screen".into(),
            format!("Writes {screen} bytes into the display file per call{rhythm}{shape}"),
        ));
    }
    if attrs > 0 {
        return Some((
            "set_colours".into(),
            format!("Writes {attrs} attribute bytes per call{rhythm}"),
        ));
    }

    // Something that only ever pokes a byte or two in the same place is a
    // variable being kept, which is worth saying even without knowing which.
    // Something that only ever pokes the same few addresses is keeping them,
    // and naming the addresses is the useful part: the same ones turn up in
    // whatever else reads or writes them.
    if !seen.hot.is_empty() && seen.hot.len() <= 4 && seen.writes.other > 0 && often {
        let mut hot = seen.hot.clone();
        hot.sort_unstable();
        let list: Vec<String> = hot.iter().map(|a| format!("${a:04X}")).collect();
        return Some((
            "update_variable".into(),
            format!(
                "Writes only to {}{rhythm}: keeping a value",
                list.join(", ")
            ),
        ));
    }

    if often && seen.writes.other > 200 {
        return Some((
            "game_state".into(),
            format!(
                "Writes {} bytes a call outside the screen{rhythm}",
                per_call(seen.writes.other)
            ),
        ));
    }
    None
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

        if ends_routine(text) || text.starts_with("JP $") {
            break;
        }
        addr = addr.wrapping_add(insn.len.max(1) as u16);
    }
}

/// Whether this instruction is the end of a routine.
///
/// Only an unconditional return is. `RET Z` is a guard clause — the routine
/// carries on underneath it — and treating it as the end meant reading four
/// instructions of anything that begins by checking something and giving up.
/// The test used to be "starts with RET and has no comma in it", which is true
/// of every conditional return there is.
pub fn ends_routine(text: &str) -> bool {
    matches!(text, "RET" | "RETI" | "RETN")
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
