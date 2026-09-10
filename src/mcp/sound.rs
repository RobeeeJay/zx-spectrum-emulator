//! What the machine is making a noise with.
//!
//! Two ways of doing it, and a program uses one or the other. The beeper is a
//! bit of port $FE flipped by hand, so its pitch is however often the program
//! gets round to flipping it — which is why a 48K game's music stops the game.
//! The AY is a chip with fourteen registers that goes on playing while the
//! program does something else.
//!
//! And there is a third way, which is whatever has been plugged into the back:
//! a Fuller Box is a second AY, a SpecDrum is an eight-bit converter fed
//! samples, and a µSpeech is a chip saying allophones. Each of the three can
//! be silenced on its own, which is what `set_sound` is for: a game's music on
//! the AY over a beeper that is only clicking is a common thing to want.

use crate::mcp::json::Json;
use crate::mcp::tools::Session;

/// Register names, as the AY's own datasheet numbers them.
const AY_NAMES: [&str; 16] = [
    "A fine",
    "A coarse",
    "B fine",
    "B coarse",
    "C fine",
    "C coarse",
    "noise period",
    "mixer",
    "A volume",
    "B volume",
    "C volume",
    "envelope fine",
    "envelope coarse",
    "envelope shape",
    "port A",
    "port B",
];

pub fn sound_state(session: &mut Session, _args: &Json) -> Result<String, String> {
    let bus = &session.spec.bus;
    let mut out = String::new();

    let audio = &bus.audio;
    out.push_str(&format!(
        "Switches: beeper {}, sound chips {}, add-ons {} — set_sound changes them, and a \
         muted part is silenced in the mixer without the machine knowing.\n",
        on_off(audio.beeper_on),
        on_off(audio.ay_on),
        on_off(audio.hardware_on),
    ));

    out.push_str(&format!(
        "Beeper: the speaker bit of port $FE is {}, MIC is {}. The last byte written to \
         $FE was ${:02X}, so the border is {}.\n",
        if bus.speaker { "high" } else { "low" },
        if bus.mic { "high" } else { "low" },
        bus.last_fe,
        bus.border
    ));

    out.push_str(&add_ons(session));
    let bus = &session.spec.bus;

    if !bus.model.has_ay() {
        out.push_str(
            "This machine has no sound chip of its own: everything it plays is the beeper, \
             one bit at a time, and the pitch is however often the program flips it.\n",
        );
        if bus.audio.extra_ay.is_none() {
            return Ok(out);
        }
        // A Fuller Box gives a 48K one, and its registers are worth reading
        // even though the machine has none: that was the point of buying one.
        out.push_str("The Fuller Box's chip is below.\n");
    }

    let (ay, whose) = match (bus.model.has_ay(), bus.audio.extra_ay.as_ref()) {
        (true, _) => (&bus.audio.ay, "the machine's own"),
        (false, Some(extra)) => (extra, "the Fuller Box's"),
        (false, None) => unreachable!("returned above"),
    };
    let regs = ay.regs;
    out.push_str(&format!(
        "\nAY-3-8912 ({whose}), register {} selected. Ports: {}\n",
        ay.selected & 0x0F,
        if bus.model.has_ay() {
            "$FFFD selects, $BFFD writes."
        } else {
            "$3F selects, $5F writes — the Fuller's own."
        }
    ));
    for (i, value) in regs.iter().enumerate() {
        out.push_str(&format!("  R{i:<2} {:<16} ${value:02X}\n", AY_NAMES[i]));
    }

    // The registers say what is playing; a model should not have to know the
    // chip to read them.
    let period =
        |fine: usize, coarse: usize| ((regs[coarse] as u32 & 0x0F) << 8) | regs[fine] as u32;
    let hz = |period: u32| {
        if period == 0 {
            0.0
        } else {
            // The AY runs at half the CPU clock on a Spectrum, and divides by
            // sixteen again.
            bus.model.cpu_hz() / 2.0 / 16.0 / period as f64
        }
    };
    let mixer = regs[7];
    out.push_str("\nWhat that means:\n");
    for (channel, (fine, coarse, volume)) in [(0usize, 1usize, 8usize), (2, 3, 9), (4, 5, 10)]
        .iter()
        .enumerate()
    {
        let tone_on = mixer & (1 << channel) == 0;
        let noise_on = mixer & (1 << (channel + 3)) == 0;
        let level = regs[*volume] & 0x0F;
        let envelope = regs[*volume] & 0x10 != 0;
        let p = period(*fine, *coarse);
        out.push_str(&format!(
            "  channel {}: {}{}{} — period {p} ({:.0} Hz), volume {}\n",
            *b"ABC".get(channel).unwrap_or(&b'?') as char,
            if tone_on { "tone" } else { "no tone" },
            if noise_on { " + noise" } else { "" },
            if envelope { " (envelope)" } else { "" },
            hz(p),
            if envelope {
                "from the envelope".to_string()
            } else {
                level.to_string()
            }
        ));
    }
    let env = period(11, 12);
    out.push_str(&format!(
        "  envelope: period {env} ({:.1} Hz), shape ${:02X}\n",
        if env == 0 {
            0.0
        } else {
            bus.model.cpu_hz() / 2.0 / 256.0 / env as f64
        },
        regs[13]
    ));
    Ok(out)
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "MUTED"
    }
}

