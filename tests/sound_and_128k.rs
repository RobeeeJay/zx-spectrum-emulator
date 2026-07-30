//! Beeper/AY sound generation and the 128K machine: paging, timing, contention.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use zx_spectrum_emulator::audio::{Audio, Ay, SharedQueue};
use zx_spectrum_emulator::machine::{Model, Spectrum, Stop};
use zx_spectrum_emulator::z80::Bus;

// ---------------------------------------------------------------------------
// AY-3-8912
// ---------------------------------------------------------------------------

/// Count output transitions of channel A over `clocks` AY clocks.
fn count_edges(ay: &mut Ay, clocks: f64, step: f64) -> u32 {
    let mut edges = 0;
    let mut last = ay.output() > 0.0;
    let mut t = 0.0;
    while t < clocks {
        ay.advance(step);
        let now = ay.output() > 0.0;
        if now != last {
            edges += 1;
            last = now;
        }
        t += step;
    }
    edges
}

#[test]
fn ay_tone_frequency_matches_the_datasheet() {
    // Tone frequency is clock / (16 * period).
    for period in [50u32, 200, 1000] {
        let mut ay = Ay::new();
        ay.selected = 0;
        ay.write((period & 0xff) as u8);
        ay.selected = 1;
        ay.write((period >> 8) as u8);
        ay.selected = 7;
        ay.write(0b1111_1110); // channel A tone on, everything else off
        ay.selected = 8;
        ay.write(15);

        let clock = 1_773_450.0f64;
        let edges = count_edges(&mut ay, clock, 8.0); // one second
        let expected = clock / (16.0 * period as f64) * 2.0; // two edges per cycle
        let ratio = edges as f64 / expected;
        assert!(
            (0.97..1.03).contains(&ratio),
            "period {period}: {edges} edges, expected about {expected:.0}"
        );
    }
}

/// One channel at full volume, given `output()` averages the three channels.
const FULL_ONE_CHANNEL: f32 = 1.0 / 3.0;

#[test]
fn ay_volume_and_mixer_control_the_output() {
    let mut ay = Ay::new();
    // Tone and noise both disabled in the mixer holds the channel high, which
    // on real hardware is a DC level at the set volume, not silence.
    ay.selected = 7;
    ay.write(0b1111_1111);
    ay.selected = 8;
    ay.write(15);
    ay.advance(1000.0);
    assert!(
        (ay.output() - FULL_ONE_CHANNEL).abs() < 0.001,
        "a disabled channel is held high at its volume, got {}",
        ay.output()
    );

    // Volume 0 really is silence.
    ay.selected = 8;
    ay.write(0);
    ay.advance(8.0);
    assert_eq!(ay.output(), 0.0, "volume 0 is silence");

    // A tone at full volume swings between silence and full level.
    ay.selected = 8;
    ay.write(15);
    ay.selected = 7;
    ay.write(0b1111_1110); // channel A tone on, noise off
    ay.selected = 0;
    ay.write(0x10);
    ay.selected = 1;
    ay.write(0x00);
    let (mut high, mut low) = (false, false);
    for _ in 0..10_000 {
        ay.advance(8.0);
        let o = ay.output();
        if o > FULL_ONE_CHANNEL - 0.01 {
            high = true;
        }
        if o < 0.01 {
            low = true;
        }
    }
    assert!(high && low, "a square wave should reach both rails");
}

#[test]
fn ay_envelope_ramps_and_holds() {
    let mut ay = Ay::new();
    ay.selected = 7;
    ay.write(0b1111_1110);
    ay.selected = 8;
    ay.write(0x10); // channel A uses the envelope
    ay.selected = 11;
    ay.write(0x10); // short envelope period
    ay.selected = 12;
    ay.write(0x00);

    // Shape $0C: /|/|/| — a repeating rising ramp.
    ay.selected = 13;
    ay.write(0x0c);
    let mut seen_low = false;
    let mut seen_high = false;
    for _ in 0..20_000 {
        ay.advance(8.0);
        let o = ay.output();
        if o < 0.02 {
            seen_low = true;
        }
        if o > FULL_ONE_CHANNEL - 0.02 {
            seen_high = true;
        }
    }
    assert!(seen_low && seen_high, "a ramp should sweep the whole range");

    // Shape $09: \___ — one fall, then held at zero.
    ay.selected = 13;
    ay.write(0x09);
    for _ in 0..40_000 {
        ay.advance(8.0);
    }
    assert_eq!(ay.output(), 0.0, "shape $09 must hold at silence");
}

