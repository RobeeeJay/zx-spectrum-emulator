//! The Interface 1: its shadow ROM, and the microdrives behind it.

use zx_rustrum::if1::{Drive, If1, Where, MAX_DRIVES, SECTOR_T};
use zx_rustrum::microdrive::Cartridge;

fn with_a_cartridge() -> If1 {
    let mut if1 = If1::new(1);
    if1.rom = Some(vec![0xC9; 8192]);
    let mut cart = Cartridge::blank("Test", 10);
    // Something recognisable in the first sector's record.
    cart.sectors[0].record[15] = 0x42;
    if1.drives[0] = Drive::loaded(cart, None, false);
    if1
}

/// The shadow ROM is not in the machine until the machine asks for it.
///
/// The interface watches the address bus: a fetch from $0008 or $1708 — the
/// ROM's error handler and its close-files hook, where a program lands after a
/// microdrive command — pages its own 8K in, and a fetch from $0700 pages it
/// out. That is the whole mechanism, and everything the microdrives can do is
/// done by that ROM.
#[test]
fn the_shadow_rom_pages_itself_in_when_the_machine_lands_on_a_hook() {
    let mut if1 = with_a_cartridge();
    assert!(!if1.paged, "not there until it is wanted");
    assert_eq!(if1.rom_byte(0x0000), None);

    assert!(if1.on_fetch(0x0008), "the error handler pages it in");
    assert!(if1.paged);
    assert_eq!(if1.rom_byte(0x0000), Some(0xC9), "and its ROM answers");
    assert_eq!(
        if1.rom_byte(0x2000),
        None,
        "over the bottom 8K and no further"
    );

    assert!(!if1.on_fetch(0x1234), "and nothing else moves it");
    assert!(if1.paged);

    assert!(if1.on_fetch(0x0700), "$0700 pages it out again");
    assert!(!if1.paged);
    assert_eq!(if1.rom_byte(0x0000), None);

    // The other hook does the same.
    assert!(if1.on_fetch(0x1708));
    assert!(if1.paged);
}

/// With no ROM in the socket the interface pages nothing, which is what a
/// machine with an empty socket does.
#[test]
fn an_interface_with_no_rom_pages_nothing() {
    let mut if1 = If1::new(1);
    assert!(!if1.on_fetch(0x0008));
    assert!(!if1.paged);
    assert_eq!(if1.rom_byte(0x0000), None);
}

/// One bit selects one of eight drives, by being shifted along the chain.
#[test]
fn the_motor_bit_walks_down_the_chain_of_drives() {
    let mut if1 = If1::new(4);
    assert_eq!(if1.selected, 0, "nothing turning to start with");

    // A 1 on the motor line starts the first drive.
    if1.write_control(0x01);
    assert_eq!(if1.selected, 1);
    assert!(if1.motor_on());

    // Each further pulse moves it one further down.
    if1.write_control(0x01);
    assert_eq!(if1.selected, 2);
    if1.write_control(0x01);
    assert_eq!(if1.selected, 3);

    // And clearing the chain stops everything.
    if1.write_control(0x00);
    assert_eq!(if1.selected, 0);
    assert!(!if1.motor_on());
}

/// The chain is as long as the interface has drives on it.
#[test]
fn the_number_of_drives_can_be_changed_without_losing_the_cartridges() {
    let mut if1 = If1::new(1);
    if1.drives[0] = Drive::loaded(Cartridge::blank("One", 10), None, false);
    if1.set_drive_count(4);
    assert_eq!(if1.drive_count(), 4);
    assert!(
        if1.drives[0].cartridge.is_some(),
        "the cartridge that was in drive 1 is still in it"
    );

    if1.set_drive_count(1);
    assert_eq!(if1.drive_count(), 1);
    assert!(if1.drives[0].cartridge.is_some());

    // Eight is as many as the interface can address.
    if1.set_drive_count(50);
    assert_eq!(if1.drive_count(), MAX_DRIVES);
}

/// The tape runs past the head: a gap, then the sector's header, then its
/// record — which is the order the ROM waits for them in.
///
/// The header is fifteen bytes of five hundred and forty-three, so it goes
/// past in a fiftieth of the time the sector takes. Walking the sector is the
/// way to test that: picking a moment and hoping it lands in the header is how
/// the first version of this test failed.
#[test]
fn the_tape_runs_past_the_head_gap_then_header_then_record() {
    let mut if1 = with_a_cartridge();
    if1.at(0);
    if1.write_control(0x01);

    let mut seen: Vec<&str> = Vec::new();
    for step in 0..200u64 {
        if1.at(step * SECTOR_T / 200);
        let now = match if1.head() {
            Where::Gap => "gap",
            Where::Header(_) => "header",
            Where::Record(_) => "record",
        };
        if seen.last() != Some(&now) {
            seen.push(now);
        }
    }
    assert_eq!(
        seen,
        vec!["gap", "header", "record"],
        "the head passes the gap, then the header, then the record"
    );

    // The lines say the same thing: the gap line low in the gap, sync low once
    // a sector has started.
    if1.at(0);
    assert_eq!(if1.read_status() & 0x01, 0, "the gap line is low in a gap");
    if1.at(SECTOR_T / 2);
    assert_eq!(if1.read_status() & 0x02, 0, "and sync is low on a sector");

    // A whole sector's time later, the next sector is under the head.
    assert_eq!(if1.drive().unwrap().sector(), 0);
    if1.at(SECTOR_T + SECTOR_T / 2);
    assert_eq!(if1.drive().unwrap().sector(), 1, "the tape moved on");

    // And it goes round: a cartridge is a loop, not a reel.
    if1.at(SECTOR_T * 10 + SECTOR_T / 2);
    assert_eq!(
        if1.drive().unwrap().sector(),
        0,
        "ten sectors on a ten-sector cartridge is back where it started"
    );
}

