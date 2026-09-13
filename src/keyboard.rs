//! The machine's own keyboard, as something to look at and press.
//!
//! Forty keys wired as eight rows of five, which is the same matrix on every
//! Spectrum and on the ZX81; what differs between the two is what is printed
//! on them. The legends are here rather than in the window because they are
//! the machine rather than the drawing of it, and because a list of forty keys
//! is worth having a test over.

use std::time::{Duration, Instant};

/// A key: where it is in the matrix, and what is written on it.
///
/// `press` is a list because a key can be more than one: the machines with
/// typewriter keyboards send CAPS SHIFT with something else for their arrows
/// and their DELETE, and a picture of a keyboard should press what the key
/// presses.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    /// The big legend: the letter, the digit, or the name of the key.
    pub main: &'static str,
    /// The BASIC keyword the key types on its own, printed on the key face.
    pub word: &'static str,
    /// What SYMBOL SHIFT — the ZX81's SHIFT — gives.
    pub sym: &'static str,
    /// The word above the key: extended mode on a Spectrum, the function mode
    /// on a ZX81, and the CAPS SHIFT job on the digits.
    pub over: &'static str,
    /// The word below the key, in red on the case: extended mode with either
    /// shift. This is where CAT, FORMAT, INVERSE and the rest live, and a
    /// keyboard without them cannot be used to find them.
    pub under: &'static str,
    /// The matrix positions it pulls down, as (row, bit).
    pub press: &'static [(usize, u8)],
}

/// How the keys sit on the case: ten across and four down, in that order.
pub const ACROSS: usize = 10;
pub const DOWN: usize = 4;

/// The cursor keys and the exponent are written out rather than drawn as
/// arrows: egui's own fonts have no arrow glyphs, and a key with an empty box
/// on it says less than one with a word.
///
/// A key is wider than it is tall, as the rubber ones were.
pub const KEY_ASPECT: f32 = 1.35;

const fn key(
    main: &'static str,
    word: &'static str,
    sym: &'static str,
    over: &'static str,
    under: &'static str,
    press: &'static [(usize, u8)],
) -> Key {
    Key {
        main,
        word,
        sym,
        over,
        under,
        press,
    }
}