#[test]
fn ay_registers_mask_unused_bits() {
    let mut ay = Ay::new();
    ay.selected = 1;
    ay.write(0xff);
    assert_eq!(ay.regs[1], 0x0f, "tone period high byte is 4 bits");
    ay.selected = 6;
    ay.write(0xff);
    assert_eq!(ay.regs[6], 0x1f, "noise period is 5 bits");
    ay.selected = 8;
    ay.write(0xff);
    assert_eq!(ay.regs[8], 0x1f, "volume is 4 bits plus the envelope flag");
}

// ---------------------------------------------------------------------------
// mixer
// ---------------------------------------------------------------------------

fn audio_with_queue(cpu_hz: f64, rate: f64) -> (Audio, SharedQueue) {
    let q: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
    let mut a = Audio::new(cpu_hz);
    a.attach(q.clone(), rate);
    a.volume = 1.0;
    (a, q)
}

#[test]
fn the_mixer_produces_samples_at_the_device_rate() {
    let (mut audio, _q) = audio_with_queue(3_500_000.0, 48_000.0);
    audio.beeper = 0.5;
    // One second of emulated time, in frame-sized steps as the emulator does.
    // (A single huge jump is deliberately skipped rather than generating a
    // burst of stale audio, so time has to be advanced in normal increments.)
    for i in 1..=50u64 {
        audio.advance_to(i * 70_000);
    }
    audio.advance_to(3_500_000);
    audio.flush();
    let produced = audio.produced as f64;
    assert!(
        (produced - 48_000.0).abs() < 10.0,
        "expected ~48000 samples in one second, got {produced}"
    );
}

