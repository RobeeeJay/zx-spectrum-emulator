//! The +3's disk controller, driven the way +3DOS drives it.
//!
//! Every command is three phases — the program writes a command and its
//! parameters, the data goes one way or the other, then the result bytes come
//! back — and these tests walk that by hand so a failure says which phase
//! went wrong.

use zx_rustrum::disk::Disk;
use zx_rustrum::fdc::{Drive, Fdc, CB, DIO, RQM};

fn with_a_disk() -> Fdc {
    let mut fdc = Fdc::new();
    fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, false));
    fdc.motor = true;
    fdc
}

/// Write a command and its parameters, then read the result bytes back.
fn command(fdc: &mut Fdc, bytes: &[u8]) -> Vec<u8> {
    for byte in bytes {
        assert!(fdc.status() & RQM != 0, "the controller should want a byte");
        fdc.write(*byte);
    }
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 && fdc.status() & RQM != 0 {
        result.push(fdc.read());
    }
    result
}

/// An empty drive is not ready, which is how +3DOS knows to ask for a disk.
#[test]
fn an_empty_drive_says_it_is_not_ready() {
    let mut fdc = Fdc::new();
    fdc.motor = true;
    // SENSE DRIVE STATUS on drive 0.
    let result = command(&mut fdc, &[0x04, 0x00]);
    assert_eq!(result.len(), 1, "ST3 and nothing else");
    assert_eq!(result[0] & 0x20, 0, "not ready: {:02X}", result[0]);

    // And with a disk in it, ready.
    fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, false));
    let result = command(&mut fdc, &[0x04, 0x00]);
    assert_eq!(result[0] & 0x20, 0x20, "ready: {:02X}", result[0]);
    assert_eq!(result[0] & 0x10, 0x10, "and the head is at track 0");

    // The motor being off is the same as having no disk, as far as the drive
    // is concerned: it cannot read a disk that is not turning.
    fdc.motor = false;
    let result = command(&mut fdc, &[0x04, 0x00]);
    assert_eq!(result[0] & 0x20, 0, "motor off is not ready");
}

/// A write-protected disk says so, which is what the machine reads to know it
/// cannot be written to.
#[test]
fn a_write_protected_disk_says_so() {
    let mut fdc = Fdc::new();
    fdc.motor = true;
    fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, true));
    let result = command(&mut fdc, &[0x04, 0x00]);
    assert_eq!(result[0] & 0x40, 0x40, "write protected: {:02X}", result[0]);
}

/// Seek and recalibrate move the head, and the interrupt status says where it
/// ended up. +3DOS polls that after every seek.
#[test]
fn seeking_moves_the_head_and_says_where_it_stopped() {
    let mut fdc = with_a_disk();
    // RECALIBRATE, then SENSE INTERRUPT STATUS.
    command(&mut fdc, &[0x07, 0x00]);
    let result = command(&mut fdc, &[0x08]);
    assert_eq!(result.len(), 2, "ST0 and the cylinder");
    assert_eq!(result[0] & 0x20, 0x20, "seek end: {:02X}", result[0]);
    assert_eq!(result[1], 0, "back at track 0");

    // SEEK to track 10.
    command(&mut fdc, &[0x0F, 0x00, 10]);
    let result = command(&mut fdc, &[0x08]);
    assert_eq!(result[0] & 0x20, 0x20);
    assert_eq!(result[1], 10, "the head is at track 10");

    // Asked again with nothing to report, it says so rather than repeating
    // itself: a polling loop has to be able to stop.
    let result = command(&mut fdc, &[0x08]);
    assert_eq!(result[0], 0x80, "invalid: nothing happened since");
}

