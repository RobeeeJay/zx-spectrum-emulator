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

/// Snow is a fetch made from the wrong address.
///
/// "If the 4th cycle of the operation code fetching cycle coincides with the
/// 3th cycle of the 8-tacts output cycle of 16 pixels … the low byte of
/// address is replaced with the current contents of the R register." So the
/// ULA reads the right third of the screen and the wrong byte of it, which is
/// why snow is made of the program's own graphics rather than of noise.
#[test]
fn snow_reads_the_line_with_r_for_the_low_byte_of_the_address() {
    let mut spec = machine(Model::Spectrum48, 0x40);
    // R is $AB, and it is bits 6..0 of it that are picked up — bit 7 is the
    // one the Z80 never increments — so the byte comes from $402B, not $40AB.
    spec.bus.poke(0x4000 + 0x2b, 0x5a);
    spec.bus.poke(0x4000 + 0xab, 0x99);
    let first = spec.bus.first_pixel_t();

    // The ULA's eight-T-state cycle starts a T-state after the contention
    // does; its third is where the second cell of the pair is fetched. The
    // M1's last T-state is one back from where the clock stands.
    spec.bus.tstates = first + 1 + 2 + 1;
    spec.bus.refresh(0x40ab); // I = $40, R = $AB
    spec.bus.tstates = first + 16;
    spec.bus.catch_up_painting();

    assert_eq!(
        spec.bus.video_painting(1),
        0x5a,
        "the second cell should have been read from $402B — the ULA's own \
         address with bits 6..0 of R underneath it"
    );
    assert_eq!(
        spec.bus.video_painting(0x1801),
        spec.bus.video(0x1801),
        "the coincidence is with one fetch, and the attribute is the next one \
         along: snow scrambles the pixels and leaves the colours"
    );
    assert_eq!(
        spec.bus.video_painting(0),
        spec.bus.video(0),
        "and the cell before it was fetched before the CPU got there"
    );
}

/// The double effect is a fetch not made at all.
///
/// "If the 4th cycle of the operation code fetching cycle coincides with the
/// 5th cycle of the 8-tacts output cycle … the pixels2/attributes2 data will
/// not be read, and the screen bar with pixels1/attributes1 data will be
/// re-displayed."
#[test]
fn the_double_effect_shows_the_bar_before_it_again() {
    let mut spec = machine(Model::Spectrum48, 0x40);
    // Two cells that cannot be confused with one another.
    spec.bus.poke(0x4000, 0xf0);
    spec.bus.poke(0x4001, 0x0f);
    spec.bus.poke(0x5800, 0x07);
    spec.bus.poke(0x5801, 0x38);
    let first = spec.bus.first_pixel_t();

    spec.bus.tstates = first + 1 + 4 + 1;
    spec.bus.refresh(0x40ab);
    spec.bus.tstates = first + 16;
    spec.bus.catch_up_painting();

    assert_eq!(
        spec.bus.video_painting(1),
        0xf0,
        "the second cell was never fetched, so the first one went out again"
    );
    assert_eq!(
        spec.bus.video_painting(0x1801),
        0x07,
        "and its attribute with it"
    );
}

/// Nothing happens on the other six T-states of the eight.
#[test]
fn a_refresh_anywhere_else_in_the_cycle_does_nothing() {
    let first = Spectrum::with_model(Model::Spectrum48).bus.first_pixel_t();
    for phase in [0u32, 1, 3, 5, 6, 7] {
        let mut spec = machine(Model::Spectrum48, 0x40);
        spec.bus.tstates = first + 1 + phase + 1;
        spec.bus.refresh(0x40ab);
        assert_eq!(
            spec.bus.snow_marks(),
            0,
            "a refresh on T-state {phase} of the eight should disturb nothing"
        );
    }
}

/// A 128K snows for I in $C0..$FF as well, when a contended page is banked at
/// $C000: "Spectrum 128/+2: addresses #C000..#FFFF (odd-numbered pages:
/// 1,3,5,7)".
#[test]
fn a_128k_snows_from_the_top_of_memory_too() {
    for (page, snows) in [
        (1u8, true),
        (3, true),
        (5, true),
        (7, true),
        (0, false),
        (2, false),
    ] {
        let mut spec = machine(Model::Spectrum128, 0xc0);
        spec.bus.write_paging(page);
        let first = spec.bus.first_pixel_t();
        spec.bus.tstates = first + 1 + 2 + 1;
        spec.bus.refresh(0xc0ab);
        assert_eq!(
            spec.bus.snow_marks() > 0,
            snows,
            "with page {page} at $C000, snow should {}happen",
            if snows { "" } else { "not " }
        );
    }
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
