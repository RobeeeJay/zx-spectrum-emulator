//! What is plugged into the back, for a program driving the emulator.
//!
//! The Hardware window's job, without the window: say what can be fitted and
//! how far each thing is emulated, fit it, and press the one button any of
//! them has. Each box that does something needs its own ROM, and a model that
//! fits one without the ROM being there has to be told that rather than left
//! to wonder why the machine ignores it.

use crate::hardware::{Emulated, Peripheral};
use crate::if1::If1;
use crate::mcp::json::Json;
use crate::mcp::tools::{count, flag, text, Session};
use crate::multiface::{Model as MfModel, Multiface};
use crate::uspeech::Uspeech;

/// Which Multiface a peripheral is, if it is one.
fn multiface_model(what: Peripheral) -> Option<MfModel> {
    match what {
        Peripheral::MultifaceOne => Some(MfModel::One),
        Peripheral::Multiface128 => Some(MfModel::OneTwentyEight),
        Peripheral::Multiface3 => Some(MfModel::Three),
        _ => None,
    }
}

fn find(session: &Session, names: &[&str], size: usize) -> Option<Vec<u8>> {
    crate::resources::find_file(&session.rom_dirs, names, size).map(|(_, data)| data)
}

/// What is on the back of the machine, and what each thing would need.
pub fn hardware(session: &mut Session, _args: &Json) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(
        "What can be plugged in. `fit` puts one on or takes it off; each one that does \
         something needs its own ROM in the directory the machine's ROMs came from.\n\n",
    );
    for (what, first) in crate::hardware::Section::ALL
        .into_iter()
        .flat_map(|section| {
            Peripheral::ALL
                .into_iter()
                .filter(move |p| p.section() == section)
                .enumerate()
                .map(|(i, p)| (p, i == 0))
        })
    {
        if first {
            out.push_str(&format!("{}:\n", what.section().name()));
        }
        let fitted = session.spec.bus.hardware.fitted(what);
        let state = match what.emulated() {
            Emulated::Yes => "emulated".to_string(),
            Emulated::NeedsRom(rom) => format!("emulated, given {rom}"),
            Emulated::No(why) => format!("not emulated — {why}"),
        };
        out.push_str(&format!(
            "  {} [{}] — {}. {state}\n",
            what.name(),
            if fitted { "fitted" } else { "not fitted" },
            what.what(),
        ));
    }

    out.push('\n');
    match &session.spec.bus.if1 {
        Some(if1) => {
            out.push_str(&format!(
                "Interface 1: {} microdrive{}, ROM {}. microdrive_info says what is in them.\n",
                if1.drive_count(),
                if if1.drive_count() == 1 { "" } else { "s" },
                if if1.rom.is_some() {
                    "loaded"
                } else {
                    "MISSING — nothing will happen"
                }
            ));
        }
        None => out.push_str("Interface 1: not fitted, so there are no microdrives.\n"),
    }
    if session.spec.bus.multifaces.is_empty() {
        out.push_str("Multiface: none fitted, so there is no red button to press.\n");
    } else {
        for mf in &session.spec.bus.multifaces {
            out.push_str(&format!(
                "{}: ROM {}{}\n",
                mf.model.name(),
                if mf.ready() { "loaded" } else { "MISSING" },
                if mf.paged { ", paged in now" } else { "" }
            ));
        }
    }
    match (&session.spec.bus.uspeech, &session.spec.bus.audio.speech) {
        (Some(u), chip) => out.push_str(&format!(
            "µSpeech: interface ROM {}, speech chip {}. {} allophones said so far.\n",
            if u.ready() { "loaded" } else { "MISSING" },
            if chip.is_some() {
                "loaded"
            } else {
                "MISSING — it will run and say nothing"
            },
            u.spoken
        )),
        (None, _) => out.push_str("µSpeech: not fitted.\n"),
    }
    Ok(out)
}

