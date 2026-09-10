//! The ZX Printer: the port, the stylus, the paper, and the real ROM using it.
//!
//! The port protocol is Fuse's `printer.c`; nothing about it can be read off a
//! ROM except by letting the ROM drive it, which the last two tests do — COPY
//! has to put the display file on the paper bit for bit, and LPRINT has to
//! print text that reads back as what was typed.

use zx_rustrum::hardware::Peripheral;
use zx_rustrum::machine::{screen_bitmap_offset, Spectrum, FRAME_T};
use zx_rustrum::mcp::json::Json;
use zx_rustrum::mcp::tools::{Reply, Session};
use zx_rustrum::printer::{self, Paper, ZxPrinter, DOTS};
use zx_rustrum::z80::Bus;

const FT: u64 = 69_888;

/// Standing still, the printer says it is there — bit 6 low — and nothing
/// else. Without one, bit 6 is high, and that is how the ROM's COPY knows to
/// give up rather than wait for an encoder that will never turn.
#[test]
fn the_port_says_whether_a_printer_is_there() {
    let printer = ZxPrinter::new(Paper::Metallised);
    assert_eq!(printer.read(0, FT), 0x3E, "present, still, stylus off");

    let mut spec = Spectrum::new();
    let none = spec.bus.io_read(0x00FB);
    assert_ne!(none & 0x40, 0, "no printer: bit 6 high, read {none:02X}");
    spec.bus.hardware.fit(Peripheral::ZxPrinter, true);
    spec.bus.printer = Some(ZxPrinter::new(Paper::Metallised));
    let some = spec.bus.io_read(0x00FB);
    assert_eq!(some & 0x40, 0, "a printer: bit 6 low, read {some:02X}");
}

/// The stylus held on for a whole sweep burns a whole line; held off, it
/// feeds blank paper. A line is 384 positions from 64 before the paper, at
/// 220 T-states each at full speed.
#[test]
fn a_stylus_held_on_across_the_paper_burns_a_solid_line() {
    let mut printer = ZxPrinter::new(Paper::Metallised);
    printer.write(0, FT, 0x80); // motor on, fast, stylus on
    assert!(printer.running());
    let end_of_line = (320 + 64) * 220;
    printer.write(end_of_line, FT, 0x00); // stylus off for the next line
    assert_eq!(printer.lines.len(), 1, "one line out");
    assert!(
        printer.lines[0].iter().all(|b| *b == 0xFF),
        "every dot burnt: {:02X?}",
        printer.lines[0]
    );
    printer.write(end_of_line * 2, FT, 0x04); // motor off
    assert_eq!(printer.lines.len(), 2, "and a blank one after it");
    assert!(
        printer.lines[1].iter().all(|b| *b == 0),
        "{:02X?}",
        printer.lines[1]
    );
    assert!(!printer.running(), "bit 2 stops the motor");
}

/// Two looks for the same dots: on silver the print is near black, on thermal
/// paper it is a blue-black on off-white — and in both the print is darker
/// than the paper and the rendering is the same every time.
#[test]
fn the_paper_decides_the_look_and_not_the_dots() {
    let mut line = [0u8; 32];
    line[0] = 0x80; // one dot, at the far left
    let lines = vec![line; 4];
    let silver = printer::render(&lines, 0, Paper::Metallised);
    let thermal = printer::render(&lines, 0, Paper::Thermal);
    assert_eq!(silver.len(), DOTS * 4 * 4);
    let luma = |px: &[u8]| px[0] as u32 * 3 + px[1] as u32 * 6 + px[2] as u32;
    for (name, image) in [("silver", &silver), ("thermal", &thermal)] {
        let dot = &image[0..4];
        let paper = &image[4..8];
        assert!(
            luma(dot) * 2 < luma(paper),
            "{name}: dot {dot:?} darker than paper {paper:?}"
        );
    }
    let (dot, paper) = (&thermal[0..4], &thermal[4..8]);
    assert!(dot[2] > dot[0] + 15, "thermal print is blue-black: {dot:?}");
    assert!(
        paper[0] > paper[2] + 10,
        "on paper gone yellowish: {paper:?}"
    );
    assert_eq!(
        silver,
        printer::render(&lines, 0, Paper::Metallised),
        "the same every time"
    );
}