/// The bytes the ROM reads are the bytes on the tape.
#[test]
fn the_data_port_hands_over_what_is_under_the_head() {
    let mut if1 = with_a_cartridge();
    if1.at(0);
    if1.write_control(0x01);

    // Walk the sector, taking each byte as it passes, and compare what came
    // out with what is on the cartridge.
    let expected = if1.drive().unwrap().cartridge.as_ref().unwrap().sectors[0].clone();
    let mut header_bytes = Vec::new();
    let mut record_start = Vec::new();
    for step in 0..4000u64 {
        if1.at(step * SECTOR_T / 4000);
        match if1.head() {
            Where::Header(at) if at == header_bytes.len() => header_bytes.push(if1.read_data()),
            Where::Record(at) if at == record_start.len() && at < 20 => {
                record_start.push(if1.read_data())
            }
            _ => {}
        }
    }
    assert_eq!(
        header_bytes,
        expected.header.to_vec(),
        "the header came off the tape as it is written on it"
    );
    assert_eq!(
        record_start,
        expected.record[..20].to_vec(),
        "and so did the front of the record"
    );

    // In the gap there is nothing to read.
    if1.at(0);
    assert_eq!(if1.read_data(), 0xFF, "a gap reads as nothing");
}

/// A drive with no cartridge in it says so, and one whose tab is broken says
/// that too — the ROM asks before it writes.
#[test]
fn an_empty_drive_and_a_protected_cartridge_both_say_so() {
    let mut if1 = If1::new(2);
    if1.at(0);
    if1.write_control(0x01);
    assert_eq!(
        if1.read_status(),
        0xFF,
        "an empty drive drives none of the lines"
    );

    let mut cart = Cartridge::blank("Protected", 10);
    cart.write_protected = true;
    if1.drives[0] = Drive::loaded(cart, None, false);
    if1.at(SECTOR_T / 2);
    assert_eq!(
        if1.read_status() & 0x04,
        0,
        "the write-protect line is low: ${:02X}",
        if1.read_status()
    );
    assert!(!if1.drives[0].writable());

    // And a cartridge mounted read-only is the same as far as the machine is
    // concerned, whatever its tab says.
    if1.drives[0] = Drive::loaded(Cartridge::blank("Fine", 10), None, true);
    assert!(!if1.drives[0].writable());
}

/// A reset pages the ROM out and stops the drives.
#[test]
fn a_reset_pages_the_rom_out_and_stops_the_tape() {
    let mut if1 = with_a_cartridge();
    if1.on_fetch(0x0008);
    if1.write_control(0x01);
    assert!(if1.paged && if1.motor_on());

    if1.reset();
    assert!(!if1.paged, "the ROM is out of the way");
    assert!(!if1.motor_on(), "and nothing is turning");
    assert!(
        if1.drives[0].cartridge.is_some(),
        "the cartridge stays in the drive"
    );
}

/// The interface's ports reach it through the machine, on any model: an
/// add-on is an add-on whether or not the machine it is plugged into has
/// paging.
///
/// The first wiring put these inside the branch that handles the 128K's paging
/// ports, so on a 48K — where an Interface 1 usually lives — nothing reached
/// it at all.
#[test]
fn the_interfaces_ports_reach_it_on_a_48k() {
    use zx_rustrum::machine::{Model, Spectrum};
    use zx_rustrum::z80::Bus;

    for model in [Model::Spectrum48, Model::Spectrum128] {
        let mut spec = Spectrum::with_model(model);
        let mut if1 = If1::new(2);
        if1.drives[0] = Drive::loaded(Cartridge::blank("Test", 10), None, false);
        spec.bus.if1 = Some(if1);

        // A pulse on the motor line starts the first drive, through the port.
        spec.bus.io_write(0x00EF, 0x01);
        let if1 = spec.bus.if1.as_ref().unwrap();
        assert_eq!(
            if1.selected, 1,
            "{model:?}: the control port should have reached the interface"
        );

        // And the status port answers with what the drive says.
        let status = spec.bus.io_read(0x00EF);
        assert_ne!(
            status, 0xFF,
            "{model:?}: a drive with a cartridge in it drives some of the lines"
        );
    }
}
