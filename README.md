# ZX Spectrum emulator

A ZX Spectrum emulator in Rust — 48K, 128K, +2A and +3 — with a cycle-accurate
Z80 core, beeper and AY sound, and a set of detachable debugging windows, built
on `eframe`/`egui` with the **wgpu** renderer (Metal on macOS, Vulkan or DX12
elsewhere).

The OpenGL renderer is deliberately not used: on macOS it goes through glutin,
whose Cocoa backend panics with *"context to have a current view"* when one of
the extra windows loses its view — which this emulator, with four of them, runs
into. If you are building somewhere wgpu cannot find a backend, swapping the
`wgpu` feature for `glow` in `Cargo.toml` restores the OpenGL path.

```
cargo run --release             # 48K
cargo run --release -- --128    # 128K
cargo run --release -- --plus2a # +2A
cargo run --release -- --plus3  # +3
```

Each debug window is a real OS window (an egui *viewport*), so you can drag them
to a second monitor and watch them while the emulator runs.

Under the menu bar is a toolbar with everything you need in one click: the
machine selector (`48K 128K +2A +3`, greyed with "(no ROM)" when the image is
missing), a **Load…** button that takes tapes, snapshots and ROMs, **Reset**, and
toggles for the four debug windows. The window title always names the machine in
use, and failures — a missing ROM, an unreadable tape — appear in red in the
status bar rather than being swallowed.

## What it does

**Cycle-accurate Z80.** Timing is not counted per instruction but per bus cycle:
the CPU tells the machine how many T-states each fetch, read, write and internal
cycle takes, and the ULA applies memory contention at the exact T-state the
access happens. That includes the 48K contention pattern `6,5,4,3,2,1,0,0` over
the 128 T-states of each pixel line, the four I/O contention cases, and the
correct extra idle cycles for `INC (HL)`, `(IX+d)` addressing, `ADD HL,rr`, the
block instructions, conditional returns, and so on.

The core implements the full documented instruction set plus the undocumented
ones (`IXH`/`IXL`, `SLL`, the `DD CB d op` register copies), MEMPTR/WZ, the Q
register that drives `SCF`/`CCF`'s F3/F5 behaviour, and interrupt modes 0/1/2
with NMI.

**Verified against real hardware.** HALT2INT (Mark Woodmass) measures the R
register at the moment an interrupt is taken after a HALT reached at exact
T-states, which pins down interrupt timing, memory contention, the halt state
and the floating bus at once. The emulator reproduces the output of a real 48K
exactly, in both timing variants — the reference photographs are in `tapes/`,
and `tests/halt2int.rs` runs the tape and compares all thirty values plus the
test's own Early/Late verdicts.

Two things that test caught, both now fixed: the halt state performs its M1
cycles at PC — the byte *after* the HALT — so a HALT at `$7FFF` refreshes from
the uncontended `$8000` while one at `$4000` is contended on every cycle; and
the floating bus is sampled at the start of the IORQ cycle and reads back `$FF`
for the four idle T-states in each group of eight.

**Early and late timing.** Real 48K machines came in two variants, one running
the display a T-state later relative to the interrupt. **Machine ▸ Late timing**
switches between them; the emulator matches the reference photograph for each.

**Race the beam.** With this on, hovering the picture shows the frame
half-drawn: everything up to the cursor is what the ULA has put out so far, and
the rest of that raster line and the lines below it are the *previous* frame,
drawn a third darker. The cursor position is converted to a T-state with the
same arithmetic the renderer uses, so the split lands exactly where the beam
would be. It works while paused, which makes it a way to read a frame's timing
by hand: park the emulator on a breakpoint and sweep the cursor to see what had
been drawn at any point in the frame.

**Overscan.** The toolbar's **Overscan** toggle chooses how much border to
draw: on (the default) shows the whole 384x304 area the ULA puts out, which is
where border-art demos work; off crops it to 304x240, roughly what a television
showed. Cropping only changes how much border surrounds the picture — every
pixel stays exactly where it was.

**Border art.** The border is rasterised by T-state rather than by scanline —
the ULA puts out two pixels per T-state, so a program writing port `$FE` in a
tight loop can draw in it at that resolution. Border Break (introspec/gonzy)
renders **pixel for pixel identically** to a real 48K: all 116,736 pixels of
`tapes/bb.png` match, border included.

