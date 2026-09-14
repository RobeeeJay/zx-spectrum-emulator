//! The debugger's Disassemble mode: code where code has run, data elsewhere.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::machine::{Spectrum, FRAME_T};
use zx_rustrum::ui::{App, Roms};

/// LD A,5 then JR to itself, and two bytes nothing ever runs: 'A' and 'B',
/// which disassembled would read as LD B,C and LD B,D.
const PROGRAM: [u8; 6] = [0x3E, 0x05, 0x18, 0xFE, 0x41, 0x42];

/// A fetch marks where code has run — the opcode, not its operand — and a
/// reset forgets it all, since what runs next may be another program.
#[test]
fn a_fetch_marks_where_code_has_run() {
    let mut spec = Spectrum::new();
    for (i, b) in PROGRAM.iter().enumerate() {
        spec.bus.poke(0x8000 + i as u16, *b);
    }
    spec.cpu.pc = 0x8000;
    spec.run(FRAME_T);
    let ran = |spec: &Spectrum, a: u16| spec.bus.tracker.executed[spec.bus.phys_index(a)];
    assert!(ran(&spec, 0x8000) && ran(&spec, 0x8002), "LD and JR ran");
    assert!(!ran(&spec, 0x8001), "the LD's operand was read, not run");
    assert!(!ran(&spec, 0x8004), "and the data never ran");
    spec.reset();
    assert!(!ran(&spec, 0x8000), "a reset forgets it");
}

fn app_with_program() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: None,
        rom_plus3: None,
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_debugger = true;
    app.running = false;
    for (i, b) in PROGRAM.iter().enumerate() {
        app.spec.bus.poke(0x8000 + i as u16, *b);
    }
    // Just before it, bytes that read as LD HL,$1234 if taken for code. They
    // never run, so in Disassemble mode the row above $8000 is the byte at
    // $7FFF; disassembling everything it would be $7FFD — which is what tells
    // the two ways of stepping back apart. Over the zeros that were here
    // before, a byte back is a NOP back, and both ways agreed.
    for (i, b) in [0x21u8, 0x34, 0x12].iter().enumerate() {
        app.spec.bus.poke(0x7FFD + i as u16, *b);
    }
    app.spec.cpu.pc = 0x8000;
    app.spec.run(FRAME_T);
    app
}

fn shown(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label(text).next().is_some() || h.query_all_by_value(text).next().is_some()
}

/// In Disassemble mode the bytes nothing has run are data, and the
/// instructions that ran are instructions — operands included, never a row of
/// their own. Off, everything is disassembled as before.
#[test]
fn disassemble_shows_data_where_nothing_has_run() {
    let peek = |a: u16| {
        PROGRAM
            .get(a.wrapping_sub(0x8000) as usize)
            .copied()
            .unwrap_or(0)
    };
    let ld = zx_rustrum::disasm::disasm(&peek, 0x8000).text;
    let as_code = zx_rustrum::disasm::disasm(&peek, 0x8004).text;

    let mut app = app_with_program();
    app.dbg.disassemble_run = true;
    app.show_in_listing(0x8004);
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(
        h.get_all_by_label("Disassemble").next().is_some(),
        "a Disassemble switch"
    );
    assert!(shown(&h, &ld), "{ld} ran, so it is code");
    assert!(shown(&h, "DEFB $41"), "the byte nothing ran is data");
    assert!(!shown(&h, &as_code), "and not {as_code}");
    assert!(!shown(&h, "DEFB $05"), "the LD's operand is part of the LD");
    assert!(
        shown(&h, "DEFB $21") && !shown(&h, "LD HL,$1234"),
        "the bytes before it never ran either, so they are data too"
    );

    // Centred on $8004, the rows above it are the JR and the LD that ran,
    // then a byte at a time back through what has not.
    let lines = h.state().dbg.lines as u16;
    assert_eq!(
        h.state().dbg.top,
        0x8000u16.wrapping_sub(lines / 2 - 2),
        "stepping back goes to the JR at $8002, the LD at $8000, then bytes"
    );

    h.state_mut().dbg.disassemble_run = false;
    h.state_mut().dbg.centre = true;
    h.run_steps(3);
    assert!(
        shown(&h, &as_code),
        "off, the byte is disassembled again: {as_code}"
    );
}

/// A prefixed instruction is one row. The Z80 fetches the byte after a DD,
/// ED, CB or FD prefix as an opcode too, so the tracker marks it as run; the
/// row above the instruction after LD IX,nn is the LD IX — not the LD HL,nn
/// that starts one byte into it.
#[test]
fn a_prefixed_instruction_is_one_row() {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: None,
        rom_plus3: None,
        rom_zx81: None,
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_debugger = true;
    app.running = false;
    // LD IX,$9000; JR to itself.
    for (i, b) in [0xDDu8, 0x21, 0x00, 0x90, 0x18, 0xFE].iter().enumerate() {
        app.spec.bus.poke(0x8000 + i as u16, *b);
    }
    app.spec.cpu.pc = 0x8000;
    app.spec.run(FRAME_T);
    let ran = |app: &App, a: u16| app.spec.bus.tracker.executed[app.spec.bus.phys_index(a)];
    assert!(
        ran(&app, 0x8001),
        "the byte after the prefix is fetched as an opcode too"
    );
    app.dbg.disassemble_run = true;
    app.show_in_listing(0x8004);
    let mut h = Harness::builder()
        .with_size([1600.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(shown(&h, "LD IX,$9000"), "one row for the LD IX");
    assert!(!shown(&h, "LD HL,$9000"), "and no LD HL starting inside it");
    let lines = h.state().dbg.lines as u16;
    assert_eq!(
        h.state().dbg.top,
        0x8000u16.wrapping_sub(lines / 2 - 1),
        "the row above $8004 is the LD IX at $8000"
    );
}
