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

**There is no network.** No crate can be added that is not already in the lock
file. That is why `src/svg.rs` exists rather than a dependency, and why the
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

**Sync is treated the way a television treats it.** A pulse held for at least a
line is a vertical sync and pulls the picture back to the top; a shorter one is
a line sync; one that arrives far too early is not a sync at all, so the beam
stays where it is and is merely blanked — which is what draws the ZX81's
loading pattern. The screen is painted over rather than wiped, so a display
that keeps restarting looks like one.

**Tapes are played as pulses**, never decoded, so turbo loaders and the ZX81's
own format work through the same path. Each machine has its own deck, because a
tape is timed in the T-states of the machine playing it and the two clocks
differ.

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
