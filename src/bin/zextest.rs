//! Runs a CP/M `.com` Z80 exerciser (zexdoc / zexall) against the CPU core.
//!
//! Usage: `cargo run --release --bin zextest -- path/to/zexdoc.com`

use std::io::Write;
use zx_rustrum::z80::{Bus, Z80};

struct FlatBus {
    mem: Vec<u8>,
    pub tstates: u64,
}

impl Bus for FlatBus {
    fn fetch_op(&mut self, addr: u16) -> u8 {
        self.tstates += 4;
        self.mem[addr as usize]
    }
    fn read(&mut self, addr: u16) -> u8 {
        self.tstates += 3;
        self.mem[addr as usize]
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.tstates += 3;
        self.mem[addr as usize] = value;
    }
    fn contend(&mut self, _addr: u16, times: u32) {
        self.tstates += times as u64;
    }
    fn io_read(&mut self, _port: u16) -> u8 {
        self.tstates += 4;
        0xff
    }
    fn io_write(&mut self, _port: u16, _value: u8) {
        self.tstates += 4;
    }
    fn peek(&self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: zextest <file.com>");
        std::process::exit(2);
    });
    let prog = std::fs::read(&path).expect("cannot read test program");

    let mut bus = FlatBus {
        mem: vec![0; 65536],
        tstates: 0,
    };
    bus.mem[0x0100..0x0100 + prog.len()].copy_from_slice(&prog);
    // CP/M entry points: RET at the BDOS call address, and a jump to 0 to exit.
    bus.mem[0x0005] = 0xc9;
    bus.mem[0x0000] = 0x76;

    let mut cpu = Z80::new();
    cpu.pc = 0x0100;
    cpu.sp = 0xf000;

    let mut out = std::io::stdout();
    let start = std::time::Instant::now();
    loop {
        if cpu.pc == 0x0005 {
            match cpu.c {
                2 => {
                    out.write_all(&[cpu.e]).unwrap();
                }
                9 => {
                    let mut addr = cpu.de();
                    loop {
                        let ch = bus.mem[addr as usize];
                        if ch == b'$' {
                            break;
                        }
                        out.write_all(&[ch]).unwrap();
                        addr = addr.wrapping_add(1);
                    }
                }
                _ => {}
            }
            out.flush().unwrap();
        }
        if cpu.pc == 0x0000 {
            break;
        }
        cpu.step(&mut bus);
    }

    println!(
        "\ndone: {} instructions, {} T-states, {:.1}s wall",
        cpu.instructions,
        bus.tstates,
        start.elapsed().as_secs_f32()
    );
}
