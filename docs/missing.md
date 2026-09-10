# What other emulators have that this one does not

An audit taken on 10 September 2026, against Fuse, ZEsarUX, Spectaculator,
CSpect and the wider 8-bit emulator crowd. Every absence below was checked
against this source tree rather than remembered — `grep` for the port, the
format, the file extension — so the list says what is not there rather than
what nobody has got round to mentioning.

It is a description of the gap, not a plan. Nothing here is promised, and
anything picked up should be taken on its own merits: several of these are an
afternoon and one of them is a month.

All five this list opened with have since been done: `.szx`, `.scr`, the
joystick interfaces, gamepads, and saving to tape (**Blank** in the tape
window records what the machine sends through MIC). What is left is below.

## The ones you would feel first

**No divMMC/divIDE with esxDOS.** This is how people load things on real
hardware now: an SD card and a file browser. The biggest job on the list — an
IDE or SPI interface plus the esxDOS ROM's hooks — and the one that would make
the emulator feel current.

## Peripherals

| | |
| --- | --- |
| **Kempston mouse** | Small, next to what is already here. Art packages and a few games use it. |
| **ZX Printer / Alphacom 32** | `COPY` does nothing. The output is a bitmap and a window to show it in. |
| **Interface 2 cartridges** | `.rom` files, sixteen games, and a joystick port. Perhaps forty lines of paging. |
| **The Fuller's joystick** | Its sound chip is emulated; the joystick in the same box is not. |
| **Interface 1's RS232 and ZX Net** | Deliberately stubbed — nothing is on the other end of $F7 — but the network is what made microdrives interesting in a classroom. |
| **Beta 128 / TR-DOS** | `.trd` and `.scl`. Note that `roms/trdos.rom` is already in the ROM directory with nothing that can use it. |
| **+D and DISCiPLE** | `.mgt` images, and the snapshot button that made them popular. |
| **Opus Discovery** | The third of the disk systems, and the rarest. |

## Machines

16K, which some programs check for; the Spanish 128K; the +2B; **Timex
TC2048 and TS2068**, whose SCLD brings 512×192 and the hi-colour modes;
**Pentagon 128**, whose ROM is also already in the directory; Scorpion 256; and
the **ZX Spectrum Next**.

**ULAplus** deserves its own line: a 64-colour palette over any of those, small,
well specified, and used by a steady trickle of modern releases.

## Recording and media

RZX in and out is done, which is the hard one. What is missing is everything
else: video or GIF capture, WAV capture of the sound, WAV and CSW tape *input*
— real tape audio, as opposed to the decoded formats — and PZX.

On the ZX81 side: ZonX AY sound, Chroma 81 colour, and the 64-column hack.

## The developer side

This is where the emulator is ahead of most: call flow, AutoDoc, the observer,
step-back, and the MCP server have no equivalent in Fuse or Spectaculator. What
the others still have that this does not:

- an **in-app assembler and monitor** (ZEsarUX and CSpect both);
- **conditional breakpoints** — the watches here are on kinds of event, plus one
  address range;
- **trace to a file**;
- **cheats**: `.pok` files, and a RAM search in the Debugger window. Half of
  that exists already but only over MCP, where `find_bytes` and `changed_since`
  are exactly the two halves of a cheat search.

## Where to start

divMMC. It is the biggest job here and the one that would change how the
emulator is used most.
