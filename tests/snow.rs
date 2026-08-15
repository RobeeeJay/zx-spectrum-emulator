//! The ULA's snow, which is what a program gets for pointing I at the screen.
//!
//! From the 48K reference: "After each instruction fetch cycle of the
//! processor, the processor puts the I-R register 'pair' … on the address bus.
//! The ULA gets confused if I is in the range 64-127, because it thinks the
//! processor wants to read from lower 16K RAM very, very often. The ULA can't
//! cope with this read-frequency, and regularly misses a screen byte. Instead
//! of the actual byte, the byte previously read is used to build up the video
//! signal."

use zx_rustrum::machine::{Model, Spectrum};

/// A machine running NOPs out of uncontended RAM with a screenful of a known
/// pattern, so anything the picture holds that memory does not is snow.
fn machine(model: Model, i: u8) -> Spectrum {
    let mut spec = Spectrum::with_model(model);
    for offset in 0..0x1800u16 {
        // Every byte different from its neighbours, so a repeat shows.
        spec.bus.poke(0x4000 + offset, (offset % 251) as u8);
    }
    for offset in 0x1800..0x1b00u16 {
        spec.bus.poke(0x4000 + offset, 0x38);
    }
    // A loop of twenty-seven T-states, which is not a multiple of the eight
    // the ULA fetches in, so the refresh works its way round every slot.
    // A program of four-T instructions alone would keep one phase for ever
    // and spoil only one kind of fetch.
    for (offset, byte) in [
        0x00u8, // NOP        4
        0x3e, 0x00, // LD A,0     7
        0x3c, // INC A      4
        0x18, 0xf9, // JR -7     12
    ]
    .iter()
    .enumerate()
    {
        spec.bus.poke(0x8000 + offset as u16, *byte);
    }
    spec.cpu.pc = 0x8000;
    spec.cpu.i = i;
    spec
}

/// Run a whole frame and count the bytes of the picture that differ from what
/// the display file holds.
fn snowed_bytes(spec: &mut Spectrum) -> usize {
    let frame = spec.bus.frame;
    while spec.bus.frame == frame {
        spec.step_instruction();
    }
    (0..0x1b00u16)
        .filter(|offset| spec.bus.video_painted(*offset) != spec.bus.video(*offset))
        .count()
}

/// I in $40..$7F points the refresh address into the screen's own RAM, and the
/// picture stops being what the display file says.
#[test]
fn the_screen_snows_when_i_points_into_the_lower_ram() {
    let mut clean = machine(Model::Spectrum48, 0x00);
    assert_eq!(
        snowed_bytes(&mut clean),
        0,
        "with I out of the way the picture should be exactly what the display \
         file holds"
    );

    let mut snowing = machine(Model::Spectrum48, 0x40);
    let snowed = snowed_bytes(&mut snowing);
    assert!(
        snowed > 500,
        "with I at $40 the screen should be full of snow, and only {snowed} \
         bytes of it came out wrong"
    );
}

/// Nothing outside that range does it: I at $80 is refreshing uncontended RAM,
/// which the ULA never looks at.
#[test]
fn an_i_above_the_screen_does_not_snow() {
    for i in [0x00u8, 0x3f, 0x80, 0xff] {
        let mut spec = machine(Model::Spectrum48, i);
        assert_eq!(
            snowed_bytes(&mut spec),
            0,
            "I at ${i:02X} is not in $40..$7F and should leave the picture alone"
        );
    }
}

/// And the machine keeps running: the reference is explicit that the Spectrum
/// does not crash, it just looks terrible.
#[test]
fn snow_does_not_stop_the_machine() {
    let mut spec = machine(Model::Spectrum48, 0x40);
    snowed_bytes(&mut spec);
    // Its own loop, so the program counter comes back round; what says it is
    // still running is that it is still fetching.
    let fetched = spec.bus.fetches;
    for _ in 0..1000 {
        spec.step_instruction();
    }
    assert!(
        spec.bus.fetches > fetched + 900,
        "the machine stopped executing: {} fetches in a thousand steps",
        spec.bus.fetches - fetched
    );
}

/// What replaces the lost byte is the one before it, not a blank or a random
/// one: the ULA puts out again whatever it last got off the bus.
#[test]
fn a_lost_fetch_puts_out_the_byte_before_it() {
    let mut spec = machine(Model::Spectrum48, 0x40);
    let first = spec.bus.first_pixel_t();
    // The ULA reads cell 0's bitmap and attribute in the first two T-states
    // of the line; let it, so there is a byte for the next fetch to repeat.
    spec.bus.tstates = first + 2;
    spec.bus.catch_up_painting();
    let read_so_far = spec.bus.video_painting(0x1800); // cell 0's attribute

    // The third T-state of the eight is cell 1's bitmap, and the refresh is
    // counted two T-states back from where the clock stands.
    spec.bus.tstates = first + 2 + 2;
    spec.bus.refresh(0x4000);
    spec.bus.tstates = first + 16;
    spec.bus.catch_up_painting();

    assert_eq!(
        spec.bus.video_painting(1),
        read_so_far,
        "the lost fetch should have put out the byte the ULA read before it, \
         which was cell 0's attribute"
    );
    assert_ne!(
        spec.bus.video_painting(1),
        spec.bus.video(1),
        "and not the byte the display file holds"
    );
    assert_eq!(
        spec.bus.video_painting(2),
        spec.bus.video(2),
        "the fetch after it is not spoiled"
    );
}

/// The +2A and +3 drive the bus themselves rather than sharing it, so there is
/// nothing for the ULA to be confused by.
#[test]
fn the_plus3_does_not_snow() {
    let mut spec = machine(Model::Plus3, 0x40);
    assert_eq!(
        snowed_bytes(&mut spec),
        0,
        "a +3 has no shared bus and should not snow"
    );
}
