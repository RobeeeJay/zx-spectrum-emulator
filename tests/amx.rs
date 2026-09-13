//! The AMX mouse: a Z80 PIO that interrupts for every step, and its buttons.
//!
//! No AMX software is in `tapes/`, so the driver cannot be run. What stands
//! in for it is a driver of the same shape, written here in machine code: it
//! programs the PIO's vectors, its mode and its interrupts through $5F and
//! $7F, runs in interrupt mode 2, and has a handler for each port that reads
//! $1F or $3F and counts the step by the direction in bit 0. The ports, the
//! control words and the direction bits are from the Sinclair Wiki,
//! dsp-emulator and zx84 — see `src/mouse.rs` — so this checks the emulation
//! against them and against the real CPU taking the interrupts, not against a
//! real mouse.

use zx_rustrum::hardware::Peripheral;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::mouse::AmxMouse;

/// Across: right counted at $A000, left at $A001. Up and down: up at $A100,
/// down at $A101.
const RIGHT: u16 = 0xA000;
const LEFT: u16 = 0xA001;
const UP: u16 = 0xA100;
const DOWN: u16 = 0xA101;

/// Set the PIO up, interrupts still off, and wait.
const SETUP: &[u8] = &[
    0xF3, // DI
    0x3E, 0xF0, // LD A,$F0
    0xED, 0x47, // LD I,A
    0xED, 0x5E, // IM 2
    0x3E, 0x10, // LD A,$10     port A's vector
    0xD3, 0x5F, // OUT ($5F),A
    0x3E, 0x12, // LD A,$12     port B's vector
    0xD3, 0x7F, // OUT ($7F),A
    0x3E, 0x4F, // LD A,$4F     mode 1: input
    0xD3, 0x5F, // OUT ($5F),A
    0xD3, 0x7F, // OUT ($7F),A
    0xFB, // EI
    0x18, 0xFE, // JR $
];

/// Turn the PIO's interrupts on, and wait.
const ENABLE: &[u8] = &[
    0x3E, 0x87, // LD A,$87     interrupts on, no mask
    0xD3, 0x5F, // OUT ($5F),A
    0xD3, 0x7F, // OUT ($7F),A
    0xFB, // EI
    0x18, 0xFE, // JR $
];

/// A handler that reads a direction port and counts the step: `port` is $1F
/// or $3F, and the count goes to `page`:00 or `page`:01 by bit 0.
fn handler(port: u8, page: u8) -> Vec<u8> {
    vec![
        0xF5, // PUSH AF
        0xE5, // PUSH HL
        0xDB, port, // IN A,(port)
        0xE6, 0x01, // AND 1
        0x6F, // LD L,A
        0x26, page, // LD H,page
        0x34, // INC (HL)
        0xE1, // POP HL
        0xF1, // POP AF
        0xFB, // EI
        0xED, 0x4D, // RETI
    ]
}

fn poke_all(spec: &mut Spectrum, at: u16, bytes: &[u8]) {
    for (i, b) in bytes.iter().enumerate() {
        spec.bus.poke(at + i as u16, *b);
    }
}

fn machine() -> Spectrum {
    let mut spec = Spectrum::new();
    poke_all(&mut spec, 0x8000, SETUP);
    poke_all(&mut spec, 0x8100, ENABLE);
    poke_all(&mut spec, 0x9000, &handler(0x1F, 0xA0));
    poke_all(&mut spec, 0x9100, &handler(0x3F, 0xA1));
    // The frame's handler: EI; RETI.
    poke_all(&mut spec, 0x9200, &[0xFB, 0xED, 0x4D]);
    // The vector table at $F0xx: port A's at $10, port B's at $12, and the
    // frame interrupt's at $FF, which is what the floating bus gives it.
    poke_all(&mut spec, 0xF010, &[0x00, 0x90]);
    poke_all(&mut spec, 0xF012, &[0x00, 0x91]);
    poke_all(&mut spec, 0xF0FF, &[0x00, 0x92]);
    for at in [RIGHT, LEFT, UP, DOWN] {
        spec.bus.poke(at, 0);
    }
    spec.bus.hardware.fit(Peripheral::AmxMouse, true);
    spec.bus.amx = Some(AmxMouse::default());
    spec.cpu.sp = 0x7F00;
    spec.cpu.pc = 0x8000;
    spec
}

fn counts(spec: &Spectrum) -> [u8; 4] {
    [RIGHT, LEFT, UP, DOWN].map(|at| spec.bus.peek_raw(at))
}

