# A faster CPU against the same ULA

A plan, not a change. Nothing in this file is implemented yet.

The Clock dropdown that exists (hidden, behind `ui::SHOW_CLOCK`) multiplies the
whole machine: at 7MHz the frame is still 69,888 T-states and those T-states go
by twice as fast, so the interrupt comes a hundred times a second and a game
reading the frame counter runs fast rather than smoothly. That is a Spectrum
with its crystal changed. What is wanted instead is an accelerator: the CPU
gets more cycles inside a frame of the ULA's own time, the picture is still
drawn fifty times a second, and a game's timing loops finish early rather than
its whole world speeding up.

## Why it is not a multiplier

Everything here counts one clock. `SpectrumBus::tstates` is the ULA's position
in the frame *and* the CPU's cost of the instruction it is running, and a dozen
things read it as one or the other:

| reads `tstates` as | what for |
| --- | --- |
| the ULA's position | the contention table, the floating bus, snow, the painted frame, the beam |
| elapsed real time | the tape's pulses, the mixer's samples, the observer's timestamps |
| the CPU's cost | `Z80` instruction timing, `run(budget)`, the profiler |

An accelerator splits the third from the first two. The CPU's cycles get
cheaper in ULA time; everything the ULA does, and everything measured in real
seconds, stays where it is.

## The model

Two clocks, one of them derived:

- **The ULA's clock** stays `tstates`: 69,888 to a 48K frame, 3.5MHz, and every
  table indexed by it keeps its meaning.
- **The CPU's clock** is `turbo` times that. An instruction that costs four
  CPU cycles costs `4 / turbo` ULA T-states.

Only whole multiples — 1, 2, 4, 8 — as the hardware offered, so the division is
exact with a remainder carried between instructions rather than a float:

```rust
/// CPU cycles that have not yet been paid for in ULA time.
cpu_debt: u32,

fn cpu_cycles(&mut self, cycles: u32) {
    self.cpu_debt += cycles;
    self.tstates += self.cpu_debt / self.turbo;
    self.cpu_debt %= self.turbo;
}
```

**Contention is not divided.** A stall is the ULA holding the CPU off the bus;
it lasts as long as the ULA says whatever the CPU's clock is doing, so it goes
straight onto `tstates`. That falls out of the code as it stands: `access` and
`contend_addr` already add the delay and the cycles separately, and only the
cycles change hands.

## What changes, by name

Everything is in `src/machine.rs` unless it says otherwise.

1. **`SpectrumBus`** gains `pub turbo: u32` (1 by default) and `cpu_debt: u32`,
   and a `fn cpu_cycles(&mut self, cycles: u32)` as above.
2. **`access(addr, t)`** — the delay stays as it is; `self.tstates += t`
   becomes `self.cpu_cycles(t)`.
3. **`contend_addr(addr, times)`** — the `delay()` per T-state stays; the
   `times` themselves go through `cpu_cycles`. The loop has to add the delay
   and the cycle separately rather than `delay + 1` in one go.
4. **`contend_io(port)`** — every `io_stall()` stays; the 1, 3 and 4 T-state
   costs go through `cpu_cycles`. The `sampled` T-state it returns is a ULA
   position and is read after the stalls, so it stays right.
   One thing to know about that: `sampled` is taken part way through the
   pattern, after a cycle that is now fractional, so the moment the ULA is
   asked for its byte is quantised to whole ULA T-states. That is right — the
   ULA can only put a byte on the bus at its own rate, whatever is asking —
   but it means two `IN`s in a row at 8× can read the same floating-bus byte
   where at 1× they read consecutive ones. Worth a test rather than a
   surprise.

5. **`set_model`** keeps `turbo` across a model change, as it keeps
   `tape_boost` and the rest.
6. **`Spectrum::run(budget)`** needs nothing: the budget is in ULA T-states, so
   a faster CPU does more instructions in the same budget and the emulator's
   pacing against the host clock is unchanged.
7. **`ui::App`** — `clock_mult` becomes `turbo`, pushed to the bus rather than
   multiplied into the work budget; `clock_hz` reports the CPU's clock for the
   dropdown's label; `apply_clock` stops touching the mixer, since the mixer
   counts ULA T-states and those still pass at 3.5MHz.