Getting there needed three things beyond per-line sampling: the ULA fetches two
T-states ahead of the pixels it is emitting, so a border write lands on screen
slightly before the fetch clock that contention counts; a frame that has only
been drawn part-way still shows the *previous* frame below the point the ULA has
reached, rather than going black; and the border log has to be big enough for
the hundreds of writes per frame that this kind of code makes.

**Verified against zexdoc and zexall.** Both exercisers pass every test,
including the undocumented flag tests in zexall:

```
cargo run --release --bin zextest -- zexall.com
```

(`zexall.com` / `zexdoc.com` are not included; they are widely available, e.g.
in the `anotherlin/z80emu` repository under `testfiles/`.)

## Debug windows

### RAM access map

One pixel per byte, 256 bytes per row, updated every frame. Two views:

* **Address space** — the 64K the CPU sees right now, with each 16K slot
  labelled with what is paged into it (`$C000 RAM7`), video RAM and any
  detected back buffer outlined, and a white line on the row PC is in.
* **All memory** — every RAM bank and ROM page the machine has, stacked as 16K
  blocks. Banks that are *not* currently paged in are still there, greyed and
  marked "paged out"; the ones that are get a bright outline saying which
  address they answer to (`RAM7 → $C000`), and the bank the ULA is displaying
  is marked "(screen)" with its display file outlined. On a 48K only the three
  reachable banks are shown.

Heat is recorded per physical location rather than per address, so a bank keeps
its history while it is paged out, and two banks that take turns at `$C000` do
not smear into each other.

* **Green** — reads
* **Red** — writes
* **Blue** — instruction fetches

Each of the three can be switched off on its own, so you can look at writes
alone, or separate code from data by hiding reads. Every access sets its pixel to
full intensity and fades from there, with a separate fade rate per channel and an
overall gain. Video RAM and
any detected back buffer are outlined, and a white line marks the row PC is in.
Hovering reports the bank, the offset within it, the address it answers to (or
"not paged in"), the byte's value and its lifetime read/write counts; clicking
jumps the debugger to it.

### Debugger

* Live disassembly around PC, aligned to real opcode boundaries (it searches
  backwards for a start address that tiles exactly onto PC, so the lines above
  the current instruction are not garbage).
* All registers including the shadow set, `IX`/`IY`, `I`, `R`, `IM`, `IFF1/2`,
  `WZ`, and the individual flag bits.
* **Pause / Run** (F5), **Step into** (F7), **Step over** (F8), **Step out**.
  Step over runs `CALL`, `RST` and the repeating block instructions to
  completion; step out runs until SP rises above its current value.
* Breakpoints: click a disassembly line to toggle one, or type an address.
* Speed presets from 1% to 2000%, plus a logarithmic slider.
* A hex/ASCII memory dump with shortcuts to follow HL or SP.

### Back buffer

Detection, preview and slow-motion controls for screens that are built outside
video RAM.

### Tape

Block list, transport and an oscilloscope for the loaded tape — see below.

### Profiler

**Start** and **Stop** record a run. Each run appears in a list with the local
date and time it started, and gains its length (wall-clock, plus the emulated
time covered) when stopped; runs are kept so you can compare them.

Next to the list is a bar graph of where the time went, one bar per function,
longest first. Each bar is labelled with the function's entry point in hex and
shows its share of the run, its time and how many times it was called; hovering
adds whether it is in ROM or a RAM bank, both time figures, and how deeply it
nested. **Clicking an entry point opens the disassembly of that function** in
the debugger.

Calls are spotted from what the CPU did rather than by decoding opcodes: a call
is any instruction that leaves SP two lower with the address of the following
instruction on the stack, which catches `CALL`, `RST` *and* interrupt
acceptance, so interrupt handlers are profiled like anything else. A return is
any instruction that pops the address it jumps to. Each function gets both
**self** time (excluding its callees) and **inclusive** time (entry to return),
switchable with **Rank by**; anything still on the stack when you press Stop
keeps the time it had spent, and time outside any call is reported separately.
While recording, the window shows the live call depth and which function the CPU
is in.

## Watching the screen being drawn