/// The 48K's keyboard, and the 128K's: the same forty keys with the same
/// words on them.
#[rustfmt::skip]
pub const SPECTRUM: [Key; 40] = [
    key("1", "", "!", "EDIT",      "DEF FN", &[(3, 0)]),
    key("2", "", "@", "CAPS LOCK", "FN", &[(3, 1)]),
    key("3", "", "#", "TRUE VID",  "LINE", &[(3, 2)]),
    key("4", "", "$", "INV VID",   "OPEN #", &[(3, 3)]),
    key("5", "", "%", "LEFT",  "CLOSE #", &[(3, 4)]),
    key("6", "", "&", "DOWN",  "MOVE", &[(4, 4)]),
    key("7", "", "'", "UP",  "ERASE", &[(4, 3)]),
    key("8", "", "(", "RIGHT",  "POINT", &[(4, 2)]),
    key("9", "", ")", "GRAPHICS",  "CAT", &[(4, 1)]),
    key("0", "", "_", "DELETE",    "FORMAT", &[(4, 0)]),

    key("Q", "PLOT",   "<=", "SIN",  "ASN", &[(2, 0)]),
    key("W", "DRAW",   "<>", "COS",  "ACS", &[(2, 1)]),
    key("E", "REM",    ">=", "TAN",  "ATN", &[(2, 2)]),
    key("R", "RUN",    "<",  "INT",  "VERIFY", &[(2, 3)]),
    key("T", "RAND",   ">",  "RND",  "MERGE", &[(2, 4)]),
    key("Y", "RETURN", "AND", "STR$", "[", &[(5, 4)]),
    key("U", "IF",     "OR", "CHR$", "]", &[(5, 3)]),
    key("I", "INPUT",  "AT", "CODE", "IN", &[(5, 2)]),
    key("O", "POKE",   ";",  "PEEK", "OUT", &[(5, 1)]),
    key("P", "PRINT",  "\"", "TAB",  "\u{00a9}", &[(5, 0)]),

    key("A", "NEW",   "STOP", "READ",    "~", &[(1, 0)]),
    key("S", "SAVE",  "NOT",  "RESTORE", "|", &[(1, 1)]),
    key("D", "DIM",   "STEP", "DATA",    "\\", &[(1, 2)]),
    key("F", "FOR",   "TO",   "SGN",     "{", &[(1, 3)]),
    key("G", "GOTO",  "THEN", "ABS",     "}", &[(1, 4)]),
    key("H", "GOSUB", "^", "SQR", "CIRCLE", &[(6, 4)]),
    key("J", "LOAD",  "\u{2212}", "VAL", "VAL$", &[(6, 3)]),
    key("K", "LIST",  "+",    "LEN",     "SCREEN$", &[(6, 2)]),
    key("L", "LET",   "=",    "USR",     "ATTR", &[(6, 1)]),
    key("ENTER", "",  "",     "",        "", &[(6, 0)]),

    key("CAPS SHIFT", "", "",     "",        "", &[(0, 0)]),
    key("Z", "COPY",   ":",  "LN",      "BEEP", &[(0, 1)]),
    key("X", "CLEAR",  "\u{00a3}", "EXP", "INK", &[(0, 2)]),
    key("C", "CONT",   "?",  "LPRINT",  "PAPER", &[(0, 3)]),
    key("V", "CLS",    "/",  "LLIST",   "FLASH", &[(0, 4)]),
    key("B", "BORDER", "*",  "BIN",     "BRIGHT", &[(7, 4)]),
    key("N", "NEXT",   ",",  "INKEY$",  "OVER", &[(7, 3)]),
    key("M", "PAUSE",  ".",  "PI",      "INVERSE", &[(7, 2)]),
    key("SYMBOL SHIFT", "", "",  "",    "", &[(7, 1)]),
    key("SPACE", "", "",  "BREAK",      "", &[(7, 0)]),
];

/// The ZX81's, whose matrix is wired the same way and whose keys say something
/// else. It has nothing under its keys: the fourth legend the Spectrum grew
/// did not exist yet. Taken from the keyboard assignment table in
/// <https://problemkaputt.de/zxdocs.htm>: the "Normal" column is the keyword,
/// "Command" is what SHIFT gives, and "Function" is the word above the key.
#[rustfmt::skip]
pub const ZX81: [Key; 40] = [
    key("1", "", "EDIT",     "", "", &[(3, 0)]),
    key("2", "", "AND",      "", "", &[(3, 1)]),
    key("3", "", "THEN",     "", "", &[(3, 2)]),
    key("4", "", "TO",       "", "", &[(3, 3)]),
    key("5", "", "LEFT", "", "", &[(3, 4)]),
    key("6", "", "DOWN", "", "", &[(4, 4)]),
    key("7", "", "UP", "", "", &[(4, 3)]),
    key("8", "", "RIGHT", "", "", &[(4, 2)]),
    key("9", "", "GRAPHICS", "", "", &[(4, 1)]),
    key("0", "", "RUBOUT",   "", "", &[(4, 0)]),

    key("Q", "PLOT",   "\"\"", "SIN",  "", &[(2, 0)]),
    key("W", "UNPLOT", "OR",   "COS",  "", &[(2, 1)]),
    key("E", "REM",    "STEP", "TAN",  "", &[(2, 2)]),
    key("R", "RUN",    "<=",   "INT",  "", &[(2, 3)]),
    key("T", "RAND",   "<>",   "RND",  "", &[(2, 4)]),
    key("Y", "RETURN", ">=",   "STR$", "", &[(5, 4)]),
    key("U", "IF",     "$",    "CHR$", "", &[(5, 3)]),
    key("I", "INPUT",  "(",    "CODE", "", &[(5, 2)]),
    key("O", "POKE",   ")",    "PEEK", "", &[(5, 1)]),
    key("P", "PRINT",  "\"",   "TAB",  "", &[(5, 0)]),

    key("A", "NEW",   "STOP",   "ARCSIN", "", &[(1, 0)]),
    key("S", "SAVE",  "LPRINT", "ARCCOS", "", &[(1, 1)]),
    key("D", "DIM",   "SLOW",   "ARCTAN", "", &[(1, 2)]),
    key("F", "FOR",   "FAST",   "SGN",    "", &[(1, 3)]),
    key("G", "GOTO",  "LLIST",  "ABS",    "", &[(1, 4)]),
    key("H", "GOSUB", "**",     "SQR",    "", &[(6, 4)]),
    key("J", "LOAD",  "\u{2212}", "VAL",  "", &[(6, 3)]),
    key("K", "LIST",  "+",      "LEN",    "", &[(6, 2)]),
    key("L", "LET",   "=",      "USR",    "", &[(6, 1)]),
    key("NEWLINE", "", "",      "",       "", &[(6, 0)]),

    key("SHIFT", "",   "",  "",       "", &[(0, 0)]),
    key("Z", "COPY",   ":", "LN",     "", &[(0, 1)]),
    key("X", "CLEAR",  ";", "EXP",    "", &[(0, 2)]),
    key("C", "CONT",   "?", "AT",     "", &[(0, 3)]),
    key("V", "CLS",    "/", "",       "", &[(0, 4)]),
    key("B", "SCROLL", "*", "INKEY$", "", &[(7, 4)]),
    key("N", "NEXT",   "<", "NOT",    "", &[(7, 3)]),
    key("M", "PAUSE",  ">", "PI",     "", &[(7, 2)]),
    key(".", "",       ",", "",       "", &[(7, 1)]),
    key("SPACE", "",   "\u{00a3}", "BREAK", "", &[(7, 0)]),
];

