//! The Currah µSpeech, with Currah's own ROM in it.
//!
//! Everything the machine can see of this box is emulated: the ROM that turns
//! over at $0038, the speech chip's register at $1000 and its busy line, and
//! the two pitches at $3000. What is not here is the sound — the SP0256-AL2
//! keeps its allophones inside itself as filter coefficients — so what these
//! tests check is that the machine drives the chip, not that anything is
//! audible.
//!
//! Skips itself when the ROMs are not there.

use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::uspeech::Uspeech;

const SYM: (usize, u8) = (7, 1);
const ENTER: (usize, u8) = (6, 0);

fn hold(spec: &mut Spectrum, keys: &[(usize, u8)], frames: u32) {
    for (row, bit) in keys {
        spec.bus.keys[*row] &= !(1 << bit);
    }
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
    for (row, bit) in keys {
        spec.bus.keys[*row] |= 1 << bit;
    }
    for _ in 0..frames {
        spec.run(FRAME_T);
    }
}

/// Which key a character is on, and whether SYMBOL SHIFT goes with it.
fn key_of(c: char) -> Option<((usize, u8), bool)> {
    const ROWS: [&str; 8] = [
        " ZXCV",
        "ASDFG",
        "QWERT",
        "12345",
        "09876",
        "POIUY",
        "\nLKJH",
        " \u{1}MNB",
    ];
    let shifted = [('"', 'P'), ('$', '4'), ('=', 'L')];
    let up = c.to_ascii_uppercase();
    let (up, shift) = match shifted.iter().find(|(sym, _)| *sym == up) {
        Some((_, key)) => (*key, true),
        None => (up, false),
    };
    for (row, keys) in ROWS.iter().enumerate() {
        if let Some(bit) = keys.chars().position(|k| k == up) {
            return Some(((row, bit as u8), shift));
        }
    }
    None
}

fn type_text(spec: &mut Spectrum, text: &str) {
    for c in text.chars() {
        let Some((key, shift)) = key_of(c) else {
            continue;
        };
        if shift {
            hold(spec, &[SYM, key], 6);
        } else {
            hold(spec, &[key], 6);
        }
    }
}

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

fn roms() -> Option<(Vec<u8>, Vec<u8>)> {
    Some((
        std::fs::read("roms/48.rom").ok()?,
        std::fs::read("roms/uspeech.rom").ok()?,
    ))
}

fn machine(rom48: &[u8], speech: &[u8]) -> Spectrum {
    let mut spec = Spectrum::new();
    spec.set_model(Model::Spectrum48, rom48);
    spec.reset();
    let mut uspeech = Uspeech::new();
    uspeech.rom = Some(speech.to_vec());
    spec.bus.uspeech = Some(uspeech);
    // The chip, if its ROM is here: the busy line the interface hands back is
    // the chip's own, so without it nothing waits for anything.
    if let Ok(chip) = std::fs::read("roms/sp0256-al2.rom") {
        spec.bus.audio.speech = Some(zx_rustrum::sp0256::Sp0256::new(&chip));
    }
    for _ in 0..250 {
        spec.run(FRAME_T);
    }
    spec
}

/// A machine with a µSpeech on the back comes up saying so.
///
/// Nothing has to be typed to bring the interface in: the ULA's interrupt is a
/// fetch from $0038, which is the address that turns the box over, so its ROM
/// gets the machine once a frame from the moment it is plugged in. Its own
/// sign-on above Sinclair's is the proof that its ROM is running.
#[test]
fn a_machine_with_one_fitted_comes_up_with_currahs_sign_on() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let spec = machine(&rom48, &speech);
    let text = screen_text(&spec, &rom48).join(" / ");
    assert!(
        text.contains("CURRAH"),
        "the interface should sign on: {text}"
    );
    assert!(
        text.contains("1982 Sinclair Research"),
        "over the machine's own: {text}"
    );

    // And it is out of the way again by the time anything else runs: the
    // handler pages it in at the interrupt and out on its way back.
    assert!(
        !spec.bus.uspeech.as_ref().unwrap().paged,
        "the machine's own ROM is what BASIC runs in"
    );
}

/// `LET s$="hello"` is how the Currah is spoken to, and the allophones come
/// out of it one at a time.
///
/// The driver writes a pause to the chip every interrupt whether or not there
/// is anything to say, so the thing to count is the allophones that are
/// sounds. Nothing is audible — the SP0256's own ROM is not here — but every
/// byte the machine sends the chip is.
#[test]
fn what_is_put_in_s_dollar_reaches_the_speech_chip() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &speech);
    let quiet = spec.bus.uspeech.as_ref().unwrap().phonemes;

    // LET is on the L key, and the rest is typed as it reads.
    hold(&mut spec, &[(6, 1)], 8);
    type_text(&mut spec, "s$=\"hello\"");
    hold(&mut spec, &[ENTER], 8);
    for _ in 0..200 {
        spec.run(FRAME_T);
    }

    let uspeech = spec.bus.uspeech.as_ref().unwrap();
    assert!(
        uspeech.phonemes > quiet,
        "the machine should have said something: {} sounds and {} allophones \
         in all, where before it had said {quiet}",
        uspeech.phonemes,
        uspeech.spoken
    );

    let text = screen_text(&spec, &rom48).join(" / ");
    assert!(
        !text.contains("Nonsense in BASIC"),
        "and the line should have run: {text}"
    );
}