/// Reading a sector: the identity is matched on the address mark, and the
/// bytes come back through the data register.
#[test]
fn a_sector_can_be_read_by_its_identity() {
    let mut fdc = with_a_disk();
    // Something to recognise in track 2, sector $C3.
    if let Some(drive) = fdc.drives[0].as_mut() {
        let track = drive.disk.track_mut(2, 0).unwrap();
        let sector = track.sectors.iter_mut().find(|s| s.r == 0xC3).unwrap();
        sector.data[0] = 0x42;
        sector.data[511] = 0x99;
    }
    command(&mut fdc, &[0x0F, 0x00, 2]); // SEEK to track 2
    command(&mut fdc, &[0x08]); // and acknowledge it

    // READ DATA: unit/head, C, H, R, N, EOT, GPL, DTL.
    for byte in [0x46u8, 0x00, 2, 0, 0xC3, 2, 0xC3, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert_eq!(
        fdc.status() & (RQM | DIO | CB),
        RQM | DIO | CB,
        "it should be handing data back"
    );
    let data: Vec<u8> = (0..512).map(|_| fdc.read()).collect();
    assert_eq!(data[0], 0x42);
    assert_eq!(data[511], 0x99);

    // Then the seven result bytes.
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 {
        result.push(fdc.read());
    }
    assert_eq!(result.len(), 7, "ST0-2 and the sector's identity");
    assert_eq!(result[0] & 0xC0, 0, "no error: {:02X}", result[0]);
}

/// A sector that is not there is reported rather than answered with rubbish.
#[test]
fn a_sector_that_is_not_there_is_an_error() {
    let mut fdc = with_a_disk();
    for byte in [0x46u8, 0x00, 0, 0, 0x77, 2, 0x77, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 {
        result.push(fdc.read());
    }
    assert_eq!(result.len(), 7);
    assert_eq!(result[0] & 0x40, 0x40, "abnormal end: {:02X}", result[0]);
    assert_eq!(result[1] & 0x04, 0x04, "no data: {:02X}", result[1]);
}

/// Writing a sector puts the bytes on the disk and marks it changed, so
/// something knows to save it.
#[test]
fn a_sector_can_be_written_and_the_disk_knows_it_changed() {
    let mut fdc = with_a_disk();
    assert!(!fdc.drives[0].as_ref().unwrap().disk.dirty);

    // WRITE DATA to track 0, sector $C1.
    for byte in [0x45u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert_eq!(fdc.status() & DIO, 0, "it should be taking data");
    for i in 0..512 {
        fdc.write((i & 0xFF) as u8);
    }
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 {
        result.push(fdc.read());
    }
    assert_eq!(result[0] & 0xC0, 0, "no error: {:02X}", result[0]);

    let drive = fdc.drives[0].as_ref().unwrap();
    let sector = drive
        .disk
        .track(0, 0)
        .unwrap()
        .sectors
        .iter()
        .find(|s| s.r == 0xC1)
        .unwrap();
    assert_eq!(sector.data[0], 0);
    assert_eq!(sector.data[255], 255);
    assert!(drive.disk.dirty, "the disk should know it was written to");
}

/// A write to a protected disk is refused, and the program is told.
#[test]
fn a_write_to_a_protected_disk_is_refused() {
    let mut fdc = Fdc::new();
    fdc.motor = true;
    fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, true));
    for byte in [0x45u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 {
        result.push(fdc.read());
    }
    assert_eq!(result[1] & 0x02, 0x02, "not writable: {:02X}", result[1]);
    assert!(
        !fdc.drives[0].as_ref().unwrap().disk.dirty,
        "and nothing was written"
    );
}

/// READ ID says what the head is over, which is how a program works out what
/// format a disk is in.
#[test]
fn read_id_says_what_is_under_the_head() {
    let mut fdc = with_a_disk();
    command(&mut fdc, &[0x0F, 0x00, 5]);
    command(&mut fdc, &[0x08]);
    let result = command(&mut fdc, &[0x4A, 0x00]);
    assert_eq!(result.len(), 7);
    assert_eq!(result[3], 5, "the cylinder it is on");
    assert_eq!(
        result[5], 0xC1,
        "and the first sector of a data-format disk"
    );
}

/// An unknown command is answered rather than swallowed, or a program waiting
/// for a result waits for ever.
#[test]
fn an_invalid_command_is_answered() {
    let mut fdc = with_a_disk();
    let result = command(&mut fdc, &[0x1F]);
    assert_eq!(result, vec![0x80], "invalid command");
}

/// Formatting a track replaces what was there, with the sector identities the
/// program hands over.
#[test]
fn formatting_a_track_writes_the_identities_it_is_given() {
    let mut fdc = with_a_disk();
    command(&mut fdc, &[0x0F, 0x00, 1]);
    command(&mut fdc, &[0x08]);
    // FORMAT TRACK: N, sectors per track, gap 3, filler.
    for byte in [0x4Du8, 0x00, 2, 3, 0x2A, 0xAA] {
        fdc.write(byte);
    }
    // Three sectors, four bytes each: C, H, R, N.
    for r in [0x41u8, 0x42, 0x43] {
        for byte in [1, 0, r, 2] {
            fdc.write(byte);
        }
    }
    let mut result = Vec::new();
    while fdc.status() & DIO != 0 {
        result.push(fdc.read());
    }
    assert_eq!(result[0] & 0xC0, 0, "no error: {:02X}", result[0]);

    let drive = fdc.drives[0].as_ref().unwrap();
    let track = drive.disk.track(1, 0).unwrap();
    assert_eq!(track.sectors.len(), 3, "three sectors now");
    let ids: Vec<u8> = track.sectors.iter().map(|s| s.r).collect();
    assert_eq!(ids, vec![0x41, 0x42, 0x43]);
    assert!(
        track.sectors[0].data.iter().all(|b| *b == 0xAA),
        "and filled with what was asked for"
    );
    assert!(drive.disk.dirty);
}

/// The whole path, with the machine's own ROM driving it: a +3 with a blank
/// disk in the drive, asked to catalogue it.
///
/// This is what says the controller works, rather than that it answers the
/// questions these tests thought to ask. +3DOS seeks, reads the directory
/// sectors and finds them empty, which is what a freshly formatted disk is.
#[test]
fn the_plus3_rom_can_read_a_blank_disk() {
    use zx_rustrum::machine::{Model, Spectrum, FRAME_T};

    let Ok(rom) = std::fs::read("roms/plus3.rom") else {
        eprintln!("need roms/plus3.rom; skipping");
        return;
    };
    let mut spec = Spectrum::new();
    spec.set_model(Model::Plus3, &rom);
    spec.reset();
    spec.bus.fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, false));

    // The +3 comes up on its menu with "+3 BASIC" already picked, so ENTER
    // takes it there.
    let press = |spec: &mut Spectrum, keys: &[(usize, u8)], frames: u32| {
        for (row, bit) in keys {
            spec.bus.keys[*row] &= !(1 << bit);
        }
        for _ in 0..frames {
            spec.run(FRAME_T);
        }
        for (row, bit) in keys {
            spec.bus.keys[*row] |= 1 << bit;
        }
        for _ in 0..frames {
            spec.run(FRAME_T);
        }
    };
    // The motor is watched from the start: the ROM looks for a disk as it
    // comes up, and turns the motor off again when it has finished with it,
    // so what matters is that it ran rather than that it is running now.
    let mut motor_ran = false;
    for _ in 0..300 {
        spec.run(FRAME_T);
        motor_ran |= spec.bus.fdc.motor;
    }
    press(&mut spec, &[(6, 0)], 6); // ENTER: +3 BASIC
    for _ in 0..100 {
        spec.run(FRAME_T);
        motor_ran |= spec.bus.fdc.motor;
    }

    let before = spec.bus.fdc.commands;
    // CAT, then ENTER.
    press(&mut spec, &[(0, 3)], 6); // C
    press(&mut spec, &[(1, 0)], 6); // A
    press(&mut spec, &[(2, 4)], 6); // T
    press(&mut spec, &[(6, 0)], 6); // ENTER
    for _ in 0..400 {
        spec.run(FRAME_T);
        motor_ran |= spec.bus.fdc.motor;
    }

    let fdc = &spec.bus.fdc;
    assert!(
        fdc.commands > before,
        "the ROM should have talked to the controller: {} commands in all",
        fdc.commands
    );
    assert!(
        motor_ran,
        "and turned the motor on, which is bit 3 of port $1FFD"
    );
    assert_eq!(
        fdc.last_command.map(|c| c & 0x1F),
        Some(0x06),
        "the last thing it did should be a READ DATA — the directory; it was \
         ${:02X?}",
        fdc.last_command
    );
    assert_eq!(
        fdc.errors,
        0,
        "and read the disk without an error; {} commands, last ${:02X}",
        fdc.commands,
        fdc.last_command.unwrap_or(0)
    );
}

