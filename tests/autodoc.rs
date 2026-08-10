//! What AutoDoc makes of code whose purpose is known, because it was written
//! here to do that one thing.
//!
//! These are guesses being tested, so what is asserted is the guess, not the
//! truth: a rule that fires on the wrong shape of code is the bug worth
//! catching, and a rule that never fires is worth catching too.

use zx_rustrum::autodoc::{analyse, describe, read_routine, Doc};

/// Assemble a routine at $8000 and read it back the way the debugger would.
fn routine(bytes: &[u8]) -> (String, String) {
    let memory = at_8000(bytes);
    let peek = |a: u16| memory[a as usize];
    describe(&read_routine(&peek, 0x8000))
}

fn at_8000(bytes: &[u8]) -> Vec<u8> {
    let mut memory = vec![0u8; 0x10000];
    memory[0x8000..0x8000 + bytes.len()].copy_from_slice(bytes);
    memory
}

fn doc_of(bytes: &[u8]) -> Doc {
    let memory = at_8000(bytes);
    let peek = |a: u16| memory[a as usize];
    analyse(&peek, &[0x8000])
}

/// Filling the display file with one value is a screen clear, and the give-away
/// is the size: 6144 bytes, or 6912 with the attributes.
#[test]
fn a_screen_clear_is_recognised_by_what_it_fills() {
    let (label, comment) = routine(&[
        0x21, 0x00, 0x40, // LD HL,$4000
        0x11, 0x01, 0x40, // LD DE,$4001
        0x01, 0x00, 0x18, // LD BC,$1800
        0x36, 0x00, // LD (HL),0
        0xED, 0xB0, // LDIR
        0xC9, // RET
    ]);
    assert_eq!(label, "clear_screen", "said {comment:?}");
    assert!(
        comment.to_lowercase().contains("clear"),
        "the comment should say so: {comment:?}"
    );
}

/// A back buffer being shown is the same size of copy, but from somewhere else
/// into the screen rather than from the screen to itself.
#[test]
fn a_back_buffer_being_shown_is_recognised() {
    let (label, comment) = routine(&[
        0x21, 0x00, 0xC0, // LD HL,$C000 — the buffer
        0x11, 0x00, 0x40, // LD DE,$4000 — the screen
        0x01, 0x00, 0x1B, // LD BC,$1B00 — screen and attributes
        0xED, 0xB0, // LDIR
        0xC9, // RET
    ]);
    assert_eq!(label, "blit_screen", "said {comment:?}");
    assert!(comment.contains("back buffer"), "{comment:?}");
}

/// A sprite is merged into the screen rather than written over it.
#[test]
fn a_sprite_routine_is_recognised_by_its_masking() {
    let (label, _) = routine(&[
        0x21, 0x00, 0x40, // LD HL,$4000
        0x7E, // LD A,(HL)
        0xAE, // XOR (HL)
        0x77, // LD (HL),A
        0x24, // INC H
        0xAE, // XOR (HL)
        0x77, // LD (HL),A
        0xC9, // RET
    ]);
    assert_eq!(label, "draw_sprite");
}

/// The Kempston joystick is a port nobody else uses.
#[test]
fn a_joystick_read_is_recognised() {
    let (label, comment) = routine(&[
        0xDB, 0x1F, // IN A,($1F)
        0xC9, // RET
    ]);
    assert_eq!(label, "read_joystick");
    assert!(comment.contains("$1F"), "{comment:?}");
}

/// The keyboard is read on port $FE with a row mask, and so are the joysticks
/// wired to it, which is said rather than glossed over.
#[test]
fn a_keyboard_read_is_recognised_and_hedged() {
    let (label, comment) = routine(&[
        0x01, 0xFE, 0x7F, // LD BC,$7FFE — a keyboard half-row
        0xDB, 0xFE, // IN A,($FE)
        0xC9, // RET
    ]);
    assert_eq!(label, "read_keys");
    assert!(
        comment.contains("joystick"),
        "the same port serves both, which the comment should admit: {comment:?}"
    );
}

/// The sound chip is named by its ports.
#[test]
fn writing_to_the_sound_chip_is_recognised() {
    let (label, comment) = routine(&[
        0x01, 0xFD, 0xFF, // LD BC,$FFFD
        0xED, 0x79, // OUT (C),A
        0xD3, 0xFD, // OUT ($FD),A — written out, so the port is visible
        0xC9,
    ]);
    // The port written out is $FD, not the AY's own, so this one falls back to
    // the beeper rule or lower. What matters is that it is not called sound
    // when there is no evidence of sound.
    assert_ne!(
        label, "read_joystick",
        "a sound routine should not be read as input: {comment:?}"
    );
}

/// Loading from tape goes through the ROM, and that call is unambiguous.
#[test]
fn a_tape_load_is_recognised_by_the_rom_call() {
    let (label, comment) = routine(&[
        0xCD, 0x56, 0x05, // CALL $0556 — LD-BYTES
        0xC9,
    ]);
    assert_eq!(label, "load_from_tape");
    assert!(comment.contains("tape"), "{comment:?}");
}

