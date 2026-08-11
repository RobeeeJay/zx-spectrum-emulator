# Per-game corpus notes

Working notes from reading each SkoolKit disassembly's prose in turn. The
purpose is to find what *generalises*; only that gets promoted into
`z80-routine-analysis.md`. Per-game specifics are kept here so the prompt
stays lean.

Method: instruction listings stripped, comment-block prose read in full, one
game at a time.

---

## Ultimate Play the Game — the Filmation engine

**Knight Lore**, **Alien 8** (also **Jetpac**, earlier and simpler).

Memory map, Knight Lore, stated outright in the disassembly:

```
$5BA0-$6107  variables
$6108-$D8F2  code and data
$D8F3-$F0F2  video buffer          <- 6144 bytes, LINEAR
$F100-$FFFF  bit-shift & bit-reverse lookup tables, built at run time
```

Alien 8 the same shape: `$D200-$EA00 = linear screen buffer`.

Findings that generalise:

- **An off-screen buffer the size of the screen, addressed linearly.** The
  game composes the frame in a buffer where row *n* follows row *n-1*, then
  converts to the Spectrum's non-linear layout on the way out. Recognise it
  by a buffer of 6144 bytes outside `$4000-$57FF` written with plain
  `INC HL`/`ADD HL,DE` arithmetic rather than the `INC H` / `AND $07` dance.
- **Lookup tables built at run time**, filling a whole page or more. Knight
  Lore builds bit-shift and bit-reverse tables into `$F100-$FFFF` at startup.
  Wally has the same bit-reverse table and says why: "the index to the table
  gives the inverse bit pattern; this saves time calculating it by hand".
  So: an init routine that writes 256 or 2048 bytes of *computed* data is
  building a lookup table, and the cheapest way to name it is by what the
  table is for.
- **Object records carry both a current and a previous position.** Alien 8's
  9-byte object: `+0 gfx, +1 x, +2 y, +3 z, +4 screen, +5 current x,
  +6 current y, +7 current z, +8 current screen`. The duplicate set is what
  erase-then-redraw needs. A record with two coordinate triples is doing
  this, not storing two objects.
