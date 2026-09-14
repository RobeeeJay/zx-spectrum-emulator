//! Finding where a game keeps a number by watching memory change, for the
//! judge — which reads memory for the reward the network never sees.

use zx_rustrum::training::search::{Search, Test, FIRST};

/// Memory as a game might have it, with a score somewhere in it and noise
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
        memory[0x9C4E] = 0x07; // the score, as a byte
        Game { memory, tick: 0 }
    }
    /// A frame goes by: a timer counts, and a few bytes change at random.
    fn frame(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.memory[0x8000] = self.tick;
        for k in 0..64u32 {
            let at = (k.wrapping_mul(40_503).wrapping_add(self.tick as u32 * 977) as usize
                & 0x7FFF)
                | 0x8000;
            if at != 0x9C4E {
                self.memory[at] = self.memory[at].wrapping_add(k as u8 | 1);
            }
        }
    }
    fn peek(&self) -> impl Fn(u16) -> u8 + '_ {
        |a| self.memory[a as usize]
    }
}

/// A few rounds of saying what the number on the screen did leave the one
/// address that keeps it, however much else in memory moves.
#[test]
fn a_score_is_found_by_saying_what_it_did() {
    let mut game = Game::new();
    let mut search = Search::new(&game.peek());
    assert_eq!(
        search.candidates().len(),
        0x10000 - FIRST as usize,
        "everything above the screen, to begin with"
    );
    for round in 0..6 {
        game.frame();
        if round % 2 == 0 {
            game.memory[0x9C4E] += 1;
            search.narrow(Test::Up, &game.peek());
        } else {
            search.narrow(Test::Same, &game.peek());
        }
    }
    game.frame();
    search.narrow(Test::Is(10), &game.peek());
    assert_eq!(
        search.candidates(),
        &[0x9C4E],
        "after {} rounds, {} left",
        search.rounds,
        search.candidates().len()
    );
}

/// A number the screen shows as 42 may be kept as the byte 42, as the BCD
/// byte $42, and a single digit as its ASCII character: "is now" finds all
/// three. The display file is never a candidate — it holds pictures of
/// numbers, not numbers.
#[test]
fn is_now_matches_a_number_however_it_is_kept() {
    let mut memory = vec![0u8; 0x10000];
    let peek = |m: &Vec<u8>| {
        let m = m.clone();
        move |a: u16| m[a as usize]
    };
    let mut search = Search::new(&peek(&memory));
    memory[0x9000] = 42;
    memory[0x9001] = 0x42;
    memory[0x4000] = 42;
    search.narrow(Test::Is(42), &peek(&memory));
    assert_eq!(search.candidates(), &[0x9000, 0x9001]);

    let mut search = Search::new(&peek(&memory));
    memory[0x9100] = b'3';
    memory[0x9101] = 3;
    search.narrow(Test::Is(3), &peek(&memory));
    assert_eq!(search.candidates(), &[0x9100, 0x9101]);
}