/// The keys of the machine in use.
pub fn layout(zx81: bool) -> &'static [Key; 40] {
    if zx81 {
        &ZX81
    } else {
        &SPECTRUM
    }
}

/// How long a key stays down, and lit, however briefly it was pressed.
///
/// The ROM reads the keyboard once a frame and wants to see a key on two scans
/// running before it believes in it, so a click that lasted one host frame
/// would be typing into nothing. A tenth of a second is five scans, and is
/// also long enough for the eye — a key that flashed for one frame is a key
/// that did not visibly flash at all.
pub const MIN_PRESS: Duration = Duration::from_millis(100);

/// Which keys are down and which are lit.
///
/// The two are not the same. Anything the machine sees pressed is lit —
/// including the host keyboard, which is the point of the window: press a key
/// on the desk and the equivalent key on the picture lights. Only the keys the
/// window itself is pressing are held.
#[derive(Clone, Debug, Default)]
pub struct Keys {
    lit_until: [[Option<Instant>; 5]; 8],
    held_until: [[Option<Instant>; 5]; 8],
    /// Shifts stay down until the next key is pressed, so a shifted key can be
    /// typed with one pointer.
    latched: Vec<(usize, u8)>,
}

impl Keys {
    /// The machine saw this key down. It lights for at least `MIN_PRESS`.
    pub fn lit(&mut self, row: usize, bit: u8, now: Instant) {
        self.lit_until[row][bit as usize] = Some(now + MIN_PRESS);
    }

    /// The window is pressing this key: held as well as lit.
    pub fn press(&mut self, row: usize, bit: u8, now: Instant) {
        self.held_until[row][bit as usize] = Some(now + MIN_PRESS);
        self.lit(row, bit, now);
    }

    /// Whether the key should be drawn lit.
    pub fn is_lit(&self, row: usize, bit: u8, now: Instant) -> bool {
        matches!(self.lit_until[row][bit as usize], Some(until) if until > now)
    }

    /// Whether the machine should see the key down.
    pub fn is_held(&self, row: usize, bit: u8, now: Instant) -> bool {
        matches!(self.held_until[row][bit as usize], Some(until) if until > now)
            || self.latched.contains(&(row, bit))
    }

