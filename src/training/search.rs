//! Finding where a game keeps a number — its score, its lives — by watching
//! memory as it is played, the way a cheat finder does.
//!
//! Every address above the screen starts as a candidate. Each time the
//! number on the screen changes, say how — it went up, it went down, it is
//! now this, it did not change — and the addresses whose byte did otherwise
//! are dropped. A few rounds usually leave a handful. What is found is for
//! the judge, which reads memory for the reward; none of it reaches the
//! network.
//!
//! A score kept in several bytes is found by its lowest-changing byte, which
//! can wrap on a carry — a BCD 99 going to 00 — and so fail "went up" once;
//! "changed" is the safer thing to say about a score.

/// Where the search starts: the display file changes whenever anything
/// moves, and holds pictures of numbers rather than numbers.
pub const FIRST: u16 = 0x5B00;

/// What happened to the number since the last round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// It is now this, kept as a game might keep it: the byte itself, one
    /// BCD byte, or an ASCII digit.
    Is(u8),
    Up,
    Down,
    Changed,
    Same,
}

impl Test {
    fn passes(self, before: u8, now: u8) -> bool {
        match self {
            Test::Is(n) => {
                now == n
                    || (n < 100 && now == ((n / 10) << 4) | (n % 10))
                    || (n < 10 && now == b'0' + n)
            }
            Test::Up => now > before,
            Test::Down => now < before,
            Test::Changed => now != before,
            Test::Same => now == before,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Search {
    candidates: Vec<u16>,
    /// Memory as it was at the last round, from `FIRST`.
    before: Vec<u8>,
    pub rounds: usize,
}

fn snapshot(peek: &dyn Fn(u16) -> u8) -> Vec<u8> {
    (FIRST..=0xFFFF).map(peek).collect()
}

impl Search {
    /// Every address a candidate, and memory as it stands to compare with.
    pub fn new(peek: &dyn Fn(u16) -> u8) -> Search {
        Search {
            candidates: (FIRST..=0xFFFF).collect(),
            before: snapshot(peek),
            rounds: 0,
        }
    }

    /// Keep the addresses whose byte did what the number did.
    pub fn narrow(&mut self, test: Test, peek: &dyn Fn(u16) -> u8) {
        let now = snapshot(peek);
        let before = &self.before;
        self.candidates.retain(|a| {
            let i = (a - FIRST) as usize;
            test.passes(before[i], now[i])
        });
        self.before = now;
        self.rounds += 1;
    }

    pub fn candidates(&self) -> &[u16] {
        &self.candidates
    }
}
