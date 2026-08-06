//! End-to-end checks of the debugging features, driven by the built-in demo
//! ROM (which builds a screen at $8000 and blits it to video RAM).

use zx_spectrum_emulator::demo_rom::DEMO_ROM;
use zx_spectrum_emulator::machine::{Spectrum, Stop, FRAME_T};
use zx_spectrum_emulator::{disasm, screen};

fn demo_machine() -> Spectrum {
    let mut s = Spectrum::new();
    s.load_rom(&DEMO_ROM);
    s
}

/// Run `frames` whole frames, doing the per-frame visual bookkeeping the UI
/// would normally do.
fn run_frames(s: &mut Spectrum, frames: u32) {
    for _ in 0..frames {
        s.run(FRAME_T);
        s.bus.frame_visuals();
    }
}

#[test]
fn demo_rom_draws_to_video_ram() {
    let mut s = demo_machine();
    run_frames(&mut s, 40);
    let written = (0x4000..0x5b00u32)
        .filter(|a| s.bus.peek_raw(*a as u16) != 0)
        .count();
    assert!(written > 4000, "only {written} bytes of video RAM written");
}

#[test]
fn heat_maps_record_reads_and_writes_and_then_fade() {
    let mut s = demo_machine();
    run_frames(&mut s, 10);
    // Heat is kept per physical location, so look each address up through
    // the current paging.
    let phys = |s: &Spectrum, addr: u16| s.bus.phys_index(addr);
    let (back, screen, loop_start, untouched) = (
        phys(&s, 0x8000),
        phys(&s, 0x4000),
        phys(&s, 0x000a),
        phys(&s, 0xc000),
    );
    let t = &s.bus.tracker;
    assert!(t.write_count[back] > 0, "back buffer never written");
    assert!(t.write_count[screen] > 0, "video RAM never written");
    assert!(
        t.exec_heat[loop_start] > 0,
        "main loop never marked as executed"
    );

    // Nothing touches $C000, so it must stay cold.
    assert_eq!(t.read_count[untouched], 0);

    // With the CPU stopped, heat decays to nothing.
    let before = s.bus.tracker.write_heat[screen];
    assert!(before > 0);
    for _ in 0..64 {
        s.bus.frame_visuals();
    }
    assert_eq!(s.bus.tracker.write_heat[screen], 0, "heat did not fade out");
}

#[test]
fn back_buffer_at_8000_is_detected() {
    let mut s = demo_machine();
    run_frames(&mut s, 60);
    let region = s
        .bus
        .tracker
        .back_buffer()
        .expect("no back buffer detected after 60 frames");
    assert_eq!(region.start, 0x8000, "detected {region:?}");
    assert!(region.len >= 6144, "region too small: {region:?}");
    assert!(s.bus.tracker.detected_confidence > 0.0);
}

#[test]
fn manual_override_beats_detection() {
    let mut s = demo_machine();
    run_frames(&mut s, 60);
    s.bus.tracker.manual = Some(zx_spectrum_emulator::tracker::Region {
        start: 0xa000,
        len: 6912,
    });
    assert_eq!(s.bus.tracker.back_buffer().unwrap().start, 0xa000);
}

#[test]
fn slow_draw_parks_the_cpu_after_its_write_allowance() {
    let mut s = demo_machine();
    // Let the demo get going and put a picture in video RAM first.
    run_frames(&mut s, 40);

    s.bus.slow.enabled = true;
    s.bus.slow.watch_screen = true;
    s.bus.slow.watch_back_buffer = false;
    s.bus.slow.writes_per_slice = 4;

    // Find a slice that actually reaches the blit; the build phase writes to
    // the back buffer only, which is not being watched here.
    let mut parked = false;
    for _ in 0..200 {
        s.bus.slow.begin_slice();
        let screen_writes = |s: &Spectrum| -> u32 {
            (0x4000..0x5b00u32)
                .map(|a| s.bus.tracker.write_count[s.bus.phys_index(a as u16)])
                .sum()
        };
        let before = screen_writes(&s);
        let stop = s.run(FRAME_T);
        let after = screen_writes(&s);
        if stop == Stop::SlowDraw {
            parked = true;
            assert_eq!(after - before, 4, "wrote more than the allowance");
            break;
        }
    }
    assert!(parked, "slow draw never parked the CPU");
}

#[test]
fn breakpoints_stop_execution() {
    let mut s = demo_machine();
    s.breakpoints.push(0x0033); // the LDIR that flips the buffer
    let mut hit = None;
    for _ in 0..60 {
        if let Stop::Breakpoint(pc) = s.run(FRAME_T) {
            hit = Some(pc);
            break;
        }
    }
    assert_eq!(hit, Some(0x0033));
    assert_eq!(s.cpu.pc, 0x0033);
}

#[test]
fn step_over_target_recognises_calls_and_block_moves() {
    let s = demo_machine();
    assert!(s.is_step_over_target(0x0033), "LDIR");
    assert!(!s.is_step_over_target(0x000a), "LD HL,nn");
}

#[test]
fn disassembly_round_trips_the_demo_rom() {
    let s = demo_machine();
    let peek = |a: u16| s.bus.peek_raw(a);
    let mut addr = 0u16;
    let mut text = Vec::new();
    while addr < 0x39 {
        let insn = disasm::disasm(&peek, addr);
        text.push(format!("{addr:04X} {}", insn.text));
        addr += insn.len as u16;
    }
    assert_eq!(addr, 0x39, "instruction lengths do not tile the ROM");
    assert!(text.iter().any(|l| l.contains("LDIR")));
    assert!(text.iter().any(|l| l.contains("LD HL,$8000")));
    assert!(text.iter().any(|l| l.contains("OUT ($FE),A")));

    // The listing above an address must start on a real opcode boundary:
    // disassembling from sync_start has to land exactly on the target.
    let start = disasm::sync_start(&peek, 0x0033, 12);
    assert!(start < 0x0033 && 0x0033 - start <= 12, "start {start:#06x}");
    let mut a = start;
    while a < 0x0033 {
        a += disasm::disasm(&peek, a).len as u16;
    }
    assert_eq!(a, 0x0033, "sync_start picked a mid-instruction address");
}

#[test]
fn screen_renderer_produces_a_full_frame() {
    let mut s = demo_machine();
    run_frames(&mut s, 40);
    let mut buf = vec![0u8; screen::View::OVERSCAN.buffer_len()];
    screen::render(&s.bus, screen::View::OVERSCAN, &mut buf, false);
    assert!(buf.chunks(4).all(|p| p[3] == 0xff), "alpha not filled in");
    let distinct: std::collections::HashSet<[u8; 3]> =
        buf.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
    assert!(distinct.len() > 1, "rendered a blank screen");
}