    /// A shift held for the next key, or let go of again.
    pub fn latch(&mut self, row: usize, bit: u8) {
        match self.latched.iter().position(|k| *k == (row, bit)) {
            Some(at) => {
                self.latched.remove(at);
            }
            None => self.latched.push((row, bit)),
        }
    }

    pub fn latched(&self, row: usize, bit: u8) -> bool {
        self.latched.contains(&(row, bit))
    }

    /// Let go of everything: nothing held, nothing latched, nothing lit.
    ///
    /// What a reset does to the keyboard. A shift clicked in the window and
    /// never followed by a key stays down until something takes it, and it
    /// used to survive a reset — so the machine came up with CAPS SHIFT held
    /// and answered every key with the shifted one, which reads as a machine
    /// that is ignoring the keyboard.
    pub fn release_all(&mut self) {
        self.held_until = [[None; 5]; 8];
        self.lit_until = [[None; 5]; 8];
        self.latched.clear();
    }

    /// Press the latched shifts along with the key they were held for, and
    /// let go of the latch. Pressing them rather than simply dropping them is
    /// what puts the shift and the key in front of the machine together: a
    /// press lasts `MIN_PRESS`, and a latch let go at the moment the key goes
    /// down would be gone before the machine looked.
    pub fn take_latched(&mut self, now: Instant) {
        for (row, bit) in std::mem::take(&mut self.latched) {
            self.press(row, bit, now);
        }
    }

    /// The matrix the window is holding down, ready to be merged with the host
    /// keyboard's.
    pub fn matrix(&self, now: Instant) -> [u8; 8] {
        let mut matrix = [0xffu8; 8];
        for (row, keys) in matrix.iter_mut().enumerate() {
            for bit in 0..5u8 {
                if self.is_held(row, bit, now) {
                    *keys &= !(1 << bit);
                }
            }
        }
        matrix
    }

    /// Whether anything is lit, which is when the window needs repainting
    /// without anybody touching it.
    pub fn anything_lit(&self, now: Instant) -> bool {
        (0..8).any(|row| (0..5).any(|bit| self.is_lit(row, bit, now)))
    }
}

/// Where each key is drawn inside `area`, in the order of the layout.
///
/// The grid keeps its shape rather than filling the window: forty keys drawn
/// to whatever aspect the window happens to have would not look like the
/// machine, and the point of the picture is that it does.
pub fn key_rects(area: egui::Rect, gap: f32) -> Vec<egui::Rect> {
    key_rects_with(area, gap, gap)
}

/// The same, with room left under each row for the word printed there.
pub fn key_rects_with(area: egui::Rect, gap: f32, row_gap: f32) -> Vec<egui::Rect> {
    let across = ACROSS as f32;
    let down = DOWN as f32;
    let by_width = (area.width() - gap * (across - 1.0)) / across;
    let by_height = ((area.height() - row_gap * (down - 1.0)) / down) * KEY_ASPECT;
    let key_w = by_width.min(by_height).max(1.0);
    let key_h = key_w / KEY_ASPECT;
    let size = egui::vec2(
        key_w * across + gap * (across - 1.0),
        key_h * down + row_gap * (down - 1.0),
    );
    let origin = egui::pos2(
        area.center().x - size.x / 2.0,
        area.center().y - size.y / 2.0,
    );
    (0..ACROSS * DOWN)
        .map(|i| {
            let (col, row) = (i % ACROSS, i / ACROSS);
            egui::Rect::from_min_size(
                origin + egui::vec2(col as f32 * (key_w + gap), row as f32 * (key_h + row_gap)),
                egui::vec2(key_w, key_h),
            )
        })
        .collect()
}

