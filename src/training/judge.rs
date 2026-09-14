//! The reward, and when a game is over.
//!
//! The judge reads the score and the lives out of memory, where the game
//! keeps them, and turns them into a number for the network. It is the only
//! part of training that reads memory, and nothing it reads goes further than
//! the reward and whether the game has ended.

/// A number as a game keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Number {
    /// One byte.
    Byte(u16),
    /// Two bytes, low first, as the Z80 keeps them.
    Word(u16),
    /// Binary-coded decimal, two digits a byte, most significant byte first.
    Bcd { at: u16, bytes: u8 },
    /// A digit a byte, most significant first, `zero` being the byte that
    /// means 0 — 0 itself, or 48 for ASCII.
    Digits { at: u16, count: u8, zero: u8 },
}

impl Number {
    pub fn read(&self, peek: &dyn Fn(u16) -> u8) -> u64 {
        match *self {
            Number::Byte(at) => peek(at) as u64,
            Number::Word(at) => peek(at) as u64 | (peek(at.wrapping_add(1)) as u64) << 8,
            Number::Bcd { at, bytes } => (0..bytes as u16).fold(0, |n, i| {
                let b = peek(at.wrapping_add(i));
                n * 100 + (b >> 4) as u64 * 10 + (b & 15) as u64
            }),
            Number::Digits { at, count, zero } => (0..count as u16).fold(0, |n, i| {
                n * 10 + peek(at.wrapping_add(i)).wrapping_sub(zero).min(9) as u64
            }),
        }
    }
}

/// How a game is scored.
#[derive(Clone, Debug, PartialEq)]
pub struct Judge {
    /// Points going up are rewarded, this much a point.
    pub score: Option<Number>,
    pub score_scale: f32,
    /// A life lost costs this much.
    pub lives: Option<Number>,
    pub life_penalty: f32,
    /// The game is over when the lives fall to this.
    pub over_at_lives: Option<u64>,
    /// Or when this byte holds this value.
    pub over_when: Option<(u16, u8)>,
    /// A little for every step survived, for games with nothing else to go on.
    pub per_step: f32,
    /// And a game is stopped after this many steps however it is going.
    pub max_steps: u32,
}

impl Default for Judge {
    fn default() -> Self {
        Judge {
            score: None,
            score_scale: 1.0,
            lives: None,
            life_penalty: 1.0,
            over_at_lives: Some(0),
            over_when: None,
            per_step: 0.0,
            max_steps: 10_000,
        }
    }
}

/// Where a game has got to, as far as the judge is concerned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    score: u64,
    lives: u64,
    pub steps: u32,
}

impl Judge {
    pub fn start(&self, peek: &dyn Fn(u16) -> u8) -> Tally {
        Tally {
            score: self.score.map_or(0, |n| n.read(peek)),
            lives: self.lives.map_or(0, |n| n.read(peek)),
            steps: 0,
        }
    }

    /// The reward for the step just taken, and whether the game is over.
    /// A score that goes down — a new game putting it back to nought — is not
    /// a punishment; only lives lost are.
    pub fn judge(&self, tally: &mut Tally, peek: &dyn Fn(u16) -> u8) -> (f32, bool) {
        tally.steps += 1;
        let mut reward = self.per_step;
        if let Some(n) = self.score {
            let now = n.read(peek);
            if now > tally.score {
                reward += (now - tally.score) as f32 * self.score_scale;
            }
            tally.score = now;
        }
        let mut over = tally.steps >= self.max_steps;
        if let Some(n) = self.lives {
            let now = n.read(peek);
            if now < tally.lives {
                reward -= (tally.lives - now) as f32 * self.life_penalty;
            }
            tally.lives = now;
            if self.over_at_lives.is_some_and(|at| now <= at) {
                over = true;
            }
        }
        if let Some((at, value)) = self.over_when {
            if peek(at) == value {
                over = true;
            }
        }
        (reward, over)
    }
}
