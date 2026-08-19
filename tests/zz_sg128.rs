//! Scratch: does Fastload hurry a 128K tape?
use std::time::Instant;
use zx_rustrum::machine::{Model, Spectrum, FRAME_T};
use zx_rustrum::tape::Tape;
use zx_rustrum::ui::{App, Roms};

const TAPE: &str = "tapes/Starglider (1986)(Rainbird Software)[128K].zip";

fn tape() -> Option<Tape> {
    let bytes = std::fs::read(TAPE).ok()?;
    let (inner, bytes) = zx_rustrum::zip::first_with_extension(&bytes, &["tzx", "tap"])?;
    Tape::from_bytes(&inner, &bytes).ok()
}

/// The 128's menu comes up with Tape Loader highlighted, so ENTER starts it.
fn press_enter(spec: &mut Spectrum) {
    for _ in 0..200 {
        spec.run(FRAME_T);
    }
    spec.bus.keys[6] &= !1;
    for _ in 0..6 {
        spec.run(FRAME_T);
    }
    spec.bus.keys[6] |= 1;
    for _ in 0..20 {
        spec.run(FRAME_T);
    }
}

#[test]
fn probe() {
    let rom48 = std::fs::read("roms/48.rom").unwrap();
    let rom128 = std::fs::read("roms/128.rom").unwrap();
    let Some(t) = tape() else {
        eprintln!("no tape");
        return;
    };
    println!("{} blocks; first few:", t.blocks.len());
    for (i, b) in t.blocks.iter().enumerate().take(6) {
        println!("  block {i} {}", b.describe());
    }
    for (name, boost, flash) in [("max", true, false), ("fastload", true, true)] {
        let roms = Roms {
            rom48: Some(rom48.clone()),
            rom128: Some(rom128.clone()),
            ..Roms::default()
        };
        let mut app = App::with_roms(
            Spectrum::with_model(Model::Spectrum128),
            String::new(),
            roms,
            None,
        );
        app.show_ram_map = false;
        app.show_debugger = false;
        app.show_back_buffer = false;
        app.show_tape = false;
        app.running = true;
        app.switch_model(Model::Spectrum128);
        app.spec.reset();
        app.spec.bus.tape_boost = boost;
        app.spec.bus.tape_flash = flash;
        app.spec.bus.tape = Some(t.clone());
        press_enter(&mut app.spec);
        let now = app.machine_t();
        app.tape_mut().unwrap().play(now);
        let at = Instant::now();
        let mut host = 0;
        let mut worst = std::time::Duration::ZERO;
        while host < 40_000 {
            let frame = Instant::now();
            app.advance(1.0 / 60.0);
            worst = worst.max(frame.elapsed());
            // Paced as the application is: a host frame every 16.6ms, or the
            // work if it takes longer than that.
            if let Some(left) =
                std::time::Duration::from_micros(16_667).checked_sub(frame.elapsed())
            {
                std::thread::sleep(left);
            }
            host += 1;
            if !app.tape_is_playing() {
                break;
            }
        }
        let drawn = (0x4000..0x5800u16)
            .filter(|a| app.spec.bus.mem(*a) != 0)
            .count();
        println!(
            "RESULT {name:9} host {host:6} in {:?} (worst frame {worst:?}) pc ${:04X} drawn {drawn}",
            at.elapsed(),
            app.spec.cpu.pc
        );
    }
}