/// Reading the screen back as text, by matching each character cell against
/// the ROM's own font. A disk test that cannot read what the machine printed
/// is a test of what the controller was asked, not of what it answered.
fn screen_text(spec: &zx_rustrum::machine::Spectrum, rom: &[u8]) -> Vec<String> {
    // The +3's four ROMs are one file; the 48K BASIC ROM is the last of them,
    // and its font is where a 48K's is.
    let font = &rom[0xC000 + 0x3D00..0xC000 + 0x3D00 + 96 * 8];
    let mut lines = Vec::new();
    for row in 0..24usize {
        let mut line = String::new();
        for col in 0..32usize {
            let mut cell = [0u8; 8];
            for (i, byte) in cell.iter_mut().enumerate() {
                let y = row * 8 + i;
                // The display file's thirds and rows, which is why this is not
                // simply row * 32.
                let addr = 0x4000 + ((y & 0xC0) << 5) + ((y & 0x07) << 8) + ((y & 0x38) << 2) + col;
                *byte = spec.bus.peek_raw(addr as u16);
            }
            let mut found = ' ';
            for c in 0..96usize {
                let glyph = &font[c * 8..c * 8 + 8];
                // Inverse video is the same glyph, so it reads as the same
                // character: the menu and the report line use it.
                let inverted: Vec<u8> = glyph.iter().map(|b| !b).collect();
                if glyph == cell || inverted == cell {
                    found = (32 + c as u8) as char;
                    break;
                }
            }
            line.push(found);
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// A +3, its own ROM, and a disk: what the machine prints when it is asked to
/// catalogue a blank one, and what happens when it is asked to save.
///
/// This is the test that says the controller works. The others say it answers
/// the questions they thought to ask.
#[test]
fn the_plus3_catalogues_and_writes_to_a_disk() {
    use zx_rustrum::machine::{Model, Spectrum, FRAME_T};

    let Ok(rom) = std::fs::read("roms/plus3.rom") else {
        eprintln!("need roms/plus3.rom; skipping");
        return;
    };
    let mut spec = Spectrum::new();
    spec.set_model(Model::Plus3, &rom);
    spec.reset();
    spec.bus.fdc.drives[0] = Some(Drive::new(Disk::blank("test"), None, false));

    let hold = |spec: &mut Spectrum, keys: &[(usize, u8)], frames: u32| {
        for (row, bit) in keys {
            spec.bus.keys[*row] &= !(1 << bit);
        }
        for _ in 0..frames {
            spec.run(FRAME_T);
        }
        for (row, bit) in keys {
            spec.bus.keys[*row] |= 1 << bit;
        }
        for _ in 0..frames {
            spec.run(FRAME_T);
        }
    };
    let key_of = |c: char| -> Option<(usize, u8, bool)> {
        const MATRIX: &[(char, usize, u8)] = &[
            ('1', 3, 0),
            ('2', 3, 1),
            ('3', 3, 2),
            ('4', 3, 3),
            ('5', 3, 4),
            ('6', 4, 4),
            ('7', 4, 3),
            ('8', 4, 2),
            ('9', 4, 1),
            ('0', 4, 0),
            ('Q', 2, 0),
            ('W', 2, 1),
            ('E', 2, 2),
            ('R', 2, 3),
            ('T', 2, 4),
            ('Y', 5, 4),
            ('U', 5, 3),
            ('I', 5, 2),
            ('O', 5, 1),
            ('P', 5, 0),
            ('A', 1, 0),
            ('S', 1, 1),
            ('D', 1, 2),
            ('F', 1, 3),
            ('G', 1, 4),
            ('H', 6, 4),
            ('J', 6, 3),
            ('K', 6, 2),
            ('L', 6, 1),
            ('\n', 6, 0),
            ('Z', 0, 1),
            ('X', 0, 2),
            ('C', 0, 3),
            ('V', 0, 4),
            ('B', 7, 4),
            ('N', 7, 3),
            ('M', 7, 2),
            (' ', 7, 0),
        ];
        let upper = c.to_ascii_uppercase();
        if let Some((_, r, b)) = MATRIX.iter().find(|(k, _, _)| *k == upper) {
            return Some((*r, *b, false));
        }
        // What SYMBOL SHIFT gives, for the few this needs.
        const SYMBOLS: &[(char, usize, u8)] = &[('"', 5, 0), (',', 7, 3)];
        SYMBOLS
            .iter()
            .find(|(k, _, _)| *k == c)
            .map(|(_, r, b)| (*r, *b, true))
    };
    let typed = |spec: &mut Spectrum, text: &str| {
        for c in text.chars() {
            let Some((row, bit, shifted)) = key_of(c) else {
                continue;
            };
            if shifted {
                spec.bus.keys[7] &= !(1 << 1);
            }
            hold(spec, &[(row, bit)], 6);
            if shifted {
                spec.bus.keys[7] |= 1 << 1;
            }
        }
    };

    // The menu comes up with Loader picked; down one is +3 BASIC.
    for _ in 0..250 {
        spec.run(FRAME_T);
    }
    let menu = screen_text(&spec, &rom).join("\n");
    assert!(
        menu.contains("Drives A: and M: available"),
        "the ROM should find the drive: {menu}"
    );
    hold(&mut spec, &[(0, 0), (4, 4)], 8); // CAPS SHIFT with 6: down
    hold(&mut spec, &[(6, 0)], 8); // ENTER
    for _ in 0..120 {
        spec.run(FRAME_T);
    }

    typed(&mut spec, "cat\n");
    for _ in 0..300 {
        spec.run(FRAME_T);
    }
    let listing = screen_text(&spec, &rom).join("\n");
    assert!(
        listing.contains("No files found"),
        "a blank disk holds nothing: {listing}"
    );
    assert!(
        listing.contains("178K free"),
        "and a +3 data disk has 178K of room on it: {listing}"
    );
    assert_eq!(spec.bus.fdc.errors, 0, "and it read it without an error");

    // Now write something. The report line eats the next keypress, so a
    // throwaway one goes first.
    let before = spec.bus.fdc.commands;
    typed(&mut spec, " ");
    for _ in 0..120 {
        spec.run(FRAME_T);
    }
    typed(&mut spec, "save \"t\" code 30000,10\n");
    for _ in 0..600 {
        spec.run(FRAME_T);
    }
    let after = screen_text(&spec, &rom).join("\n");
    assert!(
        after.contains("0 OK"),
        "the save should have finished without complaint: {after}"
    );
    assert!(
        spec.bus.fdc.commands > before,
        "and gone through the controller"
    );
    assert_eq!(spec.bus.fdc.errors, 0);

    // What was written is on the disk: +3DOS keeps its directory in the first
    // sectors of track 0, and the name is in it.
    let drive = spec.bus.fdc.drives[0].as_ref().unwrap();
    assert!(drive.disk.dirty, "the disk knows it was written to");
    let directory: Vec<u8> = drive
        .disk
        .track(0, 0)
        .unwrap()
        .sectors
        .iter()
        .flat_map(|s| s.data.iter().copied())
        .collect();
    // A CP/M directory entry is user number, eight bytes of name, three of
    // type. The name is padded with spaces, so "T" is "T       ".
    let entry = directory
        .chunks(32)
        .find(|entry| entry[0] == 0 && entry[1] == b'T' && entry[2] == b' ');
    assert!(
        entry.is_some(),
        "the file's name should be in the directory: {:02X?}",
        &directory[..64]
    );
}

/// At Normal speed the drive makes the program wait, as a real one does: the
/// motor has to come up to speed and the sector has to come round under the
/// head. At Fastload nothing waits at all.
#[test]
fn normal_speed_makes_the_program_wait_and_fastload_does_not() {
    use zx_rustrum::fdc::{delay, Speed};

    let read = |speed: Speed| -> (bool, u64) {
        let mut fdc = with_a_disk();
        fdc.speed = speed;
        // The motor has just been switched on, as the ROM does before a read.
        fdc.at(0);
        for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
            fdc.write(byte);
        }
        // How long the program has to poll before the controller answers.
        let mut waited = 0u64;
        while fdc.busy() && waited < delay::MOTOR_UP * 2 {
            waited += 1000;
            fdc.at(waited);
        }
        (fdc.status() & DIO != 0, waited)
    };

    let (answered, waited) = read(Speed::Fastload);
    assert!(answered, "Fastload should have the data ready");
    assert_eq!(waited, 0, "and not have waited at all");

    let (answered, waited) = read(Speed::Normal);
    assert!(answered, "Normal should answer in the end");
    assert!(
        waited >= delay::MOTOR_UP,
        "after the motor has come up to speed: {waited} T-states"
    );
    assert!(
        waited < delay::MOTOR_UP * 2,
        "and not for ever: {waited} T-states"
    );
}

/// Seeking across the disk takes longer than seeking next door, because the
/// head has further to go.
#[test]
fn a_long_seek_takes_longer_than_a_short_one() {
    use zx_rustrum::fdc::Speed;

    let seek_to = |track: u8| -> u64 {
        let mut fdc = with_a_disk();
        fdc.speed = Speed::Normal;
        // Long enough ago that the motor is up to speed and not in the way.
        fdc.motor = false;
        fdc.at(0);
        fdc.motor = true;
        fdc.at(10_000_000);
        command(&mut fdc, &[0x0F, 0x00, track]);
        let mut waited = 10_000_000u64;
        while fdc.busy() && waited < 20_000_000 {
            waited += 100;
            fdc.at(waited);
        }
        waited - 10_000_000
    };

    let near = seek_to(1);
    let far = seek_to(39);
    assert!(near > 0, "even one track takes a moment: {near}");
    assert!(
        far > near * 5,
        "and thirty-nine take much longer: {far} against {near}"
    );
}

/// The light on the front of the drive is on while it is being read, and goes
/// out afterwards.
#[test]
fn the_drive_light_shows_what_the_drive_is_doing() {
    let mut fdc = with_a_disk();
    fdc.at(1_000_000);
    assert!(!fdc.light(), "nothing has happened yet");

    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert!(fdc.light(), "a read lights it");

    // A twentieth of a second later it is out again.
    fdc.at(1_000_000 + 3_546_900 / 10);
    assert!(!fdc.light(), "and it goes out when nothing is happening");
}

/// What has been read and written is remembered per sector, so the window can
/// draw it, and it fades.
#[test]
fn the_sectors_touched_are_remembered_and_fade() {
    let mut fdc = with_a_disk();
    fdc.at(0);
    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert_eq!(
        fdc.reads.get(&(0, 0, 0xC1)).copied(),
        Some(255),
        "the sector just read is at full brightness"
    );
    assert!(fdc.writes.is_empty(), "and nothing has been written");

    // Finish the read: the controller is handing data back until it has been
    // taken, and a command written into that is thrown away.
    while fdc.status() & DIO != 0 {
        fdc.read();
    }

    // A write to another sector, which is remembered apart from the reads.
    for byte in [0x45u8, 0x00, 0, 0, 0xC2, 2, 0xC2, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    for i in 0..512 {
        fdc.write(i as u8);
    }
    assert_eq!(fdc.writes.get(&(0, 0, 0xC2)).copied(), Some(255));

    // And it fades, or a disk read an hour ago would look like one happening
    // now.
    for _ in 0..10 {
        fdc.fade();
    }
    let after = fdc.reads.get(&(0, 0, 0xC1)).copied().unwrap_or(0);
    assert!(after > 0 && after < 255, "faded, not gone: {after}");
    for _ in 0..100 {
        fdc.fade();
    }
    assert!(fdc.reads.is_empty(), "and gone in the end");
}

/// What the disk records against a sector comes back in the result bytes.
///
/// A preserved disk carries a deleted-data mark or a deliberate CRC error, and
/// those are the whole of some protections: telling a program it read an
/// ordinary sector where the disk says the mark is deleted is telling it the
/// disk is a copy.
#[test]
fn a_deleted_sector_is_reported_as_one() {
    let mut fdc = with_a_disk();
    // Mark one sector deleted, as a protection would, and put a CRC error on
    // another.
    if let Some(drive) = fdc.drives[0].as_mut() {
        let track = drive.disk.track_mut(0, 0).unwrap();
        track.sectors.iter_mut().find(|s| s.r == 0xC2).unwrap().st2 = 0x40;
        let bad = track.sectors.iter_mut().find(|s| s.r == 0xC3).unwrap();
        bad.st1 = 0x20;
        bad.st2 = 0x20;
    }

    let read = |fdc: &mut Fdc, sector: u8| -> Vec<u8> {
        for byte in [0x46u8, 0x00, 0, 0, sector, 2, sector, 0x2A, 0xFF] {
            fdc.write(byte);
        }
        while fdc.status() & DIO != 0 && fdc.status() & 0x20 != 0 {
            fdc.read();
        }
        let mut result = Vec::new();
        while fdc.status() & DIO != 0 {
            result.push(fdc.read());
        }
        result
    };

    // An ordinary sector: nothing to report.
    let result = read(&mut fdc, 0xC1);
    assert_eq!(result[2] & 0x40, 0, "ST2: {:02X}", result[2]);

    // The deleted one: the data is still handed over, and the Control Mark
    // says what it was.
    let result = read(&mut fdc, 0xC2);
    assert_eq!(
        result[2] & 0x40,
        0x40,
        "the control mark should be set: ST2 {:02X}",
        result[2]
    );
    assert_eq!(result[0] & 0xC0, 0x40, "and the command ends abnormally");

    // And the one with a bad CRC says so in both status bytes, as the chip
    // does: the error is in the data field.
    let result = read(&mut fdc, 0xC3);
    assert_eq!(result[1] & 0x20, 0x20, "ST1: {:02X}", result[1]);
    assert_eq!(result[2] & 0x20, 0x20, "ST2: {:02X}", result[2]);
}

/// With the skip flag set, a sector marked deleted is passed over rather than
/// read — which is the other half of what the flag is for.
#[test]
fn a_deleted_sector_is_skipped_when_the_command_says_to() {
    let mut fdc = with_a_disk();
    if let Some(drive) = fdc.drives[0].as_mut() {
        let track = drive.disk.track_mut(0, 0).unwrap();
        track.sectors.iter_mut().find(|s| s.r == 0xC1).unwrap().st2 = 0x40;
        // Something to tell the second sector's data apart by.
        track.sectors.iter_mut().find(|s| s.r == 0xC2).unwrap().data[0] = 0x5A;
    }

    // READ DATA with SK set, from $C1 to $C2.
    for byte in [0x66u8, 0x00, 0, 0, 0xC1, 2, 0xC2, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    let mut data = Vec::new();
    while fdc.status() & 0x20 != 0 && fdc.status() & DIO != 0 {
        data.push(fdc.read());
    }
    assert_eq!(
        data.first().copied(),
        Some(0x5A),
        "the deleted sector should have been passed over, and $C2 read instead"
    );
}

/// The reset line goes to the controller too.
///
/// A machine reset in the middle of a command left it handing over data nobody
/// was going to take, so the ROM's next command found it talking rather than
/// listening — and the ROM sat at $211A polling the status register until it
/// gave up, twenty-two seconds later.
#[test]
fn a_reset_puts_the_controller_back_to_waiting_for_a_command() {
    let mut fdc = with_a_disk();
    // Half a read: the controller is handing data back.
    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert_eq!(fdc.status() & DIO, DIO, "it is talking, not listening");

    fdc.reset();
    assert_eq!(
        fdc.status(),
        RQM,
        "after a reset it wants a command and nothing else: ${:02X}",
        fdc.status()
    );
    assert!(
        fdc.drives[0].is_some(),
        "and the disk is still in the drive"
    );

    // And it takes one.
    let result = command(&mut fdc, &[0x04, 0x00]);
    assert_eq!(result.len(), 1, "SENSE DRIVE STATUS answers");
}

/// A wait timed against the machine's clock is not a wait once that clock has
/// gone backwards.
///
/// A reset puts the machine's T-state count to zero, and a snapshot puts it
/// wherever it was saved. A drive left waiting for a moment millions of
/// T-states in the future reports itself busy until the clock catches up,
/// which is the same twenty-two seconds by another route.
#[test]
fn a_clock_that_goes_backwards_does_not_leave_the_drive_busy() {
    use zx_rustrum::fdc::Speed;

    let mut fdc = with_a_disk();
    fdc.speed = Speed::Normal;
    fdc.at(10_000_000);
    // A read, which makes it wait for the motor and the sector.
    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        fdc.write(byte);
    }
    assert!(fdc.busy(), "it is waiting for the drive");

    // The machine is reset: the clock goes back to nothing.
    fdc.at(0);
    assert!(
        !fdc.busy(),
        "a wait that ends ten million T-states from now is not a wait any more"
    );
    assert_ne!(fdc.status() & RQM, 0, "and it answers again");
}

/// The whole of it, with the machine's own ROM: a +3 reset in the middle of a
/// disk command comes back to its menu instead of sitting on the status
/// register.
///
/// $211A is where the +3's disk driver polls the controller. A machine that
/// spends twenty-two seconds there is a machine that looks dead, and that is
/// what a reset used to leave behind.
#[test]
fn a_plus3_reset_mid_command_comes_straight_back() {
    use zx_rustrum::fdc::Speed;
    use zx_rustrum::machine::{Model, Spectrum, FRAME_T};

    let Ok(rom) = std::fs::read("roms/plus3.rom") else {
        eprintln!("need roms/plus3.rom; skipping");
        return;
    };
    let mut spec = Spectrum::new();
    spec.set_model(Model::Plus3, &rom);
    spec.reset();
    spec.bus.fdc.drives[0] = Some(Drive::new(Disk::blank("t"), None, false));
    spec.bus.fdc.speed = Speed::Normal;
    for _ in 0..200 {
        spec.run(FRAME_T);
    }

    // A command left half-done, and the reset button.
    for byte in [0x46u8, 0x00, 0, 0, 0xC1, 2, 0xC1, 0x2A, 0xFF] {
        spec.bus.fdc.write(byte);
    }
    spec.reset();

    // The controller comes back waiting for a command, not half-way through
    // the one it was given: the reset line reaches it as well as the CPU.
    assert_eq!(
        spec.bus.fdc.status(),
        RQM,
        "the controller should be waiting for a command: ${:02X}",
        spec.bus.fdc.status()
    );

    let mut polling = 0u32;
    for _ in 0..250u32 {
        for _ in 0..200 {
            spec.step_instruction();
            if spec.cpu.pc == 0x211A {
                polling += 1;
            }
        }
        spec.run(FRAME_T);
    }
    assert_eq!(
        polling, 0,
        "the machine should not be sitting on the controller's status register"
    );
    assert!(
        spec.bus.fdc.drives[0].is_some(),
        "and the disk is still in the drive"
    );
}
