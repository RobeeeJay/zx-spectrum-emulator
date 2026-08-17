# Tapes and their loaders

What has been learned about getting a tape into the machine: the formats, the
deck, the ROM's loader, and the three ways of making it quicker.

## Tapes are played as pulses, never decoded

The deck makes edges and the machine reads them through port $FE bit 6. It
never looks at a block and decides what the bytes are, which is why turbo
loaders, speedlock variants and the ZX81's own format all work through the same
path without a special case for any of them.

**Each machine has its own deck.** A tape is timed in the T-states of the
machine playing it, and a 48K's clock is not a 128K's, so the deck belongs to
the machine rather than to the emulator.

Edges go to the sound mixer with the T-state they happened at, rather than the
level being sampled whenever the CPU happens to look — otherwise loading noise
comes out as aliased mush.

## Formats

**TAP** is nothing but standard blocks: a length, then a flag byte, the data
and a checksum. Everything in a .tap can be handed to the ROM.

**TZX** is a container of block types, of which the ones that matter are the
standard block ($10), the turbo block ($11), pure tone ($12), pulse lists
($13), pure data ($14), pauses and the flow-control blocks. Two traps found
the hard way:

- **The custom info block ($35) names itself in sixteen bytes, not ten.**
  Reading ten took the length from the last four characters of the name —
  spaces, $20202020 — and the parser reported "block claims 538976288 bytes but
  only 54052 remain". Everything behind that block was unreadable, which for
  Sidewize is the whole tape.
- A block that cannot be understood should be stepped over whole rather than
  guessed at, and a badly formed one should be reported with the number that
  went wrong in the message. Both of those are how the $35 bug was found in a
  minute rather than an afternoon.

**Zip archives** are read directly: the first file inside with a matching
extension is loaded, and if there is none, nothing happens. `src/zip.rs` is a
small reader written here rather than a dependency, because the build stays
offline-reproducible against the crates already in the lock file.

## The pause at the end of a block

A block ends with a pause, and the pause is part of the block rather than a
gap between blocks. `in_block_pause` says the deck is in one;
`pause_ends_the_tape` says the pause is the last thing on the tape or the block
behind it stops the tape.

That distinction is what Max speed uses: it returns to normal speed only for
the pause at the end of the *last* block, or one whose next block stops the
tape — a zero-length pause block or a "stop if 48K".
Returning to normal speed at every in-block pause makes a multi-load tape crawl
for a second between every part.

## The ROM's loader

LD-BYTES lives at **$0556** in the 48K ROM. Its first eight bytes are
`INC D / EX AF,AF' / DEC D / DI / LD A,$0F / OUT ($FE),A` — `14 08 15 F3 3E 0F
D3 FE` — and checking those is the only safe way to know the routine is really
there. A 128K is running its own ROM at that address until a game pages the
other one back, and a program may put anything it likes there in RAM.

Its contract:

| Register | On entry                       | On exit                    |
| -------- | ------------------------------ | -------------------------- |
| A        | flag byte wanted (0 header, $FF data) | —                   |
| F carry  | set to load, clear to verify   | set on success             |
| IX       | where the bytes go             | past the bytes loaded      |
| DE       | how many bytes are wanted      | what is left, 0 on success |
| H        | —                              | running parity, 0 if right |

The checksum byte at the end of a block makes flag ⊕ data ⊕ checksum = 0.

The header a program loads first is seventeen bytes, and it says what the data
block behind it is and where it goes. A program looking for its data steps over
any headers in between: a flag byte that is not the one asked for means "not
this block", not "error".

## Three speeds

**Normal.** The pulses are played and the ROM counts them. 1,500 baud, so a
full tape is minutes.

**Max speed.** The CPU is run as fast as the emulator will go while the tape
moves. This shortens the wait; it cannot remove it, because the pulses still
take as long as they take in machine time.

**Ludicrous speed** (`src/flashload.rs`). The call to LD-BYTES is answered
rather than run: the next block with the flag byte asked for is copied into
memory, the registers are left as the routine would have left them, and the
return is taken there and then. Border Break loads in two emulated frames
instead of 2,605, ending in the same place, running, with the same 975 bytes on
screen.

Three things it has to get right, all of them found by loading a real tape:

- **Hand over the next block with the flag asked for, not the block under the
  head.** Handing a program a header when it asked for data makes BASIC report
  a loading error.