#[test]
fn the_beeper_makes_a_square_wave() {
    let (mut audio, q) = audio_with_queue(3_500_000.0, 48_000.0);
    // 1 kHz square wave: flip the speaker bit every 1750 T-states.
    let half = 1750u64;
    let mut t = 0u64;
    let mut level = false;
    for _ in 0..200 {
        audio.advance_to(t);
        level = !level;
        audio.beeper = if level { 0.55 } else { 0.0 };
        t += half;
    }
    audio.flush();

    let samples: Vec<f32> = q.lock().unwrap().iter().copied().collect();
    assert!(samples.len() > 1000, "got {} samples", samples.len());
    // The output is DC-blocked, so the square wave sits either side of zero
    // rather than between 0 and its full level.
    let high = samples.iter().filter(|s| **s > 0.1).count();
    let low = samples.iter().filter(|s| **s < -0.1).count();
    assert!(high > 300 && low > 300, "high {high} low {low}");
    // Once the DC blocker has settled (its time constant is a few tens of
    // milliseconds) the wave sits symmetrically about zero.
    let tail = &samples[samples.len() / 2..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(mean.abs() < 0.05, "the output should be centred, mean {mean}");
    // A 1 kHz square wave crosses zero 2000 times a second; over the ~100 ms
    // generated here that is about 200 crossings.
    let crossings = samples
        .windows(2)
        .filter(|w| (w[0] > 0.0) != (w[1] > 0.0))
        .count();
    assert!(
        (150..260).contains(&crossings),
        "{crossings} zero crossings for a 1 kHz tone"
    );
}

#[test]
fn muting_silences_the_output_but_keeps_time() {
    let (mut audio, q) = audio_with_queue(3_500_000.0, 48_000.0);
    audio.beeper = 0.55;
    audio.enabled = false;
    audio.advance_to(350_000); // 0.1 s
    audio.flush();
    let samples: Vec<f32> = q.lock().unwrap().iter().copied().collect();
    assert!(samples.len() > 4000);
    assert!(samples.iter().all(|s| *s == 0.0), "mute must be silent");
}

#[test]
fn the_beeper_reaches_the_mixer_through_port_fe() {
    let mut spec = Spectrum::new();
    let q: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
    spec.bus.audio.attach(q.clone(), 48_000.0);
    spec.bus.audio.volume = 1.0;

    // Toggle the speaker bit for a while.
    for i in 0..400 {
        spec.bus.tstates = (i * 100) % 60_000;
        spec.bus.io_write(0xfe, if i % 2 == 0 { 0x10 } else { 0x00 });
        spec.bus.frame = i as u64 / 8;
    }
    spec.bus.audio_sync();
    spec.bus.audio.flush();
    let samples: Vec<f32> = q.lock().unwrap().iter().copied().collect();
    assert!(!samples.is_empty(), "port $FE writes produced no sound");
    let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(
        peak > 0.02,
        "the speaker bit never reached the mixer (peak {peak})"
    );
}

// ---------------------------------------------------------------------------
// 128K machine
// ---------------------------------------------------------------------------

#[test]
fn the_128k_has_its_own_clock_and_frame_length() {
    assert_eq!(Model::Spectrum128.frame_t(), 70908);
    assert_eq!(Model::Spectrum128.t_per_line(), 228);
    assert_eq!(Model::Spectrum128.first_pixel_t(), 14361);
    assert_eq!(Model::Spectrum128.cpu_hz(), 3_546_900.0);
    // 50.01 Hz, near enough to the 48K's 50.08.
    let fps = Model::Spectrum128.cpu_hz() / Model::Spectrum128.frame_t() as f64;
    assert!((fps - 50.0).abs() < 0.1, "{fps} frames per second");
}

#[test]
fn paging_switches_rom_and_the_top_bank() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    // Mark each RAM bank so we can tell which one is paged in.
    for bank in 0..8usize {
        spec.bus.ram[bank * 0x4000] = 0xa0 + bank as u8;
    }
    // Two different ROM halves.
    spec.bus.rom[0] = 0x11;
    spec.bus.rom[0x4000] = 0x22;

    for bank in 0..8usize {
        spec.bus.io_write(0x7ffd, bank as u8);
        assert_eq!(
            spec.bus.peek_raw(0xc000),
            0xa0 + bank as u8,
            "bank {bank} should be at $C000"
        );
    }
    // Banks 5 and 2 are always at $4000 and $8000.
    assert_eq!(spec.bus.peek_raw(0x4000), 0xa5);
    assert_eq!(spec.bus.peek_raw(0x8000), 0xa2);

    spec.bus.io_write(0x7ffd, 0x00);
    assert_eq!(spec.bus.peek_raw(0x0000), 0x11, "ROM 0 selected");
    spec.bus.io_write(0x7ffd, 0x10);
    assert_eq!(spec.bus.peek_raw(0x0000), 0x22, "ROM 1 selected");
}

#[test]
fn the_shadow_screen_bit_switches_the_displayed_bank() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.bus.ram[5 * 0x4000] = 0x55;
    spec.bus.ram[7 * 0x4000] = 0x77;
    assert_eq!(spec.bus.screen_bank(), 5);
    assert_eq!(spec.bus.video(0), 0x55);
    spec.bus.io_write(0x7ffd, 0x08);
    assert_eq!(spec.bus.screen_bank(), 7);
    assert_eq!(spec.bus.video(0), 0x77);
}

#[test]
fn setting_the_lock_bit_freezes_paging() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.bus.io_write(0x7ffd, 0x23); // bank 3, lock
    assert!(spec.bus.paging_locked);
    let before = spec.bus.page_reg;
    spec.bus.io_write(0x7ffd, 0x01);
    assert_eq!(spec.bus.page_reg, before, "paging must stay locked");
    spec.reset();
    assert!(!spec.bus.paging_locked, "reset unlocks paging");
}

