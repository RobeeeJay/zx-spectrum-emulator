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
| **Currah µSpeech** | Fitted, not emulated |
| **RAM Music Machine** | Fitted, not emulated |
| **Multiface One / 128 / 3** | Fitted, not emulated |

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
shipped; put one at `roms/if1.rom` and the interface works.

The drives are a loop of tape running past a head at about seventy-six sectors
a second. The ROM finds its way round by waiting for a gap and then for sync,
so the tape has to move: its position follows the machine's clock rather than a
count of accesses, and a clock that goes backwards — a reset, a snapshot — is
not time passing. One bit selects one of eight drives by being walked down the
chain: a pulse on the motor line starts the first, and each further pulse moves
it one further along.

Ports `$E7` (data) and `$EF` (control and status) are decoded on the low bits,
as the interface does. `$F7` is the RS232 and network side, and nothing is on
the other end of it here.

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

**The Multifaces** need their own ROM, which cannot be shipped, and the three
differ from one another in how they page it and which ports they answer.