**Slow draw** parks the CPU once it has written a set number of bytes to the
watched area during the current host frame, so a screen that normally appears
instantly assembles over several seconds. Set it to 8 writes/frame and you can
watch a routine fill the display byte by byte; the emulator still renders 60
times a second, so the picture updates smoothly as it fills.

What is "watched" is configurable:

* **video RAM** — `$4000`–`$5AFF`, the normal display file.
* **back buffer** — the region found by the detector, or one you set by hand.

### Back-buffer detection

Plenty of programs assemble a frame somewhere in ordinary RAM and then blit it
into `$4000`, which means the interesting drawing never touches video memory.
The detector looks for that:

* It counts writes per 256-byte page over a 25-frame window and looks for runs
  of at least 24 pages (6144 bytes — one bitmap's worth) outside video RAM.
* Whenever a write into video RAM immediately follows a read from somewhere
  else — the signature of `LDIR`, a stack blit or unrolled `LD` copies — the
  source page's score goes up. A copy is worth 32 plain writes when ranking
  candidates, so a buffer that is actually flipped to the screen beats one that
  is merely written to.
* The result is reported with a confidence figure, and a strong copy signal on
  its own is enough even without a long contiguous run.

The back-buffer window renders the detected region as if it were video RAM, so
you can watch the *off-screen* image being built — and with slow draw watching
the back buffer, you can watch it being built a few bytes at a time. Set an
address manually if the heuristic guesses wrong.

Running the emulator with no ROM demonstrates all of this: the built-in demo ROM
(`src/demo_rom.rs`) builds an animated 6912-byte screen at `$8000` and `LDIR`s
it into `$4000`, and the detector finds it within a couple of seconds.

## Tapes

**File ▸ Load tape…** opens `.tzx` and `.tap` files. TZX is a pulse-level
format, so the player does not decode bytes: it turns blocks into a stream of
pulse lengths in T-states and drives the EAR bit through port `$FE`, exactly
like a real tape feeding the ULA. Ordinary ROM loading, turbo loaders and
custom pulse schemes therefore all work through the same path — nothing is
trapped or faked.

Load a game the way you would have in 1983: type `LOAD ""`, press ENTER, then
press Play — the tape does not start on its own. **Boost speed while playing** (on by default) runs the CPU at 8x
while the tape moves, so a 48K game arrives in seconds rather than minutes.

Supported TZX blocks: standard speed ($10), turbo ($11), pure tone ($12), pulse
sequence ($13), pure data ($14), direct recording ($15), pause and stop-the-tape
($20), group start/end ($21/$22), jump ($23), loops ($24/$25), call sequence and
return ($26/$27), stop-if-48K ($2A), set signal level ($2B), and the
informational blocks ($30–$35, $5A). CSW and generalized-data blocks ($18/$19)
are skipped over rather than played.

### Tape window

* **Block list** — every block with its type, size and, for standard blocks, the
  decoded ZX header (`Program "JETPAC"`, `Bytes "JPSP"`, …). Click any block to
  move the tape straight to it. The block being played is highlighted and kept in
  view as the tape advances; **Follow playing block** turns that off if you want
  to browse the list while it runs.
* **Progress** — one bar for the tape as a whole and another for the block
  being played, which names it, shows the percentage through it and how much
  playing time is left. The position comes from where the pulse generator has
  got to rather than from the clock, so it stays right after a seek or a pause.
* **Transport** — Start, Rewind, Play/Pause, Stop, Fast forward. Rewind and fast
  forward step backwards and forwards through the tape's sections, skipping the
  informational blocks that make no sound. A tape loads **stopped**, like a real
  one: press Play when the loader is waiting. (**Play on load** turns that off if
  you would rather it started immediately.)
* **Oscilloscope** — the EAR waveform as it plays, drawn from the player's edge
  log so it is exact rather than sampled. The sweep is edge-triggered (rising by
  default, selectable, or free-running) so the trace stands still instead of
  scrolling; the trigger point is marked at the left edge and the threshold is
  drawn across the middle. The sweep width goes from 50 µs — individual pulses
  of a turbo loader — up to 40 ms.

## Preferences

A preferences file is created the first time the emulator runs, in the usual
place for the platform:

| Platform | Location |
| --- | --- |
| macOS | `~/Library/Application Support/ZX Spectrum Emulator/preferences.toml` |
| Windows | `%APPDATA%\ZX Spectrum Emulator\preferences.toml` |
| Linux and friends | `$XDG_CONFIG_HOME/zx-spectrum-emulator/preferences.toml`, or `~/.config/…` |

It also keeps the position and size of every window, the display scale and the
overscan setting, written when the emulator closes and again a couple of
seconds after anything settles, so a crash does not lose the layout.

It is a TOML-compatible `key = "value"` file, meant to be readable and editable;
keys the emulator does not know about are left alone when it saves. Set
`ZX_SPECTRUM_CONFIG_DIR` to put it somewhere else.

What it remembers so far is where files came from: **open a ROM and the ROM
picker starts there next time**, the same for tapes and snapshots, each tracked
separately so a tape does not send you looking for ROMs.

Opening a ROM also **scans that directory for other ROMs** and adopts the ones
for machines you have no image for, recognised by size (16K, 32K, 64K) with the
file name breaking ties — so pointing at one `128.rom` typically lights up the
48K, 128K and +3 buttons at once, and says which files it found. A ROM you have
already loaded is never replaced by a scanned one. That directory is scanned
again at the next launch, so the machines stay available between sessions.

## ROMs and snapshots

The 48K ROM is copyrighted and not included. Drop one at `roms/48.rom` (or use
**File ▸ Load ROM…**) and it is picked up at startup; without one, the built-in
demo ROM above runs instead.

**File ▸ Load snapshot…** loads `.sna` and `.z80` (v1/v2/v3) for 48K, 128K and
+3. Snapshots carry their machine type, so loading one switches the emulator to
the machine it needs first (provided that ROM is there); `.z80` v3 also restores
the `$1FFD` latch on a +3.

Files can also be named on the command line, dispatched by extension:

```
cargo run --release -- "tapes/Jetpac (1983)(Ultimate Play The Game)[16K].tzx"
```

## Sound

Both machines are audible. Sound is generated on the emulated clock, not on a
timer: the mixer integrates the output level over each sample period, so a
beeper edge lands in the right sample even when it happens mid-instruction.

* **Beeper** — bit 4 of port `$FE`. Bit 3 (MIC) and the EAR input are mixed in
  quietly as well, which is why you can hear a tape loading.
* **AY-3-8912** (128K) — three square-wave channels with 12-bit periods, the
  17-bit noise generator, all sixteen envelope shapes, and the logarithmic
  volume table. It runs at half the CPU clock and is averaged over each output
  sample rather than point-sampled, which keeps high tones from aliasing.

The tape's EAR line is mixed in at the T-state each edge happens, not sampled
whenever the CPU last looked at the port — otherwise a burst of pilot tone
collapses into noise. The output is DC-blocked, as the real machine's is AC
coupled, so a held beeper level decays to silence instead of thumping, and gain
changes are ramped so muting never clicks.

The emulator paces itself against the sound device: each host frame it runs
slightly more or less emulation to hold the buffer near its target depth
(60 ms by default, adjustable under **Sound ▸**), which is what stops the queue
drifting into underruns or overflows — both of which crackle.

**Sound ▸** has an on/off switch, a volume slider (also in the status bar), the
buffer depth and the device details. By default sound mutes itself when the emulator is not
running at roughly normal speed, since fast-forwarding and tape boost otherwise
produce a shriek; turn that off if you want to hear it anyway. Samples go to the
host device through a quarter-second queue: run slow and it underruns quietly,
run fast and the oldest samples are dropped rather than letting latency grow.

## The 128K, +2A and +3

The toolbar's machine buttons, or **Machine ▸**, switch between the four models
at any time; each needs its ROM
in `roms/` (`48.rom`, `128.rom` — 32K, and `plus3.rom` — 64K, shared by the +2A
and +3). The same choices exist as `--48`, `--128`, `--plus2a`, `--plus3`.

What the 128K changes over the 48K:

* **Memory** — eight 16K RAM banks with `$7FFD` paging: bank at `$C000`,
  shadow-screen select, ROM select and the paging lock bit. Banks 5 and 2 stay
  at `$4000` and `$8000`.
* **Timing** — 3.5469 MHz, 70908 T-states per frame, 228 per line, first pixel
  at 14361. All of it is per-model, so contention lands correctly on both.
* **Contention** — follows the *bank*, not the address: any odd-numbered bank is
  contended wherever it is paged, so `$C000` is contended only when bank 1, 3, 5
  or 7 is there.
* **Display** — the ULA draws from bank 5 or bank 7 depending on the
  shadow-screen bit, and the main window follows it.
* **AY** — `$FFFD` selects a register (and reads it back), `$BFFD` writes it.

And what the +2A/+3 change again:

* **Four ROMs** instead of two. The ROM number is two bits: the low one from
  `$7FFD` bit 4, the high one from `$1FFD` bit 2.
* **Port `$1FFD`** — the second paging latch. Bit 0 selects *special* paging, in
  which there is no ROM at all and four RAM banks fill the address space, in one
  of four fixed layouts chosen by bits 1-2: `0,1,2,3` / `4,5,6,7` / `4,5,6,3` /
  `4,7,6,3`. Bit 3 is the disk motor.
* **Contention follows different banks** — the top four (4-7) rather than the
  odd ones, *and* uses a different delay sequence: `1,0,7,6,5,4,3,2` where the
  earlier machines use `6,5,4,3,2,1,0,0`.
* **Stricter port decoding** — `$7FFD` needs A15 low and A14 high, so unlike a
  128K a write to `$3FFD` does not disturb paging.
* **No floating bus** — the gate array drives the bus high, so unattached ports
  read as `$FF`.

The +3's disk controller is not emulated, so disk images will not load; the ROM
boots to its menu and reports "Drive M: available" (the RAM disk, which needs no
hardware), and tapes and snapshots work normally.

The debugger shows the live memory map (`$0000:ROM0 $4000:RAM5 $8000:RAM2
$C000:RAM7`), the `$7FFD` and `$1FFD` latches (including which all-RAM
configuration is active), which bank is being displayed, and an expandable AY
panel with the raw registers plus decoded periods, frequencies in
Hz, volumes, mixer routing and envelope settings.

## Emulated hardware

16K/32K/64K of ROM plus 48K or 128K of RAM, ULA display with FLASH and
per-scanline border capture (so raster bars show up), keyboard, beeper,
AY-3-8912 on the 128K and later, cassette input, and an approximate floating bus
on the machines that have one. Not emulated: the +3 disk controller, Interface 1
and the printer.

## Tests

```
cargo test --release
```

`tests/prefs.rs` checks the configuration directory for each platform's
convention (including `XDG_CONFIG_HOME` and the no-home case), that the file and
its directory are created on launch, that settings round-trip while hand-added
keys survive, that ROM scanning recognises images by size and prefers a name
that mentions the machine, and that opening a ROM or a tape remembers the right
directory without replacing ROMs already loaded.

`tests/race_the_beam.rs` checks that the beam splits the picture between the
two frames at the right place — including part-way along a single line — that
the older part is dimmed to exactly two thirds, that the border is split and
dimmed with it, and that moving the beam changes the picture with nothing
running.

`tests/border.rs` loads Border Break, waits for its border routine to start and
compares the rendered border against rows taken from the photograph of real
hardware, run-length encoded. It also checks that cropping the border shows
the identical picture with less around it, that alternating the border in a
tight loop produces stripes 38 and 62 pixels wide — the 19 and 31 T-states that
loop actually takes — and that a half-drawn frame keeps the rest of the previous
one.

`tests/halt2int.rs` boots a real 48K ROM, types `LOAD ""`, plays HALT2INT off
tape and reads the results back off the screen by matching character cells
against the ROM font, then compares them with the photographs of real hardware
in both timing modes. It also unit-tests the halt state's refresh address and
the return address pushed when an interrupt wakes it.

`tests/timing.rs` checks T-state counts for around 60 instructions against the
published timings, the interrupt/NMI acknowledge sequences, `EI`'s one
instruction interrupt shadow, and the contention behaviour at frame positions
where it should and should not apply. `tests/debug_features.rs` drives the demo
ROM and checks the heat maps, the fade, back-buffer detection, the slow-draw
allowance, breakpoints and the disassembler end to end.

`tests/sound_and_128k.rs` also covers the +2A/+3: four-ROM selection across both
paging ports, all four all-RAM configurations (including that `$0000` becomes
writable and stops being when ROM returns), bank-4-to-7 contention with the
`1,0,7,6,5,4,3,2` sequence, the stricter `$7FFD` decoding, the absent floating
bus, and that the real +3 ROM boots to its menu. It checks AY tone frequencies against `clock / (16 ×
period)`, the volume table, mixer routing, envelope ramps and holds, and
register masking; that the mixer emits exactly one sample period's worth of
audio per sample and that a 1 kHz beeper square wave comes out at 1 kHz; and, on
the 128K, `$7FFD` paging, the shadow screen, the paging lock, bank-following
contention, the AY ports, and that the real 128K ROM boots to its menu.
`tests/ui_menu.rs` drives the interface itself through `egui_kittest`: it clicks
the machine selector and every Machine menu entry and checks the model actually
changed, that a missing ROM reports a visible error instead of doing nothing,
that a switch leaves the new machine running, and that the toolbar's window
toggles and Reset work.

`tests/audio_quality.rs` guards the things that make sound go wrong: that a tape
tone keeps its shape (and pitch) even when the CPU never reads port $FE, that a
constant level decays to silence rather than sitting as DC, that muting ramps
instead of stepping, that an AY tone comes out at the frequency its registers
ask for with no sample-to-sample discontinuities, that the buffer pacing pushes
back in the right direction, and that the mixer clock never runs backwards.

`tests/ui_tape.rs` checks that a block's length adds up from its pilot, sync,
data and pause, that progress through it rises from nothing to everything
without going backwards, that a block making no sound reports none, that seeking
resets it, that a loaded tape stays stopped and emits no pulses
until Play, that the opt-in auto-play still works, and that the block list
scrolls the playing block into view when playback moves on but leaves the list
alone when following is off. `tests/ui_ram_map.rs` covers the read/write/execute
toggles and their fade sliders, that heat follows the bank rather than the
address (including that a paged-out bank keeps its history), that ROM pages are
tracked separately, the layout of the all-memory view for both a 128K and a 48K,
that each block reports the slot it is paged into, and what hovering returns for
a bank that is not paged in.

`tests/profiler.rs` checks time attribution against hand-counted T-states: two
leaf functions come out at exactly 30 and 50 T-states per call, a nested pair
splits 35 self / 85 inclusive correctly, interrupt handlers are counted once per
frame at 14 T-states each, a function that never returns still gets its time,
nothing accumulates while stopped, and the window's Start/Stop, run list and
bar clicks do what they should.

`tests/audio_device.rs` is a smoke test that opens the host audio device and
checks the callback drains samples; it skips itself when there is no device.

`tests/tape.rs` parses every tape in `tapes/`, checks the generated pulse trains
against the ROM's published timings (pilot, sync, bit lengths, turbo overrides,
loops and stop blocks), and then does the real thing twice over: it calls the
48K ROM's `LD-BYTES` routine at `$0556` and verifies the header it reads back
matches the tape byte for byte, and it types `LOAD ""` on the emulated keyboard
and loads a whole game from a real TZX. Tests that need `roms/48.rom` or
`tapes/` skip themselves when those are absent.

## Layout

| Path | What |
| --- | --- |
| `src/z80/` | CPU: state, decode/execute, ALU and flag tables |
| `src/machine.rs` | Memory map, ULA timing, contention, I/O, run loop |
| `src/tracker.rs` | Access heat maps and back-buffer detection |
| `src/screen.rs` | Display rendering |
| `src/tape.rs` | TZX/TAP parsing and pulse-level playback |
| `src/prefs.rs` | Preferences file and its platform-appropriate location |
| `src/profiler.rs` | Call profiler: call/return detection and time attribution |
| `src/audio.rs` | AY-3-8912 and the beeper/AY mixer |
| `src/audio_out.rs` | cpal output device |
| `src/disasm.rs` | Disassembler |
| `src/ui/` | Main window, RAM map, debugger, back-buffer, tape and profiler windows |
| `src/bin/zextest.rs` | CP/M harness for zexdoc/zexall |