/// The busy line is what a program waits on between allophones, and it is the
/// chip's own: it is up for exactly as long as the sound takes.
#[test]
fn the_busy_line_lasts_as_long_as_the_allophone_does() {
    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &speech);
    if spec.bus.audio.speech.is_none() {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    }

    // Drive the interface the way the Currah manual says to: read $0038 to
    // turn it on, write the allophone, poll the busy bit.
    let uspeech = spec.bus.uspeech.as_mut().unwrap();
    uspeech.touch(0x0038);
    assert!(uspeech.paged, "a read of $0038 turns it on");
    let told = uspeech.poke(0x1000, 0x05).expect("the chip's register");
    spec.bus.tell_speech(told);
    assert_ne!(
        spec.bus.mem(0x1000) & 1,
        0,
        "busy the moment it is told to talk"
    );

    // /OY/ is the longest allophone there is: 291ms at the chip's own rate,
    // which is 2,850 samples. Run the mixer through it and watch the line.
    let quiet_at = (0..4000)
        .position(|_| {
            spec.bus.audio.speech.as_mut().unwrap().sample();
            !spec.bus.audio.speech.as_ref().unwrap().busy()
        })
        .expect("it should stop talking");
    let ms = quiet_at as f64 / (3_050_000.0 / 312.0) * 1000.0;
    assert!(
        (ms - 291.2).abs() < 20.0,
        "/OY/ should take about 291ms, and took {ms:.0}ms"
    );
    assert_eq!(spec.bus.mem(0x1000) & 1, 0, "and the line drops");
}

/// The whole path, end to end: `LET s$="hello"` and sound comes out.
///
/// This is the test that says the µSpeech works. Everything else says a part
/// of it does: this types at the keyboard, lets Currah's ROM drive the chip
/// through its interrupt handler, and listens to what the mixer produced.
#[test]
fn saying_something_makes_a_noise_in_the_mixer() {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    let Some((rom48, speech)) = roms() else {
        eprintln!("need roms/48.rom and roms/uspeech.rom; skipping");
        return;
    };
    let mut spec = machine(&rom48, &speech);
    if spec.bus.audio.speech.is_none() {
        eprintln!("need roms/sp0256-al2.rom; skipping");
        return;
    }

    let queue: zx_rustrum::audio::SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
    spec.bus.audio.attach(queue.clone(), 48_000.0);
    spec.bus.audio.volume = 1.0;
    // The beeper and the chips off, so what is left can only be the speech.
    spec.bus.audio.beeper_on = false;
    spec.bus.audio.ay_on = false;

    // Drained every frame, the way a sound card drains it: the queue holds a
    // quarter of a second and throws the oldest away, so reading it at the end
    // reads the silence after the machine has finished speaking. That is what
    // "it makes no sound" looked like for an hour.
    let mut samples: Vec<f32> = Vec::new();
    let drain = |spec: &mut Spectrum, samples: &mut Vec<f32>| {
        spec.bus.audio.flush();
        let mut q = queue.lock().unwrap();
        samples.extend(q.drain(..));
    };

    hold(&mut spec, &[(6, 1)], 8); // LET
    type_text(&mut spec, "s$=\"hello\"");
    hold(&mut spec, &[ENTER], 8);
    for _ in 0..200 {
        spec.run(FRAME_T);
        spec.bus.audio_sync();
        drain(&mut spec, &mut samples);
    }
    drain(&mut spec, &mut samples);

    let loudest = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    assert!(
        loudest > 0.02,
        "the machine should have made a noise: loudest sample {loudest} of \
         {} samples",
        samples.len()
    );

    // And it is speech rather than a click: at 48kHz, a click is a handful of
    // samples and a spoken word is thousands. Most of the recording is the
    // machine sitting at its prompt, so what matters is how long the sound
    // lasts and not what fraction of the whole it is.
    let busy = samples.iter().filter(|s| s.abs() > loudest / 8.0).count();
    assert!(
        busy > 400,
        "and it should last: only {busy} of {} samples are loud, which is {}ms",
        samples.len(),
        busy * 1000 / 48_000
    );

    // Five sounds went to the chip, which is "hello" as the Currah says it.
    let uspeech = spec.bus.uspeech.as_ref().unwrap();
    assert!(
        uspeech.phonemes >= 4,
        "and it should be a word rather than one noise: {} sounds",
        uspeech.phonemes
    );
}