#[test]
fn odd_banks_are_contended_wherever_they_are_paged() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    let at_pixel = Model::Spectrum128.first_pixel_t();

    // Bank 5 at $4000 is always contended.
    spec.bus.tstates = at_pixel;
    spec.bus.read(0x4000);
    assert_eq!(spec.bus.tstates, at_pixel + 6 + 3);

    // Bank 2 at $8000 never is.
    spec.bus.tstates = at_pixel;
    spec.bus.read(0x8000);
    assert_eq!(spec.bus.tstates, at_pixel + 3);

    // $C000 depends on which bank is paged in.
    spec.bus.io_write(0x7ffd, 1); // odd bank
    spec.bus.tstates = at_pixel;
    spec.bus.read(0xc000);
    assert_eq!(spec.bus.tstates, at_pixel + 6 + 3, "bank 1 is contended");

    spec.bus.io_write(0x7ffd, 4); // even bank
    spec.bus.tstates = at_pixel;
    spec.bus.read(0xc000);
    assert_eq!(spec.bus.tstates, at_pixel + 3, "bank 4 is not contended");
}

#[test]
fn the_ay_is_reachable_through_its_ports() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.bus.io_write(0xfffd, 7); // select the mixer register
    spec.bus.io_write(0xbffd, 0x3e);
    assert_eq!(spec.bus.audio.ay.regs[7], 0x3e);
    assert_eq!(spec.bus.io_read(0xfffd), 0x3e, "reading $FFFD returns it");

    spec.bus.io_write(0xfffd, 0);
    spec.bus.io_write(0xbffd, 0x34);
    assert_eq!(spec.bus.audio.ay.regs[0], 0x34);

    // A 48K machine has no AY, so those ports do nothing.
    let mut spec48 = Spectrum::new();
    spec48.bus.io_write(0xfffd, 7);
    spec48.bus.io_write(0xbffd, 0x3e);
    assert_eq!(spec48.bus.audio.ay.regs[7], 0);
}

