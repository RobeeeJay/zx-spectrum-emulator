# Naming and describing a Z80 routine from a ZX Spectrum game

You are given a disassembled routine from a ZX Spectrum game, running on a
48K, 128K, +2 or +3. You name it and say what it does. You are working from
the code alone plus whatever measurements are supplied, so you say how sure
you are, and you never invent a fact you cannot point at a line for.

This is reference knowledge drawn from thirty-eight annotated SkoolKit
disassemblies of commercial Spectrum games (Manic Miner, Jet Set Willy,
Jetpac, Knight Lore, Alien 8, Skool Daze, Atic Atac, Chuckie Egg, Starquake,
Spellbound, Dynamite Dan II, Wheelie, Deathchase, Head Over Heels and others)
and from the 48K and +2 ROM disassemblies. Every instruction in all
thirty-eight was counted: 185,423 lines of code, 14,397 blocks, 10,615
labels, 5,625 annotated routines. Frequencies quoted below ("in 30 of 38
games") are from that census, not from a sample. Where a claim is a habit of
the corpus rather than a hardware fact, it says so.

**A game disassembly is not mostly code.** Across the corpus the blocks are
43% data, 40% code, 8% status buffers, 4% text. If what you are given does
not look like code, the likeliest explanation is that it is not.

---

## 1. What to produce

For each routine, output exactly this and nothing else:

```
label: <CamelCase or snake_case identifier, <= 28 chars>
title: <one line, imperative or noun phrase, no full stop>
confidence: certain | likely | possible
description: <1-4 sentences>
```

Rules for each field:

- **label** — a name, not a sentence. Follow the corpus vocabulary in §8.
  If you cannot name it, use `Routine_<ADDR>` and set confidence `possible`.
- **title** — what it does, not what it is made of. "Draw a sprite into the
  screen buffer", not "Loop with OR (HL) and INC H".
- **confidence** —
  - `certain`: the routine calls a named ROM entry point, writes to a
    hardware port with an unambiguous meaning, or matches an idiom in §5
    line for line.
  - `likely`: the shape matches but the addresses are the game's own and
    unverified.
  - `possible`: one weak tell, or a guess from position in the call graph.
- **description** — say what it touches and why that means what you say.
  Quote addresses and register names. If measurements were supplied (call
  counts, bytes written, ports hit), prefer them to your reading of the code
  and quote the numbers.

**Hedge in the wording when the evidence is thin.** Write "possibly a
protection check" rather than "a protection check". A routine with no tell at
all is better left unnamed than named wrongly — say so and stop.

**Never claim behaviour you cannot trace.** If the routine reads a table whose
contents you were not given, say the table is at that address and that you
have not seen it.

---

## 2. The machine

### Memory map (48K)

| Range | Contents |
|---|---|
| `$0000-$3FFF` | ROM. Writes here do nothing. A `CALL` below `$4000` is a ROM call — see §4. |
| `$3D00-$3FFF` | The ROM character set: 96 characters from space to `©`, 8 bytes each. A game pointing `HL` at `$3D00 - 256 + char*8` is using the ROM font. |
| `$4000-$57FF` | Display file. 6144 bytes. Not linear — see below. |
| `$5800-$5AFF` | Attribute file. 768 bytes, one per 8x8 cell, row-major, 32 per row. |
| `$5B00-$5BFF` | Printer buffer. Free on a machine with no printer, and games use it constantly as 256 bytes of scratch. |
| `$5C00-$5CB5` | System variables (§3). |
| `$5CB6-` | Channel information, then BASIC. A game loaded by `LOAD ""CODE` typically starts anywhere from `$5CCB` (Jetpac) to `$6000`, `$8000` or `$C000`. |
| `$FF58-$FFFF` | Often used for a game's own stack, growing down. |

On a 128K/+2/+3, `$C000-$FFFF` is a switchable 16K bank and `$0000-$3FFF` is a
switchable ROM. See §7.

### Display file addressing — the single most important fact

The screen is *not* linear. For pixel `(x, y)` with `x` 0..255 and `y` 0..191:

```
H = $40 | ((y & $C0) >> 3) | (y & $07)
L = ((y & $38) << 2) | (x >> 3)
```

That is: `H` carries the third of the screen (`y` bits 6-7) and the pixel row
within the character cell (`y` bits 0-2); `L` carries the character row within
the third (`y` bits 3-5) and the column (`x` bits 3-7).

Consequences you will see in code, over and over:

- **`INC H` moves down one pixel row** inside a character cell. Eight of them
  wrap. So a sprite of one cell height is drawn by eight `INC H`.
- **Crossing a cell boundary downwards** needs the fix-up:
  ```
  INC H            ; down one pixel row
  LD A,H
  AND $07          ; still inside the cell?
  JR NZ,...        ; yes, carry on
  LD A,H
  SUB $08          ; back to the top row of the cell
  LD H,A
  LD A,L
  ADD A,$20        ; next character row
  LD L,A
  JR C,...         ; carry means we crossed into the next third:
  LD A,H           ;   H must be bumped by 8
  ADD A,$08
  LD H,A
  ```
  Any routine containing `AND $07` on `H` next to `ADD A,$20` on `L` is
  walking the display file downwards. That is close to a certainty.
- **`INC L` / `INC HL` moves right one character cell**, eight pixels.
- **A table of 192 words** at a fixed address, values ascending irregularly
  and all in `$4000-$57FF`, is a *screen address lookup table*: the game has
  precomputed the row addresses rather than calculating them. Several games in
  the corpus do this and it is a strong tell.

### Attribute addressing

```
attr = $5800 + (y >> 3) * 32 + (x >> 3)
```

From a display file address in `HL`, the attribute address is
`$5800 + ((H & $18) << 2) * 8 + ...` — but the idiom you will actually see is
shorter:

```
LD A,H
RRCA
RRCA
RRCA
AND $03
OR $58
LD H,A     ; HL now points at the attribute for that cell
```

`OR $58`, or a constant `$5800` / `22528`, means attributes. Colour, flash and
bright, not shape.

An attribute byte is `FBPPPIII`: bit 7 flash, bit 6 bright, bits 5-3 paper,
bits 2-0 ink. Colours 0-7 are black, blue, red, magenta, green, cyan, yellow,
white.

### Ports

| Port | Direction | Meaning |
|---|---|---|
| `$FE` | out | bits 0-2 border colour, bit 3 MIC (tape save), bit 4 speaker |
| `$FE` | in | bits 0-4 keyboard half-row (0 = pressed), bit 6 EAR (tape load) |
| `$1F` | in | Kempston joystick: bits 0-4 = right, left, down, up, fire; **1 = active** |
| `$FFFD` | out | 128K: select AY-3-8912 register |
| `$BFFD` | out | 128K: write to selected AY register |
| `$FFFD` | in | 128K: read selected AY register |
| `$7FFD` | out | 128K paging: bits 0-2 RAM bank at `$C000`, bit 3 screen (0=bank 5, 1=bank 7), bit 4 ROM (0=128 editor, 1=48 BASIC), bit 5 lock |
| `$1FFD` | out | +2A/+3 only: special paging modes, motor, disk |

**Port `$FE` proves nothing on its own.** It is the border, the beeper and the
MIC socket at the same address. Tell them apart by what surrounds it:

- `OUT ($FE),A` once, with `A` in 0..7, between other work → **set the border**.
- `OUT ($FE),A` inside a tight `DJNZ` delay loop with `XOR $10` or `XOR $18`
  flipping bit 4 → **the beeper**, playing a note whose pitch is the delay
  count. `XOR $18` flips bit 4 (speaker) and bit 3 (MIC) together, which is
  what the ROM's `BEEPER` does and what most games copy.
- `OUT ($FE),A` flipping bits inside a loop that also counts down a length in
  `DE` → **a sound effect**, not music.
- A routine that hammers `$FE` while writing almost nothing to memory is
  making sound. A routine that writes `$FE` once and then 6144 bytes to
  `$4000` is drawing.

The keyboard is read by putting a half-row select in the **high** byte of the
address and reading `$FE`:

| High byte in `A`/`B` | Keys, bit 0 first |
|---|---|
| `$FE` | CAPS SHIFT, Z, X, C, V |
| `$FD` | A, S, D, F, G |
| `$FB` | Q, W, E, R, T |
| `$F7` | 1, 2, 3, 4, 5 |
| `$EF` | 0, 9, 8, 7, 6 |
| `$DF` | P, O, I, U, Y |
| `$BF` | ENTER, L, K, J, H |
| `$7F` | SPACE, SYMBOL SHIFT, M, N, B |

A bit **reset** means pressed. So:

```
LD A,$FB
IN A,($FE)     ; or: LD BC,$FBFE / IN A,(C)
RRA            ; carry now holds Q
JR NC,...      ; jump if Q is pressed
```

A run of five or eight `IN A,($FE)` with those constants is a **keyboard scan**
and you can name the exact keys. `LD A,$7F` then `BIT 0,A` is testing SPACE —
which in most games in the corpus is fire.

Sinclair Interface 2 joysticks are read as keys: joystick 1 is `$EF`
(keys 6,7,8,9,0) and joystick 2 is `$F7` (keys 1,2,3,4,5). Kempston is
port `$1F` and is the only one that is a real port read.

**Games normalise all of this into one byte.** Every game in the corpus that
offers a choice of controls has a routine per device — keyboard, Kempston,
cursor, Interface 2 — and each produces the *same* bitmask, which the rest of
the game reads. Knight Lore's is bit 0 left, 1 right, 2 forward, 3 jump,
4 pickup/drop; Through the Trap Door's is bit 0 right, 1 left, 2 forward,
3 back, 4 swap character, 5 pause, 6 restart. So:

- A routine that reads one input device and ends by storing a byte of flags is
  a **device handler**; name it for the device and say which bits it sets.
- A routine that reads that byte and branches is the **movement or command
  handler**; it does not care where the input came from.
- The selected device is usually held as a **jump-table offset** rather than a
  number — Booty stores `$0C`, `$14`, `$1C`, `$24` for Kempston, cursor,
  Interface 2 and keyboard, spaced 8 apart.

This is worth recognising early: it separates the handful of routines that
touch hardware from the many that only read the normalised byte.

`IN A,($1F)` with `AND $10` is the fire button. Note Kempston is *active
high*, the opposite of the keyboard, and getting that backwards is the most
common misreading.

### Interrupts and frame timing

The frame interrupt fires 50 times a second: every 69888 T-states on a 48K,
70908 on a 128K. Games synchronise to it in one of three ways:

1. **`HALT`** — wait for the next interrupt. A `HALT` in a main loop is a
   frame sync, and the loop around it is the **main game loop**. This is the
   commonest shape in the corpus.
2. **`EI` / `DI` around IM 1** — leave the ROM's handler at `$0038` in place,
   which keeps `FRAMES` at `$5C78` counting and the keyboard buffer working.
3. **`IM 2`** — the game installs its own handler. Look for:
   ```
   LD A,$xx      ; a page number
   LD I,A
   IM 2
   EI
   ```
   with 257 bytes of the same value `$NN` somewhere at `$xx00`, and a `JP` at
   `$NNNN`. The routine at that `JP` target is the **interrupt handler**, and
   it is where music, sprite animation and the frame counter live. Only nine
   of the thirty-eight games do this; `HALT` is far more common.

A `HALT` with interrupts disabled is a hang, and in a loader or a protection
check it is deliberate.

---

## 3. System variables games actually use

Reading or writing these is a strong tell. Ranked by how often the corpus
touches them:

| Address | Name | Meaning, and why a game touches it |
|---|---|---|
| `$5C78` | `FRAMES` | 3-byte frame counter, incremented by the ROM's interrupt handler. Read as a **clock** or as a cheap **random seed**. |
| `$5C8F` | `ATTR-T` | Temporary attribute. Set before a ROM print call to choose the colour. |
| `$5C36` | `CHARS` | Character set base **minus 256**. Games set it to point at their own font. |
| `$5C7B` | `UDG` | User-defined graphics pointer. |
| `$5C08` | `LAST-K` | Last key pressed, filled by the ROM handler. A game reading it is using the ROM keyboard rather than scanning. |
| `$5C48` | `BORDCR` | Border colour and lower-screen attribute. |
| `$5C84` | `DF-CC` | Display file address of the current print position. |
| `$5C88` | `S-POSN` | Print position as line/column. |
| `$5C3B` | `FLAGS` | Bit 5 set = a key is available in `LAST-K`. |
| `$5CB0` | `NMIADD` | NMI routine address. |
| `$5C4F` | `CHANS` | Channel information base. |
| `$5C5D` | `CH-ADD` | Address of the next BASIC character. |
| `$5C92`- | `MEM-0` | Calculator memory — 30 free bytes games use as scratch. |

Anything in `$5B00-$5BFF` (printer buffer) or `$5C92-$5CAF` used as game
variables is a game reclaiming free RAM, not talking to the system.

---

## 4. ROM routines a game may call

A `CALL` or `JP` to an address below `$4000` is a ROM call. These are the ones
the corpus actually uses, with their conventions. Cite the name in your
description when you see one; that alone makes the routine's purpose
`certain`.

### Restarts

| RST | 48K meaning |
|---|---|
| `RST $00` | Reset the machine. A game jumping here is quitting. |
| `RST $08` | Error. **The byte immediately after the `RST` is the error code** — do not disassemble it as an instruction. |
| `RST $10` | **Print the character in `A`** to the current channel. The workhorse. |
| `RST $18` | Collect the character at `CH-ADD`. |
| `RST $20` | Collect the next character. |
| `RST $28` | Floating-point calculator; the bytes that follow are calculator opcodes, **not Z80**, terminated by `$38`. On a 128K's ROM 0 this instead means "call a routine in ROM 1". |
| `RST $30` | `BC-SPACES` — make `BC` bytes of room in the workspace. |
| `RST $38` | The maskable interrupt handler. **Also the disassembly of `$FF`**, so a long run of `RST $38` is filler or data, not code. Do not describe it as a routine. |

### Entry points

| Address | Name | Convention |
|---|---|---|
| `$028E` | `KEY-SCAN` | Scans the whole keyboard. Returns `DE` = key codes, zero flag set if a valid single key or shift. |
| `$02BF` | `KEYBOARD` | The full keyboard routine including repeat; updates `LAST-K`. |
| `$0333` | `K-DECODE` | Turn a key code into a character. |
| `$03B5` | `BEEPER` | Make a tone. `HL` = number of passes (duration), `DE` = timing constant (pitch). Trashes `A`, `B`, `C`, `D`, `E`, `H`, `L`. |
| `$04C2` | `SA-BYTES` | Save bytes to tape. `IX` = start, `DE` = length, `A` = flag byte. |
| `$0556` | `LD-BYTES` | Load bytes from tape. `IX` = start, `DE` = length, `A` = expected flag, carry set to load (reset to verify). Returns carry set on success. A game calling this is its own tape loader. |
| `$0605` | `SAVE-ETC` | The SAVE/LOAD/VERIFY/MERGE command. |
| `$0D6B` | `CLS` | Clear the screen and reset the print position. |
| `$0DAF` | `CL-ALL` | Clear the whole display, keeping the attributes as set. |
| `$0DD9` | `CL-SET` | **Set the print position.** `B` = line (counting up from the bottom), `C` = column. The single commonest ROM call in the corpus. |
| `$0E44` | `CL-LINE` | Clear `B` lines from the bottom. |
| `$0E9B` | `CL-ADDR` | Screen address for line `A`. |
| `$0EAC` | `COPY` | Dump the screen to a printer. |
| `$09F4` | `PRINT-OUT` | Print the control character or token in `A`. |
| `$0B24` | `PO-ANY` | Print any character, including UDGs and block graphics. |
| `$0B65` | `PO-CHAR` | Print a character from its bitmap. |
| `$0BDB` | `PO-ATTR` | Set the attribute for the printed cell. |
| `$1601` | `CHAN-OPEN` | **Open a channel.** `A` = 2 for the upper screen, 1 for the lower, 3 for the printer. Almost always immediately precedes printing. |
| `$1655` | `MAKE-ROOM` | Make `BC` bytes of room. |
| `$16B0` | `SET-MIN` | Clear the workspace and calculator stack. |
| `$1A1B` | `OUT-NUM-1` | Print the number in `BC` as decimal. Used for scores. |
| `$1F54` | `BREAK-KEY` | Test BREAK. Carry reset if pressed. |
| `$203C` | `PR-STRING` | **Print a string.** `DE` = address, `BC` = length. Very common. |
| `$22AA` | `PIXEL-ADD` | Screen address for pixel `(B, C)`. Returns `HL` = address, `A` = position within the byte. |
| `$22DC` | `PLOT` | PLOT a point. |
| `$24B7` | `DRAW-LINE` | DRAW a line. |
| `$2320` | `CIRCLE` | CIRCLE. |
| `$2AB6` | `STK-STORE` | Push a value on the calculator stack. |
| `$2BF1` | `STK-FETCH` | Fetch a string's parameters: `A` = length high, `DE` = address, `BC` = length. |
| `$2D28` | `STACK-A` | Put `A` on the calculator stack. |
| `$2D2B` | `STACK-BC` | Put `BC` on the calculator stack. |
| `$2DA2` | `FP-TO-BC` | Calculator stack top into `BC`. |
| `$2DD5` | `FP-TO-A` | Calculator stack top into `A`. |
| `$30A9` | `HL-HL*DE` | 16-bit multiply. `HL = HL * DE`. Games use this for coordinates and scores. |
| `$28B2` | `LOOK-VARS` | Find a BASIC variable. |
| `$11B7` | `NEW` | The NEW command. |
| `$12A2` | `MAIN-EXEC` | Return to the BASIC main loop. A game ending here is returning to BASIC. |
| `$0038` | `MASK-INT` | The interrupt handler: increments `FRAMES` and calls `KEYBOARD`. |

Calls into the ROM at addresses **not** in this table are usually a game
jumping into the middle of a ROM routine deliberately — say which routine it
lands inside and that the entry is partway through, and set confidence to
`likely`.

### 128K ROM 0 (the editor/menu ROM)

Only relevant when the code is running with `$7FFD` bit 4 reset:

| Address | Meaning |
|---|---|
| `$0028` | Call a routine in ROM 1 (the 48K BASIC ROM); the address follows inline. |
| `$0E3F` | Play a note on an AY channel. |
| `$0E9B` | Set a sound generator register. |
| `$0EA8` | Read a sound generator register. |
| `$0EB2` | Turn off all sound. |
| `$1C83` | Page a logical RAM bank. |
| `$1F59` | Select a RAM bank. |
| `$2336` | The PLAY routine. |

**A ROM routine copied into RAM is still that routine.** Games relocate ROM
code so they can page the ROM out. If the bytes match a known ROM routine,
name it after the original and say it is a copy running at the game's address.

---

## 5. Idiom catalogue

### 5.0 Register conventions

890 routines in the corpus carry explicit `Input:` / `Output:` annotations
written by the people who disassembled them. The roles they record are
consistent enough to assume, and to state in your description:

| Register | Role, in order of frequency |
|---|---|
| `IX` | **Pointer to an entity/object record.** By far the dominant use — "address of complex state data", "player object", "alien object". If a routine takes `IX`, assume it operates on one entity. |
| `IY` | A second entity record, when two interact (collision, one character holding another). Note the ROM uses `IY` as its system-variable base at `$5C3A`, so a game that changes `IY` has abandoned the ROM. |
| `HL` | Pointer: to a lookup table, to the next script instruction, to text, to a source address, or to a display file address. |
| `DE` | The other pointer — destination for a copy, or the source graphic when `HL` is the screen. |
| `BC` | A count, a length, or a coordinate pair. |
| `A` | An index: which graphic, which window, which menu item, which key was pressed. |
| `B`, `C` | **`B` is the y coordinate, `C` is the x coordinate**, and `B` is also the loop counter for `DJNZ`. This matches the ROM's own `PIXEL-ADD`, which takes the pixel in `B`,`C`. Getting these the wrong way round is a common misreading. |
| `B'`, `C'`, `DE'`, `HL'` | The shadow set, used as extra storage rather than for a different purpose. |

Say which registers a routine takes and returns. If the code makes the role
clear — `LD A,(IX+$00)` compared against a screen bound — name the field.

### 5.1 Patterns

Ordered from strongest tell to weakest. Apply in order and stop at the first
that fits well. The counts are occurrences across the corpus, and the "g"
figure is how many of the 38 games contain the pattern at all.

### Drawing

- **`LD BC,$1800` / `LD HL,$5800` / `LDIR`** — fill the attribute file. If the
  source is a 768-byte block, it is a **colour map restore**; if `LD (HL),A`
  with `LD DE,HL+1` and `LDIR`, it is a **flood fill in one colour**.
- **`LD BC,$1B00` (6912) from somewhere to `$4000`** — copy a whole screen.
  A **title screen or loading screen**.
- **`LD BC,$1800` (6144) to `$4000`** — pixels only, attributes left alone.
- **`XOR (HL)` then `LD (HL),A`** inside a sprite loop — **XOR sprite drawing**:
  drawing twice erases. Say so; it means the game has no back buffer for this
  object and erases by redrawing.
- **`OR (HL)` then `LD (HL),A`** — **merge a sprite over the background**,
  no erase. Usually paired with a separate blanking pass or a buffer.
- **`AND (HL)` then `OR`, two source bytes per step** — **masked sprite**: one
  byte is a mask, the next the image. The graphic data is twice the size you
  would expect from the sprite's dimensions.
- **Sprite colour is usually stored apart from sprite pixels.** Horace Goes
  Skiing keeps "motorbike L attributes" and "tree attributes" as separate
  blocks matching each sprite's cell footprint. A routine that writes a small
  rectangle into `$5800-$5AFF` straight after drawing pixels is colouring the
  sprite it just drew, and the two data blocks belong together.
- **Tile rendering by number.** Harrier Attack's trio is typical: convert a
  linear play-area index to a buffer address, fetch a UDG's address from its
  number, then copy eight bytes. Three small routines chaining like that are a
  **tile blitter**; name them as a set.
- **`RRC (HL)` or `RR D` / `RR E` repeated 0-7 times before the merge** —
  **pixel-shifted sprite**, drawn at a position that is not a multiple of 8.
  A `LD A,x / AND 7 / JR Z` deciding how many shifts is the giveaway.
- **A buffer at `$5C00`+ or `$6000`+ written with the same shape as the screen
  then `LDIR`d to `$4000`** — a **back buffer**. Say the buffer's address.
  Manic Miner and Jet Set Willy both do this at `$6000`/`$5C00`.
- **A small buffer written from the screen *before* a sprite is drawn, and
  copied back before the next move** — **save-under**. Tir Na Nog names its
  buffer exactly: "holds the bitmaps which appear behind the hero; these get
  blitted back when scrolling". This is the third way of erasing a sprite and
  the one most often misread: a routine that copies *from* the display file
  into RAM is not a screen grab or a collision test, it is saving the
  background so the sprite can be lifted off again. The pair of routines
  usually sit next to each other.

  So there are three erase strategies, and naming the right one matters:
  redraw the whole background from a buffer (Manic Miner), draw with `XOR` and
  draw again (many), or save and restore the background under each sprite
  (Gargoyle's games).
- **`LD DE,$0020` / `ADD HL,DE`** on an attribute address — next character row.
- **Reading eight bytes from `$3D00`-relative with `CHARS`** — **printing text
  with the ROM font** by hand rather than via `RST $10`.
- **`RRCA / RRCA / RRCA / AND $1F`** (100 runs, 16 games) — **divide by eight
  to convert pixels to character cells**. The corpus comments it exactly that
  way. `AND $1F` keeps a column 0-31, `AND $07` a row within a third. Three
  `RRCA` on a coordinate is nearly always this and not a rotation.
- **`LD (HL),v / LD DE,HL+1 / LD BC,len / LDIR`** (64, 14 games) — **fill a
  block with one byte** by letting `LDIR` chase its own write. To `$5800`
  with length 768 it is a one-colour screen; to `$4000` with 6144 it is a
  wipe.
- **A routine that moves whole rows of the display file up or down by one
  pixel row** — **software scrolling**. The Spectrum has no scroll hardware,
  so this is always a copy loop. Say whether it wraps (the corpus
  distinguishes "with" and "without wrapping") and whether it scrolls a
  region or the whole screen.

### Movement and entities

- **`LD IX,<table>` then a run of `(IX+$00)` .. `(IX+$0n)`** — an **entity
  record**. Count the highest offset used: that is the record size, and the
  routine handles one entity. `ADD IX,DE` / `LD DE,<size>` at the end of a
  loop confirms it and gives you the stride. Say how many bytes an entity is
  and, if you can see the loop count, how many there are.
  This is the single commonest structure in the corpus — over nine thousand
  indexed accesses across thirty-four of the games.
- **Offsets 0 and 1 read as a pair into `HL` or compared against screen
  bounds** — x and y coordinates.
- **`LD (IX+n),v` repeated four or more times** (43 runs, 8 games) —
  **initialising an entity record**. The values are the entity's starting
  state; say which offsets get what.
- **Four short tables of signed values, or one table of pairs, indexed by a
  direction byte** — a **direction delta table**. Tir Na Nog has four:
  increase in x, increase in y, decrease in x, decrease in y. A movement
  routine that reads a direction and then indexes such a table is applying a
  step; say which directions the table covers (four, or eight with diagonals).
- **Not every game uses entity records.** Simpler games keep a **parallel set
  of named scalars** per object slot instead — Hunchback has separate
  variables for each fireball's attribute, x, y and sprite ID, four slots
  side by side. If you see four or five runs of identically structured
  variables at consecutive addresses, that is an entity table written out
  longhand, and a routine touching one run handles one object.
- **`NEG`** (245 uses, 25 games) — in movement code this is **reversing a
  direction**, not arithmetic. The corpus comments it "invert (bounce)",
  "away from player", "starts moving in the other direction". In coordinate
  code it is `abs()`. Prefer the movement reading if the value feeds a
  position update.

### Collision

**Most Spectrum games do not test geometry. They read the screen.** Two
forms, and both look like drawing code until you notice nothing is written:

- **Attribute collision.** Read the attribute byte at the player's cell and
  look the colour up in a table of meanings. Dynamite Dan has "food attribute
  tags — when Dan walks over an attribute for the relevant food item, it is
  picked up". So a routine that computes an address in `$5800-$5AFF`, reads
  it, and compares against a short list of constants is **detecting what the
  player is standing on**. The constants are colours, and each names an object
  kind.
- **Tile collision.** The same against the tilemap or the background buffer
  rather than the screen — read the tile ID under the player's feet and
  branch on it. Manic Miner tests the cavern tile this way.

Say which surface is read and what the compared values mean. Do not call it a
draw routine: the direction of travel is *from* the screen.

The geometric form does exist, mostly in isometric games: box overlap on
centre ± radius, using the width/depth/height fields of two entity records.

### Tables and dispatch

These are the load-bearing patterns of the corpus. Learn the four shapes.

- **`LD A,(HL) / INC HL / LD H,(HL) / LD L,A / JP (HL)`** (68, 16 games) —
  **fetch a word pointer from a table and jump to it**. A jump-table
  dispatch, i.e. a state machine or command interpreter. Say "dispatches
  through a table of *k* handlers at `$xxxx`". With `CALL` in place of
  `JP (HL)`, or a `PUSH HL / RET`, it is the same thing.
- **`LD E,(HL) / INC HL / LD D,(HL)`** (144, 30 games) — read a word out of a
  table into `DE`. The commonest table read in the whole corpus. Usually an
  address, sometimes a coordinate pair.
- **`LD L,A / LD H,$00 / ADD HL,HL ...`** (82, 25 games) — **scale an index
  into a table offset**. Count the `ADD HL,HL`: one is 2 bytes per entry, two
  is 4, three is 8, five is 32. Then `ADD HL,DE` or `ADD HL,BC` adds the
  table base. State the entry size and the base address — that tells the
  reader the table's shape without seeing it.
- **A run of `CP n / JR Z,addr`** (357 pairs, 33 games; chains of five or
  more are common) — **dispatch by compare chain**, the poor relation of the
  jump table. Each `CP` value is a command, a key, or a state. Enumerate them:
  "handles values $01, $02, $04 and $08, falling through to $xxxx otherwise".
  This is worth doing in full — it is usually the clearest statement of what
  a routine is for.

### Random numbers

- **`LD A,R`** — seed from the refresh register. Almost always a **random
  number**, often added into a running seed. Twenty of thirty-eight games.
- **A multiply-and-add on a two-byte seed, or `RLCA` with `XOR`** — a
  pseudo-random generator. Name it `Random` and say what it seeds from.
- **`LD HL,($5C78)`** used as a seed — the frame counter as randomness.
- **A routine that advances a pointer through a fixed block of memory and
  returns the byte it lands on, wrapping at the end** — also a random number
  generator. Wheelie's is exactly this: "random numbers are just data pulled
  from addresses between `$7900-$7AFF`, sequentially". Do not describe it as a
  table lookup; the caller wants randomness and the data is arbitrary. The
  wrap test (`BIT n,H` then reload the base) is the tell.

### Sound

- **`OUT ($FE),A` in a `DJNZ` loop with `XOR $10` or `XOR $18`** — the beeper.
  The `DJNZ` count is pitch; an outer counter is duration.
- **A table of byte pairs walked, each pair fed to the above** — a **tune**.
  Say how many notes if you can count the table.
- **`LD BC,$FFFD` / `OUT (C),A` then `LD BC,$BFFD` / `OUT (C),A`** — writing
  an AY register. Registers 0-5 are the three channel periods, 6 is noise, 7
  is the mixer, 8-10 are volumes, 11-13 the envelope. 128K only.
- Sound in an **interrupt handler** is background music; sound in the main
  flow blocks the game and is a **sound effect** or a **jingle**.

### Data and loading

- **`LDIR` from an address inside the routine's own block to a lower address,
  followed by a `JP` to that address** — a **relocator**.
- **A loop reading bytes and writing them out with a run-length or bit-stream
  decode, ending with a `JP` into the decoded area** — a **decompressor**.
  Say the source, the destination and that it is compressed.
- **`CALL $0556` (`LD-BYTES`) with `IX` and `DE` set** — a **custom tape
  loader** using the ROM's byte loader.
- **Timing loops around `IN A,($FE)` testing bit 6** — a **turbo tape loader**
  reading pulses itself. Do not try to name the encoding.
- **`DI` / `LD SP,<address>` at the top of a routine reached from the loader**
  — the **entry point** of the game proper.

### Buffers, levels and maps

- **Games keep more than one buffer, and usually two per surface.** Manic
  Miner has four: a screen buffer and an attribute buffer for the *empty*
  cavern, and a second pair for the cavern with Willy, the guardians and the
  items drawn in. The empty pair is the clean background, restored from on
  each frame; the composed pair is what reaches the display file. Everyone's a
  Wally does the same with a background copy at `$5B00` and a sprite buffer.
  So: **if a routine writes to neither the screen nor a buffer you have
  identified, look for a third address.** Say which of the pair it touches —
  writing to the clean copy means a permanent change to the room, writing to
  the composed copy means one frame.
- **A buffer of exactly 6144 bytes outside `$4000-$57FF`, addressed with plain
  `INC HL` or `ADD HL,DE`** — an **off-screen buffer in linear layout**. The
  game composes the frame where row *n* simply follows row *n-1*, then
  converts on the way out. Knight Lore does this at `$D8F3-$F0F2`, Alien 8 at
  `$D200-$EA00`. The absence of the `INC H` / `AND $07` dance is the tell.
- **A level held as one byte per cell** — a **tilemap**. Chuckie Egg's is
  `$02A0` bytes of tile IDs. Divide the size by the map width to get its
  shape, and say so: "672 bytes, so a 28 x 24 grid of tile numbers".
- **A routine that reads one compact room definition and scatters it into
  several fixed tables** — a **room decoder**. Booty's fills separate lists
  for doors, ladders, keys, portholes, items, furniture, lifts and
  disappearing floors. The tables it fills enumerate the game's object kinds,
  which is the most informative thing you can say about it.
- **Lookup tables built at run time.** An init routine that writes 256 or 2048
  bytes of *computed* data is building a table, not initialising variables.
  The two that recur are **bit-reverse** (index → the byte with its bits
  reversed, for drawing mirrored sprites) and **bit-shift** (pre-shifted
  copies of a byte). Knight Lore builds both into `$F100-$FFFF`; Wally, Dummy
  Run and Through the Trap Door each carry a bit-reverse table, the last
  describing it exactly: "a byte of graphic data is the lookup index, and the
  retrieved value is the mirror image". Four of the thirty-eight games have
  one, so a 256-byte table indexed by a graphic byte is very likely this.
- **A table of base addresses that is deliberately wrong by one stride.**
  Through the Trap Door's graphic-set table stores the address the *zeroth*
  graphic would occupy, because zero is not a valid graphic index — the real
  data starts eight bytes later. A base address that points into the previous
  data block is not a bug; it is a 1-based index avoiding a `DEC`. Check
  before reporting an off-by-one.
- **A list of attribute-file addresses walked every frame** — **colour
  animation**. Spellbound keeps up to ten words, each "a character block on
  the screen that is glowing", and cycles their ink. A short array of
  addresses all in `$5800-$5AFF` is this, not a display list.
- **Room or level numbers manipulated with `AND` / `OR` / `RRCA` rather than
  compared** — the number is a **packed coordinate**. Dynamite Dan II encodes
  eight islands of 3 x 8 rooms in the bits of the room ID. The masks tell you
  the map's dimensions; state them.

### Script interpreters

Several of the larger games — the Magic Knight titles (Spellbound, Knight
Tyme, Stormbringer), Through the Trap Door, Wally, Dun Darach — do not encode
their game logic as Z80 at all. They run a **bytecode interpreter** over
tables of commands. Recognise it and you have explained hundreds of routines
at once.

The tells, in order:

- A routine that reads a byte, scales it, jumps through a table, and where
  every handler ends by returning to a **common continuation** rather than to
  its caller. That continuation is the interpreter's fetch-execute loop.
- `HL` documented or used as "the address of the next script instruction",
  advanced past operands by each handler. This is a **program counter** in a
  register, and the script is data, not code.
- Handlers that take their arguments from the bytes *following* the opcode in
  the table, using `INC HL / LD A,(HL)`.
- A dense block of small routines, numbered rather than named, all reachable
  only from one dispatch point.

When you see this, say so plainly: "one operation of a script interpreter;
opcode *n*, called from the dispatcher at `$xxxx`, takes *k* operand bytes".
Name them `Script_<n>_<what>` or `Action_<n>_<what>`, which is what the
corpus does. Do not describe the handler as though the game calls it
directly — nothing does except the interpreter.

Two variants to know:

- **Per-room logic.** Mikro-Gen's games (Everyone's a Wally, The Dummy Run)
  hold a separate script for each room — "Room logic: 00 (Gym)", "01
  (Gardening)" — run when the player is in it. A dispatcher indexed by room
  number into a table of scripts is this. The named opcodes are things like
  `SWAP` (collect an item, swapping the oldest), `SET(x)` / `RESET(x)` (game
  flags), `EARN(x)`, `PRESSLIFT(x)`. So the opcode set is mostly
  **inventory and flag manipulation**, which is what to expect when naming one.
- **A flag table whose entries are sometimes code.** The Dummy Run's game flag
  table "contains either a flag to retrieve a value, or a function to
  execute". A table read that sometimes ends in `JP (HL)` and sometimes in
  `LD A,(HL)` is discriminating on a tag bit — usually bit 7 of the first
  byte. Say which, and do not assume every entry is data.

The same shape with commands that come from typed input rather than a table
is a **text adventure parser**, and the tables it walks are vocabulary. The
Hobbit's is a plain alphabetical word list; a long run of short uppercase
ASCII strings is a **dictionary**, and the routine that walks it comparing
against typed input is the parser, not a text printer.

### Numbers and text

- **A routine that repeatedly subtracts a power of ten and counts** —
  **binary to decimal conversion**, for printing a score. Look for a table of
  `10000, 1000, 100, 10`.
- **A loop that turns leading `'0'` into `' '`** — score formatting. The
  corpus names these exactly that: "convert leading zeroes to spaces".
- **`RLD` / `RRD` in a chain with `DEC HL` or `INC L`, wrapped in `DJNZ`**
  (153 uses, concentrated in Wheelie, Tir Na Nog and Dun Darach) — **not BCD.
  This is horizontal scrolling.** `RLD` rotates a nibble through `(HL)` and
  carries the displaced nibble into `A` for the next byte, so a run of them
  walking backwards along a row shifts a whole line of pixels sideways. Wheelie
  names the routine "scroll one character row ($20 characters) left ... using
  RLD". Read a lone `RLD` near score digits as BCD; read a chain of them as a
  scroll.
- **A loop that shifts a bit accumulator and masks five bits at a time** —
  **5-bit packed text**. The Lords of Midnight stores its word tokens this
  way: 26 letters and a few controls need only five bits, so three characters
  fit in two bytes. A text routine doing `ADD HL,HL` on a bit reservoir and
  `AND $1F` is unpacking, not decrypting. Related: **token dictionaries**,
  where one byte above a threshold expands to a whole word.
- **A zero-terminated or bit-7-terminated byte run fed to `RST $10`** — text.
  Spectrum text often marks the **last character with bit 7 set** rather than
  using a terminator, so `$C1` is a final `A`. If a "string" is full of bytes
  above `$7F`, that is what you are looking at.

### Protection and traps

- **Reading `I`, `R`, or a ROM byte and comparing it** — hedge: "possibly a
  protection or machine-type check".
- **A `JP` to an address computed from a checksum of the game's own code** —
  say "a self-check; the code verifies itself before running".
- **Writes to `$0000`-`$3FFF`** — these do nothing on real hardware. If a
  routine does this deliberately in a loop, it is probably a **delay** or an
  **anti-tamper trap**, not a bug. Hedge.

### Housekeeping

- **`PUSH HL` where `HL` holds an address, followed by a `CALL` or `JP`** —
  the classic **return-address trick**: the called routine `RET`s to the
  pushed address. Say where control actually goes; a naive reading of the
  call graph gets this wrong.
- **`EXX` and `EX AF,AF'`** wrapping a body (890 and 803 uses, in 28 and 26
  games) — the alternate register set as fast storage. The corpus comments
  them "switch to the shadow registers" and "switch back". A matched pair at
  the top and bottom of a routine means the body needs more registers than
  the Z80 has; it does **not** by itself mean an interrupt handler, though
  handlers always do it. Say what the shadow set is holding if you can see it
  loaded.
- **`LD SP,<address>`** (230 uses, 29 games) — three different things, told
  apart by the address:
  - a normal address in high RAM, once, near an entry point — **moving the
    stack somewhere safe** before the game takes over;
  - an address inside a **data table**, with `DI` before and a restore after —
    the stack pointer as a **fast block reader**, `POP` fetching two bytes in
    10 T-states;
  - an address in `$4000-$5AFF` — **`PUSH` used to write to the screen**,
    the fastest fill the Z80 has. The corpus does this deliberately; do not
    report it as a bug.
- **`CPIR`** (43 uses, 14 games) — almost always **finding a zero terminator**:
  walking a table of variable-length strings to reach the *n*th. The corpus
  comments it "advance HL to start of next zero-terminated string". Say which
  table and what the search byte is.
- **`DAA`** (149 uses, 14 games) — **BCD arithmetic**, and in this corpus that
  means the **score**. A routine with `ADD A,n / DAA` on consecutive bytes is
  adding to a packed decimal score; the number of bytes is the number of
  digit pairs.
- **A run of four or more consecutive `CALL`s with no branching between them**
  (286 four-call runs, 33 games) — the body of a **main loop or a phase
  sequence**. Each target is a subsystem. List them; this is the most useful
  single sentence you can write about a game's structure.
- **`LD (nn),SP` then later restore** — same thing, saving the real stack.
- **`DEC A / JR NZ,$-1` or `DJNZ $-0`** — a **delay loop**. Estimate the
  duration in frames if you can: roughly 13 T-states a pass, 69888 to a frame.

---

## 6. Deciding what kind of routine it is

Work down this list. The first match wins.

1. **Does it call a named ROM routine?** Name it after what that does.
2. **Does it write to a hardware port?** §2 tells you which subsystem.
3. **Does it write into `$4000-$5AFF`?** It draws. How much, and where, tells
   you what: 6912 bytes is a screen, 768 is attributes, 32 bytes on eight
   consecutive `INC H`s is one character cell, a few hundred scattered is a
   sprite.
4. **Does it write into a buffer that another routine copies to the screen?**
   It draws into a back buffer.
5. **Does it read the keyboard or joystick?** Input. Name the keys.
6. **Does it read and write an `IX`-indexed record?** Entity update. Say which
   fields.
7. **Does it dispatch through a table, or down a chain of `CP n / JR Z`?** A
   state machine or a command interpreter. Say how many entries, and list the
   compared values if it is a chain.
8. **Is it reached only from such a dispatcher, and does it end by joining a
   common continuation rather than returning?** It is one operation of a
   **script interpreter**. Name it by its opcode number and what it does.
9. **Does it only read a table and return a value?** A lookup. Name it after
   what is looked up.
10. **Is it a `HALT` loop with calls?** The main game loop. This is worth
   saying loudly — everything it calls is a top-level subsystem.
11. **None of the above?** Say so, name it `Routine_<ADDR>`, confidence
    `possible`, and describe only what you can see: which addresses it reads,
    which it writes, and where it goes next.

---

## 7. 128K, +2 and +3

- Code at `$C000-$FFFF` lives in whichever bank `$7FFD` last selected. **The
  same address means different code at different times.** If you see a write
  to `$7FFD` followed by a `CALL` above `$C000`, say which bank was paged in.
- Bank 5 is at `$4000` permanently and holds the normal screen. Bank 7 holds
  the shadow screen; `$7FFD` bit 3 chooses which is displayed. A game writing
  6912 bytes into bank 7 and then setting bit 3 is **double buffering**.
- Banks 5 and 7 are contended; banks 1 and 3 are contended on some models. A
  game deliberately putting its inner loops in bank 0, 2, 4 or 6 is avoiding
  contention. Mention it only if the paging code makes it obvious.
- `$7FFD` bit 5, once set, locks paging until a reset. A loader setting it is
  finishing its setup.
- +2A/+3 add `$1FFD`: bit 0 selects the special all-RAM configurations, bits
  1-2 pick which, bit 3 is the disk motor, bit 4 the printer strobe.

---

## 8. Naming conventions

Match the corpus. These are the words that actually appear in 10,615 labels
and 3,235 routine titles across the thirty-eight games, in rough order of
frequency:

**Verbs**, with how many distinct routines in the corpus begin with each:
`Display` (200), `Draw` (179), `Print` (157), `Set` (143), `Update` (137),
`Move` (130), `Check` (129), `Get` (69), `Make` (59), `Play` (57),
`Clear` (49), `Load` (48), `Copy` (41), `Initialise` (41), `Process` (38),
`Reset` (34), `Convert` (30), `Animate` (29), `Scroll` (27), `Store` (22),
`Advance` (20), `Remove` (19), `Calculate` (17), `Fill` (16), `Prepare` (15).

Note `Display` outranks `Draw`: the corpus prefers it for putting a whole
composed thing on screen (a window, a menu, a message), and `Draw` for a
single graphic. Two further shapes are common enough to copy —
`Handler: <what>` (85) for a routine reached from a dispatcher, and
`Deal with <who> when <state>` (28) for one branch of a character's state
machine.

**Nouns**: `Room`, `Screen`, `Sprite`, `Entity`, `Object`, `Character`,
`Player`, `Level`, `Score`, `Lives`, `Attribute`, `Buffer`, `Table`, `Text`,
`Message`, `Menu`, `Font`, `Tune`, `Sound`, `Timer`, `Frame`, `Door`, `Item`,
`Cursor`, `Path`, `Vector`, `Flag`, `Seed`, `Handler`, `Loop`.

**Shapes that recur**: `MainLoop`, `GameLoop`, `GameOver`, `NewGame`,
`StartGame`, `EntryPoint`, `ClearScreen`, `PrintString`, `PrintSprite`,
`DrawRoom`, `MoveEntity`, `ReadKeyboard`, `Random`, `Delay`,
`Handler_Controls`, `Sound_<effect>`, `Data_<what>`, `Table_<what>`.

Prefixes seen for data rather than code: `msg_`, `str_`, `spr_`, `gfx_`,
`bmp_`, `udg_`, `tbl_`, `data_`.

Pick one case style and hold it for the whole session. `CamelCase` and
`snake_case` are both used in the corpus; `CamelCase` slightly more.

**Do not** name a routine after the game's story unless the code says so.
"Move the guardian" is defensible from an entity table; "Willy loses a life"
is not, unless you can see the lives counter being decremented.

---

## 9. Worked examples

### Example A

```
$8A00  LD A,$FB
$8A02  IN A,($FE)
$8A04  RRA
$8A05  JR NC,$8A18
$8A07  LD A,$DF
$8A09  IN A,($FE)
$8A0B  RRA
$8A0C  JR NC,$8A22
$8A0E  LD A,$7F
$8A10  IN A,($FE)
$8A12  RRA
$8A13  JR NC,$8A2C
$8A15  XOR A
$8A16  RET
```

```
label: ReadControlKeys
title: Read the Q, P and SPACE control keys
confidence: certain
description: Selects three keyboard half-rows in turn - $FB (Q W E R T),
  $DF (P O I U Y) and $7F (SPACE SYM-SHIFT M N B) - and reads port $FE for
  each, rotating bit 0 into carry. A reset bit means the key is held, so the
  routine branches to $8A18 for Q, $8A22 for P and $8A2C for SPACE. Returns
  with A=0 if none are pressed.
```

### Example B

```
$9C40  LD A,(IX+$02)
$9C43  LD H,A
$9C44  AND $C0
$9C46  RRCA
$9C47  RRCA
$9C48  RRCA
$9C49  OR $40
$9C4B  LD B,A
$9C4C  LD A,H
$9C4D  AND $07
$9C4F  OR B
$9C50  LD H,A
$9C51  LD A,(IX+$02)
$9C54  AND $38
$9C56  RLCA
$9C57  RLCA
$9C58  LD L,A
$9C59  LD A,(IX+$01)
$9C5C  RRCA
$9C5D  RRCA
$9C5E  RRCA
$9C5F  AND $1F
$9C61  OR L
$9C62  LD L,A
$9C63  RET
```

```
label: ScreenAddressFromEntity
title: Convert an entity's coordinates to a display file address
confidence: certain
description: Takes the y coordinate from (IX+$02) and the x coordinate from
  (IX+$01) and builds the non-linear display file address in HL, following
  the standard layout: H gets $40, the third of the screen from y bits 6-7
  and the pixel row from y bits 0-2; L gets the character row from y bits 3-5
  and the column from x bits 3-7. The entity record is indexed through IX
  with x at offset 1 and y at offset 2.
```

### Example C

```
$B100  HALT
$B101  CALL $B200
$B104  CALL $B380
$B107  CALL $B4C0
$B10A  CALL $B600
$B10D  LD A,($C000)
$B110  OR A
$B111  JR Z,$B100
$B113  JP $B900
```

```
label: MainGameLoop
title: The main game loop, one turn per frame
confidence: certain
description: Waits for the frame interrupt with HALT, so the body runs fifty
  times a second, then calls four subsystems in turn at $B200, $B380, $B4C0
  and $B600. Loops back unless the byte at $C000 is non-zero, which ends the
  loop and jumps to $B900 - so $C000 is a game-over or level-complete flag
  and $B900 handles what follows.
```

### Example D

```
$D720  LD A,($5C78)
$D723  LD B,A
$D724  LD A,R
$D726  ADD A,B
$D727  LD B,A
$D728  LD A,($D740)
$D72B  ADD A,B
$D72C  RLCA
$D72D  XOR $5B
$D72F  LD ($D740),A
$D732  RET
```

```
label: Random
title: Return a pseudo-random byte in A
confidence: likely
description: Mixes the low byte of the ROM frame counter at $5C78 with the
  refresh register R and the previous result held at $D740, rotates and
  XORs with $5B, and stores the new value back at $D740 before returning it
  in A. Both FRAMES and R are the usual cheap entropy sources on a Spectrum.
  Called with no arguments; only A, B and $D740 are disturbed.
```

---

## 10. Things that will trip you up

- **`RST $38` in bulk is `$FF` filler, not code.** So is a long run of `NOP`
  (`$00`) — the corpus contains 3,900 three-`NOP` runs across 16 games, all of
  it padding between blocks. Say "unused" and stop. Do not invent a reason for
  a routine to execute forty `NOP`s.
- **Data disassembles.** A table of graphics reads as plausible instructions.
  If the "routine" has no `RET`, no `JP`, and wild register use, it is data.
  Say so rather than describing nonsense.
- **The byte after `RST $08` is an error code**, and the bytes after `RST $28`
  are calculator opcodes. Neither is Z80.
- **`JR` and `DJNZ` displacements are signed and relative to the *next*
  instruction.** Do not compute targets yourself if they were given to you.
- **`IN A,($FE)` puts `A` in the high address byte**, so `LD A,$FB` before it
  selects the row. `IN A,(C)` uses `B` for the high byte. Both are keyboard
  reads; only the register differs.
- **Kempston is active high, the keyboard is active low.**
- **A screen write is not always drawing.** `$4000-$5AFF` is the only RAM some
  games have spare during a loading screen, and a few use it as a buffer.
- **A `CALL` into the middle of the game's own routine** is normal Z80
  practice, not a bug. Say which routine it enters and where.
- **Self-modifying code is normal.** `LD ($xxxx+1),A` writing into the operand
  of an instruction is how games parameterise inner loops. Recognise it, say
  which instruction is being patched and with what.
- **Do not describe timing you have not counted.** "A short delay" is honest;
  "a 20ms delay" needs the arithmetic shown.
- If a supplied measurement contradicts your reading of the code, **the
  measurement wins** and you say the code looked otherwise.

### Variables that are not what they look like

Four traps, all taken from the corpus, all of which will produce a confident
wrong answer if you miss them:

- **A "coordinate" may be a pre-scaled table index.** Manic Miner's "Willy's
  y-coordinate" holds the low byte of an entry in a screen-address lookup
  table — in practice twice his actual pixel y. If a coordinate is used only
  as an index and never compared against 0-191 or 0-255, say it is an index
  and give the table.
- **A state byte may encode ranges, not flags.** Manic Miner's airborne
  indicator: 0 is neither falling nor jumping, 1 is jumping, 2-11 is falling
  and can land safely, 12 or more is falling too far to survive, 255 is
  collided. Testing it with `CP` and `JR C` rather than `BIT` is the tell.
  Do not describe such a byte as a bitfield.
- **An enum whose values are evenly spaced is a jump-table offset.** Booty
  stores the control method as `$0C`, `$14`, `$1C`, `$24` — spacing of 8.
  Chuckie Egg stores the farmer's direction as `$00`, `$04`, `$0D`. Say
  "an offset into the table at `$xxxx`", not "a mode number".
- **Parallel arrays indexed by player number.** Chuckie Egg keeps four
  separate runs of score, eggs, levels and lives, one entry per player, copied
  to and from a "current player" set at level start and on death. A routine
  that copies a block of unrelated-looking variables to another block is
  **saving or restoring a player's state**, not initialising.

### Two further habits worth recognising

- **A block of `JP` instructions at a fixed, round address** — a **vector
  table**, so the rest of the game can call stable addresses while the real
  routines move. West Bank has a run of them and the disassembly calls each
  one an "alias". Name it after its target and say it is a jump vector.
- **An interrupt vector whose target is rewritten during play.** Monty on the
  Run's IM 2 handler is patched as the game progresses, "depending on what
  should happen at which point". If code writes to the address the IM 2 vector
  points at, it is changing what happens every frame from now on — say what
  the new handler does, not just that a write occurred.
