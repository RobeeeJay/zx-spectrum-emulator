# What plugs into the back

The **Hardware** window is the list, and it says for each one what it is and
how far it is emulated. That distinction is the point of the window: a switch
that turns on nothing is worse than no switch, because it looks like the thing
is working.

| | |
| --- | --- |
| **Interface 1** | Emulated, once `roms/if1.rom` is there |
| **Fuller Audio Box** | Emulated |
| **Cheetah SpecDrum** | Emulated |
| **Multiface One** | Emulated, once `roms/multiface1.rom` is there |
| **Multiface 128** | Emulated, once `roms/multiface128.rom` is there |
| **Multiface 3** | Emulated, once `roms/multiface3.rom` is there |
| **Currah µSpeech** | Fitted, not emulated |
| **RAM Music Machine** | Fitted, not emulated |

What is plugged in — and which machine it is plugged into — is remembered
between launches, along with how many microdrives are on the chain.

## Interface 1 and the microdrives

The interface is an 8K ROM that is not in the machine's memory map until the
machine asks for it. It watches the address bus, and a fetch from `$0008` or
`$1708` — the ROM's error handler and its close-files hook, which is where a
program lands after a `LOAD *` or a `CLOSE` — pages its own ROM over the bottom
of the machine's. A fetch from `$0700` pages it out again.

Everything the microdrives do is done by that ROM, so **without one in the
socket the interface pages in nothing**, which is exactly what the hardware
does with an empty socket. The ROM is somebody else's copyright and is not
shipped; put one at `roms/if1.rom` and the interface works. It is the second
edition that is wanted — 8K, with ` 1983 Sinclair Research Ltd MJB ` at
`$16DC`.

**The ROM pages out one fetch later than it pages in.** `$0700` in the shadow
ROM is a `RET`, and that is how a microdrive routine hands back: it jumps
there, the `RET` runs out of the shadow ROM, and the ROM is gone by the time
the return address is fetched. Paging out before that byte is read runs
whatever the machine's own ROM holds at `$0700` — `$71` on a 48K, the middle of
an unrelated routine — and the Interface 1's initialisation goes round for
ever.

The status port `$EF` reads the **gap** on bit 2, high while the tape between
two blocks is under the head; **sync** on bit 1, low once the block's preamble
is; and the **write-protect** tab on bit 0, low for a cartridge that may not be
written. That is not a guess: the ROM's sector-finding loop at `$165A` waits
for eight reads with bit 2 set, then six with it clear, then for bit 1 to go
low, and its write test at `$136C` refuses when bit 0 reads clear. The motor
line is bit 0 of a write and it is **low** for a drive that is to run; it is
latched on the falling edge of the comms clock (bit 1) and walks one place down
the chain of eight each time.

**The tape moves as the ROM reads it, not on the clock.** A block is read with
`INIR` — 21 T-states a byte — where the tape itself hands over a byte every 170
or so; a tape running at its own speed gives the same byte to a dozen reads in
a row. On the hardware the interface paces the CPU; here the reads pace the
tape, which comes to the same thing from the ROM's side, and is what Fuse does.
A write to the control port puts the head at the start of the next block, since
that is what the ROM does when it has finished with one.

Ports `$E7` (data) and `$EF` (control and status) are decoded on the low bits,
as the interface does. `$F7` is the RS232 and network side, and nothing is on
the other end of it here.

### What has actually been run

`tests/if1_rom.rs` puts the real ROM in the socket and drives it from the
keyboard: `FORMAT "m";1;"newcart"`, a one-line program saved with `SAVE *`,
`NEW`, `LOAD *`, `LIST` — and reads `10 REM hello` back off the screen through
the ROM's own font. `CAT 1` of a 180-sector cartridge prints its name and 90K
free. A real cartridge — Hewson's, 200 sectors — catalogues as ten files with
1K free, which is what the emulator's own reading of it says as well.

## Cartridges

An MDR file is a cartridge written out sector by sector: 543 bytes each,
fifteen of header — a flag, the sector number, the cartridge name and a
checksum — and 528 of record: a flag, which record of the file this is, how
many of its 512 bytes are used, the file's name, and two more checksums. A byte
on the end says whether the write-protect tab has been broken off, and plenty of
files in the wild have not got it.

The arithmetic is **mod 255**, not mod 256. A sector whose bytes add to 255
checksums as nothing, and getting that wrong fails one sector in a few hundred
— which reads as a worn cartridge rather than as a bug.

A record is **in use when bit 2 of its flag byte is set**. Bit 1 is something
else, and testing that instead dropped ten of Hewson's records — the ones
flagged `$06` — out of the files they belong to. An empty record is all zeros,
which is what the interface's own `FORMAT` writes: flags, length, name and both
checksums, the lot. A blank cartridge made with anything else in those bytes
catalogues as having no room on it at all.

