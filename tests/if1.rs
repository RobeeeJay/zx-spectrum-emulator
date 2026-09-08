//! The Interface 1: its shadow ROM, and the microdrives behind it.

use zx_rustrum::if1::{Drive, If1, MAX_DRIVES};
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

    // $0700 pages it out, but only after the byte there has been fetched: the
    // shadow ROM's own $0700 is the RET that hands back to the machine's ROM,
    // so that byte has to come from the shadow ROM. This used to page out
    // before the fetch, which ran the 48K ROM's $0700 — the middle of another
    // routine — and the Interface 1 never got past its own initialisation.
    assert!(
        !if1.on_fetch(0x0700),
        "the fetch itself still reads the shadow"
    );
    assert!(if1.paged);
    assert_eq!(if1.rom_byte(0x0700), Some(0xC9), "and it reads the RET");
    assert!(if1.after_fetch(0x0700), "then it is gone");
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
///
/// The bit is latched on the comms clock's falling edge, and it is *low* for a
/// drive that is to run — the ROM starts drive 1 by writing $EE. This test used
/// to drive it the other way up, which is a convention nothing checked until
/// the real ROM was put in the socket and no drive ever turned.
#[test]
fn the_motor_bit_walks_down_the_chain_of_drives() {
    let mut if1 = If1::new(4);
    assert_eq!(if1.selected, 0, "nothing turning to start with");

    clock(&mut if1, true);
    assert_eq!(if1.selected, 1);
    assert!(if1.motor_on());

    // Each further pulse moves the running drive one further down.
    clock(&mut if1, false);
    assert_eq!(if1.selected, 2);
    clock(&mut if1, false);
    assert_eq!(if1.selected, 3);

    // And it falls off the end of the chain: four more pulses and nothing is
    // turning.
    for _ in 0..2 {
        clock(&mut if1, false);
    }
    assert_eq!(if1.selected, 0);
    assert!(!if1.motor_on());
}

/// One pulse of the comms clock, with the motor line high or low.
fn clock(if1: &mut If1, motor: bool) {
    let bit = if motor { 0x00 } else { 0x01 };
    if1.write_control(bit | 0x02);
    if1.write_control(bit);
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

/// The status port says gap for a while, then sync, over and over — which is
/// the pattern the ROM's sector-finding loop at $165A waits for: eight reads
/// with the gap line high, six with it low, then sync.
#[test]
fn the_status_port_alternates_gap_and_sync_as_the_rom_expects() {
    let mut if1 = with_a_cartridge();
    clock(&mut if1, true);

    let seen: Vec<bool> = (0..64).map(|_| if1.read_status() & 0x04 != 0).collect();
    assert!(
        seen[..15].iter().all(|gap| *gap),
        "the gap comes first: {seen:?}"
    );
    assert!(
        seen[15..31].iter().all(|gap| !*gap),
        "then the block, with sync low: {seen:?}"
    );
    assert!(seen[31], "and then the next gap");

    // Sync goes low with the gap line, because it is the block's preamble
    // under the head.
    let mut if1 = with_a_cartridge();
    clock(&mut if1, true);
    for _ in 0..15 {
        assert_ne!(if1.read_status() & 0x02, 0, "no sync while the gap runs");
    }
    assert_eq!(if1.read_status() & 0x02, 0, "sync once the block starts");
}

/// The bytes the ROM reads are the bytes on the tape, and reading them is what
/// moves the tape.
///
/// The head advances with the reads rather than with the clock: the ROM reads
/// a block with `INIR`, 21 T-states a byte, and a tape running at its own
/// speed would hand the same byte over a dozen times.
#[test]
fn the_data_port_hands_over_the_block_under_the_head() {
    let mut if1 = with_a_cartridge();
    clock(&mut if1, true);
    let expected = if1.drive().unwrap().cartridge.as_ref().unwrap().sectors[0].clone();

    let header: Vec<u8> = (0..15).map(|_| if1.read_data()).collect();
    assert_eq!(
        header,
        expected.header.to_vec(),
        "the header came off the tape as it is written on it"
    );

    // Reading past the end of a block gives the last byte again: the block is
    // as long as it is, and the ROM knows how much to take.
    assert_eq!(if1.read_data(), expected.header[14]);

    // A write to the control port puts the head at the start of the next
    // block, which here is the record.
    if1.write_control(0x00);
    let record: Vec<u8> = (0..20).map(|_| if1.read_data()).collect();
    assert_eq!(
        record,
        expected.record[..20].to_vec(),
        "and so did the front of the record"
    );
}

/// What the ROM writes goes onto the tape, after the twelve bytes of preamble
/// that are not part of the block.
#[test]
fn what_the_machine_writes_lands_in_the_sector_under_the_head() {
    let mut if1 = with_a_cartridge();
    clock(&mut if1, true);
    // Past the header, so the head is at the start of the record.
    for _ in 0..15 {
        if1.read_data();
    }
    if1.write_control(0x00);

    for _ in 0..12 {
        if1.write_data(0x00);
    }
    for byte in 0..8u8 {
        if1.write_data(0xA0 + byte);
    }
    let record = &if1.drives[0].cartridge.as_ref().unwrap().sectors[0].record;
    assert_eq!(
        &record[..8],
        &[0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7],
        "the preamble is not part of the block; what follows it is"
    );

    // A cartridge the user asked to keep is not written to.
    let mut if1 = with_a_cartridge();
    if1.drives[0].read_only = true;
    clock(&mut if1, true);
    for _ in 0..15 {
        if1.read_data();
    }
    if1.write_control(0x00);
    for _ in 0..12 {
        if1.write_data(0x00);
    }
    if1.write_data(0x55);
    assert_ne!(
        if1.drives[0].cartridge.as_ref().unwrap().sectors[0].record[0],
        0x55,
        "a read-only cartridge stays as it is"
    );
}

/// A drive with no cartridge in it says so, and one whose tab is broken says
/// that too — the ROM asks before it writes.
#[test]
fn an_empty_drive_and_a_protected_cartridge_both_say_so() {
    let mut if1 = If1::new(2);
    clock(&mut if1, true);
    assert_eq!(
        if1.read_status(),
        0xFF,
        "an empty drive drives none of the lines"
    );

    let mut cart = Cartridge::blank("Protected", 10);
    cart.write_protected = true;
    if1.drives[0] = Drive::loaded(cart, None, false);
    clock(&mut if1, true);
    assert_eq!(
        if1.read_status() & 0x01,
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
    clock(&mut if1, true);
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
        spec.bus.io_write(0x00EF, 0x02);
        spec.bus.io_write(0x00EF, 0x00);
        let if1 = spec.bus.if1.as_ref().unwrap();
        assert_eq!(
            if1.selected, 1,
            "{model:?}: the control port should have reached the interface"
        );

        // And the status port answers with what the drive says: the gap for a
        // few reads, then the block with the gap and sync lines low.
        let seen: Vec<u8> = (0..32).map(|_| spec.bus.io_read(0x00EF)).collect();
        assert!(
            seen.iter().any(|s| s & 0x04 == 0),
            "{model:?}: a drive with a cartridge in it should read a block: {seen:02X?}"
        );
    }
}
