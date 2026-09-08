//! The three Multifaces, with Romantic Robot's own ROMs in them.
//!
//! The test is the one that matters and the only one that can be trusted:
//! press the red button on a running machine and read the menu off the screen.
//! Everything else about these boxes — which port pages them in, which way up
//! the latches are — is a claim about hardware nobody here has, and the ROM is
//! the only thing that can check it.
//!
//! Skips itself when the ROMs are not there, as every other ROM test does.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::multiface::{Model as Mf, Multiface};

fn screen_text(spec: &Spectrum, rom: &[u8]) -> Vec<String> {
    let font = &rom[0x3D00..0x3D00 + 96 * 8];
    let mut lines = Vec::new();
    for row in 0..24usize {
        let mut line = String::new();
        for col in 0..32usize {
            let mut cell = [0u8; 8];
            for (i, byte) in cell.iter_mut().enumerate() {
                let y = row * 8 + i;
                let addr = 0x4000 + ((y & 0xC0) << 5) + ((y & 0x07) << 8) + ((y & 0x38) << 2) + col;
                *byte = spec.bus.peek_raw(addr as u16);
            }
            let mut found = ' ';
            for c in 0..96usize {
                let glyph = &font[c * 8..c * 8 + 8];
                let inverted: Vec<u8> = glyph.iter().map(|b| !b).collect();
                if glyph == cell || inverted == cell {
                    found = (32 + c as u8) as char;
                    break;
                }
            }
            line.push(found);
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn printed(spec: &Spectrum, rom: &[u8]) -> String {
    screen_text(spec, rom)
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn rom_of(model: Mf) -> Option<Vec<u8>> {
    model
        .rom_names()
        .iter()
        .find_map(|name| std::fs::read(format!("roms/{name}")).ok())
}

/// A machine of the right sort for the model, booted, with the box on the back.
fn machine(model: Mf) -> Option<(Spectrum, Vec<u8>)> {
    let (machine, file) = match model {
        Mf::One => (Model::Spectrum48, "roms/48.rom"),
        Mf::OneTwentyEight => (Model::Spectrum128, "roms/128.rom"),
        Mf::Three => (Model::Plus3, "roms/plus3.rom"),
    };
    let rom = std::fs::read(file).ok()?;
    let mf_rom = rom_of(model)?;

    let mut spec = Spectrum::new();
    spec.set_model(machine, &rom);
    spec.reset();
    let mut mf = Multiface::new(model);
    mf.rom = Some(mf_rom);
    spec.bus.multifaces.push(mf);
    for _ in 0..250 {
        spec.run(FRAME_T);
    }
    // The font is read out of the 48K BASIC ROM wherever it lives: it is the
    // last 16K of the 128K's image and of the +3's four.
    let font_rom = rom[rom.len() - 0x4000..].to_vec();
    Some((spec, font_rom))
}

/// Pressing the button stops the machine and puts the Multiface's own menu on
/// the screen, with Romantic Robot's name on it.
///
/// The whole path: the button pulls /NMI, the CPU takes it and fetches from
/// $0066, the interface decodes that address and pages its ROM and RAM over
/// the bottom 16K, and the ROM that finds itself running draws the menu.
#[test]
fn the_red_button_stops_the_machine_and_puts_the_menu_on_the_screen() {
    for (model, expected) in [
        (Mf::One, "MULTIFACE 1"),
        (Mf::OneTwentyEight, "MULTIFACE 128"),
        (Mf::Three, "MULTIFACE 3"),
    ] {
        let Some((mut spec, font)) = machine(model) else {
            eprintln!("need the machine's ROM and {:?}'s; skipping", model);
            continue;
        };

        assert!(spec.bus.press_red_button(), "{expected}: the button works");
        for _ in 0..60 {
            spec.run(FRAME_T);
        }

        let text = printed(&spec, &font);
        assert!(
            text.contains(expected),
            "{expected}: its menu should be on the screen, not {text:?}"
        );
        // And the menu itself, which is what the box is for: the One and the
        // 3 sign themselves "Romantic Robot Ltd" and the 128 gives its version
        // instead, so the thing to look for is the options.
        assert!(
            text.contains("return") && text.contains("save"),
            "{expected}: with its menu options on it: {text:?}"
        );
        // Not that it is still paged in: the menu runs from a stub the ROM
        // puts in the machine's own RAM and pages the box in only when it
        // wants something out of it, which is what the trace shows it doing
        // several times a frame.
    }
}

/// The menu gives the machine back: R for return puts the paging as it was and
/// the program carries on from the instruction it was stopped at.
#[test]
fn returning_from_the_menu_puts_the_machine_back_as_it_was() {
    let Some((mut spec, font)) = machine(Mf::One) else {
        eprintln!("need roms/48.rom and a Multiface One ROM; skipping");
        return;
    };
    let before = printed(&spec, &font);

    spec.bus.press_red_button();
    for _ in 0..60 {
        spec.run(FRAME_T);
    }
    assert!(
        printed(&spec, &font).contains("MULTIFACE 1"),
        "the menu is up"
    );

    // R returns. The key is held the way the keyboard window holds one, since
    // the Multiface scans the keyboard itself and wants it down for a while.
    spec.bus.keys[2] &= !(1 << 3);
    for _ in 0..20 {
        spec.run(FRAME_T);
    }
    spec.bus.keys[2] |= 1 << 3;
    for _ in 0..60 {
        spec.run(FRAME_T);
    }

    assert_eq!(
        printed(&spec, &font),
        before,
        "and the machine should be back where it was, with its own screen on"
    );
}

/// One press is one entry into the menu.
///
/// The button clears a latch that only an `OUT` to the interface's own port
/// sets again, and the ROM does that on its way out. Without it a held button
/// would take the machine into the menu over and over.
#[test]
fn a_second_press_does_nothing_until_the_menu_has_let_go() {
    let Some((mut spec, _)) = machine(Mf::One) else {
        eprintln!("need roms/48.rom and a Multiface One ROM; skipping");
        return;
    };
    assert!(spec.bus.press_red_button(), "the first press is taken");
    assert!(
        !spec.bus.press_red_button(),
        "and the second is not, until the ROM has armed it again"
    );
}
