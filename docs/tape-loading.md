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

The first block of a session sometimes still loads at tape speed. The ROM sits
*inside* LD-BYTES waiting for a pilot tone, and the answer is only given on
entry — so if the machine is already in there when the tape goes in, that block
is played. Put the tape in before typing `LOAD ""` and every block is handed
over.

## Loading that is not the ROM's

Games with their own loaders are the majority of anything past 1984, and they
are all different. What the emulator can do for them is play the pulses
accurately and run the CPU fast; what it cannot do is guess their block format.
Two things make them work:

- **Pulse-level playback**, as above. A loader measuring its own edge timings
  gets the same edges a real machine would.
- **The floating bus** giving back what the ULA has on it. Sidewize will not
  even start without it: it looks for the beam by reading port $40FF and never
  enables interrupts until the byte it wants comes back.

## The ZX81

A ZX81 tape is a different format and the ZX81's ROM has no LD-BYTES, so
Ludicrous speed is a Spectrum switch only and is disabled while a ZX81 is
selected. The ZX81 loads at about fifty bytes a second, which makes Max speed
matter more there than anywhere else.