8. **`ui::SHOW_CLOCK`** comes out, and the dropdown says what it now is.

## Three places it has to be held at 1×

- **While a recording plays.** An RZX frame is a number of opcode fetches, and
  the recording's frame boundary is the video frame; a CPU getting through
  those fetches in a quarter of the ULA time would put four frames' worth of
  input into one frame of picture. `rzx.is_some()` should force 1× and say so.
- **While the tape is being answered.** Fastload hands blocks over at $0556 and
  leaves the machine where the routine would have; nothing there depends on the
  CPU's clock, so this one is a check rather than a rule — but a loader
  counting pulses in CPU cycles will fail at 2× exactly as it did on the real
  thing, which is worth a word in the tooltip rather than a fix.
- **Race the Beam and Cursor Beam.** Both replay a frame from its interrupt to
  a T-state; that still works, but "what the machine had done by this point in
  the frame" means something different when the CPU is running four times as
  fast, so the drawing wants a word saying which clock the cursor is in.

## What it does to the parts that were not touched

- **Sound** comes out right without being told anything. The beeper is a level
  the program writes; at 7MHz a toggling loop comes round in half the ULA
  T-states, so the note is an octave up — which is what an accelerated machine
  sounded like — and the mixer, which counts ULA T-states, hears it happen.
- **The tape** loads at the same rate in seconds, since its pulses are in ULA
  time, and a loader counting them in CPU cycles will miscount exactly as it
  would on the real thing. Loaders are why accelerators had a switch.
- **The floating bus and snow** are read at the ULA's position, which is still
  the ULA's position. Whether a real accelerator snows the same way is not
  known here and would want a note rather than a guess.
- **The observer, the profiler and the call-flow timeline** timestamp in ULA
  T-states, so a turn of the loop still lands against the frame it ran in —
  which is the point of that drawing. A routine's instruction count is its own
  and does not change; its cost in T-states does, which is the thing being
  bought.
- **The interrupt** is raised at the frame boundary and held for 32 ULA
  T-states. At 8× the CPU can get through sixty-odd instructions inside that
  window rather than eight, which is what lets an accelerated machine answer an
  interrupt it would otherwise have missed. Nothing to change; something to
  check.

## Contention: the question worth deciding first

Three things a real accelerator might do, and what each means here:

1. **Stall as the ULA always did** — the CPU waits out the full ULA delay. What
   the plan above does, and the simplest to defend: the ULA is unchanged and it
   is the one holding the bus.
2. **Do not contend at all above 1×** — some accelerators only run fast in
   uncontended RAM and drop to 3.5MHz for the rest. Would need the turbo to be
   asked per access rather than held once.
3. **Contend in CPU cycles** — treat the delay as if the ULA were faster too,
   which is not what the hardware does and would make the timing tests
   meaningless.

Recommend (1), with (2) as a switch later if a real machine is found to differ.

## How it is kept honest

At 1× nothing may move: `tests/timing.rs`, `tests/reference_48k.rs`,
`tests/halt2int.rs` and the Border Break comparison are the ones that would
catch a mistake in the arithmetic, and they all run at 1×. That is the first
milestone — the mechanism in place, every existing test unchanged.

New tests, once it multiplies:

- A frame is still 69,888 T-states and the interrupt still comes 50.08 times a
  second at 2×, 4× and 8×.
- Twice the instructions in a frame at 2×; eight times at 8×, within the
  rounding the debt carries.
- A contended access still costs what the table says: a `LD A,(HL)` on $4000
  at a known T-state costs the same stall at 4× as at 1×, and only its own
  four cycles get cheaper.
- The debt never loses a cycle: run a million instructions at 8× and the ULA
  time is the CPU time divided by eight, to the T-state.
- A tape still loads: Head over Heels at 4× reaches the same place, since the
  pulses are in ULA time.

## Staging

1. `cpu_cycles` and `turbo`, fixed at 1. Nothing changes; the suite proves it.
2. Let `turbo` be set. Add the tests above.
3. The dropdown, unhidden, saying CPU rather than machine.
4. A note in `docs/timing.md` on what the two clocks mean, and this file
   deleted or reduced to what was decided.