- **Isometric games use x/y/z plus radii.** Knight Lore's object: `+0 gfx,
  +1 x centre, +2 y centre, +3 z bottom, +4 width (X radius), +5 depth
  (Y radius), +6 height, +7 flags`. Flags: bit 7 vflip, 6 hflip, 5 wipe,
  4 draw, 3 auto-adjust near arches, 2 moveable. Collision is a box overlap
  test on centre ± radius.
- **Sprites stored as image/mask pairs**, confirmed by Alien 8's
  `[0...d][0...w] Image, Mask`.
- **Room data is run-length encoded** with the count in the low bits and the
  type in the high bits of a byte (Alien 8: `type [bits 3-7], count
  [bits 0-2]`), terminated by `$FF`.

## Two-buffer compositing — Wally Week (Everyone's a Wally)

- `$5B00`: "copy of the main playing area ... background graphics that don't
  change until a room is redrawn".
- A second **sprite buffer**, "a copy of the lower two thirds of the screen,
  sorted sequentially by rows" — linear again.
- Order: background buffer initialised once per room; copied to the sprite
  buffer; sprites drawn on top; buffer blitted to screen.

Generalises to: **three addresses matter, not one.** If a routine writes to
neither `$4000` nor a buffer you can identify, look for a third: many games
keep a clean background copy *and* a working buffer.

## Room-decoding front ends — Booty

- A room is decoded once on entry into several **typed reference lists**:
  doors, ladders, keys and locked doors, portholes, pirates, items,
  furniture, lifts, disappearing floors. All "populated by $AB44".
- Generalises: a routine that reads one compact room definition and scatters
  it into several fixed tables is a **room decoder**, and the tables it fills
  name the game's object kinds.
- **Control method stored as a jump-table offset, not an enum**: `$0C`
  Kempston, `$14` cursor, `$1C` Interface 2, `$24` keyboard — spacing of 8.
  A "constant" whose values are evenly spaced is an offset; say so.
- Entry-point code copied from a data block to `$CD14` and executed there.

## Room numbering as bit fields — Dynamite Dan II

Room IDs encode the grid position: eight islands of 3x8 rooms, with island in
the high bits and x/y in the low. Generalises: **a room or level number that
is manipulated with `AND`/`OR`/`RRCA` rather than compared is a packed
coordinate**, and the masks tell you the map's dimensions.

## Front ends are a large fraction of every game — Starquake, and all others

Starquake's first ~1000 lines of annotated code are: main menu, print
options, print joystick/keyboard choice, highlight selection, detect a
Kempston, define keys, translate a key into a scanning option, high score
table. None of it is gameplay.

**This is the single most useful orientation fact from the sweep.** A routine
that prints a short list, reads the keyboard, highlights one line and loops is
the **control-selection menu**, and nearly every game in the corpus has one,
along with a define-keys routine and a high-score table. Recognising the front
end stops the model straining to find gameplay meaning in it.

The **Kempston detection** idiom recurs: read port `$1F` many times and OR the
results; a floating bus gives set bits in 5-7, a real interface gives 0. Manic
Miner does 256 reads and tests bit 5.

## Pseudo-3D — Deathchase

Per-variable documentation of a road-racer: bike direction as -1/0/1, frames
until direction change, "distance to next enemy bike frame state", "enemy bike
frame to draw (0 far, 1 medium, 2 near, 3 in range)".

Generalises: **depth as a small integer selecting one of a few sprite sizes**,
not as perspective arithmetic. A routine indexing a sprite table by a
"distance" byte is doing 3D the way 1983 did it. Trees are "shunted out to the
side" as they approach — perspective by table, not by division.

## Per-character handlers — Atic Atac

"run player, weapon, and sound handlers"; separate "wizard character handler",
"axe animation handler", "fireball animation handler". Also a useful
primitive: "fill C rows of B columns of value A at address HL" — a
**rectangle fill** on the attribute or display file, which is a distinct
routine from a linear fill.

## Documented entity layouts — Jetpac

8-byte records throughout, and the disassembly gives the fields. Laser beam:
`0 in-use flag, 1 y, 2-5 x of four pulses, 6 length, 7 attribute`. Rocket:
`0 movement state, 1 x, 2 y, 3 attribute, 4 modules on pad, 5 fuel pods, 7
always $1C`. Collectible: `0 type, 1 x, 2 y, 3 attribute, 4 state, 6 sprite
jump table offset, 7 height`.

Generalises: **8 bytes is the usual entity stride**, and the layout is
near-universal: a type/in-use byte first, then x, then y, then the attribute,
then state. Offset 0 being zero means the slot is free — a scan for a free
slot is `LD A,(IX+$00) / OR A / JR Z`.

Score held as **3-byte BCD**, maximum 999999. Sound parameters as
frequency/duration byte pairs.

## Four buffers, not two — Manic Miner, Jet Set Willy

A screen buffer and an attribute buffer for the *empty* cavern, and a second
pair with Willy, the guardians, the items and the portal drawn over them. The
empty pair is the restore source; the composed pair goes to the display file.
Writing to the empty pair changes the room permanently; writing to the
composed pair lasts one frame.

Two variables worth remembering as traps, both promoted to the prompt:

- "Willy's y-coordinate" is not a coordinate. It holds the low byte of an
  entry in a screen-address lookup table — in practice twice his pixel y.
- The airborne indicator encodes ranges: 0 neither, 1 jumping, 2-11 falling
  safely, 12+ fatal, 255 collided. Tested with `CP`/`JR C`, not `BIT`.

## Code in the display file — Back to Skool

"Populate a row of the screen with machine code ... copies 256 bytes of
machine code from the source (either the top row of the screen, or character
buffers) to the destination (the second or third row from the bottom of the
screen), in eight 32-byte blocks." The game executes from the display file.
Also a "POKE table" applied before the game starts, and the concept of
**animatory states** — one byte per character encoding sprite, direction and
phase together.

## Self-modifying interrupt vector — Monty on the Run

"IM2 vector jump. This routine is called by the IM2 vector jump at $C700. The
specific code changes throughout the game, depending on what should happen at
which point." Tune data as length/frequency word pairs, terminated by 0,0.

## RLD is scrolling, not BCD — Wheelie, Tir Na Nog, Dun Darach

The corpus's 153 `RLD`/`RRD` uses are concentrated in these three Gargoyle and
Positive Image games. Wheelie: "scroll one character row ($20 characters) left
using RLD". Tir Na Nog uses the same chain to "rotate left an area of the
screen (clouds)". Exactly one comment in the whole corpus reads `RLD` as a
score digit. **The first version of the prompt got this backwards and it has
been corrected.**

Wheelie also has the laziest RNG in the corpus: "random numbers are just data
pulled from addresses between $7900-$7AFF, sequentially."

## Save-under — Tir Na Nog, Dun Darach (Gargoyle)

"Holds the bitmaps which appear behind the hero. These get blitted back when
scrolling." The third erase strategy, and the one most easily misread: a copy
*from* the display file into RAM. Also four direction delta tables (increase
in x, increase in y, decrease in x, decrease in y) and per-compass-setting
bounding rectangles for edge detection.

## Room logic as bytecode — Mikro-Gen (Everyone's a Wally, The Dummy Run)

One script per room: "Room logic: 00 (Gym)", "01 (Gardening)", and so on.
Opcodes are inventory and flag operations — `SWAP`, `SWAPFOR`, `SET(x)`,
`RESET(x)`, `EARN(x)`, `PRESSLIFT(x)`, `WALL`, `SAFE`. The Dummy Run's game
flag table "contains either a flag to retrieve a value, or a function to
execute", so a table read may end in `JP (HL)`.

## Magic Knight engine — Spellbound, Knight Tyme, Stormbringer

Windowed menu interface over a flick-screen world. Spell handlers all share
one shape: "Cast X if Possible, else Display Failure Message". Fifty bytes of
visited-room flags, one per room. A list of up to ten attribute-file addresses
that are animated each frame ("character blocks that are glowing").

## Bit-reverse tables — four games

Knight Lore, Everyone's a Wally, The Dummy Run, Through the Trap Door. Through
the Trap Door states it plainly: "a byte of graphic data is the lookup index,
and the retrieved value is the mirror image". Through the Trap Door also
biases its graphic-set base addresses by one stride so a 1-based index needs
no `DEC` — a base that points into the previous block is deliberate.

## Collision by reading the screen — Dynamite Dan, Manic Miner

"Food attribute tags: when Dan walks over an attribute for the relevant food
item, it is picked up." Collision is an attribute-file read plus a table
lookup, not geometry. Promoted to the prompt as its own subsection.

## Packed text — The Lords of Midnight

"Tokens are stored using 5-bit bytes compressed together." Also a contiguous
"main game data area, which is also used in the saved games", and records with
packed bitfields ("bits 0-1: Race, bit 2: Type (0=warriors, 1=riders)").

## Flat variables instead of entity tables — Hunchback

Separate named scalars per object slot: R-L fireball attribute, x, y, sprite
ID; then the same again for L-R, high L-R, high R-L. Small games write the
entity table out longhand.

## Tile blitting — Harrier Attack

Three chained routines: linear play-area index → buffer address; UDG number →
UDG address; copy eight bytes. Horace Goes Skiing keeps sprite attributes in
blocks separate from sprite pixels.

## Games with too little prose to learn from

`battlezone` (92 lines), `howtobeacompletebastard` (39), `splitpersonalities`
(5). Read; nothing generalisable. Recorded so the coverage claim stays honest.