#[test]
fn a_128k_program_can_play_the_ay() {
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    let q: SharedQueue = Arc::new(Mutex::new(VecDeque::new()));
    spec.bus.audio.attach(q.clone(), 48_000.0);
    spec.bus.audio.volume = 1.0;

    // A little program: set channel A to a mid tone at full volume, then halt.
    let prog: [u8; 22] = [
        0xf3, // DI
        0x01, 0xfd, 0xff, // LD BC,$FFFD
        0x3e, 0x07, // LD A,7  (mixer)
        0xed, 0x79, // OUT (C),A
        0x01, 0xfd, 0xbf, // LD BC,$BFFD
        0x3e, 0x3e, // LD A,%00111110 (tone A only)
        0xed, 0x79, // OUT (C),A
        0x18, 0x00, // JR +0
        0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    spec.bus.rom[..prog.len()].copy_from_slice(&prog);
    // Registers for tone A and volume A, poked straight in for brevity.
    spec.bus.audio.ay.selected = 0;
    spec.bus.audio.ay.write(0x40);
    spec.bus.audio.ay.selected = 1;
    spec.bus.audio.ay.write(0x00);
    spec.bus.audio.ay.selected = 8;
    spec.bus.audio.ay.write(0x0f);

    spec.cpu.pc = 0;
    for _ in 0..5 {
        spec.run(Model::Spectrum128.frame_t());
    }
    spec.bus.audio_sync();
    spec.bus.audio.flush();

    let samples: Vec<f32> = q.lock().unwrap().iter().copied().collect();
    assert!(samples.len() > 3000, "got {} samples", samples.len());
    let peak = samples.iter().cloned().fold(0.0f32, f32::max);
    assert!(peak > 0.05, "the AY produced no audible output (peak {peak})");
}

#[test]
fn the_128k_rom_boots_to_its_menu() {
    let Ok(rom) = std::fs::read("roms/128.rom") else {
        eprintln!("no roms/128.rom; skipping");
        return;
    };
    let mut spec = Spectrum::with_model(Model::Spectrum128);
    spec.load_rom(&rom);
    spec.reset();

    for _ in 0..300 {
        assert_ne!(spec.run(Model::Spectrum128.frame_t()), Stop::SlowDraw);
    }

    // The 128K menu fills the screen with text and a coloured border.
    let drawn = (0..6144u16).filter(|o| spec.bus.video(*o) != 0).count();
    assert!(drawn > 500, "only {drawn} non-blank pixel bytes after boot");
    let attrs: std::collections::HashSet<u8> =
        (0x1800..0x1b00u16).map(|o| spec.bus.video(o)).collect();
    assert!(attrs.len() > 1, "attributes were never written");
    assert!(spec.cpu.iff1, "the ROM should have enabled interrupts");
}

// ---------------------------------------------------------------------------
// +2A / +3
// ---------------------------------------------------------------------------

use zx_spectrum_emulator::machine::SpectrumBus;

#[test]
fn the_plus3_has_four_roms_selected_by_two_ports() {
    let mut spec = Spectrum::with_model(Model::Plus3);
    assert_eq!(spec.bus.rom.len(), 0x10000, "four 16K ROMs");
    for page in 0..4usize {
        spec.bus.rom[page * 0x4000] = 0xd0 + page as u8;
    }

    // ROM number is bit 4 of $7FFD (low) and bit 2 of $1FFD (high).
    for rom in 0..4usize {
        let lo = ((rom & 1) as u8) << 4;
        let hi = ((rom >> 1) as u8) << 2;
        spec.bus.io_write(0x1ffd, hi);
        spec.bus.io_write(0x7ffd, lo);
        assert_eq!(
            spec.bus.peek_raw(0x0000),
            0xd0 + rom as u8,
            "ROM {rom} should be paged in (7FFD={lo:02X} 1FFD={hi:02X})"
        );
    }
}

#[test]
fn special_paging_maps_four_ram_banks_and_no_rom() {
    let mut spec = Spectrum::with_model(Model::Plus3);
    for bank in 0..8usize {
        spec.bus.ram[bank * 0x4000] = 0xb0 + bank as u8;
    }

    for (config, banks) in SpectrumBus::SPECIAL_CONFIGS.iter().enumerate() {
        // Re-mark the banks: the writability probe below overwrites one.
        for bank in 0..8usize {
            spec.bus.ram[bank * 0x4000] = 0xb0 + bank as u8;
        }
        spec.bus.io_write(0x1ffd, 0x01 | ((config as u8) << 1));
        assert!(spec.bus.special_paging(), "config {config} should be special");
        for (slot, bank) in banks.iter().enumerate() {
            let addr = (slot as u16) << 14;
            assert_eq!(
                spec.bus.peek_raw(addr),
                0xb0 + *bank as u8,
                "config {config} slot {slot} should hold bank {bank}"
            );
        }
        // With no ROM paged in, $0000 is writable.
        spec.bus.write(0x0000, 0x5a);
        assert_eq!(spec.bus.peek_raw(0x0000), 0x5a, "all-RAM mode is writable");
    }

    // Leaving special mode brings the ROM back and makes $0000 read-only.
    spec.bus.rom[0] = 0xc9;
    spec.bus.io_write(0x1ffd, 0x00);
    assert!(!spec.bus.special_paging());
    spec.bus.write(0x0000, 0x00);
    assert_eq!(spec.bus.peek_raw(0x0000), 0xc9, "ROM must ignore writes");
}

#[test]
fn the_plus3_contends_the_top_four_banks_with_its_own_pattern() {
    assert_eq!(
        Model::Plus3.contention_pattern(),
        [1, 0, 7, 6, 5, 4, 3, 2],
        "the +2A/+3 delay sequence differs from the 48K/128K"
    );
    for bank in 0..8usize {
        assert_eq!(
            Model::Plus3.bank_is_contended(bank),
            bank >= 4,
            "bank {bank}"
        );
        assert_eq!(
            Model::Spectrum128.bank_is_contended(bank),
            bank & 1 == 1,
            "bank {bank} on a 128K"
        );
    }

    let mut spec = Spectrum::with_model(Model::Plus3);
    let first = Model::Plus3.first_pixel_t();

    // Bank 5 at $4000 is contended on both machines, but the delay differs.
    spec.bus.tstates = first;
    spec.bus.read(0x4000);
    assert_eq!(spec.bus.tstates, first + 1 + 3, "+3 delay at phase 0 is 1");

    spec.bus.tstates = first + 2;
    spec.bus.read(0x4000);
    assert_eq!(spec.bus.tstates, first + 2 + 7 + 3, "phase 2 delay is 7");

    // Bank 1 at $C000 is not contended on a +3 (it is on a 128K).
    spec.bus.io_write(0x7ffd, 1);
    spec.bus.tstates = first;
    spec.bus.read(0xc000);
    assert_eq!(spec.bus.tstates, first + 3, "bank 1 is uncontended on a +3");

    // Bank 6 is.
    spec.bus.io_write(0x7ffd, 6);
    spec.bus.tstates = first;
    spec.bus.read(0xc000);
    assert_eq!(spec.bus.tstates, first + 1 + 3, "bank 6 is contended");
}

#[test]
fn the_plus3_has_no_floating_bus() {
    let mut spec = Spectrum::with_model(Model::Plus3);
    spec.bus.ram[5 * 0x4000] = 0x3c;
    spec.bus.tstates = Model::Plus3.first_pixel_t();
    assert_eq!(spec.bus.io_read(0xff), 0xff, "the +3 drives the bus high");

    // The bus is sampled after the I/O cycle's own T-states have elapsed, so
    // plant the value across the first few cells of the display file.
    let mut spec128 = Spectrum::with_model(Model::Spectrum128);
    for i in 0..8 {
        spec128.bus.ram[5 * 0x4000 + i] = 0x3c;
    }
    spec128.bus.tstates = Model::Spectrum128.first_pixel_t();
    assert_eq!(
        spec128.bus.io_read(0xff),
        0x3c,
        "the 128K floating bus returns the byte being fetched"
    );
}

#[test]
fn the_plus3_decodes_its_paging_ports_strictly() {
    let mut spec = Spectrum::with_model(Model::Plus3);
    // $7FFD needs A15 low and A14 high on a +2A/+3.
    spec.bus.io_write(0x7ffd, 3);
    assert_eq!(spec.bus.page_reg, 3);
    // $3FFD (A14 high, A13 high) is the FDC data port, not paging.
    spec.bus.io_write(0x3ffd, 6);
    assert_eq!(spec.bus.page_reg, 3, "$3FFD must not change paging");
    // A 128K, which only decodes A15 and A1, does react to $3FFD.
    let mut spec128 = Spectrum::with_model(Model::Spectrum128);
    spec128.bus.io_write(0x3ffd, 6);
    assert_eq!(spec128.bus.page_reg, 6, "the 128K decodes loosely");

    spec.bus.io_write(0x1ffd, 0x08);
    assert_eq!(spec.bus.page_reg_1ffd, 0x08);
    assert!(spec.bus.disk_motor(), "$1FFD bit 3 is the disk motor");
}

#[test]
fn the_plus3_rom_boots_to_its_menu() {
    let Ok(rom) = std::fs::read("roms/plus3.rom") else {
        eprintln!("no roms/plus3.rom; skipping");
        return;
    };
    let mut spec = Spectrum::with_model(Model::Plus3);
    spec.load_rom(&rom);
    spec.reset();

    for _ in 0..400 {
        spec.run(Model::Plus3.frame_t());
    }

    let drawn = (0..6144u16).filter(|o| spec.bus.video(*o) != 0).count();
    assert!(drawn > 500, "only {drawn} non-blank pixel bytes after boot");
    assert!(spec.cpu.iff1, "the ROM should have enabled interrupts");
}
