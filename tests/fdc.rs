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