/// How a legend is reached from the keyboard: which shifts, and in what order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// On its own: the letter, or the keyword at the start of a statement.
    Plain,
    /// With CAPS SHIFT held: the jobs over the digits, and BREAK.
    Caps,
    /// With SYMBOL SHIFT held — the ZX81's SHIFT.
    Symbol,
    /// Extended mode — CAPS SHIFT and SYMBOL SHIFT together, then let go — and
    /// then the key: the green word above it.
    Extended,
    /// Extended mode, then SYMBOL SHIFT with the key: the red word below it.
    ExtendedSymbol,
    /// The ZX81's function mode: SHIFT with NEWLINE, then the key.
    Function,
}

/// A legend that matched, on which key, and how it is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where the key is in the layout.
    pub key: usize,
    pub legend: &'static str,
    pub reach: Reach,
}

fn position(keys: &[Key; 40], main: &str) -> usize {
    keys.iter()
        .position(|k| k.main == main)
        .expect("every layout has its shift keys")
}

impl Found {
    /// The keys other than this one that have to go down to reach it.
    pub fn shifts(&self, zx81: bool) -> Vec<usize> {
        let keys = layout(zx81);
        let (caps, symbol) = if zx81 {
            (position(keys, "SHIFT"), position(keys, "SHIFT"))
        } else {
            (position(keys, "CAPS SHIFT"), position(keys, "SYMBOL SHIFT"))
        };
        match self.reach {
            Reach::Plain => vec![],
            Reach::Caps => vec![caps],
            Reach::Symbol => vec![symbol],
            Reach::Extended | Reach::ExtendedSymbol => vec![caps, symbol],
            Reach::Function => vec![position(keys, "SHIFT"), position(keys, "NEWLINE")],
        }
    }

    /// How to type it, in a line.
    pub fn how(&self, zx81: bool) -> String {
        let key = layout(zx81)[self.key].main;
        let shift = if zx81 { "SHIFT" } else { "SYMBOL SHIFT" };
        let how = match self.reach {
            Reach::Plain if self.legend == key => format!("the {key} key"),
            Reach::Plain => format!("{key}, at the start of a statement"),
            Reach::Caps => format!("CAPS SHIFT with {key}"),
            Reach::Symbol => format!("{shift} with {key}"),
            Reach::Extended => format!("extended mode (CAPS SHIFT with SYMBOL SHIFT), then {key}"),
            Reach::ExtendedSymbol => format!("extended mode, then SYMBOL SHIFT with {key}"),
            Reach::Function => format!("function mode (SHIFT with NEWLINE), then {key}"),
        };
        format!("{}: {how}", self.legend)
    }
}

/// Find a word on the keys, in any case: every legend that contains it.
///
/// Several can be looked for at once, split by commas. A phrase that is on no
/// key is tried a word at a time instead, so "print cat" finds both while
/// "DEF FN" finds the one legend with a space in it.
pub fn search(zx81: bool, text: &str) -> Vec<Found> {
    let mut found = Vec::new();
    for term in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let whole = matches(zx81, term);
        if whole.is_empty() {
            for word in term.split_whitespace() {
                found.extend(matches(zx81, word));
            }
        } else {
            found.extend(whole);
        }
    }
    found.dedup();
    found
}

fn matches(zx81: bool, term: &str) -> Vec<Found> {
    let term = term.to_uppercase();
    let mut found = Vec::new();
    for (i, key) in layout(zx81).iter().enumerate() {
        let digit_or_space =
            key.main.len() == 1 && key.main.as_bytes()[0].is_ascii_digit() || key.main == "SPACE";
        let over = match (zx81, key.main) {
            (true, "SPACE") => Reach::Plain,
            (true, _) => Reach::Function,
            (false, _) if digit_or_space => Reach::Caps,
            (false, _) => Reach::Extended,
        };
        for (legend, reach) in [
            (key.main, Reach::Plain),
            (key.word, Reach::Plain),
            (key.sym, Reach::Symbol),
            (key.over, over),
            (key.under, Reach::ExtendedSymbol),
        ] {
            if !legend.is_empty() && legend.to_uppercase().contains(&term) {
                found.push(Found {
                    key: i,
                    legend,
                    reach,
                });
            }
        }
    }
    found
}
