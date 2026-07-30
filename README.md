# ZX Spectrum emulator

A ZX Spectrum emulator in Rust — 48K, 128K, +2A and +3 — with a cycle-accurate
Z80 core, beeper and AY sound, and a set of detachable debugging windows, built
on `eframe`/`egui`.

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

**Verified against zexdoc and zexall.** Both exercisers pass every test,
including the undocumented flag tests in zexall:

```
cargo run --release --bin zextest -- zexall.com
```

(`zexall.com` / `zexdoc.com` are not included; they are widely available, e.g.
in the `anotherlin/z80emu` repository under `testfiles/`.)

## Debug windows

### RAM access map

One pixel per byte of the 64K address space, 256 bytes per row, updated every
frame.

* **Green** — reads
* **Red** — writes
* **Blue** — instruction fetches

Each of the three can be switched off on its own, so you can look at writes
alone, or separate code from data by hiding reads. Every access sets its pixel to
full intensity and fades from there, with a separate fade rate per channel and an
overall gain. Video RAM and
any detected back buffer are outlined, and a white line marks the row PC is in.
Hovering reports the address, its current value and its lifetime read/write
counts; clicking jumps the debugger to it.

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

`tests/ui_tape.rs` checks that a loaded tape stays stopped and emits no pulses
until Play, that the opt-in auto-play still works, and that the block list
scrolls the playing block into view when playback moves on but leaves the list
alone when following is off. `tests/ui_ram_map.rs` covers the read/write/execute
toggles and their fade sliders.

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
| `src/audio.rs` | AY-3-8912 and the beeper/AY mixer |
| `src/audio_out.rs` | cpal output device |
| `src/disasm.rs` | Disassembler |
| `src/ui/` | Main window, RAM map, debugger, back-buffer and tape windows |
| `src/bin/zextest.rs` | CP/M harness for zexdoc/zexall |