/// What the boxes on the back are making, if any of them are.
fn add_ons(session: &Session) -> String {
    let bus = &session.spec.bus;
    let mut out = String::new();
    if bus.hardware.fitted(crate::hardware::Peripheral::SpecDrum) {
        // The converter idles at half scale, which is silence: a value away
        // from zero here means a sample is being played this instant.
        out.push_str(&format!(
            "\nSpecDrum: an eight-bit converter on $DF, sitting at {:+.2} of full scale. \
             A program feeds it drum samples as fast as it can; watch port writes to $DF to \
             find the routine doing it.\n",
            bus.audio.dac
        ));
    }
    if let Some(uspeech) = &bus.uspeech {
        let chip = bus.audio.speech.as_ref();
        out.push_str(&format!(
            "\nµSpeech: {} allophones said, {} of them sounds rather than pauses. The last \
             was ${:02X}, and the chip is {}. Pitch: the {} of its two.\n",
            uspeech.spoken,
            uspeech.phonemes,
            uspeech.allophone,
            match chip {
                Some(chip) if chip.busy() => "saying it now",
                Some(_) => "quiet",
                None => "not there — roms/sp0256-al2.rom is missing, so it says nothing",
            },
            if uspeech.high_pitch {
                "higher"
            } else {
                "lower"
            }
        ));
        out.push_str(
            "The driver writes a pause to the chip every interrupt whether or not anything \
             is being said, so the count of sounds is what says the machine is talking.\n",
        );
    }
    out
}

/// Silence part of the sound, or bring it back.
pub fn set_sound(session: &mut Session, args: &Json) -> Result<String, String> {
    let audio = &mut session.spec.bus.audio;
    let mut changed = Vec::new();
    for (name, field) in [("beeper", 0), ("ay", 1), ("hardware", 2)] {
        if let Some(on) = args.get(name).and_then(|v| v.as_bool()) {
            match field {
                0 => audio.beeper_on = on,
                1 => audio.ay_on = on,
                _ => audio.hardware_on = on,
            }
            changed.push(format!("{name} {}", if on { "on" } else { "muted" }));
        }
    }
    if changed.is_empty() {
        return Err(
            "nothing to change: pass beeper, ay or hardware as true or false. They are the \
             machine's speaker, its sound chips (its own and a Fuller Box's), and what the \
             other add-ons make."
                .into(),
        );
    }
    Ok(format!(
        "{}. Beeper {}, chips {}, add-ons {}.",
        changed.join(", "),
        on_off(session.spec.bus.audio.beeper_on),
        on_off(session.spec.bus.audio.ay_on),
        on_off(session.spec.bus.audio.hardware_on),
    ))
}
