//! What the machine is making a noise with.
//!
//! Two ways of doing it, and a program uses one or the other. The beeper is a
//! bit of port $FE flipped by hand, so its pitch is however often the program
//! gets round to flipping it — which is why a 48K game's music stops the game.
//! The AY is a chip with fourteen registers that goes on playing while the
//! program does something else.

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

    out.push_str(&format!(
        "Beeper: the speaker bit of port $FE is {}, MIC is {}. The last byte written to \
         $FE was ${:02X}, so the border is {}.\n",
        if bus.speaker { "high" } else { "low" },
        if bus.mic { "high" } else { "low" },
        bus.last_fe,
        bus.border
    ));

    if !bus.model.has_ay() {
        out.push_str(
            "This machine has no sound chip: everything it plays is the beeper, one bit at \
             a time, and the pitch is however often the program flips it.\n",
        );
        return Ok(out);
    }

    let ay = &bus.audio.ay;
    let regs = ay.regs;
    out.push_str(&format!(
        "\nAY-3-8912, register {} selected. Ports: $FFFD selects, $BFFD writes.\n",
        ay.selected & 0x0F
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
