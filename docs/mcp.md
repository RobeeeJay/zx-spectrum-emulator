# The emulator as an MCP server

`cargo run --release --bin mcp` speaks [MCP](https://modelcontextprotocol.io)
over stdin and stdout: JSON-RPC 2.0, one message a line. It is the emulator
without a window — the same `Spectrum`, the same observer, the same
disassembler — driven by a program rather than by a person.

What it is for is taking a game apart: load it, run it under control, watch
what it does, and write down what has been worked out.

```jsonc
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call",
 "params":{"name":"load_tape","arguments":{"path":"tapes/Jetpac.tzx"}}}
```

Every reply is text. The thing reading it is a language model, and a table it
can quote back is worth more than a structure it has to re-serialise.

## The tools

**Getting a machine going.** `machine_info`, `set_machine`, `reset`,
`load_tape`, `load_snapshot`, `load_recording`, `play_recording`.

`load_tape` types `LOAD ""` and runs the tape to its end with the ROM loader
handed over, so a four-minute tape loads in a second or two of real time; pass
`autoload: false` to put it in the deck without starting it. `load_snapshot` is
quicker still and starts in the middle of a game, which is usually where the
interesting code is.

**Running it.** `step` (with `over` for stepping over a `CALL`), `run_frames`,
`run_tstates`, `run_until`, `watch_events`.

`run_until` is the breakpoint. `watch_events` is the more useful one for
finding your way around a program nobody has documented: it stops on *what the
program did* rather than on where it is — a write to the display file, the
beeper or the sound chip being touched, the frame interrupt, a call into the
ROM, any `IN`, any `OUT`. Switch on `screen` and run, and the machine stops at
the instruction that drew something.

**Looking at it.** `screen`, `graphics`.

`screen` sends a PNG in an image block — a model with eyes sees the picture —
along with a sketch of the 32x24 character cells and what the attributes are
set to, for one that cannot. `graphics` reads memory as characters and sprites,
eight bytes each, which is how graphics are found: point it at a candidate
address and see whether letters, a sprite or rubbish comes out.

**Reading it.** `registers`, `read_memory`, `write_memory`, `disassemble`.

Addresses may be numbers or strings: `32768`, `"$8000"`, `"0x8000"`, `"8000h"`.
A bare string of digits is decimal, deliberately — reading decimal addresses as
hex is how `tools/skool-symbols.py` once found nothing at all and said so
quietly.

`write_memory` pokes the running machine and nothing else; it is for testing a
theory (freeze the counter and see what stops moving), not for patching a file.

**Watching it.** `watch_routines`, then `routines`, `routine`, `call_graph`,
`code_map`, `autodoc`.

The observer attributes every write, read, port access and loop to the routine
on top of the call stack, worked out from what the CPU did rather than by
decoding opcodes. Nothing is counted until `watch_routines` switches it on, and
the tools say so rather than answering with an empty table that reads like an
answer. It costs a branch on every access, so switch it off when finished.

**Keeping it.** `save_state`, `restore_state` — by name in the session, or as a
`.sna` file. An experiment that goes wrong is undone by putting the machine
back.

**Writing it down.** `set_comment`, `comments`, `save_comments`. Notes go to
`<name>.zxrs.txt` beside the tape or snapshot: plain text, one line an address,
readable and diffable without the emulator. A guess made by AutoDoc is marked
as a guess and never overwrites something typed.

## Things worth knowing

**A machine with no ROM looks like a game that crashed.** A fresh `Spectrum`
has `$FF` where the ROM should be, which is `RST $38` forty thousand times
over: it runs, it fills the screen with rubbish, and it sits at `$0038`. The
session tracks whether a real ROM has been put in rather than looking at the
bytes, because "not all zeros" is true of an empty machine. Every game
"loaded" against that and ran for thousands of frames of nothing, and the
report said so cheerfully — which is what the end-to-end test against a real
tape was written to catch.

**The observer needs calls to watch.** An edge in the call graph is a call made
from inside something. Code entered by a jump has no frame for a call to come
from, so a main loop that was never itself called is not a routine and its
calls are attributed to whatever is under it.

**Where in the frame a routine ran decides what is seen.** The ULA draws
between T-state 14,336 and 57,344 on a 48K. A write above the beam is on the
screen this frame; one below it waits for the next. `run_tstates` is how to
stop part-way through a frame and look.

**A recording is played by fetch count.** A frame of an RZX is a number of
opcode fetches, and the interrupt comes at the recording's own frame boundary
rather than on the T-state clock. `play_recording` does that; see
[`timing.md`](timing.md).

**There is no JSON crate in the lock file**, and none can be added, so
`src/mcp/json.rs` is a small implementation of exactly what JSON-RPC uses —
the same bargain as `src/svg.rs`.

## What is not exposed

The emulator can do these and the server does not offer them yet, which is
worth knowing before assuming they are missing from the emulator too:

- searching memory for bytes, and comparing two snapshots to find what changed
  (which is how a variable is found);
- the RAM heat map (`tracker.rs`): what has been written and read, and when;
- the profiler (`profiler.rs`): where the time goes;
- Race the Beam (`race.rs`): replaying a frame instruction by instruction to
  see the picture being built;
- the timeline (`timeline.rs`): a turn of the loop drawn against the frames it
  ran in;
- the tape deck's own contents — blocks, loaders recognised by
  `flashload::CORES`, where a turbo block starts;
- ROM symbol files and SkoolKit disassemblies (`autodoc::Symbols`);
- the ZX81, and the 128K's AY registers;
- keyboard input, other than the `LOAD ""` that `load_tape` types.
