//! Finding where a game keeps a number — its score, its lives — by watching
//! memory as it is played, the way a cheat finder does.
//!
//! A number is seldom one byte. A score is kept across several: in binary,
//! as binary-coded decimal two digits a byte, or a byte a digit — and a byte a
//! digit may count from 0, from the character `0`, or from wherever the game's
//! own font keeps its digits. So a candidate is a place and a way of keeping a
//! number there, every way the judge can read one: a byte, a word, BCD of one
//! to four bytes, one to eight digit bytes. Bytes that cannot be that way of
//! keeping a number — a BCD nibble above 9, digit bytes more than ten apart —
//! drop it, every round.
//!
//! Each time the number on the screen changes, say how, and the candidates
//! that did otherwise go. Saying what it is now, as the screen shows it —
//! `001230`, noughts and all — also says how wide it is, which is the only way
//! to tell a three-byte BCD score from the two bytes at its end that read the
//! same number while it is small; and it settles which byte stands for nought
//! in a number kept a digit a byte.
//!
//! What is found is for the judge, which reads memory for the reward; none of
//! it reaches the network. Numbers kept lowest digit first are not looked for,
//! since the judge cannot read them.

use std::cmp::Ordering;

use super::judge::Number;

/// Where the search starts: the display file changes whenever anything
/// moves, and holds pictures of numbers rather than numbers.
pub const FIRST: u16 = 0x5B00;
const MOST_BCD: u8 = 4;
const MOST_DIGITS: u8 = 8;

/// A way a game might keep a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Byte,
    /// Two bytes, low first.
    Word,
    /// Binary-coded decimal in this many bytes, most significant first.
    Bcd(u8),
    /// A byte a digit, most significant first, and the byte that stands for
    /// nought once it is known.
    Digits {
        count: u8,
        zero: Option<u8>,
    },
}

impl Form {
    fn width(self) -> usize {
        match self {
            Form::Byte => 1,
            Form::Word => 2,
            Form::Bcd(n) => n as usize,
            Form::Digits { count, .. } => count as usize,
        }
    }
}

/// A place a number might be kept, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub at: u16,
    pub form: Form,
}

/// What happened to the number since the last round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// It is now this, shown in this many digits — the noughts in front
    /// counted — or 0 when the width is not known.
    Is {
        value: u64,
        digits: u8,
    },
    Up,
    Down,
    Changed,
    Same,
}

impl Test {
    /// The number as the screen shows it: `001230` is 1230 in six digits.
    pub fn is_shown(text: &str) -> Result<Test, String> {
        let text = text.trim();
        if text.is_empty() || text.len() > 19 || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!(
                "{text:?} is not a number as the screen shows it: digits only, noughts and all"
            ));
        }
        Ok(Test::Is {
            value: text.parse().expect("digits"),
            digits: text.len() as u8,
        })
    }
}

fn bcd_value(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0, |n, b| n * 100 + (b >> 4) as u64 * 10 + (b & 15) as u64)
}

/// The digits of a number, most significant first, padded to `count`, or
/// None when it has more than that.
fn digits_of(mut value: u64, count: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; count];
    for d in out.iter_mut().rev() {
        *d = (value % 10) as u8;
        value /= 10;
    }
    (value == 0).then_some(out)
}

impl Candidate {
    /// How the judge would read it — once there is enough known to say: a
    /// number kept a digit a byte needs its nought.
    pub fn number(&self) -> Option<Number> {
        let at = self.at;
        match self.form {
            Form::Byte => Some(Number::Byte(at)),
            Form::Word => Some(Number::Word(at)),
            Form::Bcd(bytes) => Some(Number::Bcd { at, bytes }),
            Form::Digits {
                count,
                zero: Some(zero),
            } => Some(Number::Digits { at, count, zero }),
            Form::Digits { zero: None, .. } => None,
        }
    }

    /// In words, for the window.
    pub fn describe(&self) -> String {
        let at = self.at;
        match self.form {
            Form::Byte => format!("a byte at {at:04X}"),
            Form::Word => format!("two bytes at {at:04X}, low first"),
            Form::Bcd(1) => format!("BCD, a byte at {at:04X}"),
            Form::Bcd(n) => format!("BCD, {n} bytes at {at:04X}"),
            Form::Digits {
                count,
                zero: Some(z),
            } => format!("{count} digit bytes at {at:04X}, nought as {z:02X}"),
            Form::Digits { count, zero: None } => {
                format!("{count} digit bytes at {at:04X}, nought not known yet")
            }
        }
    }

