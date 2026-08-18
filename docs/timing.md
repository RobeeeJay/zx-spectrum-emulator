# T-states, contention and the ULA

What has been measured, what it was measured against, and what is still open.
Numbers here are the ones the code uses; where a claim was checked against a
real machine it says so, and where it was not, it says that too.

The reference throughout is the
[48K reference](https://worldofspectrum.org/faq/reference/48kreference.htm).
`tests/reference_48k.rs` holds its numbers as assertions, so they go on
matching.

## The frame

|                        | 48K    | 128K / +2 / +2A / +3 |
| ---------------------- | ------ | -------------------- |
| T-states a frame       | 69888  | 70908                |
| T-states a line        | 224    | 228                  |
| First display fetch    | 14335  | 14361                |
| CPU clock              | 3.5MHz | 3.5469MHz            |

A 48K frame is (64 + 192 + 56) × 224, and the "50 Hz" interrupt is really
3.5MHz / 69888 = 50.08 Hz. A line is 128 T-states of screen, 24 of right
border, 48 of horizontal retrace and 24 of left border.

**14335 or 14336.** The reference says the first byte is *displayed* 14336
T-states after the interrupt, and its contention table starts at 14335. The
emulator anchors the display at 14335 because that is where the delays start.
The two are the same statement one T-state apart, and where the pixels land
was settled against photographs of a real 48K rather than against either
number.

The ULA's own eight-T-state fetch cycle starts one T-state *after* the
contention anchor — see the snow section, which is what settled it.

**The interrupt line is held for 32 T-states** (`IRQ_LEN`). While a recording
is playing, that window is counted from when the interrupt was raised rather
than from the start of the frame, because a recording's frame boundary is not
the T-state frame's. Getting this wrong once made every interrupt acceptable
anywhere in the frame — the stored raise time went stale after a reset and
`saturating_sub` turned it into zero — and the Space Harrier flicker got
worse, not better.

**48K late timing.** Later 48K machines run the display one T-state later
relative to the interrupt. Both variants existed; HALT2INT tells them apart,
and the emulator offers both.

## Contention

Contended addresses are $4000-$7FFF on a 48K. On a 128K the odd RAM banks
(1, 3, 5, 7) are contended wherever they are paged; on a +2A/+3 it is banks
4-7 instead.

The delay for an access beginning at each T-state of the ULA's eight, from
14335:

```
14335  6      14343  6
14336  5      14344  5
14337  4      14345  4
14338  3      14346  3
14339  2      14347  2
14340  1      14348  1
14341  none   14349  none
14342  none   14350  none
```

That repeats for 128 T-states of each of the 192 display lines and nowhere
else: not in the border, not in the retrace, not in the 56 lines below the
display. The +2A/+3 pattern is different — `[1, 0, 7, 6, 5, 4, 3, 2]` — and
its gate array drives the bus itself, so it has no floating bus and no snow.

**Timing is counted per bus cycle, not per instruction.** The `Bus` trait is
called at each cycle, so contention lands at the right T-state *inside* an
instruction. zexdoc and zexall pass, undocumented flags included.

An M1 fetch is contended once, at its start, and only if PC is in contended
memory. A HALT keeps performing M1 cycles with PC — the address *after* the
HALT — on the bus, which matters: a HALT at $7FFF refreshes from the
uncontended $8000 while one at $4000 is contended on every cycle.

### I/O

Four patterns, by whether the port's high byte is in $40-$7F and whether A0 is
low:

| High byte in 40-7F | A0    | Pattern              | From 14335 |
| ------------------ | ----- | -------------------- | ---------- |
| no                 | reset | N:1, C:3             | 9 T        |
| no                 | set   | N:4                  | 4 T        |
| yes                | reset | C:1, C:3             | 10 T       |
| yes                | set   | C:1, C:1, C:1, C:1   | 16 T       |

The right-hand column is worked through by hand from the delay table and is
asserted in `tests/reference_48k.rs`.

## The floating bus

Reading an unattached port on a 48K or 128K gives back whatever the ULA last
put on the bus. In each eight T-states the ULA fetches bitmap, attribute,
bitmap, attribute for a pair of cells and leaves the bus alone for the other
four; **in those idle slots the last byte fetched is what is on it**, not
$FF.

That last point cost real time. Reading idle slots back as $FF meant every
floating-bus read gave $FF, because an I/O read is stalled to a free slot
before it samples — so a game waiting for a particular byte waited for ever.
Sidewize sits in `LD A,R / IN A,(C) / CP E / JP NZ` on port $40FF and does not
enable interrupts until the byte it wants comes back; it hung there, and the
symptom looked like "interrupts are not firing".

The fix that did **not** work: adding `DISPLAY_LEAD_T` to the fetch phase. It
let Sidewize run and broke HALT2INT, which is checked against photographs of
real hardware — `Float: Unknown` where the real machine says `Float:
Late/Early`.

`DISPLAY_LEAD_T` is 2: the ULA fetches two T-states ahead of the pixels it
puts out. Measured against a real machine with Border Break.

## Snow

After every opcode fetch the CPU puts I:R on the address bus. If that address
is RAM the ULA is reading, the ULA is disturbed, and what happens depends on
where the M1's last T-state falls in the ULA's eight:

- **third T-state** — the pixel fetch is made from the wrong address, bits 6..0
  of R standing in for the low seven of it. Snow is therefore made of the
  program's own graphics from the same part of the screen, and the colours
  stay right because only the pixel fetch coincides.
- **fifth T-state** — the second cell of the pair is not fetched at all and the
  first goes out again in its place. The "double effect": an eight-pixel bar
  repeated.

Addresses that do it: $4000-$7FFF on a 48K; also $C000-$FFFF on a 128K when an
odd page is banked there. Not at all on a +2A/+3. Games avoid it by keeping I
in $80..$BF.

**Where the ULA's cycle begins was settled by `tapes/SnowTests/snow.tap`.** Its
interrupt handler does `LD R,A` and then fills the display with a solid field
of NOPs, so every eight-T-state block gets an M1 in the same place. Sweeping
all eight possible anchors, it lands on the snow window only with the cycle
anchored one T-state after contention starts — which is where the reference
independently says the first byte is displayed. At that anchor all 3,072
blocks snow, which is what a program written to demonstrate snow should do; at
the anchor either side it shows nothing at all.

**Still open:** `tapes/SnowTests/ula128.tap` sets I to $FE on a 128K and its
instruction stream lands on the other parity, so it shows nothing here. The two
tapes disagree about the anchor and only a real machine can say which is right.

The description came from
[redcode's notes](https://github-wiki-see.page/m/redcode/ZXSpectrum/wiki/Snow-effect),
which are more exact than the World of Spectrum FAQ. The FAQ says the ULA
"regularly misses a screen byte" and repeats the one before; that is the double
effect only, and modelling it alone gets snow itself wrong.

Cost: the CPU only tells the bus about the refresh when I could point at
contended RAM. With that test in, a screenful of NOPs runs a fifth slower and
real code about seven per cent — measured at 400 frames of the 48K ROM in
31.1ms against 28.8ms.

## The painted frame

The picture is what the ULA put out, not what the display file holds when the
window repaints. The frame is copied cell by cell as the beam reaches each one
— a cell every four T-states, not a line at a time, because a game racing the
beam writes to a cell the moment that cell has been fetched, often several
times within a line.

Reading the display file at repaint time is what made Space Harrier's text
blink: it writes a line ahead of the beam and rubs it out behind, so the
display file at any one moment is missing whatever it has just rubbed out.

At full speed the picture holds the last *finished* frame, so no repaint can
catch one half drawn. While the machine is crawling — slow draw, or a speed
below 100% — it shows the frame being painted instead, or slow draw would
freeze the picture for the seconds an emulated frame takes.

## Replaying recordings

An RZX frame is a number of *opcode fetches*, not a length of time. Two things
about that are easy to get wrong and both were:

- `run_fetches` must report what it ran, not what was asked for. Instructions
  run whole, so a prefixed one overshoots; clamping the count to the budget
  loses the overshoot on every piece, and a frame run in several pieces then
  thinks it has further to go. Manic Miner read 3,278 bytes of input that were
  never recorded.
- The recording's frame is the video frame, and nothing else may end one.
  Ending frames on the T-state count as well paints the screen twice inside a
  recorded frame whenever the machine takes longer over the recorded
  instructions than the machine that recorded them did.

That second one is measurable: Space Harrier's recorded frames hold 9,400 to
11,600 fetches from about frame 1,100 on, and at the ~11 T-states a fetch that
game averages — the same live from tape as under replay — a video frame holds
about 6,300. So its recorded frames run 97,000 to 147,000 T-states. Manic
Miner's land within a dozen T-states of the clock every frame.

**Still open:** why those frames are that long. If the game misses interrupts
on real hardware then the ULA really did paint twice; but the lengths are not
near whole multiples of a frame, which fits that story no better than the
other.

## Running the clock faster

**Not offered at the moment.** The machinery is here and tested; the control is
hidden until it is an accelerator rather than a faster crystal, for the reason
in the paragraph after next. What it would offer:

The **Clock** dropdown in the Machine row offers the machine's own clock and
three doublings of it: 3.50, 7.00, 14.00 and 28.00MHz on a 48K, and 3.55, 7.09
and so on on a 128K, whose own clock is 3.5469MHz. The numbers come from the
model rather than from a list, which is why they differ.

It is the same machine running quicker, not a different machine. Everything the
ULA does is counted in T-states — the frame, the contention table, the tape's
pulses — so nothing about the emulation changes; what changes is how many
T-states go by in a second of the user's time. That is the difference between
this and the Speed dropdown beside it: speed is how fast the emulator is being
run, and the clock is what the machine believes its own to be.

**It speeds the whole machine, not just the CPU.** The ULA is counted in the
same T-states, so at twice the clock the frame still takes 69,888 of them and
those 69,888 go by twice as fast: measured, a second of the user's time gets 50
video frames at 3.5MHz, 100 at 7MHz and 400 at 28MHz, and the interrupt comes
at each of them. A game reading the frame counter therefore runs fast rather
than smoothly, which is what a Spectrum with its clock crystal changed did.

An accelerator that leaves the video at 50Hz is a different thing: it gives the
CPU more cycles inside a frame of the ULA's own time, which means the CPU's
clock and the ULA's are no longer the same clock. Everything here counts one —
contention is a table indexed by the ULA's T-state, and an instruction's cost
is in those T-states — so that would be a change to how time is kept rather
than a multiplier on it. Not done.

The mixer is told, because sound is made of T-states too. A beeper note is a
number of T-states between one toggle and the next, and at twice the clock those
T-states take half as long: the note comes out an octave up, which is what an
accelerated machine sounded like. That only happens if the mixer counts in the
same T-states the machine does, so changing the clock sets its rate as well.

## The television at the other end

The picture the emulator has is what the ULA put out: exact pixels, exact
colours, sharp to the sample. A set on the end of an aerial lead showed
something else, and two switches in the Video row show that instead — **CRT**
for the tube and **Composite** for the lead, because a monitor fed RGB had the
one and none of the other. The lead's switch only works with the set's on:
there was no picture that came down an aerial lead and was then shown on
something that was not a television. What it was left set to is kept while the
set is off.

Three things, none of them invented:

- **The line structure** (CRT). Every line is drawn with a gap under it, which is
  what a shadow mask looks like once there is room to see it. The picture is
  built at twice the height for this, so the gaps survive whatever the window
  is scaled to.
- **Composite colour** (Composite). Colour rides on a subcarrier with a fraction of the
  luminance's bandwidth, so it is smeared sideways while the brightness stays
  where it is: a red caption on black bleeds and a white one does not. The
  luminance is put back over the softened colour afterwards — except where
  that would want light of a negative amount, which a tube has none of.
- **Dot crawl** (Composite). PAL's colour subcarrier is 4,433,618.75 Hz and the
  Spectrum's dot clock is 7 MHz, so the subcarrier advances 0.6334 of a cycle
  every pixel. Sampled once a pixel, that aliases to a ripple every 2.7 pixels
  — the pattern anybody who used one on a television will remember. A line is
  448 dots, which is 283.75 cycles: the quarter left over is why it leans over
  instead of standing in columns. A frame is 139,776 dots, which is not a whole
  number of cycles either, so it arrives somewhere else next time and the whole
  thing crawls.

  It is not spread evenly over the picture. Crawl is the colour and the
  brightness being carried on one wire and not coming apart cleanly at the
  other end: there is none of it on a grey field, some on a flat colour, and
  most where the colour changes from one pixel to the next — which is why it is
  remembered as something that creeps along the edges of coloured blocks. Once
  it is only where it belongs it can be a fifth of the picture's brightness
  where it lands, which is what it looked like, instead of the few per cent an
  even wash has to be kept to.

### Keeping it off the moiré

Everything here is about a pixel across — the gap under each line, and a
herringbone with a period of two and a half — so the picture beats against the
screen it is shown on unless three things are watched:

- **The set's picture is sampled smoothly**, and the machine's is not. A tube
  has no pixel edges; drawing one through a nearest sample takes some gaps
  twice and some not at all, which is a moiré over the whole screen. With both
  switches off a pixel goes back to being a hard square.
- **The line gaps are only drawn where there is room for them.** They need two
  rows of screen for every row of picture, so below 2x zoom there are none:
  asking for them at 1x is asking for a pattern of gaps rather than a line
  structure.
- **The crawl is kept to the colour.** An even wash over the whole picture
  beats against whatever it is being scaled by, and has to be kept to a few per
  cent to stay tolerable; put where the colour is instead, it can be five times
  that and still leave a grey screen alone.

The curve of the glass is geometry rather than pixels, and it is in where each
quad *reads from* rather than in where it sits: the picture is a grid filling
the same rectangle it always did, and each vertex takes its colour from a point
moved outwards by the square of its distance from the middle. What comes back
is squeezed at the edges and swollen in the middle, which is what a tube looks
like from in front. Moving the quads instead — the first way this was written —
pushes the corners out past the edges and gives a pincushion, which is the
shape of a badly adjusted monitor. The whole sample is pulled in by however far
the corners overshoot, so nothing is read from outside the picture.

## What is checked against what

- **HALT2INT** — thirty values compared with photographs of a real 48K. Settles
  the interrupt timing and the early/late variants.
- **Border Break** — rendered and diffed against a photograph pixel by pixel:
  0 of 116,736 differ. Settles the border and `DISPLAY_LEAD_T`.
- **zexdoc and zexall** — the instruction set including the undocumented flags.
- **`tests/reference_48k.rs`** — the FAQ's frame, line, contention table and
  I/O patterns.
- **`tapes/SnowTests`** — the snow anchor, as far as it goes.
