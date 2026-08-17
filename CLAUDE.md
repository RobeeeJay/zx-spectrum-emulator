# Working notes

ZX-Rustrum: a ZX Spectrum (48K, 128K, +2A, +3) and ZX81 emulator in Rust, on
`eframe`/`egui` with the wgpu renderer. This file is what a new session needs to
know that the code does not say for itself.

## How to work on it

**Verify against the real machine, not against expectations.** The timing was
settled by running HALT2INT and comparing all thirty values with photographs of
a real 48K, and the border by rendering Border Break and diffing it against a
photograph pixel by pixel — 0 of 116,736 differ. When something looks wrong,
find a reference and measure; do not adjust a constant until it looks right.

**Every change gets a test, and the test is checked against the bug.** Revert
the fix, watch the test fail with a message that explains the failure, restore
it. Several bugs in this project were found because a test that "passed" was
passing for the wrong reason — one tape test was matching the toolbar's `100%`
button rather than the progress bar it meant to check.

**Tests are named as sentences** describing the behaviour, and their comments
say why the behaviour matters, not what the code does. Assertion messages carry
the actual values.

**`cargo fmt` and `cargo clippy --all-targets` stay clean.** CI gates on
`-D warnings`.

**No crate can be added that is not already in the lock file.** (The network
itself does work — that is how the ROM symbol files were fetched — but the
build must stay offline-reproducible.) That is why `src/svg.rs` exists rather than a dependency, and why the
logo is drawn in code rather than decoded from a file.

**Prose:** plain, no salesmanship, British spelling. Say what happened,
including what did not work.

## What is written down elsewhere

Two files hold what has been learned about the hardware, so this one can stay a
set of decisions rather than a manual:

- [`docs/timing.md`](docs/timing.md) — T-states, contention, the floating bus,
  snow, the painted frame and how recordings are paced. What was measured
  against what, and what is still open.
- [`docs/tape-loading.md`](docs/tape-loading.md) — the tape formats and their
  traps, the ROM loader's contract, and the three loading speeds.

## Boundaries

- `roms/` and `tapes/` are gitignored: the images and games are still under
  copyright. Tests that need them skip themselves when they are absent, so the
  suite is meaningful on a machine without them.
- `designs/` holds the user's artwork and is committed. Do not restyle or
  rebrand it — the cassette carries its own maker's marks, not the emulator's.
- Never commit `*.afdesign~lock~`.
- The preferences directory is still named `ZX Spectrum Emulator` after the
  rename to ZX-Rustrum, so nobody loses their ROM paths and window layout.
- Do not drive the keyboard with AppleScript to test the running app. It types
  into whatever has focus, and once typed into the user's editor.

## Decisions worth knowing

**Timing is counted per bus cycle**, not per instruction: contention lands at
the right T-state inside an instruction because the `Bus` trait is called at
each cycle. zexdoc and zexall pass, including the undocumented flags.

**The ZX81 has no video hardware.** The CPU walks the display file and the ULA
watches the bus; an opcode fetched above `$8000` with bit 6 clear is fed to the
CPU as a NOP while the ULA turns it into eight pixels. Emulating it at that
level is what makes hi-res programs work without special cases.

**Racing the beam replays a frame; it does not read the picture.** A copy of
the machine is taken at the interrupt that starts a frame and run forward to
wherever the cursor is (`src/race.rs`), so what is on screen is the machine as
it stood after every instruction up to that T-state and none after it: the
display file part-written, the border wherever it had been set, the registers
where they had got to. Behind the cursor is what the ULA painted on the way
there; ahead of it is what the display file holds at that moment, dimmed,
because the ULA has not put it out yet. Reading the live picture instead — what
this used to do — shows a single moment of the machine either way, which
answers nothing about when within the frame anything happened.

It only works on a stopped machine, and starting one switches it off: a running
machine is somewhere else by the time the cursor has been read. The copy is
silent and records nothing (a copy shares the sound queue, and would play its
own frame over the real one). Going down the screen runs the copy on from where
it is; going back up starts the frame again, because nothing can be
un-executed. A downward sweep of all 192 lines costs 59µs and an upward one
13ms, so no cleverness is needed.

