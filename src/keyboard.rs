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
    press: &'static [(usize, u8)],
) -> Key {
    Key {
        main,
        word,
        sym,
        over,
        press,
    }
}

/// The 48K's keyboard, and the 128K's: the same forty keys with the same
/// words on them.
#[rustfmt::skip]
pub const SPECTRUM: [Key; 40] = [
    key("1", "", "!", "EDIT",      &[(3, 0)]),
    key("2", "", "@", "CAPS LOCK", &[(3, 1)]),
    key("3", "", "#", "TRUE VID",  &[(3, 2)]),
    key("4", "", "$", "INV VID",   &[(3, 3)]),
    key("5", "", "%", "LEFT",  &[(3, 4)]),
    key("6", "", "&", "DOWN",  &[(4, 4)]),
    key("7", "", "'", "UP",  &[(4, 3)]),
    key("8", "", "(", "RIGHT",  &[(4, 2)]),
    key("9", "", ")", "GRAPHICS",  &[(4, 1)]),
    key("0", "", "_", "DELETE",    &[(4, 0)]),

    key("Q", "PLOT",   "<=", "SIN",  &[(2, 0)]),
    key("W", "DRAW",   "<>", "COS",  &[(2, 1)]),
    key("E", "REM",    ">=", "TAN",  &[(2, 2)]),
    key("R", "RUN",    "<",  "INT",  &[(2, 3)]),
    key("T", "RAND",   ">",  "RND",  &[(2, 4)]),
    key("Y", "RETURN", "AND", "STR$", &[(5, 4)]),
    key("U", "IF",     "OR", "CHR$", &[(5, 3)]),
    key("I", "INPUT",  "AT", "CODE", &[(5, 2)]),
    key("O", "POKE",   ";",  "PEEK", &[(5, 1)]),
    key("P", "PRINT",  "\"", "TAB",  &[(5, 0)]),

    key("A", "NEW",   "STOP", "READ",    &[(1, 0)]),
    key("S", "SAVE",  "NOT",  "RESTORE", &[(1, 1)]),
    key("D", "DIM",   "STEP", "DATA",    &[(1, 2)]),
    key("F", "FOR",   "TO",   "SGN",     &[(1, 3)]),
    key("G", "GOTO",  "THEN", "ABS",     &[(1, 4)]),
    key("H", "GOSUB", "^", "SQR", &[(6, 4)]),
    key("J", "LOAD",  "\u{2212}", "VAL", &[(6, 3)]),
    key("K", "LIST",  "+",    "LEN",     &[(6, 2)]),
    key("L", "LET",   "=",    "USR",     &[(6, 1)]),
    key("ENTER", "",  "",     "",        &[(6, 0)]),

    key("CAPS SHIFT", "", "",     "",        &[(0, 0)]),
    key("Z", "COPY",   ":",  "LN",      &[(0, 1)]),
    key("X", "CLEAR",  "\u{00a3}", "EXP", &[(0, 2)]),
    key("C", "CONT",   "?",  "LPRINT",  &[(0, 3)]),
    key("V", "CLS",    "/",  "LLIST",   &[(0, 4)]),
    key("B", "BORDER", "*",  "BIN",     &[(7, 4)]),
    key("N", "NEXT",   ",",  "INKEY$",  &[(7, 3)]),
    key("M", "PAUSE",  ".",  "PI",      &[(7, 2)]),
    key("SYMBOL SHIFT", "", "",  "",    &[(7, 1)]),
    key("SPACE", "", "",  "BREAK",      &[(7, 0)]),
];