- **Seeking past the last block must stop the deck, not play it.** Playing from
  the end rewinds, so the tape started loading all over again and the run took
  *longer* than without any of this.
- **Report failure the way the ROM does.** A block shorter than the program
  asked for is the "R Tape loading error" every mistyped POKE ends in, and a
  loader that reports success for it leaves the machine somewhere it could
  never have got to.

It only helps where the ROM is doing the loading. A game with a loader of its
own is reading the port and counting its own pulses; nothing here can help it,
which is why switching Ludicrous on switches Max speed on with it. Measured:
Border Break 2,605 emulated frames → 2; Anabasis, which loads a small BASIC
stub and then reads the rest itself, 18,722 → 18,149.

**The silence behind a block belongs to the block**, and is left on the tape
when the bytes are handed over. That pause is what the program does its work in
before the next block starts — loading its own loader, say — and taking it away
breaks games that need it. Cobra's Alkatraz loader has under two seconds of
pilot to catch and never caught it; with the pause left in it finds all 256 of
the pilot pulses it looks for, exactly as it does when the tape is played. So a
flash-loaded tape is not instant: it is the pauses plus a memcpy, about a second
for Border Break against fifty-two.

**A stopped deck is a stopped deck.** Blocks are only handed over while the
tape is running: `LOAD ""` with the deck paused waits for Play, as it should.
Without that check the whole tape ran through the moment the ROM asked for its
first block, leaving the machine part way into a tape nobody had started.

The first block of a session sometimes still loads at tape speed. The ROM sits
*inside* LD-BYTES waiting for a pilot tone, and the answer is only given on
entry — so if the machine is already in there when Play is pressed, that block
is played. Start the deck before typing `LOAD ""` and every block is handed
over.

## Loading that is not the ROM's

