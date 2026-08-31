# Three-inch disks

The +3 is a +2A with a disk interface: a µPD765A controller, a three-inch
drive, and +3DOS in ROM. This is what the emulator does with it and what it
does not.

## Putting one in

**File ▸ Load disk…** reads a `.dsk` — or a `.zip` with one inside, which is
how a download usually arrives — and then asks a question before the machine
can have it:

- **Read-only** — the controller reports the disk write-protected. The file is
  never opened for writing.
- **Write to a copy** — the image is copied to a new file *now*, and writes go
  to the copy. The original is left exactly as it was.
- **Write to this file** — writes go back to the image it came from, as they
  would to a real disk.

A disk that came out of a zip is offered read-only or a copy, and nothing else:
an archive is not a place to keep a changing disk, and writing one back into it
would mean rewriting the archive around it. The copy goes beside the zip under
the name the disk had inside.

Writable is not the default, and the question is not skipped. A game writes its
high scores to the disk it loaded from, and the first time that happens should
not be a surprise; a disk somebody has downloaded is not one they want quietly
rewritten. The copy is made when the disk goes in rather than at the first
write, because a copy that does not exist until something changes is a copy
nobody can find.

**File ▸ Create blank disk…** asks for a filename and makes a blank one,
formatted the way the machine's own FORMAT formats a disk, and puts it in
**writable** — somebody who has just made a disk means to write to it. Either
one brings up a +3 if the machine running has no drive, and opens the disk
window.

A disk that has been written to is saved when it is ejected, when the machine
is changed, and when the emulator closes. The window says which file is in the
drive, how it was mounted, and whether it has changed.

## The window

Fixed to the tape window's width, and the same three parts under the controls.

**The drive**, as it looks from the front: the slot with the disk in it, the
eject button, and the green light above them. The light means what the one on a
real +3 means — lit while something is being read or written, a dim glow while
the motor turns with nothing to do, dark otherwise. Whether there is a disk in
is the slot, not the light.

**Two speeds.** *Normal* is the waits a real drive makes the program sit
through: about a second for the motor to come up to speed, a step of the head
per track with a settle after it, and a sector coming round under the head
every twenty-second of a second. *Fastload* is no waits at all — every answer
ready the moment it is asked for, which is how a disk behaved here before there
was a choice. The controller has no clock of its own: it is handed the
machine's time on every port access, which is the only moment the time can
matter.

**The disk**, where the tape window has its oscilloscope: drawn as a disk, a
ring per track with track 0 outermost — where it is on a real one. The written
band is the outer third of the radius, as it is on a three-inch disk whose
forty tracks lie between about 24mm and 35mm from the middle; drawing them
across the whole face made every disk look the same, since most of the picture
was then empty tracks. The bits are drawn as bits, white for a one and black
for a nought, clockwise from the top, with a gap between one sector and the
next so the nine of them can be counted.

Not all of them: a track is nine 512-byte sectors, 36,864 bits, and a ring a
few hundred pixels round cannot hold them. What is drawn is the top bit of
every nth byte, n being whatever fits the ring at three pixels a bit.

The byte alignment matters more than it sounds. Sampling every nth *bit*
walks through the bits of a repeating pattern and turns it into noise: a track
of the formatter's $E5 came out looking exactly like a track of code, which is
the one thing the picture is for telling apart. Aligned to bytes, a track of
one repeated byte is one flat colour, so an unwritten disk is a set of clean
rings and a full one is speckle — and where a game's files stop is visible at
a glance.

Reads light green over the sector they touched and writes amber, both fading,
so a load draws itself round the disk as it happens; a ring marks the track the
head is on. A highlight is a band that follows the track round, drawn as a
strip of quads: an annular sector is not convex, and egui fans a convex polygon
from its first vertex, so the first attempt came out as a triangle across the
disk rather than a curve along the track. The picture is rasterised into a texture and kept until the disk
changes, because fifty thousand line segments a frame is not a thing to ask of
a window that is also running a Spectrum. What says the disk has changed is a
revision number bumped when a sector is written, rather than a comparison of a
hundred and eighty kilobytes.

**The catalogue**, where the tape window lists blocks: the CP/M directory read
as CAT reads it, with each file's size and whether it is read-only or hidden.
The machine's own CAT does not print the hidden ones; this does, marked,
because on a game disk that is usually where the game is. The extents of one
file are not added up — the last extent already says how long the file is, and
adding them made a 32K file 48K.

## What a DSK file is

Not a filesystem: what the controller would have read off the surface. Tracks,
and in each track a list of sectors carrying the identity in their address
marks — cylinder, head, record, size — and the bytes. That is why a protected
disk survives a round trip: its odd sector numbering and its deliberate CRC
errors are in the file the same as they were on the disk.

Two versions exist. The original writes one track length for the whole disk;
the extended one writes a length per track, which anything with unusual tracks
needs. Both are read. What is written is always the original, since nothing
this emulator makes needs the other.

