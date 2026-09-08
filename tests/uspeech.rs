//! The Currah µSpeech's decoding, without a machine around it.
//!
//! Every address here is from Thomas Busse's measurements of the real
//! hardware; `tests/uspeech_rom.rs` is what says the reading is right, by
//! running Currah's ROM on a 48K and watching it talk.

use zx_rustrum::uspeech::{Uspeech, ROM_LEN};

fn fitted() -> Uspeech {
    let mut u = Uspeech::new();
    // A ROM whose bytes say where in it they came from.
    u.rom = Some((0..ROM_LEN).map(|i| (i & 0xFF) as u8).collect());
    u
}

/// With no ROM in it the box does nothing: there is nothing to page in.
#[test]
fn an_empty_socket_never_pages_anything_in() {
    let mut u = Uspeech::new();
    assert!(!u.touch(0x0038));
    assert!(!u.paged);
    assert_eq!(u.mem(0x0000), None);
}

/// Every access to $0038 turns the interface over, whichever kind it is.
///
/// That one address is the whole of the switching, and it is why the interface
/// works without being asked: the ULA's interrupt is a fetch from $0038, so
/// its ROM gets the machine once a frame and hands it back on the way out.
#[test]
fn every_kind_of_access_to_0038_turns_the_interface_over() {
    let mut u = fitted();
    assert!(u.touch(0x0038), "a fetch, a read or a write");
    assert!(u.paged);
    assert!(u.touch(0x0038), "and the next one puts it back");
    assert!(!u.paged);

    // An IN and an OUT do it too: the decoding is on the address bus and does
    // not care which cycle put the address there.
    u.io_read(0x0038);
    assert!(u.paged);
    u.io_write(0x0038, 0x00);
    assert!(!u.paged);

    assert!(!u.touch(0x0039), "and nothing else moves it");
    assert!(!u.paged);
}

/// Its 2K ROM is over the bottom 2K and again over the next, and the machine's
/// own ROM is not readable behind it.
#[test]
fn the_rom_is_mirrored_and_the_machines_own_is_not_there() {
    let mut u = fitted();
    u.touch(0x0038);

    assert_eq!(u.mem(0x0000), Some(0x00));
    assert_eq!(u.mem(0x0123), Some(0x23));
    assert_eq!(u.mem(0x0823), Some(0x23), "mirrored over $0800-$0FFF");
    assert_eq!(
        u.mem(0x2000),
        Some(0xFF),
        "and the machine's ROM is not readable up here"
    );
    assert_eq!(u.mem(0x4000), None, "the interface stops at the RAM");
}

/// Writing to $1000 says an allophone; reading it gives the chip's busy line
/// back in bit 0. Both have mirrors across the whole 4K block.
#[test]
fn the_speech_chip_answers_across_the_whole_1000_block() {
    let mut u = fitted();
    u.touch(0x0038);
    assert_eq!(u.mem(0x1000), Some(0xFE), "quiet: bit 0 clear");

    u.at(0, 3_500_000.0);
    u.poke(0x1800, 0x18); // /AA/ at one of the mirrors
    assert_eq!(u.allophone, 0x18);
    assert_eq!(u.mem(0x1001), Some(0xFF), "busy: bit 0 set, at a mirror");
    assert_eq!(u.spoken, 1);
    assert_eq!(u.phonemes, 1);

    // $2000 is not one of the mirrors, whatever it looks like.
    u.poke(0x2000, 0x19);
    assert_eq!(u.allophone, 0x18, "$2000 is not the chip");

    // The five pauses are counted apart from the sounds: the driver writes one
    // every interrupt whether or not the machine is saying anything.
    u.poke(0x1000, 0x02);
    assert_eq!(u.spoken, 2);
    assert_eq!(u.phonemes, 1, "a pause is not a sound");
}

/// The pitch is chosen by which address is written to, not by what is written.
#[test]
fn the_intonation_is_in_the_address_and_not_the_byte() {
    let mut u = fitted();
    u.touch(0x0038);
    assert!(!u.high_pitch);

    u.poke(0x3001, 0x00);
    assert!(u.high_pitch, "an odd address is the higher of the two");
    u.poke(0x3000, 0xFF);
    assert!(!u.high_pitch, "and an even one the lower");
    u.poke(0x3FFF, 0x00);
    assert!(u.high_pitch, "mirrored across the block");

    // The higher pitch runs the chip faster, so an allophone is shorter.
    u.at(0, 3_500_000.0);
    u.poke(0x1000, 0x05);
    // /OY/ is 291.2ms at the low pitch and 272.1ms at the high.
    u.at(940_000, 3_500_000.0);
    assert!(u.busy(), "268ms in, it is still going");
    u.at(960_000, 3_500_000.0);
    assert!(!u.busy(), "and by 274ms it has finished early");
}

/// A reset puts the box out of the way and stops the chip.
#[test]
fn a_reset_pages_it_out_and_stops_the_chip() {
    let mut u = fitted();
    u.touch(0x0038);
    u.at(0, 3_500_000.0);
    u.poke(0x1000, 0x05);
    assert!(u.busy());

    u.reset();
    assert!(!u.paged);
    assert!(!u.busy(), "and nothing is being said");
}

/// The toggle reaches the interface through every path the machine has.
///
/// The interrupt is a fetch, which is the path that matters most, but a
/// program drives the box with `ld a,($0038)` and `in a,(c)` as well — the
/// Currah manual's own suggestion. Wiring only the fetch leaves those doing
/// nothing, and a test that only runs the ROM would not notice: its handler
/// arrives by fetch.
#[test]
fn a_read_a_write_an_in_and_an_out_all_reach_it_through_the_machine() {
    use zx_rustrum::machine::{Model, Spectrum};

    let mut spec = Spectrum::with_model(Model::Spectrum48);
    spec.bus.uspeech = Some(fitted());

    // LD A,($0038) / LD ($0038),A / LD BC,$0038 / IN A,(C) / OUT (C),A
    for (at, byte) in [
        0x3Au8, 0x38, 0x00, 0x32, 0x38, 0x00, 0x01, 0x38, 0x00, 0xED, 0x78, 0xED, 0x79,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, b)| (0x8000 + i as u16, b))
    {
        spec.bus.poke(at, byte);
    }
    spec.cpu.pc = 0x8000;

    let paged = |spec: &Spectrum| spec.bus.uspeech.as_ref().unwrap().paged;
    spec.run(1);
    assert!(paged(&spec), "a memory read of $0038 turns it on");
    spec.run(1);
    assert!(!paged(&spec), "a memory write turns it back off");
    spec.run(1); // LD BC,$0038
    assert!(!paged(&spec), "loading the address is not an access to it");
    spec.run(1);
    assert!(paged(&spec), "an IN does it too");
    spec.run(1);
    assert!(!paged(&spec), "and so does an OUT");
}