fn run(spec: &mut Spectrum, frames: u32) {
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
}

/// Each step of the mouse is an interrupt, through the vector the program gave
/// the PIO, and the handler reads which way it went: five right and three up
/// come out as five rights and three ups — and nothing at all until the
/// program has turned the PIO's interrupts on, since a PIO comes up with them
/// off and an unprogrammed one would vector through a table that is not there.
#[test]
fn every_step_is_an_interrupt_and_says_which_way() {
    let mut spec = machine();
    run(&mut spec, 3);
    spec.bus.amx.as_mut().unwrap().queue(5, -3);
    run(&mut spec, 5);
    assert_eq!(counts(&spec), [0; 4], "the PIO's interrupts are still off");
    let amx = spec.bus.amx.as_ref().unwrap();
    assert_eq!((amx.pending_x, amx.pending_y), (5, -3), "so the steps wait");

    spec.cpu.pc = 0x8100;
    run(&mut spec, 5);
    assert_eq!(
        counts(&spec),
        [5, 0, 3, 0],
        "right, left, up, down: five right and three up"
    );
    let amx = spec.bus.amx.as_ref().unwrap();
    assert_eq!((amx.pending_x, amx.pending_y), (0, 0), "all delivered");

    spec.bus.amx.as_mut().unwrap().queue(-2, 4);
    run(&mut spec, 5);
    assert_eq!(counts(&spec), [5, 2, 3, 4], "then two left and four down");
}

/// The frame's own interrupt still arrives through the vector the floating bus
/// makes, $FF: the PIO drives the bus only for its own.
#[test]
fn the_frame_interrupt_still_uses_the_floating_bus() {
    let mut spec = machine();
    // The frame's handler counts at $A200 instead of doing nothing.
    poke_all(
        &mut spec,
        0x9200,
        &[
            0xF5, 0x3A, 0x00, 0xA2, 0x3C, 0x32, 0x00, 0xA2, 0xF1, 0xFB, 0xED, 0x4D,
        ],
    );
    spec.bus.poke(0xA200, 0);
    spec.cpu.pc = 0x8000;
    run(&mut spec, 2);
    spec.cpu.pc = 0x8100;
    spec.bus.amx.as_mut().unwrap().queue(3, 0);
    run(&mut spec, 4);
    assert!(
        spec.bus.peek_raw(0xA200) >= 3,
        "the frame interrupts arrived: {}",
        spec.bus.peek_raw(0xA200)
    );
    assert_eq!(counts(&spec)[0], 3, "and so did the mouse's");
}

/// The buttons are at $DF, active low: left bit 7, middle bit 6, right bit 5.
/// The direction ports answer in bit 0, and an even port with A7 low is left
/// to the ULA.
#[test]
fn the_buttons_and_the_ports() {
    let mut amx = AmxMouse::default();
    assert_eq!(amx.io_read(0x00DF), Some(0xFF), "nothing held");
    amx.set_buttons(true, false, false);
    assert_eq!(amx.io_read(0x00DF), Some(0x7F), "left is bit 7");
    amx.set_buttons(false, true, false);
    assert_eq!(amx.io_read(0x00DF), Some(0xBF), "middle is bit 6");
    amx.set_buttons(false, false, true);
    assert_eq!(amx.io_read(0x00DF), Some(0xDF), "right is bit 5");

    assert_eq!(amx.io_read(0x001F), Some(0), "across, at rest");
    assert_eq!(amx.io_read(0x003F), Some(0), "up and down, at rest");
    assert_eq!(amx.io_read(0x005F), None, "a control port reads nothing");
    assert_eq!(amx.io_read(0x001E), None, "an even port is the ULA's");
    assert_eq!(amx.io_read(0x00FE), None, "and so is $FE");
}

/// Fitting the AMX mouse takes the Kempston mouse off: both answer at $DF.
#[test]
fn the_two_mice_cannot_both_be_fitted() {
    use zx_rustrum::ui::{App, Roms};
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.fit(Peripheral::KempstonMouse, true);
    app.fit(Peripheral::AmxMouse, true);
    assert!(!app.spec.bus.hardware.fitted(Peripheral::KempstonMouse));
    assert!(app.spec.bus.amx.is_some(), "the AMX is there");
    app.fit(Peripheral::KempstonMouse, true);
    assert!(!app.spec.bus.hardware.fitted(Peripheral::AmxMouse));
    assert!(app.spec.bus.amx.is_none(), "and gone again");
}