**Race the Beam runs at five seconds a frame and fades the picture behind the
beam.** A pixel is at full brightness the moment the beam draws it and at the
slider's floor — half, by default — just before the beam comes round to draw it
again, so the trail down the screen says how long ago each part was drawn. At
fifty frames a second that would be a flicker nobody can see, which is why it
comes with the speed rather than as a switch of its own. The oldest part of the
picture is the line just *below* the beam, not the bottom of the screen: a
quarter of the frame is spent below the display in the border and the sync.
Hovering with the cursor is a different thing and is called Cursor Beam.

**Writes are marked by which side of the beam they landed on**, while Race the
Beam is on. Red says the beam had already been over that byte, so the change
will not be seen until the next frame — which is what a flickering sprite is;
green says it will be shown this frame. The colour blends into the colour the
write is going to show over two seconds of the user's time, which is worked out
in T-states from the speed the machine is being run at, so it is the same
number of instructions however the speed is changed.

A marked cell is drawn from the display file rather than from what the beam put
out: the mark is on its way to the colour the write will show, and for a late
write there is nothing on the screen yet. The mark itself lasts until the beam
goes over the byte — not until its colour has finished blending, or the picture
would pop back to the old content two seconds after every late write. The beam
crossing clears it, and by then the painted frame holds the same bytes anyway,
so nothing moves. Marking costs a branch on every write to the display file,
which is why `bus.tints` is `None` at every other time.

**The picture is the frame being painted while the machine crawls**, and the
last finished frame otherwise. Holding the finished frame is what stops a
repaint catching a picture half drawn at full speed; under slow draw it would
freeze the picture for the seconds an emulated frame takes while the beam
crawled over it. Both are what the ULA put out, never the display file as it
stands.

