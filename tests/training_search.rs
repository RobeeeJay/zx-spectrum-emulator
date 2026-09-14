//! Finding where a game keeps a number by watching memory change, for the
//! judge — which reads memory for the reward the network never sees. A
//! number is found however the judge could read it: a byte, a word, BCD
//! across several bytes, or a byte a digit counting from whatever the game
//! uses for nought.

use zx_rustrum::training::judge::Number;
use zx_rustrum::training::search::{Candidate, Search, Test};

/// Memory as a game might have it, with a number somewhere in it and noise
/// elsewhere that changes on its own.
struct Game {
    memory: Vec<u8>,
    tick: u8,
}

impl Game {
    fn new() -> Game {
        let mut memory = vec![0u8; 0x10000];
        for (i, b) in memory.iter_mut().enumerate() {
            *b = (i as u32).wrapping_mul(2_654_435_761).to_be_bytes()[0];
        }
        Game { memory, tick: 0 }
    }
    /// A frame goes by: a timer counts, and bytes change here and there,
    /// never in the number itself.
    fn frame(&mut self, spare: std::ops::Range<usize>) {
        self.tick = self.tick.wrapping_add(1);
        self.memory[0x8000] = self.tick;
        for k in 0..64u32 {
            let at = (k.wrapping_mul(40_503).wrapping_add(self.tick as u32 * 977) as usize
                & 0x7FFF)
                | 0x8000;
            if !spare.contains(&at) {
                self.memory[at] = self.memory[at].wrapping_add(k as u8 | 1);
            }
        }
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.memory[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn peek(&self) -> impl Fn(u16) -> u8 + '_ {
        |a| self.memory[a as usize]
    }
}

fn numbers(search: &Search) -> Vec<Option<Number>> {
    search.candidates().iter().map(Candidate::number).collect()
}

fn shown(text: &str) -> Test {
    Test::is_shown(text).unwrap()
}

/// A score kept in three bytes of BCD, among memory that changes on its own
/// every frame, is found by saying what it did and then what it is — six
/// digits on the screen being three bytes of BCD.
#[test]
fn a_bcd_score_across_three_bytes_is_found() {
    let mut game = Game::new();
    let score = 0x9C4E..0x9C51;
    game.put(0x9C4E, &[0x00, 0x01, 0x20]);
    let mut search = Search::new(&game.peek());
    for (i, bcd) in [[0x00, 0x01, 0x50], [0x00, 0x01, 0x50], [0x00, 0x02, 0x10]]
        .iter()
        .enumerate()
    {
        game.frame(score.clone());
        game.put(0x9C4E, bcd);
        let test = if i == 1 { Test::Same } else { Test::Up };
        search.narrow(test, &game.peek());
    }
    game.frame(score.clone());
    search.narrow(shown("000210"), &game.peek());
    assert_eq!(
        numbers(&search),
        vec![Some(Number::Bcd {
            at: 0x9C4E,
            bytes: 3
        })],
        "left: {:?}",
        search.candidates()
    );
}

/// A score kept a digit a byte as characters — Manic Miner's way — is found,
/// and saying what it is settles that nought is the character 0.
#[test]
fn a_score_kept_as_characters_is_found_and_its_nought_settled() {
    let mut game = Game::new();
    let score = 0x8429..0x842F;
    game.put(0x8429, b"000120");
    let mut search = Search::new(&game.peek());
    game.frame(score.clone());
    game.put(0x8429, b"000150");
    search.narrow(Test::Up, &game.peek());
    game.frame(score.clone());
    search.narrow(Test::Same, &game.peek());
    game.frame(score.clone());
    game.put(0x8429, b"000230");
    search.narrow(shown("000230"), &game.peek());
    assert_eq!(
        numbers(&search),
        vec![Some(Number::Digits {
            at: 0x8429,
            count: 6,
            zero: b'0'
        })],
        "left: {:?}",
        search.candidates()
    );
}

/// A game with a font of its own keeps its digits wherever the font has
/// them — here nought is $10 — and that is found as well as the usual ways.
#[test]
fn digits_counting_from_a_font_of_its_own_are_found() {
    let mut game = Game::new();
    let score = 0xA000..0xA004;
    game.put(0xA000, &[0x10, 0x10, 0x14, 0x12]);
    let mut search = Search::new(&game.peek());
    game.frame(score.clone());
    game.put(0xA000, &[0x10, 0x10, 0x15, 0x17]);
    search.narrow(Test::Up, &game.peek());
    game.frame(score.clone());
    search.narrow(shown("0057"), &game.peek());
    assert_eq!(
        numbers(&search),
        vec![Some(Number::Digits {
            at: 0xA000,
            count: 4,
            zero: 0x10
        })],
        "left: {:?}",
        search.candidates()
    );
}

/// While a BCD score is small, the bytes at its end read the same number as
/// the whole of it. Saying it as the screen shows it, noughts and all, is
/// what tells them apart: four digits are two bytes, six are three.
#[test]
fn the_width_shown_on_the_screen_tells_a_score_from_its_own_end() {
    let mut memory = vec![0x77u8; 0x10000];
    memory[0x9000..0x9003].copy_from_slice(&[0x00, 0x12, 0x34]);
    let peek = |m: &Vec<u8>| {
        let m = m.clone();
        move |a: u16| m[a as usize]
    };
    let bcd = |search: &Search| -> Vec<Option<Number>> {
        numbers(search)
            .into_iter()
            .filter(|n| matches!(n, Some(Number::Bcd { .. })))
            .collect()
    };
    let mut search = Search::new(&peek(&memory));
    search.narrow(shown("1234"), &peek(&memory));
    assert_eq!(
        bcd(&search),
        vec![Some(Number::Bcd {
            at: 0x9001,
            bytes: 2
        })],
        "four digits are two bytes"
    );
    let mut search = Search::new(&peek(&memory));
    search.narrow(shown("001234"), &peek(&memory));
    assert_eq!(
        bcd(&search),
        vec![Some(Number::Bcd {
            at: 0x9000,
            bytes: 3
        })],
        "six are three"
    );
}

/// Lives shown as a single 3 may be the byte 3 or the character 3, and both
/// are found. One look at a single digit settles nothing about which byte is
/// nought — every byte reads as 3 counting from three below it — so it takes
/// the number changing: a life lost, and only the places that went from 3 to
/// 2 are left. The display file is never a candidate: it holds pictures of
/// numbers, not numbers.
#[test]
fn lives_are_found_as_a_byte_or_a_character() {
    let mut memory = vec![0xEEu8; 0x10000];
    memory[0x9100] = 3;
    memory[0x9200] = b'3';
    memory[0x4000] = 3;
    let peek = |m: &Vec<u8>| {
        let m = m.clone();
        move |a: u16| m[a as usize]
    };
    let mut search = Search::new(&peek(&memory));
    search.narrow(shown("3"), &peek(&memory));
    let after_one = search.candidates().len();
    memory[0x9100] = 2;
    memory[0x9200] = b'2';
    memory[0x4000] = 2;
    search.narrow(shown("2"), &peek(&memory));
    let found = numbers(&search);
    for n in [
        Number::Byte(0x9100),
        Number::Bcd {
            at: 0x9100,
            bytes: 1,
        },
        Number::Digits {
            at: 0x9200,
            count: 1,
            zero: b'0',
        },
    ] {
        assert!(found.contains(&Some(n)), "{n:?} among {found:?}");
    }
    let stray: Vec<_> = search
        .candidates()
        .iter()
        .filter(|c| ![0x9100, 0x9200].contains(&c.at))
        .collect();
    assert!(
        stray.is_empty(),
        "{after_one} after one look, and after the life was lost {} elsewhere, such as {:?}",
        stray.len(),
        &stray[..stray.len().min(4)]
    );
}

/// What the screen shows has to be digits.
#[test]
fn a_number_as_shown_is_digits() {
    assert_eq!(
        Test::is_shown(" 001230 "),
        Ok(Test::Is {
            value: 1230,
            digits: 6
        })
    );
    assert!(Test::is_shown("12a").is_err());
    assert!(Test::is_shown("").is_err());
}