/// Plug something in, or take it off.
pub fn fit(session: &mut Session, args: &Json) -> Result<String, String> {
    let key = text(args, "what")?;
    let what = Peripheral::from_key(&key).ok_or_else(|| {
        format!(
            "no peripheral called {key:?}. They are: {}",
            Peripheral::ALL
                .iter()
                .map(|p| p.key())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let on = flag(args, "fitted", true);
    session.spec.bus.hardware.fit(what, on);

    let mut out = format!(
        "{} {}.",
        what.name(),
        if on { "fitted" } else { "taken off" }
    );
    match what {
        Peripheral::Interface1 if on => {
            let drives = count(args, "microdrives", 1)?.clamp(1, crate::if1::MAX_DRIVES as u32);
            session.spec.bus.hardware.if1_drives = drives as usize;
            let mut if1 = If1::new(drives as usize);
            if1.rom = find(session, &["if1.rom", "interface1.rom", "if1-2.rom"], 8192);
            if if1.rom.is_none() {
                out.push_str(
                    " There is no if1.rom to page in, so nothing will happen: everything the \
                     microdrives do is done by that ROM.",
                );
            }
            out.push_str(&format!(" {drives} microdrive(s) on the chain."));
            session.spec.bus.if1 = Some(if1);
        }
        Peripheral::Interface1 => session.spec.bus.if1 = None,
        Peripheral::Uspeech if on => {
            let mut uspeech = Uspeech::new();
            uspeech.rom = find(
                session,
                &["uspeech.rom", "currah.rom", "microspeech.rom"],
                crate::uspeech::ROM_LEN,
            );
            if uspeech.rom.is_none() {
                out.push_str(" There is no uspeech.rom, so the interface does nothing.");
            }
            session.spec.bus.uspeech = Some(uspeech);
            session.spec.bus.audio.speech =
                find(session, &["sp0256-al2.rom", "sp0256-al2.bin"], 2048)
                    .map(|rom| crate::sp0256::Sp0256::new(&rom));
            if session.spec.bus.audio.speech.is_none() {
                out.push_str(
                    " There is no sp0256-al2.rom, so it will drive a chip that says nothing.",
                );
            }
        }
        Peripheral::Uspeech => {
            session.spec.bus.uspeech = None;
            session.spec.bus.audio.speech = None;
        }
        // Both printers are the same device to the machine, on the same port,
        // so fitting one takes the other off.
        // One stick, so one interface to plug it into.
        Peripheral::KempstonJoystick if on => session
            .spec
            .bus
            .set_joystick(crate::joystick::Kind::Kempston),
        Peripheral::DkTronicsJoystick if on => {
            session
                .spec
                .bus
                .set_joystick(crate::joystick::Kind::DkTronicsKempston);
            out.push_str(" The stick is in its Kempston socket, port No. 2.");
        }
        Peripheral::KempstonJoystick | Peripheral::DkTronicsJoystick => {
            if session.spec.bus.joystick.kind.interface() == Some(what) {
                session.spec.bus.set_joystick(crate::joystick::Kind::None);
            }
        }
        // The two mice both answer at $DF, so there is room for one.
        Peripheral::KempstonMouse | Peripheral::AmxMouse if on => {
            let other = if what == Peripheral::KempstonMouse {
                Peripheral::AmxMouse
            } else {
                Peripheral::KempstonMouse
            };
            session.spec.bus.hardware.fit(other, false);
            session.spec.bus.amx =
                (what == Peripheral::AmxMouse).then(crate::mouse::AmxMouse::default);
        }
        Peripheral::AmxMouse => session.spec.bus.amx = None,
        Peripheral::ZxPrinter | Peripheral::Alphacom32 if on => {
            let (other, paper) = if what == Peripheral::ZxPrinter {
                (Peripheral::Alphacom32, crate::printer::Paper::Metallised)
            } else {
                (Peripheral::ZxPrinter, crate::printer::Paper::Thermal)
            };
            session.spec.bus.hardware.fit(other, false);
            session.spec.bus.printer = Some(crate::printer::ZxPrinter::new(paper));
            out.push_str(" COPY, LPRINT and LLIST print to it; printout reads the paper back.");
        }
        Peripheral::ZxPrinter | Peripheral::Alphacom32 => session.spec.bus.printer = None,
        Peripheral::Fuller if on => session.spec.bus.audio.extra_ay = Some(Default::default()),
        Peripheral::Fuller => session.spec.bus.audio.extra_ay = None,
        Peripheral::SpecDrum if !on => session.spec.bus.audio.dac = 0.0,
        _ if multiface_model(what).is_some() => {
            let model = multiface_model(what).expect("checked");
            session.spec.bus.multifaces.retain(|mf| mf.model != model);
            if on {
                let mut mf = Multiface::new(model);
                mf.rom = find(session, model.rom_names(), crate::multiface::ROM_LEN);
                if mf.rom.is_none() {
                    out.push_str(&format!(
                        " There is no {}, so the red button has nothing behind it.",
                        model.rom_names()[0]
                    ));
                }
                session.spec.bus.multifaces.push(mf);
                session.spec.bus.multifaces.sort_by_key(|mf| mf.model as u8);
            }
        }
        _ => {}
    }
    if let Emulated::No(why) = what.emulated() {
        out.push_str(&format!(" It does nothing yet: {why}"));
    }
    Ok(out)
}

/// The Multiface's red button: stop the machine where it stands.
pub fn red_button(session: &mut Session, _args: &Json) -> Result<String, String> {
    if session.spec.bus.multifaces.is_empty() {
        return Err(
            "no Multiface is fitted: fit one with fit {\"what\": \"multiface1\"} — or \
             multiface128 or multiface3 — and it needs its own ROM."
                .into(),
        );
    }
    if !session.spec.bus.press_red_button() {
        return Err(
            "the button did nothing: either no Multiface has a ROM in it, or its menu has \
             not finished with the last press. One press is one entry into the menu, and \
             the ROM arms it again on its way out."
                .into(),
        );
    }
    Ok(
        "Pressed. The machine takes an NMI at its next instruction and lands on $0066, \
         where the Multiface pages its ROM and RAM over the bottom 16K and draws its menu. \
         run_frames to let that happen, then screen to see it; press_keys sends it a key."
            .into(),
    )
}