Games with their own loaders are the majority of anything past 1984. Nearly all
of them are built round the same handful of
[loading routine cores](https://sinclair.wiki.zxnet.co.uk/wiki/Loading_routine_%22cores%22),
of which the commonest is the ROM's own edge sampler with the BREAK check taken
out:

```text
LD-SAMPLE  INC B          04
           RET Z          C8
           LD A,$7F       3E 7F
           IN A,($FE)     DB FE
           RRA            1F
           XOR C          A9
           AND $20        E6 20
           JR Z,LD-SAMPLE 28 xx
```

Speedlock uses exactly that. Found in Head over Heels at $FD30, byte for byte,
with the jump back reading `28 F4` rather than the ROM's `F6` because its loop
starts two bytes earlier — which is why the first search for it, using the
ROM's displacement, found nothing at all. `flashload::at_sampler` matches the
first eleven bytes and ignores the displacement.

### What cannot be done, and why

A Speedlock block cannot be handed over the way a ROM block can, because the
bytes on the tape are not the bytes that reach memory. Its byte loop, from Head
over Heels:

```text
$FE04  LD A,$12          ; both immediates are rewritten as it goes
$FE06  XOR L             ; L is the byte assembled from the tape
$FE07  ADD A,$86
$FE09  LD (IX+$00),A
$FE0C  INC IX
$FE0E  DEC DE
$FE1A  CALL $FD2C        ; the next bit
```

The obvious shortcut is not to hand over blocks but to skip the *waiting*: when
the machine is sitting in the sampler, the deck already knows when the next
edge is due, so move the clock there and give B the turns it would have
counted. That was tried and **it does not work**. The port the loop reads is
$7FFE, whose high byte is in the contended range, so the ULA stalls the read by
an amount that depends on where the beam is: measured over a load of Head over
Heels, a turn costs 54 T-states most of the time but 56, 58 or 60 often enough
to matter. Divide the wait by a fixed 54 and B comes out wrong, the loader
mis-measures its pulses, and both test tapes failed to load. Charging each
skipped turn its true contended cost would mean reproducing the I/O contention
in closed form for a saving of a few instructions a bit, which is not worth it.

### What is done instead

Ludicrous speed lifts the work cap while a tape is moving. Max speed is held to
twenty-four frames of machine time a host frame so that fast-forward cannot
lock up the window; with a tape loading, that cap is what a person is waiting
on. Ludicrous keeps working until fifty milliseconds of the host's own time
have gone, then stops to draw, so the window still answers twenty times a
second.

Measured on Daley Thompson's Decathlon, from `LOAD ""` to the tape stopping:

| | host frames | what that is at 60 Hz |
| --- | --- | --- |
| Max speed | 615 | about ten seconds |
| Ludicrous speed | 11 | about a fifth of a second |

And it loads the same game: comparing all of memory at the moment the tape
stops, the loading screen's pixels and everything above the screen are
identical byte for byte. What differs is the stack below SP ($FFCE-$FFDF for
Head over Heels, $FFF3-$FFF9 for Daley) and, for Daley, 151 attribute bytes —
its loading screen cycles colours while the tape runs, so how far round it has
got depends on exactly when the tape ran out.

## The rest of what a custom loader needs

What the emulator can do for a loader it cannot answer is play the pulses
accurately and run the CPU fast. Two things make that work:

- **Pulse-level playback**, as above. A loader measuring its own edge timings
  gets the same edges a real machine would.
- **The floating bus** giving back what the ULA has on it. Sidewize will not
  even start without it: it looks for the beam by reading port $40FF and never
  enables interrupts until the byte it wants comes back.

## Every pulse ends with an edge, including the last one of a block

The silence behind a block is at the low level. A block whose last pulse left
the line low therefore used to end with no change at all: the deck went
straight from the final data pulse to holding the line low, which it already
was. A loader waiting for the edge that closes that pulse waits for ever.

The deck now finishes the last pulse first — a millisecond at the level the
pulse toggles to, and then the silence — which is the same thing the TZX
reference says when it describes the pause as beginning with a millisecond of
the current level.

**That is what stopped Cobra and 720 Degrees loading**, and it looked nothing
like a tape fault. Alkatraz reads all eight bits of the block's last byte,
waits for the closing edge, times out, and jumps to $F067: which wipes its own
code with an `LDDR`, plays 224 beeps through the ROM's beeper at $03B5, and
resets the machine. Everything before that is correct — the pilot search passes
all 256 of its measurements, the whole block is assembled byte for byte, and
both of the loader's checks on it pass — so the failure arrives long after the
mistake, wearing a copy-protection costume.

Two measurements settled it. The byte the loader wanted was on the tape: it
asked 258 T-states after finishing the previous one, with 12,316 T-states of
data still to come. And following the last read edge by edge showed all eight
bits arriving — 1135 and 1117 T-states for a one, 546 and 542 for a nought —
and then nothing at all where the sixteenth pulse should have been closed.

A ZX81 test had the old behaviour written into it, expecting fifteen bit-gaps
from sixteen bits because "the last of whose gaps is swallowed by the pause".
It gets sixteen now.

## The EAR line is never dead

Reading port $FE bit 6 with no tape playing does not give zero: the machine
hears its own loudspeaker. On an issue 3 board the bit follows bit 4 of the
last write to $FE; on an issue 2 it follows the MIC bit, bit 3, as well. The
emulator models both and has the MIC feedback on by default, with a switch
beside Late timing.

**Head over Heels does not load without it**, and the way it fails is worth
knowing because nothing about it looks like a tape problem. Every block of that
tape decodes byte for byte — all 48K of it, checked against the file — and then
the loader does this:

```text
$FECC  LD HL,$9000
$FECF  LD B,$FF
$FED1  PUSH BC
$FED2  CALL $FEDE     ; listen to the EAR line
$FED5  LD (HL),E      ; and write down what it heard
$FED6  INC HL
$FED8  DJNZ $FED1
```

`$FEDE` reads $7FFE two hundred and fifty-five times looking for bit 6 to
change. On a dead line it never does, the table at $9000 comes out all zeros,
and the loader calls $FD20 — which walks IY through the whole address space
writing zeros. A black screen and a machine that never comes back, which is
exactly what it is meant to look like when somebody has taken the tape away.

On a real machine the line is alive at that moment for either of two reasons:
the tape is still rolling long after its last block, which no tape file has
anything to say about, and the loudspeaker feeds back into the input besides.
Feeding a trailing tone into the deck by hand fixes it too, and gives the same
table — $9000 reads `77 04 C3 B7 92 DD 7E 0A` either way — so the check is
asking whether the line is alive rather than measuring what is on it.

## The ZX81

A ZX81 tape is a different format and the ZX81's ROM has no LD-BYTES, so
Ludicrous speed is a Spectrum switch only and is disabled while a ZX81 is
selected. The ZX81 loads at about fifty bytes a second, which makes Max speed
matter more there than anywhere else.
