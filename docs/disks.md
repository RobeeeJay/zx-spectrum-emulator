# Three-inch disks

The +3 is a +2A with a disk interface: a µPD765A controller, a three-inch
drive, and +3DOS in ROM. This is what the emulator does with it and what it
does not.

## Putting one in

The **Disk** section of the toolbar appears on a +3 and nowhere else, because
no other machine here has a drive.

**Insert…** reads a `.dsk` and then asks a question before the machine can have
it:

- **Read-only** — the controller reports the disk write-protected. The file is
  never opened for writing.
- **Write to a copy** — the image is copied to a new file *now*, and writes go
  to the copy. The original is left exactly as it was.
- **Write to this file** — writes go back to the image it came from, as they
  would to a real disk.

Writable is not the default, and the question is not skipped. A game writes its
high scores to the disk it loaded from, and the first time that happens should
not be a surprise; a disk somebody has downloaded is not one they want quietly
rewritten. The copy is made when the disk goes in rather than at the first
write, because a copy that does not exist until something changes is a copy
nobody can find.

**New disk…** asks for a filename and makes a blank one, formatted the way the
machine's own FORMAT formats a disk, and puts it in **writable** — somebody who
has just made a disk means to write to it.

A disk that has been written to is saved when it is ejected, when the machine
is changed, and when the emulator closes. The window says which file is in the
drive, how it was mounted, and whether it has changed.

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

## Not done

- The second drive (`B:`) exists in the controller and nothing mounts one.
- No timing, as above.
- Nothing writes the extended format, so a disk with unusual tracks read in and
  written out comes back regular.
- +3DOS's own file operations are not shortcut the way the tape's are by
  `flashload`: a disk load runs through the ROM at the speed the ROM does it,
  which on a disk is quick enough that nobody has asked.