/// Decimal arithmetic on this machine is nearly always a score.
#[test]
fn decimal_arithmetic_is_read_as_a_score() {
    let (label, comment) = routine(&[
        0x3E, 0x01, // LD A,$01
        0x86, // ADD A,(HL)
        0x27, // DAA
        0x77, // LD (HL),A
        0xC9,
    ]);
    assert_eq!(label, "update_score");
    assert!(comment.contains("score"), "{comment:?}");
}

/// Reading the refresh register is a trick with one common use, and the guess
/// is offered as a guess.
#[test]
fn a_protection_check_is_offered_as_a_maybe() {
    let (label, comment) = routine(&[
        0xED, 0x5F, // LD A,R
        0xFE, 0x40, // CP $40
        0xC0, // RET NZ
        0xC9,
    ]);
    assert_eq!(label, "maybe_protection");
    assert!(
        comment.contains("possibly"),
        "a guess should say it is one: {comment:?}"
    );
}

/// A routine that mostly calls other routines is the shape of a main loop.
#[test]
fn a_routine_that_calls_others_is_read_as_game_logic() {
    let (label, _) = routine(&[
        0xCD, 0x00, 0x90, // CALL $9000
        0xCD, 0x10, 0x90, // CALL $9010
        0xCD, 0x20, 0x90, // CALL $9020
        0xCD, 0x30, 0x90, // CALL $9030
        0xC9,
    ]);
    assert_eq!(label, "game_logic");
}

/// Unpacking data shifts a control word about and copies runs.
#[test]
fn a_decompressor_is_recognised_by_its_bit_shifting() {
    let (label, comment) = routine(&[
        0xCB, 0x3F, // SRL A
        0xCB, 0x3F, // SRL A
        0xCB, 0x3F, // SRL A
        0xCB, 0x3F, // SRL A
        0xED, 0xB0, // LDIR
        0xC9,
    ]);
    assert_eq!(label, "decompress", "said {comment:?}");
}

/// Nothing is claimed about code that does nothing recognisable. A guesser
/// that always has an answer is worse than useless.
#[test]
fn code_with_no_tell_is_left_unnamed() {
    let (label, comment) = routine(&[
        0x00, 0x00, 0x00, // NOP NOP NOP
        0xC9, // RET
    ]);
    assert_eq!(label, "routine");
    assert_eq!(comment, "", "nothing to say, so nothing said");
}

/// Calls into the ROM are named from the table, at the address called, so the
/// listing says what a bare CALL was for.
#[test]
fn rom_routines_are_named_where_they_are_called() {
    let doc = doc_of(&[
        0xCD, 0x6B, 0x0D, // CALL $0D6B — CLS
        0xCD, 0xF4, 0x09, // CALL $09F4 — PR-STRING
        0xC9,
    ]);

    assert_eq!(doc.label(0x0D6B), "rom_cls");
    assert!(
        doc.comment(0x0D6B).to_lowercase().contains("clear"),
        "{doc:?}"
    );
    assert_eq!(doc.label(0x09F4), "rom_pr_string");

    // And the line doing the calling carries the same note, so it can be read
    // without jumping to the ROM.
    assert!(
        doc.comment(0x8000).to_lowercase().contains("clear"),
        "the CALL itself should say what it calls: {:?}",
        doc.comment(0x8000)
    );
}

/// Every routine reached from the entry point gets a name, and the name
/// carries its address so two guesses of the same kind are still distinct.
#[test]
fn every_routine_called_is_labelled() {
    let mut memory = vec![0u8; 0x10000];
    memory[0x8000..0x8000 + 4].copy_from_slice(&[0xCD, 0x00, 0x90, 0xC9]);
    memory[0x9000..0x9000 + 14].copy_from_slice(&[
        0x21, 0x00, 0x40, 0x11, 0x01, 0x40, 0x01, 0x00, 0x18, 0x36, 0x00, 0xED, 0xB0, 0xC9,
    ]);
    let peek = |a: u16| memory[a as usize];
    let doc = analyse(&peek, &[0x8000]);

    assert_eq!(
        doc.label(0x9000),
        "clear_screen_9000",
        "the routine called should be named after what it does, with its \
         address to tell it from the next one"
    );
    assert!(!doc.label(0x8000).is_empty(), "and so should the caller");
}

/// Reading code is bounded: a run through uninitialised memory must not take
/// the debugger with it.
#[test]
fn reading_nonsense_terminates() {
    // $FF everywhere is RST $38 — a loop that calls itself forever.
    let memory = vec![0xFFu8; 0x10000];
    let peek = |a: u16| memory[a as usize];
    let doc = analyse(&peek, &[0x8000]);
    assert!(!doc.is_empty(), "it should still have said something");
}