`FORMAT` leaves one sector holding the pattern of `$FC` it wrote to test the
tape with, which is why the ROM reports 89K free on a 180-sector cartridge
rather than 90, and why that one sector does not add up.

Cartridges are mounted the way disks are: **read-only**, **writing to a copy**,
or **writing to the file itself**, and the question is asked rather than
guessed at, because a program writes to the cartridge it loaded from. A
cartridge out of a zip is offered the first two only. A cartridge whose own tab
is broken off cannot be written to whatever is chosen, and the question says so.

The window draws a drive per cartridge with its lamp — on while that drive's
tape is running — the cartridge's label written in the same hand as the tape's,
and the loop of tape underneath: a mark per sector, lit where a file is and
amber where the head is. Where the tape window lists blocks, this one lists the
files, with a note when a sector does not add up: a cartridge that has been in
a drawer for forty years may well have one, and that is a fact about the
cartridge rather than about the reading.

## The two that make a noise

**The Fuller Audio Box** is a sound chip of its own — register select at `$3F`,
data at `$5F` — so a 48K with one fitted has one and a 128K with one has two.
Both go through the mixer.

**Cheetah's SpecDrum** is an eight-bit converter on `$DF` and nothing else: a
byte written to it is a sample, and a program feeds it drum sounds out of
memory as fast as it can. It idles at half scale, so silence is silence.

The port assignments are as documented rather than as measured here — there is
no hardware to check them against — which is worth knowing if something that
should be making a noise is not.

## The ones that are switches only

**The Currah µSpeech** holds its allophones in the SP0256's own ROM as filter
coefficients. Playing them means synthesising the chip rather than replaying
samples, and neither the ROM nor the synthesiser is here.

**The RAM Music Machine**'s port map has not been checked against a reference
here. Inventing one would make a switch that looks as though it works, which is
the failure this window exists to avoid.

## The Multifaces

Romantic Robot's box does one thing: pressing its red button pulls the CPU's
`/NMI`, and the interface pages its own 8K of ROM and 8K of RAM over the bottom
16K in time for the fetch from `$0066`. Whatever was running stops where it
stood with every register still in it, and the ROM's menu can save the lot —
which is how a game with no save game got one, and where most of the snapshots
in the archives came from. The button is in the Hardware window; that is the
whole of the box's front panel.

The paging is hung on the **fetch from `$0066`**, not on the CPU taking the
interrupt: the latch the button sets is clocked by /M1 with that address on the
bus, which is the same mechanism as the Interface 1's `$0008`.

Three models, and they do not agree on much:

| | pages in | pages out | saves to |
| --- | --- | --- | --- |
| **One** (48K) | `IN` with A7 set — $9F | A7 clear — $1F | tape, cartridge |
| **128** | A7 set — $BF | A7 clear — $3F | tape, microdrive |
| **3** (+2A/+3) | A7 **clear** — $3F | A7 set — $BF | tape, disk |

The One decodes `x001 xx1x` and the other two `x011 xx1x`, so the numbers in
the manuals are one port each out of a family. The 128 hands back a byte saying
whether bit 3 of the last write to `$7FFD` was set, and the 3 watches every
write to `$1FFD`, `$3FFD`, `$5FFD` and `$7FFD` and hands back whichever of the
four the address asks for: both need to put the machine's paging back before
they give it up. Getting the 3's two ports the same way round as the 128's —
the obvious guess — leaves the +3's own menu on the screen and no Multiface.

Two latches decide whether a press does anything. One is set by the button and
cleared by the fetch from `$0066`, so a press pages the ROM in exactly once;
the other is cleared by the button and set again by the next `OUT` to the
interface, which the ROM does on its way out. Press the button twice without
letting the menu finish and the second press does nothing, as on the desk.

All three can be on the back at once and one button serves them, as the
hardware daisy-chains: the last one that is ready takes it.

The port decoding and the latch behaviour are Fuse's `multiface.c`. The manuals
give one port number each and say nothing about which address lines are
decoded, so there was nothing else precise enough to work from.
`tests/multiface_rom.rs` is what says the reading is right: it presses the
button on a running machine of the right sort for each model and reads the menu
off the screen — `exit return save tool copy jump` over
`MULTIFACE 1 © Romantic Robot Ltd`, and the same for the other two — then
presses R and checks the machine comes back exactly as it was.

Watching one work is worth knowing about: the menu does not run with the box
paged in. The ROM puts a stub in the machine's own RAM and pages itself in and
out several times a frame, which is why the interface is usually *out* while
its menu is on the screen.
