//! The Multiface's latches and ports, without a machine around them.
//!
//! What each model answers, and which way up, is Fuse's reading of the
//! hardware; `tests/multiface_rom.rs` is what says the reading is right, by
//! pressing the button with Romantic Robot's own ROM in the box.

use zx_rustrum::multiface::{Model, Multiface, RAM_LEN, ROM_LEN};

fn fitted(model: Model) -> Multiface {
    let mut mf = Multiface::new(model);
    // A ROM whose bytes say where they came from.
    mf.rom = Some((0..ROM_LEN).map(|i| (i & 0xFF) as u8).collect());
    mf
}

/// The button does nothing at all with an empty box: there is no menu to run.
#[test]
fn an_empty_box_has_nothing_to_run() {
    let mut mf = Multiface::new(Model::One);
    assert!(!mf.press(), "no ROM, no menu");
    assert!(!mf.on_fetch(0x0066), "and nothing pages in at the NMI");
    assert_eq!(mf.mem(0x0000), None);
}

/// The button pulls /NMI, and the fetch from $0066 is what pages the interface
/// in — the hardware decodes that address rather than the interrupt itself.
#[test]
fn the_button_pages_the_interface_in_at_the_nmi() {
    let mut mf = fitted(Model::One);
    assert!(mf.press());
    assert!(!mf.paged, "not until the machine gets there");

    assert!(!mf.on_fetch(0x0038), "the maskable interrupt is not it");
    assert!(mf.on_fetch(0x0066), "the NMI's address is");
    assert!(mf.paged);
    assert_eq!(mf.mem(0x0100), Some(0x00), "its ROM is over the bottom 8K");
    assert_eq!(
        mf.mem(0x4000),
        None,
        "and no further than the machine's own"
    );

    // One press, one entry: the latch the button cleared is set again by an
    // OUT to the interface, which the ROM does on its way out.
    assert!(!mf.press(), "a second press does nothing yet");
    mf.io_write(0x001F, 0);
    assert!(mf.press(), "and something again once the ROM has let go");
}

/// Its 8K of RAM is over the second 8K, and is where the menu works.
#[test]
fn the_interfaces_own_ram_is_written_and_read_back() {
    let mut mf = fitted(Model::One);
    mf.press();
    mf.on_fetch(0x0066);

    assert!(mf.poke(0x2000, 0x42), "the RAM takes a write");
    assert_eq!(mf.mem(0x2000), Some(0x42));
    assert!(mf.poke(0x1000, 0x42), "the ROM takes none");
    assert_eq!(mf.mem(0x1000), Some(0x00));
    assert!(
        !mf.poke(0x8000, 0x42),
        "and the machine's RAM is not its business"
    );
    assert_eq!(mf.ram.len(), RAM_LEN);

    // A reset puts it out of the way, but the RAM is not on the reset line.
    mf.reset();
    assert!(!mf.paged);
    assert_eq!(mf.ram[0], 0x42, "what it was holding is still there");
}

/// Reading one of its ports is what pages it in and out, and the three models
/// do not agree on which way round that is.
#[test]
fn each_model_pages_on_the_port_its_manual_gives() {
    // The One: in at $9F, out at $1F.
    let mut one = fitted(Model::One);
    assert_eq!(one.io_read(0x009F), Some(0xFF), "$9F is one of its ports");
    assert!(one.paged);
    one.io_read(0x001F);
    assert!(!one.paged, "$1F pages it out");
    assert_eq!(one.io_read(0x00FE), None, "and the ULA's port is not its");

    // The 128: in at $BF, out at $3F.
    let mut mf128 = fitted(Model::OneTwentyEight);
    assert_eq!(
        mf128.io_read(0x009F),
        None,
        "the One's port is not the 128's"
    );
    mf128.io_read(0x00BF);
    assert!(mf128.paged);
    mf128.io_read(0x003F);
    assert!(!mf128.paged);

    // The 3: the other way round again.
    let mut mf3 = fitted(Model::Three);
    mf3.io_read(0x003F);
    assert!(mf3.paged, "$3F pages the 3 in");
    mf3.io_read(0x00BF);
    assert!(!mf3.paged, "and $BF pages it out");
}

/// The 128 tells its ROM which of the machine's ROMs was paged in, because it
/// has to put it back before it hands the machine over again.
#[test]
fn the_128_hands_back_what_the_machine_had_banked() {
    let mut mf = fitted(Model::OneTwentyEight);
    mf.io_write(0x7FFD, 0x00);
    assert_eq!(mf.io_read(0x00BF), Some(0x7F), "bit 3 of $7FFD was clear");

    mf.io_read(0x003F);
    mf.io_write(0x7FFD, 0x08);
    assert_eq!(mf.io_read(0x00BF), Some(0xFF), "and now it is set");
}

/// The 3 watches all four of the +3's paging ports, since the machine's memory
/// can be in any of a dozen shapes when the button goes in.
#[test]
fn the_3_remembers_every_write_to_the_paging_ports() {
    let mut mf = fitted(Model::Three);
    mf.io_write(0x1FFD, 0x07);
    mf.io_write(0x7FFD, 0x03);

    // Which of the four is being asked after is in the address, not in a
    // register: A13 and A14 pick it, the same two bits that tell $1FFD from
    // $7FFD.
    assert_eq!(mf.io_read(0x003F), Some(0xF7), "what went to $1FFD");
    assert_eq!(mf.io_read(0x603F), Some(0xF3), "and what went to $7FFD");
}