**The ULA snows when I points at RAM it is reading.** After every opcode fetch
the CPU puts I:R on the address bus, and the ULA — which tells the CPU's
accesses from its own by watching that bus — is disturbed by it. Two different
things happen, depending on where the last T-state of the M1 falls in the ULA's
eight-T-state cycle
([redcode's notes](https://github-wiki-see.page/m/redcode/ZXSpectrum/wiki/Snow-effect)):
on its third, the pixel fetch is made from the wrong address, bits 6..0 of R
standing in for the low seven of it — so snow is made of the program's own
graphics, from the same part of the screen, and the colours stay right. On its
fifth, the second cell of the pair is not fetched at all and the first goes out
again in its place: the "double effect", an eight-pixel bar repeated.

The address has to be RAM the ULA reads: $4000-$7FFF on a 48K, and on a 128K
also $C000-$FFFF when an odd page is banked there. The +2A/+3 drive the bus
themselves and do neither. The CPU only calls `Bus::refresh` when I could point
at either range, since it is on the busiest path there is — with the test a
screenful of NOPs runs a fifth slower, and real code about seven per cent.

**The ULA's fetch cycle starts a T-state after contention does.** The reference
FAQ says the first byte is displayed at 14336 while its own contention table
starts at 14335, and `snow.tap` settles it: its interrupt handler fills the
display with a solid field of NOPs after `LD R,A`, so every eight-T-state block
gets an M1 in the same place, and it lands on the snow window only with the
cycle anchored at 14336. All 3,072 blocks snow, which is what a program written
to demonstrate snow should do. Against that, `ula128.tap` sets I to $FE on a
128K and lands on the other parity, so it shows nothing here — worth checking
against a real machine before trusting the anchor further.

`tests/reference_48k.rs` holds the
[48K reference](https://worldofspectrum.org/faq/reference/48kreference.htm)'s
own numbers: the 69888-T frame, the 224-T line, the contention table T-state by
T-state from 14335, the four I/O patterns worked through by hand, and which
addresses are contended. All of them already matched; the file is there so they
go on matching.

**Sync is treated the way a television treats it.** A pulse held for at least a
line is a vertical sync and pulls the picture back to the top; a shorter one is
a line sync; one that arrives far too early is not a sync at all, so the beam
stays where it is and is merely blanked — which is what draws the ZX81's
loading pattern. The screen is painted over rather than wiped, so a display
that keeps restarting looks like one.

**An RZX recording is played back by fetch count, not by time.** A frame of a
recording is a number of *opcode fetches* — a prefixed instruction is two or
more — and every IN takes the next byte the recording holds instead of reading
the hardware. Counting whole instructions instead runs past the end of every
frame and reads input that was never recorded; that showed up as thousands of
"short reads" until it was fixed. The frame interrupt is raised at the
recording's frame boundary rather than by the T-state count, and the video
frame ends there too — nowhere else. The T-state clock used to end one as well,
so a recorded frame holding more instructions than fit in a frame of the
machine's own time painted the screen twice: once on the clock, once at the
boundary. Space Harrier's recording does that from about frame 1,100 on — its
frames hold 9,400 to 11,600 fetches where a video frame at the ~11 T-states a
fetch this game runs at holds about 6,300 — and at five seconds a frame the
second painting reads as flicker. Manic Miner's recording never does: its
boundaries land within a dozen T-states of the clock. What is not settled is
why those frames are so long — whether the game misses interrupts on real
hardware, in which case the ULA really did paint twice, or the recording was
made on something whose clock ran differently. The lengths are not near whole
multiples of a frame, which fits neither story exactly. In slow motion a
frame is played in pieces — Race the Beam runs at five seconds a frame, and
whole frames at a time would stand still for five seconds and then jump — but
only in slow motion: a 128K frame is a shade longer than a host frame, so a
budget-based rule split every frame at full speed for nothing. A frame is not
begun, and its input not put in front of the machine, until there is budget to
run some of it. `run_fetches` reports what it ran rather than what was asked
for: instructions run whole, so a prefixed one overshoots, and clamping the
count to the budget loses the overshoot on every piece. A recording that
asks for more input than it holds has come adrift, and the toolbar says so
rather than letting the picture look authentic. `recordings/` is gitignored for
the same reason as `tapes/`.

**Fastload answers the ROM instead of playing to it.** A tape loads at
1,500 baud however fast the machine is run — the pulses take as long as they
take — so Max speed shortens the wait and cannot remove it. `src/flashload.rs`
removes it: when the machine calls LD-BYTES at $0556, the next block with the
flag byte it asked for is copied into memory, the registers are left as the
routine would have left them, and the return is taken there and then. Border
Break loads in two frames instead of 2,605, ending in the same place with the
same screen.

Nothing else can be handed over: a game's own loader reads the tape itself and
often decrypts each byte as it arrives. What the emulator knows about those is
which sampling loop they count pulses in — seven of them, read off the tapes
rather than off a list, in `flashload::CORES` — so the tape window can say who
is reading. Speedlock, Bleepload, Microsphere, Paul Owens, Alkatraz, Dinamic,
the Search loader and its variant, Hewson's and Digital Integration's are all
recognised, and thirteen games are checked against their tapes.

It only works where the ROM is doing the loading, and only where that ROM is
the one paged in — the eight bytes at $0556 are checked against the routine
rather than assumed, since a 128K is running its own ROM there and a program
may put anything it likes at that address. A game with a loader of its own is
counting its own pulses and cannot be helped, which is why switching this on
switches Max speed on with it.

**Tapes are played as pulses**, never decoded, so turbo loaders and the ZX81's
own format work through the same path. Each machine has its own deck, because a
tape is timed in the T-states of the machine playing it and the two clocks
differ.

**Nothing is written against an address until somebody says so.** The Call
flow window's detectors are asked to look — pressing *Main game loop* is what
starts the machine being watched — and what they find is offered with a score
and a reason. It goes into the notes only when *Label it* is pressed. The
emulator used to guess at everything continuously and write its guesses in; it
does not any more.

**AutoDoc guesses, and says so.** `src/autodoc.rs` reads the code from where
the machine is, names every routine that gets called, and applies ordered rules
to what each one touches — ports, screen ranges, ROM calls, block moves. It
hedges in the wording when the evidence is thin ("possibly a protection
check"), and a rule that always has an answer would be worse than none, so code
with no tell is left unnamed.

**What a routine did beats what its code looks like.** `src/observe.rs`
attributes every write, port access, instruction and loop iteration to the
routine on top of the call stack, which `src/flow.rs` works out from what the
CPU did rather than by decoding opcodes (the profiler uses the same
classifier). AutoDoc prefers those measurements to its static rules and quotes
the numbers — "writes 6144 bytes into the display file per call, every frame" —
so the user can check the claim. Port $FE is the border, the beeper and the MIC
socket at once, so it proves nothing on its own: the beeper is told apart by
hammering the port while writing almost nothing to memory. Watching costs a
branch on every access, so it only runs while AutoDoc is on.

**Anything else known goes in a file, not in the source.** `tools/rom-symbols.py`
turns a disassembly from
<https://github.com/ZXSpectrumVault/rom-disassemblies> into a symbol file in
the preferences directory. They are somebody else's work, so they are
generated rather than committed.

Three conventions are in use across those files and the converter reads all
three: `;; NAME` then `Lxxxx:` for the 48K and ZX81, a title between rules of
dashes for the 128K, and dZ80's `.lxxxx` with `defc NAME=$xxxx` for the +3.
A title only names a label within a dozen lines of it, or the pages of prose
at the top of a file end up naming the reset vector.

**A paged machine needs a symbol file per ROM.** The 128K's ROM image is two
16K ROMs and the +3's is four, each addressed `$0000-$3FFF` in its own right,
so a name means nothing without knowing which is in: they are
`symbols-128-rom0.txt` and so on, and only the one paged in at `$0000` is
read. `<name>.symbols.txt`
beside the notes takes `ADDR name ; comment` lines and `bytes …` signatures. A
full ROM disassembly cannot be shipped here and a signature that cannot be
checked should not be invented, so the mechanism is in the emulator and the
knowledge is the user's to supply.

**`tools/skool-symbols.py` reads the SkoolKit game disassemblies** at
<https://github.com/mrcook/zx-spectrum-games> — 6,677 routines across fifteen
games, 5,736 of them described. SkoolKit takes addresses either way, `$8000`
or `32768`, and four of those games are written in decimal: reading them as hex
finds nothing at all and says so quietly, which is how it looked at first.
Its `#R$8000` cross-reference markup means nothing in a text file and is taken
out, along with titles that only say "Routine at 32768".

**Signatures come from the ROM in the machine, not from a table somebody
typed.** Games copy ROM routines into RAM; the same bytes hash the same
wherever they land, so a copy is named after the original and said to be one.
Only the first twelve bytes are hashed, which stays in front of most absolute
addresses inside a routine. Anything else worth recognising can be added by
whoever has the code in front of them to check it against — inventing
signatures that cannot be verified would be worse than having none.

**A guess is marked with `@` and never overwrites a person.** Guesses are kept
in the same notes file as everything else, written as `@label ; @comment`. A
later guess replaces an earlier one — by then the program may have unpacked
itself, and the second look is the better one — but nothing the user typed is
ever replaced, and typing over a guess makes it theirs. Guesses are drawn in
the dim colour so the two are told apart at a glance.

**Listings are annotated in a file beside the tape.** Labels and comments
typed into the disassembly go to `<name>.zxrs.txt` next to the tape, or next to
the ROM when the deck is empty — plain text, one line per address, so it can be
read, edited and diffed without the emulator, and a badly edited line is
skipped rather than throwing the file away.

**The cassette is rendered from the SVGs** in `designs/` by `src/svg.rs`, a
deliberately small renderer covering only what the artwork uses: groups with
matrix transforms, lines and cubic curves, rectangles, circles, solid and
linear-gradient fills, strokes, and the even-odd rule that makes the shell's
window a hole. It is not to grow into a general implementation. Re-exporting
the artwork changes the emulator without anyone redrawing anything in code.

## egui, learned the hard way

- **A viewport builder's geometry must be supplied every frame.** A window that
  has not been drawn for a while is retired and rebuilt, and a builder without
  a size gets the window system's default — which is why the debug windows used
  to open centred at 800x600 and resize on alt-tab.
- **Minimum and maximum sizes have to be sent as viewport commands**, not left
  to the builder, for the same reason. That is how the tape window's width is
  fixed rather than corrected after the fact.
- **A window can be laid out more than once for a frame.** Anything accumulated
  per draw — the cassette's hubs turning, for instance — must come from the
  clock, or it runs at two or three times the intended rate.
- **Anything the debug windows need must happen in `eframe::App::logic`, not
  `ui`.** eframe skips `ui` while the main window is not visible — which on
  macOS includes switching away from the application — and then prunes every
  viewport that frame did not declare, destroying the windows. `logic` runs
  either way. `App::draw` still does both, for the tests.
- **Accessibility labels are not where you expect.** A plain label's text and a
  combo box's selection are in the node's `value`, not its `label`. Tests query
  by value for those.
- Rasterising is slow enough to cache: the cassette is rasterised once per size,
  and the cogs once, then turned by rotating the quad they are drawn on.

## Verifying the running app

Screenshots are unreliable — the window loses focus to whatever the user is
doing. Prefer, in order: a headless test that writes a PNG (`tests/svg.rs` has
one); querying window geometry through System Events; a screenshot last. State
plainly in the report which of these was used.