/// The ZX81's, whose matrix is wired the same way and whose keys say something
/// else. Taken from the keyboard assignment table in
/// <https://problemkaputt.de/zxdocs.htm>: the "Normal" column is the keyword,
/// "Command" is what SHIFT gives, and "Function" is the word above the key.
#[rustfmt::skip]
pub const ZX81: [Key; 40] = [
    key("1", "", "EDIT",     "", &[(3, 0)]),
    key("2", "", "AND",      "", &[(3, 1)]),
    key("3", "", "THEN",     "", &[(3, 2)]),
    key("4", "", "TO",       "", &[(3, 3)]),
    key("5", "", "LEFT", "", &[(3, 4)]),
    key("6", "", "DOWN", "", &[(4, 4)]),
    key("7", "", "UP", "", &[(4, 3)]),
    key("8", "", "RIGHT", "", &[(4, 2)]),
    key("9", "", "GRAPHICS", "", &[(4, 1)]),
    key("0", "", "RUBOUT",   "", &[(4, 0)]),

    key("Q", "PLOT",   "\"\"", "SIN",  &[(2, 0)]),
    key("W", "UNPLOT", "OR",   "COS",  &[(2, 1)]),
    key("E", "REM",    "STEP", "TAN",  &[(2, 2)]),
    key("R", "RUN",    "<=",   "INT",  &[(2, 3)]),
    key("T", "RAND",   "<>",   "RND",  &[(2, 4)]),
    key("Y", "RETURN", ">=",   "STR$", &[(5, 4)]),
    key("U", "IF",     "$",    "CHR$", &[(5, 3)]),
    key("I", "INPUT",  "(",    "CODE", &[(5, 2)]),
    key("O", "POKE",   ")",    "PEEK", &[(5, 1)]),
    key("P", "PRINT",  "\"",   "TAB",  &[(5, 0)]),

    key("A", "NEW",   "STOP",   "ARCSIN", &[(1, 0)]),
    key("S", "SAVE",  "LPRINT", "ARCCOS", &[(1, 1)]),
    key("D", "DIM",   "SLOW",   "ARCTAN", &[(1, 2)]),
    key("F", "FOR",   "FAST",   "SGN",    &[(1, 3)]),
    key("G", "GOTO",  "LLIST",  "ABS",    &[(1, 4)]),
    key("H", "GOSUB", "**",     "SQR",    &[(6, 4)]),
    key("J", "LOAD",  "\u{2212}", "VAL",  &[(6, 3)]),
    key("K", "LIST",  "+",      "LEN",    &[(6, 2)]),
    key("L", "LET",   "=",      "USR",    &[(6, 1)]),
    key("NEWLINE", "", "",      "",       &[(6, 0)]),

    key("SHIFT", "",   "",  "",       &[(0, 0)]),
    key("Z", "COPY",   ":", "LN",     &[(0, 1)]),
    key("X", "CLEAR",  ";", "EXP",    &[(0, 2)]),
    key("C", "CONT",   "?", "AT",     &[(0, 3)]),
    key("V", "CLS",    "/", "",       &[(0, 4)]),
    key("B", "SCROLL", "*", "INKEY$", &[(7, 4)]),
    key("N", "NEXT",   "<", "NOT",    &[(7, 3)]),
    key("M", "PAUSE",  ">", "PI",     &[(7, 2)]),
    key(".", "",       ",", "",       &[(7, 1)]),
    key("SPACE", "",   "\u{00a3}", "BREAK", &[(7, 0)]),
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
    let across = ACROSS as f32;
    let down = DOWN as f32;
    let by_width = (area.width() - gap * (across - 1.0)) / across;
    let by_height = ((area.height() - gap * (down - 1.0)) / down) * KEY_ASPECT;
    let key_w = by_width.min(by_height).max(1.0);
    let key_h = key_w / KEY_ASPECT;
    let size = egui::vec2(
        key_w * across + gap * (across - 1.0),
        key_h * down + gap * (down - 1.0),
    );
    let origin = egui::pos2(
        area.center().x - size.x / 2.0,
        area.center().y - size.y / 2.0,
    );
    (0..ACROSS * DOWN)
        .map(|i| {
            let (col, row) = (i % ACROSS, i / ACROSS);
            egui::Rect::from_min_size(
                origin + egui::vec2(col as f32 * (key_w + gap), row as f32 * (key_h + gap)),
                egui::vec2(key_w, key_h),
            )
        })
        .collect()
}
