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

**Racing the beam shows two different things either side of it.** Behind the
beam is what the ULA actually painted — the border and attributes as they were
at each T-state, which is where a raster effect lives. Ahead of it is what the
display file holds *now*, drawn plainly with one border colour, and dimmed to
say it has not been painted yet. So the screen carries the machine's execution
on one side and the program's intention on the other. It used to show the
previous frame ahead of the beam, which is what a television does and tells you
nothing about the program.

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
recording's frame boundary rather than by the T-state count. A recording that
asks for more input than it holds has come adrift, and the toolbar says so
rather than letting the picture look authentic. `recordings/` is gitignored for
the same reason as `tapes/`.

**Tapes are played as pulses**, never decoded, so turbo loaders and the ZX81's
own format work through the same path. Each machine has its own deck, because a
tape is timed in the T-states of the machine playing it and the two clocks
differ.

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
