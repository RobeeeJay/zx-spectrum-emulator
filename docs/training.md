# Training a network to play

The **Training** window teaches a neural network to play whatever the machine
is running, inside the emulator, with nothing to install. It learns by proximal
policy optimisation (PPO) — the method that learnt Atari games from their
pixels — written in [`burn`](https://burn.dev) and run on the graphics card
through wgpu, or on the processor.

## The rule: it sees the screen and nothing else

The network is shown the picture — what the ULA painted, the frame a player
saw, border and racing-the-beam effects and all — and never an address, a
register or anything else about the machine. `src/training/sight.rs` is the one
place that decides what it sees, and it builds the picture from
`screen::render` and nothing else. `tests/training_env.rs` holds it to that: two
machines with the same picture over different memory and registers look the
same to it.

It has to be told how well it is doing, though, and that is read out of memory
by a **judge**, as the Arcade Learning Environment reads Atari RAM: the score
going up is rewarded, a life lost costs, and the lives running out ends the
game. What the judge reads goes no further than the reward and whether the
game is over.

## Setting up a game

- **What it may press.** An interface — Kempston, Sinclair, Cursor, Fuller or
  one of the DK'Tronics — with or without the diagonals and fire while moving,
  or a list of keys by their legends (`O, P, SPACE`). Each action is a
  combination held together for a step, and choosing another lets go of the
  last. Doing nothing is always one of them.
- **What it sees.** The display alone or with a border, at full, half or
  quarter size, grey or colour, with a few frames stacked — one picture cannot
  say which way anything is moving. The network's size follows from the
  picture's, and one smaller than 36 pixels either way is refused.
- **How long a step is.** How many frames each choice is held for, and up to
  how many frames of doing nothing each game starts with — a different number
  each time, so games that start alike do not all go alike.
- **The reward.** Where the score and the lives are, written as the file has
  them: `byte ADDR`, `word ADDR`, `bcd ADDR BYTES` or `digits ADDR COUNT ZERO`,
  addresses in hex. What a point is worth, what a life costs, the lives at
  which the game is over, and the steps after which a game is stopped however
  it is going. A reward for every step survived is there for games with
  nothing else to go on.
- **Where every game starts.** The machine as it is when Start is pressed, or
  a quicksave. Put the game where play begins — past the title screen, one
  life in — before pressing Start.

### Finding the score

**Find a number** in the window looks for where the game keeps it, the way a
cheat finder does. Start looking, play until the number on the screen
changes, and say how: it went up, went down, is now this, did not change.
Addresses whose byte did otherwise are dropped, and a few rounds usually leave
a handful, each with a button to use it as the score or the lives. The search
starts above the screen at `$5B00`: the display file holds pictures of
numbers, not numbers. A score kept in several bytes is found by the byte that
changes most, which can wrap on a carry — a BCD 99 going to 00 — so *changed*
is the safer thing to say about a score than *went up*. What is found is
offered as `byte ADDR`; make it `bcd` or `digits` if that is how the game
keeps it.

## Learning

Every update plays all the games at once for some steps each, on as many
threads as there are cores, then learns from what happened: how much better or
worse each choice turned out than the network expected, nudging it towards the
better ones a few passes over, never far in one go. The defaults are the Atari
ones — 16 games, 128 steps, a learning rate of 0.00025, a discount of 0.99.

**The discount is the setting most worth thinking about.** It says how much a
reward a step later counts against one now. At 0.99 each choice is judged
against the next hundred or so steps, which is right for a game whose rewards
come long after the move that earned them, and wrong for one whose come at
once: every choice then carries twenty-odd steps of other choices' luck, and
learning is slow. The tiny game in `tests/training_ppo.rs` — fire scores, a
bar is on the screen while fire is held, and only letting go and pressing
again scores again — took about 140 updates to learn at 0.99 and about 25 at
0.9. A learning rate two and a half times higher did nothing for it.

### Graphics card or processor

Measured on an M5 Pro at the default size — 16 games of 128 steps, the display
at half size, four frames:

| | an update of 2,048 steps |
| --- | --- |
| Graphics card (wgpu, Metal) | 4.7 s |
| Processor (ndarray, all cores) | 45 s |

The emulation itself is not what takes the time: one core runs about 6,400
frames a second, and an update of that size is 8,192 frames spread across all
of them. A machine without a graphics card wgpu can use says so in the window
and the processor can be chosen instead.

## Keeping a network

The network is kept beside the tape, in `<name>.zxrs-net/`: `network.bin`,
and the `setup.txt` it was trained with, since it cannot be rebuilt without the
picture and the actions it was made for. It is kept every ten updates and when
stopped. **Go on from the kept network** starts the next run from it rather
than afresh. Only the network is kept, not the optimiser's running averages,
so the first few updates of a run that goes on are a little rougher than they
would have been.

The set-up is written beside the tape as `<name>.zxrs-train.txt` whenever a
run starts, and read back when the tape is loaded again. It is one
`key = value` a line and can be written by hand; a key left out takes its
default, and a line that cannot be read stops the reading and is named — a
mistyped score address skipped quietly would train for hours on nothing. The
set-up file can also end a game when a byte holds a value
(`over_when = 8123 FF`), which the window does not show.

## Letting it play

**Let it play** puts the kept network in charge of the machine on the screen.
It plays the way it was trained: every step's worth of frames it looks at the
picture, chooses, and holds the choice until the next step. The keyboard and
the stick on the desk are not read while it plays. Its choice is drawn from
its preferences as it was while learning; **Its favourite every time** takes
the one it likes best instead, which can get stuck doing one thing forever
when the chance it learnt with is taken away. It plays on the processor, which
is quick enough for one choice at a time and leaves the graphics card to
training, so it can play a kept network while another run goes on.

## What is not done

- The ZX81 cannot be trained on: it cannot be copied, and every game starts
  from a copy.
- The machine every game starts from is not kept with the network. Going on
  from a kept network means starting from the same place again.
- Only one of the games is shown while it trains.