The +3's own format — what FORMAT produces, and what **New disk…** makes — is
forty tracks, one side, nine 512-byte sectors a track, numbered from `$C1`,
every byte `$E5`. The filler is not arbitrary: `$E5` is what an empty CP/M
directory entry starts with, so a disk full of it catalogues as empty rather
than as full of rubbish. A blank disk reads as `No files found` and `178K
free`, which is what the machine says about one it formatted itself.

## Which disk is this?

Three questions the window and `disk_info` answer, because the difference
between them is the difference between a disk that will not load and an
emulator that will not load it.

**What format is it in?** The two the machine makes are told by their sector
numbering — from `$C1` for the data format its FORMAT writes, from `$41` for
the system format it was sold with. Anything else says what it is in the first
sector of track 0: a ten-byte specification giving the tracks, sides, sectors,
reserved tracks, block size and how much of the disk the directory takes. That
is how a 720K disk works on a machine whose own FORMAT only makes 180K ones,
and reading it is what lets the catalogue be shown for one.

CP/M counts in tracks of its own, and on a double-sided disk each side is one
of them: unit 0 is track 0 side 0, unit 1 is track 0 side 1, unit 2 is track 1
side 0. The reserved count is in those units too. Skipping a whole physical
track instead put the directory of a 720K disk on the wrong side and read
somebody else's data as filenames.

**Whose files are on it?** A +3DOS file begins with the eight letters of
`PLUS3DOS`; an AMSDOS file begins with a header whose first sixty-seven bytes
add up to the checksum at byte 67. The +3 and the Amstrad CPC use the same
disks, the same controller and the same filesystem, so a CPC disk mounts,
catalogues and reads perfectly well on a +3 — and then does not load, because
the files in it are for another machine. The window says so in amber rather
than leaving somebody to wonder.

A file with neither header is a game's own loader reading its own data, which
is not a fault either.

## The controller

`src/fdc.rs` is a µPD765A as far as +3DOS can tell. Every command goes through
the same three phases — the program writes a command and its parameters, the
data goes one way or the other, then the result bytes are read back — and the
commands implemented are the ones the ROM uses: SPECIFY, RECALIBRATE, SEEK,
SENSE INTERRUPT STATUS, SENSE DRIVE STATUS, READ ID, READ DATA, WRITE DATA and
FORMAT TRACK. Anything else is answered as an invalid command rather than
ignored, since a program waiting for a result byte that never comes waits for
ever.

Two ports: `$2FFD` is the status register the program polls and `$3FFD` the
data register. The motor is bit 3 of `$1FFD`, and the controller is told: a
drive whose motor is off is not ready, which is how +3DOS knows to wait.

**Nothing is timed.** A real controller makes the program wait while the head
steps and the disk turns, and reports not-ready until the motor is up to speed.
This one answers at once. +3DOS polls rather than counting, so it cannot tell —
but a loader that measures the wait could, and that is worth knowing before
trusting this with a protected disk.

**Sectors are matched on their identity, not on where they sit.** A read asks
for a cylinder, head and record, and gets the sector whose address mark says
so. A disk that numbers its sectors oddly is read correctly; one that lies
about its cylinder is not read at all, which is what the hardware does.

## What was checked

Against the machine, not against what the tests thought to ask:

- a +3 with the real ROM says `Drives A: and M: available` at its menu, which
  it only says when the controller answers;
- `CAT` on a blank disk prints `No files found` and `178K free`;
- `CAT` on a real game disk prints its catalogue — `DRILLER . 1K`, `108K free`;
- `SAVE "t" CODE 30000,10` finishes with `0 OK`, and the name is in the
  directory in the first sectors of track 0 afterwards.

The tests read the screen back as text by matching each character cell against
the ROM's own font, because a disk test that cannot read what the machine
printed is a test of what the controller was asked rather than of what it
answered.

## From a program

The MCP server has the drive too: `mount_disk` (read-only unless asked, with
`copy_to` for writing to a copy), `new_disk`, `eject_disk`, `disk_info`,
`disk_speed`, `disk_catalogue`, `read_sector` — which reads the image rather
than driving the drive, so it works whatever the machine is doing — and
`disk_activity`, which is the sector map as a list.

## Not done

- The second drive (`B:`) exists in the controller and nothing mounts one.
- Normal speed is the drive's own waits and not the disk's rotation: a sector
  costs the time one takes to come round, rather than the time until *that*
  sector comes round, so a program timing the gaps between sectors would see
  them evenly spaced.
- No timing, as above.
- Nothing writes the extended format, so a disk with unusual tracks read in and
  written out comes back regular.
- +3DOS's own file operations are not shortcut the way the tape's are by
  `flashload`: a disk load runs through the ROM at the speed the ROM does it,
  which on a disk is quick enough that nobody has asked.