    /// Its bytes in memory kept from `FIRST`.
    fn bytes<'a>(&self, memory: &'a [u8]) -> &'a [u8] {
        let from = (self.at - FIRST) as usize;
        &memory[from..from + self.form.width()]
    }

    /// Whether these bytes can be a number kept this way at all.
    fn fits(&self, b: &[u8]) -> bool {
        match self.form {
            Form::Byte | Form::Word => true,
            Form::Bcd(_) => b.iter().all(|x| x >> 4 <= 9 && x & 15 <= 9),
            Form::Digits { zero: Some(z), .. } => b.iter().all(|x| x.wrapping_sub(z) <= 9),
            Form::Digits { zero: None, .. } => {
                let (lo, hi) = b
                    .iter()
                    .fold((u8::MAX, 0), |(l, h), x| (l.min(*x), h.max(*x)));
                hi - lo <= 9
            }
        }
    }

    /// Which way it went. Digit bytes with one nought, like BCD, compare in
    /// the order their digits do, whatever nought is; a word does not.
    fn went(&self, before: &[u8], now: &[u8]) -> Ordering {
        match self.form {
            Form::Word => {
                let word = |b: &[u8]| b[0] as u16 | (b[1] as u16) << 8;
                word(now).cmp(&word(before))
            }
            _ => now.cmp(before),
        }
    }

    /// Whether it reads as the number shown, settling the nought of digit
    /// bytes if it does.
    fn is(&mut self, b: &[u8], value: u64, digits: u8) -> bool {
        match &mut self.form {
            Form::Byte => b[0] as u64 == value,
            Form::Word => (b[0] as u64 | (b[1] as u64) << 8) == value,
            Form::Bcd(n) => {
                (digits == 0 || (digits as usize).div_ceil(2) == *n as usize)
                    && bcd_value(b) == value
            }
            Form::Digits { count, zero } => {
                if digits != 0 && digits != *count {
                    return false;
                }
                let Some(ds) = digits_of(value, *count as usize) else {
                    return false;
                };
                let z = zero.unwrap_or(b[0].wrapping_sub(ds[0]));
                let reads = b.iter().zip(&ds).all(|(x, d)| *x == z.wrapping_add(*d));
                if reads {
                    *zero = Some(z);
                }
                reads
            }
        }
    }
}

/// Every way of keeping a number that is looked for.
fn forms() -> Vec<Form> {
    let mut forms = vec![Form::Byte, Form::Word];
    forms.extend((1..=MOST_BCD).map(Form::Bcd));
    forms.extend((1..=MOST_DIGITS).map(|count| Form::Digits { count, zero: None }));
    forms
}

#[derive(Clone, Debug)]
pub struct Search {
    candidates: Vec<Candidate>,
    /// Memory as it was at the last round, from `FIRST`.
    before: Vec<u8>,
    pub rounds: usize,
}

fn snapshot(peek: &dyn Fn(u16) -> u8) -> Vec<u8> {
    (FIRST..=0xFFFF).map(peek).collect()
}

impl Search {
    /// Every place and way that memory as it stands could be keeping a
    /// number, to compare with next time.
    pub fn new(peek: &dyn Fn(u16) -> u8) -> Search {
        let before = snapshot(peek);
        let forms = forms();
        let mut candidates = Vec::new();
        for at in FIRST..=0xFFFF {
            for &form in &forms {
                if at as usize + form.width() > 0x10000 {
                    continue;
                }
                let c = Candidate { at, form };
                if c.fits(c.bytes(&before)) {
                    candidates.push(c);
                }
            }
        }
        Search {
            candidates,
            before,
            rounds: 0,
        }
    }

    /// Keep the candidates whose number did what the number on the screen
    /// did.
    pub fn narrow(&mut self, test: Test, peek: &dyn Fn(u16) -> u8) {
        let now = snapshot(peek);
        let before = &self.before;
        self.candidates.retain_mut(|c| {
            let (was, is) = (c.bytes(before), c.bytes(&now));
            if !c.fits(is) {
                return false;
            }
            match test {
                Test::Up => c.went(was, is) == Ordering::Greater,
                Test::Down => c.went(was, is) == Ordering::Less,
                Test::Changed => was != is,
                Test::Same => was == is,
                Test::Is { value, digits } => c.is(is, value, digits),
            }
        });
        self.before = now;
        self.rounds += 1;
    }

    /// What is left, in address order.
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }
}