/// A session with the real 48K ROM in it. `Session::new` has none, and a
/// machine with no ROM sits at $0038 running RST $38 — which types nothing and
/// prints nothing, and looked at first like a printer that did not work.
fn session_with_printer() -> Option<Session> {
    let rom = std::fs::read("roms/48.rom").ok()?;
    let mut session = Session::new();
    session
        .spec
        .set_model(zx_rustrum::machine::Model::Spectrum48, &rom);
    session.spec.reset();
    session.rom_loaded = true;
    call(&mut session, "fit", [("what", Json::str("zx_printer"))]).expect("fitted");
    Some(session)
}

fn call<const N: usize>(
    session: &mut Session,
    name: &str,
    args: [(&str, Json); N],
) -> Result<String, String> {
    session
        .call(name, &Json::obj(args))
        .map(|reply| match reply {
            Reply::Text(text) => text,
            Reply::Picture { text, .. } => text,
        })
}

/// COPY puts the top 22 rows of the screen on the paper, and each line of
/// dots is the display file's line, bit for bit — the real ROM driving the
/// stylus off the encoder, dot by dot.
#[test]
fn copy_puts_the_screen_on_the_paper_bit_for_bit() {
    let Some(mut session) = session_with_printer() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    for _ in 0..100 {
        session.spec.run(FRAME_T);
    }
    call(
        &mut session,
        "type_text",
        [("text", Json::str("PRINT \"HELLO PRINTER\""))],
    )
    .expect("typed");
    call(&mut session, "type_text", [("text", Json::str("COPY"))]).expect("typed");
    for _ in 0..600 {
        session.spec.run(FRAME_T);
        if session
            .spec
            .bus
            .printer
            .as_ref()
            .is_some_and(|p| p.lines.len() >= 176 && !p.running())
        {
            break;
        }
    }
    let bus = &session.spec.bus;
    let lines = &bus.printer.as_ref().unwrap().lines;
    assert_eq!(lines.len(), 176, "twenty-two rows of eight lines");
    let mut inked = 0;
    for (y, line) in lines.iter().enumerate() {
        let screen: Vec<u8> = (0..32)
            .map(|cell| bus.peek_raw(0x4000 + screen_bitmap_offset(y as u16, cell)))
            .collect();
        assert_eq!(
            &line[..],
            &screen[..],
            "line {y} of the paper is line {y} of the screen"
        );
        inked += line.iter().filter(|b| **b != 0).count();
    }
    assert!(
        inked > 20,
        "and there was something on the screen to print: {inked} bytes"
    );
}

/// LPRINT prints a line of text, and it reads back as what was typed.
#[test]
fn lprint_prints_text_that_reads_back() {
    let Some(mut session) = session_with_printer() else {
        eprintln!("need roms/48.rom; skipping");
        return;
    };
    for _ in 0..100 {
        session.spec.run(FRAME_T);
    }
    call(
        &mut session,
        "type_text",
        [("text", Json::str("LPRINT \"hello\""))],
    )
    .expect("typed");
    for _ in 0..200 {
        session.spec.run(FRAME_T);
    }
    let lines = session.spec.bus.printer.as_ref().unwrap().lines.len();
    assert_eq!(lines, 8, "a row of text is eight lines of dots");
    let out = call(&mut session, "printout", [("image", Json::Bool(false))]).expect("read");
    // Typed in L mode, so it is in lower case: type_text gives letters in the
    // machine's own case, and the printer prints what it was sent.
    assert!(out.contains("hello"), "the paper reads hello: {out}");
}
