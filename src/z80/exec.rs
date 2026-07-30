//! Instruction decode and execution.
//!
//! Decoding follows the octal layout of the Z80 opcode map
//! (`x = op >> 6`, `y = (op >> 3) & 7`, `z = op & 7`).
//!
//! T-state accounting lives in the [`Bus`] calls: 4 for an M1 fetch, 3 for a
//! memory access, and explicit `contend` calls for the internal cycles, which
//! is what makes ULA contention land on the right T-state inside multi-cycle
//! instructions.

use super::flags::*;
use super::{Bus, Idx, Z80};

pub fn execute(cpu: &mut Z80, bus: &mut impl Bus, op: u8, idx: Idx) {
    match op {
        0xdd => {
            let next = cpu.fetch(bus);
            execute(cpu, bus, next, Idx::Ix);
        }
        0xfd => {
            let next = cpu.fetch(bus);
            execute(cpu, bus, next, Idx::Iy);
        }
        0xed => ed(cpu, bus),
        0xcb => {
            if idx == Idx::Hl {
                cb(cpu, bus)
            } else {
                ddcb(cpu, bus, idx)
            }
        }
        _ => base(cpu, bus, op, idx),
    }
}

// ---------------------------------------------------------------------------
// register helpers
// ---------------------------------------------------------------------------

/// Read register `code` (0..7, 6 excluded). When an index prefix is active,
/// H and L address the halves of IX/IY instead.
fn rd_r(cpu: &Z80, code: u8, idx: Idx) -> u8 {
    match code {
        0 => cpu.b,
        1 => cpu.c,
        2 => cpu.d,
        3 => cpu.e,
        4 => match idx {
            Idx::Hl => cpu.h,
            Idx::Ix => (cpu.ix >> 8) as u8,
            Idx::Iy => (cpu.iy >> 8) as u8,
        },
        5 => match idx {
            Idx::Hl => cpu.l,
            Idx::Ix => cpu.ix as u8,
            Idx::Iy => cpu.iy as u8,
        },
        7 => cpu.a,
        _ => unreachable!("(HL) must be handled by the caller"),
    }
}

fn wr_r(cpu: &mut Z80, code: u8, idx: Idx, v: u8) {
    match code {
        0 => cpu.b = v,
        1 => cpu.c = v,
        2 => cpu.d = v,
        3 => cpu.e = v,
        4 => match idx {
            Idx::Hl => cpu.h = v,
            Idx::Ix => cpu.ix = (cpu.ix & 0x00ff) | ((v as u16) << 8),
            Idx::Iy => cpu.iy = (cpu.iy & 0x00ff) | ((v as u16) << 8),
        },
        5 => match idx {
            Idx::Hl => cpu.l = v,
            Idx::Ix => cpu.ix = (cpu.ix & 0xff00) | v as u16,
            Idx::Iy => cpu.iy = (cpu.iy & 0xff00) | v as u16,
        },
        7 => cpu.a = v,
        _ => unreachable!("(HL) must be handled by the caller"),
    }
}

/// Effective address for `(HL)` / `(IX+d)` / `(IY+d)`, including the five
/// internal T-states the CPU spends adding the displacement.
fn ea(cpu: &mut Z80, bus: &mut impl Bus, idx: Idx) -> u16 {
    match idx {
        Idx::Hl => cpu.hl(),
        _ => {
            let at = cpu.pc;
            let d = cpu.imm8(bus) as i8;
            bus.contend(at, 5);
            let addr = cpu.idx_val(idx).wrapping_add(d as i16 as u16);
            cpu.wz = addr;
            addr
        }
    }
}

fn rp(cpu: &Z80, p: u8, idx: Idx) -> u16 {
    match p {
        0 => cpu.bc(),
        1 => cpu.de(),
        2 => cpu.idx_val(idx),
        _ => cpu.sp,
    }
}

fn set_rp(cpu: &mut Z80, p: u8, idx: Idx, v: u16) {
    match p {
        0 => cpu.set_bc(v),
        1 => cpu.set_de(v),
        2 => cpu.set_idx(idx, v),
        _ => cpu.sp = v,
    }
}

fn rp2(cpu: &Z80, p: u8, idx: Idx) -> u16 {
    match p {
        0 => cpu.bc(),
        1 => cpu.de(),
        2 => cpu.idx_val(idx),
        _ => cpu.af(),
    }
}

fn set_rp2(cpu: &mut Z80, p: u8, idx: Idx, v: u16) {
    match p {
        0 => cpu.set_bc(v),
        1 => cpu.set_de(v),
        2 => cpu.set_idx(idx, v),
        _ => cpu.set_af(v),
    }
}

fn cond(cpu: &Z80, y: u8) -> bool {
    match y {
        0 => !cpu.flag(ZF),
        1 => cpu.flag(ZF),
        2 => !cpu.flag(CF),
        3 => cpu.flag(CF),
        4 => !cpu.flag(PF),
        5 => cpu.flag(PF),
        6 => !cpu.flag(SF),
        _ => cpu.flag(SF),
    }
}

fn alu(cpu: &mut Z80, y: u8, v: u8) {
    match y {
        0 => cpu.add8(v),
        1 => cpu.adc8(v),
        2 => cpu.sub8(v),
        3 => cpu.sbc8(v),
        4 => cpu.and8(v),
        5 => cpu.xor8(v),
        6 => cpu.or8(v),
        _ => cpu.cp8(v),
    }
}

fn rot(cpu: &mut Z80, y: u8, v: u8) -> u8 {
    match y {
        0 => cpu.rlc(v),
        1 => cpu.rrc(v),
        2 => cpu.rl(v),
        3 => cpu.rr(v),
        4 => cpu.sla(v),
        5 => cpu.sra(v),
        6 => cpu.sll(v),
        _ => cpu.srl(v),
    }
}

// ---------------------------------------------------------------------------
// unprefixed / index-prefixed opcodes
// ---------------------------------------------------------------------------

fn base(cpu: &mut Z80, bus: &mut impl Bus, op: u8, idx: Idx) {
    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;
    let p = y >> 1;
    let q = y & 1;

    match x {
        0 => match z {
            0 => match y {
                0 => {}
                1 => {
                    std::mem::swap(&mut cpu.a, &mut cpu.a_);
                    std::mem::swap(&mut cpu.f, &mut cpu.f_);
                }
                2 => {
                    bus.contend(cpu.ir(), 1);
                    let at = cpu.pc;
                    let d = cpu.imm8(bus) as i8;
                    cpu.b = cpu.b.wrapping_sub(1);
                    if cpu.b != 0 {
                        bus.contend(at, 5);
                        cpu.pc = cpu.pc.wrapping_add(d as i16 as u16);
                        cpu.wz = cpu.pc;
                    }
                }
                3 => {
                    let at = cpu.pc;
                    let d = cpu.imm8(bus) as i8;
                    bus.contend(at, 5);
                    cpu.pc = cpu.pc.wrapping_add(d as i16 as u16);
                    cpu.wz = cpu.pc;
                }
                _ => {
                    let at = cpu.pc;
                    let d = cpu.imm8(bus) as i8;
                    if cond(cpu, y - 4) {
                        bus.contend(at, 5);
                        cpu.pc = cpu.pc.wrapping_add(d as i16 as u16);
                        cpu.wz = cpu.pc;
                    }
                }
            },
            1 => {
                if q == 0 {
                    let nn = cpu.imm16(bus);
                    set_rp(cpu, p, idx, nn);
                } else {
                    let hl = cpu.idx_val(idx);
                    cpu.wz = hl.wrapping_add(1);
                    bus.contend(cpu.ir(), 7);
                    let v = rp(cpu, p, idx);
                    let r = cpu.add16(hl, v);
                    cpu.set_idx(idx, r);
                }
            }
            2 => match (q, p) {
                (0, 0) => {
                    let addr = cpu.bc();
                    bus.write(addr, cpu.a);
                    cpu.wz = ((cpu.a as u16) << 8) | (addr.wrapping_add(1) & 0xff);
                }
                (0, 1) => {
                    let addr = cpu.de();
                    bus.write(addr, cpu.a);
                    cpu.wz = ((cpu.a as u16) << 8) | (addr.wrapping_add(1) & 0xff);
                }
                (0, 2) => {
                    let nn = cpu.imm16(bus);
                    let v = cpu.idx_val(idx);
                    cpu.write16(bus, nn, v);
                    cpu.wz = nn.wrapping_add(1);
                }
                (0, _) => {
                    let nn = cpu.imm16(bus);
                    bus.write(nn, cpu.a);
                    cpu.wz = ((cpu.a as u16) << 8) | (nn.wrapping_add(1) & 0xff);
                }
                (_, 0) => {
                    let addr = cpu.bc();
                    cpu.a = bus.read(addr);
                    cpu.wz = addr.wrapping_add(1);
                }
                (_, 1) => {
                    let addr = cpu.de();
                    cpu.a = bus.read(addr);
                    cpu.wz = addr.wrapping_add(1);
                }
                (_, 2) => {
                    let nn = cpu.imm16(bus);
                    let v = cpu.read16(bus, nn);
                    cpu.set_idx(idx, v);
                    cpu.wz = nn.wrapping_add(1);
                }
                (_, _) => {
                    let nn = cpu.imm16(bus);
                    cpu.a = bus.read(nn);
                    cpu.wz = nn.wrapping_add(1);
                }
            },
            3 => {
                bus.contend(cpu.ir(), 2);
                let v = rp(cpu, p, idx);
                let v = if q == 0 {
                    v.wrapping_add(1)
                } else {
                    v.wrapping_sub(1)
                };
                set_rp(cpu, p, idx, v);
            }
            4 | 5 => {
                let inc = z == 4;
                if y == 6 {
                    let addr = ea(cpu, bus, idx);
                    let v = bus.read(addr);
                    bus.contend(addr, 1);
                    let r = if inc { cpu.inc8(v) } else { cpu.dec8(v) };
                    bus.write(addr, r);
                } else {
                    let v = rd_r(cpu, y, idx);
                    let r = if inc { cpu.inc8(v) } else { cpu.dec8(v) };
                    wr_r(cpu, y, idx, r);
                }
            }
            6 => {
                if y == 6 {
                    // LD (IX+d),n has its own timing: d, n, then 2 idle cycles.
                    match idx {
                        Idx::Hl => {
                            let n = cpu.imm8(bus);
                            let addr = cpu.hl();
                            bus.write(addr, n);
                        }
                        _ => {
                            let d = cpu.imm8(bus) as i8;
                            let at = cpu.pc;
                            let n = cpu.imm8(bus);
                            bus.contend(at, 2);
                            let addr = cpu.idx_val(idx).wrapping_add(d as i16 as u16);
                            cpu.wz = addr;
                            bus.write(addr, n);
                        }
                    }
                } else {
                    let n = cpu.imm8(bus);
                    wr_r(cpu, y, idx, n);
                }
            }
            _ => match y {
                0 => cpu.rlca(),
                1 => cpu.rrca(),
                2 => cpu.rla(),
                3 => cpu.rra(),
                4 => cpu.daa(),
                5 => cpu.cpl(),
                6 => cpu.scf(),
                _ => cpu.ccf(),
            },
        },

        1 => {
            if y == 6 && z == 6 {
                cpu.halted = true;
                cpu.pc = cpu.pc.wrapping_sub(1);
            } else if z == 6 {
                // LD r,(HL) / LD r,(IX+d): the destination is always a real
                // register, never IXH/IXL.
                let addr = ea(cpu, bus, idx);
                let v = bus.read(addr);
                wr_r(cpu, y, Idx::Hl, v);
            } else if y == 6 {
                let addr = ea(cpu, bus, idx);
                let v = rd_r(cpu, z, Idx::Hl);
                bus.write(addr, v);
            } else {
                let v = rd_r(cpu, z, idx);
                wr_r(cpu, y, idx, v);
            }
        }

        2 => {
            let v = if z == 6 {
                let addr = ea(cpu, bus, idx);
                bus.read(addr)
            } else {
                rd_r(cpu, z, idx)
            };
            alu(cpu, y, v);
        }

        _ => match z {
            0 => {
                bus.contend(cpu.ir(), 1);
                if cond(cpu, y) {
                    let addr = cpu.pop16(bus);
                    cpu.pc = addr;
                    cpu.wz = addr;
                }
            }
            1 => {
                if q == 0 {
                    let v = cpu.pop16(bus);
                    set_rp2(cpu, p, idx, v);
                } else {
                    match p {
                        0 => {
                            let addr = cpu.pop16(bus);
                            cpu.pc = addr;
                            cpu.wz = addr;
                        }
                        1 => {
                            std::mem::swap(&mut cpu.b, &mut cpu.b_);
                            std::mem::swap(&mut cpu.c, &mut cpu.c_);
                            std::mem::swap(&mut cpu.d, &mut cpu.d_);
                            std::mem::swap(&mut cpu.e, &mut cpu.e_);
                            std::mem::swap(&mut cpu.h, &mut cpu.h_);
                            std::mem::swap(&mut cpu.l, &mut cpu.l_);
                        }
                        2 => cpu.pc = cpu.idx_val(idx),
                        _ => {
                            bus.contend(cpu.ir(), 2);
                            cpu.sp = cpu.idx_val(idx);
                        }
                    }
                }
            }
            2 => {
                let nn = cpu.imm16(bus);
                cpu.wz = nn;
                if cond(cpu, y) {
                    cpu.pc = nn;
                }
            }
            3 => match y {
                0 => {
                    let nn = cpu.imm16(bus);
                    cpu.pc = nn;
                    cpu.wz = nn;
                }
                1 => unreachable!("CB is handled in execute()"),
                2 => {
                    let n = cpu.imm8(bus);
                    let port = ((cpu.a as u16) << 8) | n as u16;
                    bus.io_write(port, cpu.a);
                    cpu.wz = ((cpu.a as u16) << 8) | (n.wrapping_add(1) as u16);
                }
                3 => {
                    let n = cpu.imm8(bus);
                    let port = ((cpu.a as u16) << 8) | n as u16;
                    cpu.a = bus.io_read(port);
                    cpu.wz = port.wrapping_add(1);
                }
                4 => {
                    let sp = cpu.sp;
                    let lo = bus.read(sp);
                    let hi = bus.read(sp.wrapping_add(1));
                    bus.contend(sp.wrapping_add(1), 1);
                    let v = cpu.idx_val(idx);
                    bus.write(sp.wrapping_add(1), (v >> 8) as u8);
                    bus.write(sp, v as u8);
                    bus.contend(sp, 2);
                    let nv = ((hi as u16) << 8) | lo as u16;
                    cpu.set_idx(idx, nv);
                    cpu.wz = nv;
                }
                5 => {
                    std::mem::swap(&mut cpu.d, &mut cpu.h);
                    std::mem::swap(&mut cpu.e, &mut cpu.l);
                }
                6 => {
                    cpu.iff1 = false;
                    cpu.iff2 = false;
                }
                _ => {
                    cpu.iff1 = true;
                    cpu.iff2 = true;
                    cpu.defer_int = true;
                }
            },
            4 => {
                let nn = cpu.imm16(bus);
                cpu.wz = nn;
                if cond(cpu, y) {
                    bus.contend(cpu.pc.wrapping_sub(1), 1);
                    let pc = cpu.pc;
                    cpu.push16(bus, pc);
                    cpu.pc = nn;
                }
            }
            5 => {
                if q == 0 {
                    bus.contend(cpu.ir(), 1);
                    let v = rp2(cpu, p, idx);
                    cpu.push16(bus, v);
                } else {
                    // p == 0 is CALL nn; p 1/2/3 are the DD/ED/FD prefixes,
                    // already routed by execute().
                    let nn = cpu.imm16(bus);
                    cpu.wz = nn;
                    bus.contend(cpu.pc.wrapping_sub(1), 1);
                    let pc = cpu.pc;
                    cpu.push16(bus, pc);
                    cpu.pc = nn;
                }
            }
            6 => {
                let n = cpu.imm8(bus);
                alu(cpu, y, n);
            }
            _ => {
                bus.contend(cpu.ir(), 1);
                let pc = cpu.pc;
                cpu.push16(bus, pc);
                cpu.pc = (y as u16) * 8;
                cpu.wz = cpu.pc;
            }
        },
    }
}

// ---------------------------------------------------------------------------
// CB prefix
// ---------------------------------------------------------------------------

fn cb(cpu: &mut Z80, bus: &mut impl Bus) {
    let op = cpu.fetch(bus);
    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;

    if z == 6 {
        let addr = cpu.hl();
        let v = bus.read(addr);
        match x {
            0 => {
                bus.contend(addr, 1);
                let r = rot(cpu, y, v);
                bus.write(addr, r);
            }
            1 => {
                bus.contend(addr, 1);
                let hi = (cpu.wz >> 8) as u8;
                cpu.bit_mem(y, v, hi);
            }
            2 => {
                bus.contend(addr, 1);
                bus.write(addr, v & !(1 << y));
            }
            _ => {
                bus.contend(addr, 1);
                bus.write(addr, v | (1 << y));
            }
        }
    } else {
        let v = rd_r(cpu, z, Idx::Hl);
        match x {
            0 => {
                let r = rot(cpu, y, v);
                wr_r(cpu, z, Idx::Hl, r);
            }
            1 => cpu.bit(y, v),
            2 => wr_r(cpu, z, Idx::Hl, v & !(1 << y)),
            _ => wr_r(cpu, z, Idx::Hl, v | (1 << y)),
        }
    }
}

/// `DD CB d op` / `FD CB d op`. The result is written back to `(IX+d)` and,
/// for `z != 6`, also copied into the named register.
fn ddcb(cpu: &mut Z80, bus: &mut impl Bus, idx: Idx) {
    let d = cpu.imm8(bus) as i8;
    let at = cpu.pc;
    let op = cpu.imm8(bus);
    bus.contend(at, 2);

    let addr = cpu.idx_val(idx).wrapping_add(d as i16 as u16);
    cpu.wz = addr;

    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;

    let v = bus.read(addr);
    bus.contend(addr, 1);

    match x {
        1 => {
            let hi = (cpu.wz >> 8) as u8;
            cpu.bit_mem(y, v, hi);
        }
        _ => {
            let r = match x {
                0 => rot(cpu, y, v),
                2 => v & !(1 << y),
                _ => v | (1 << y),
            };
            bus.write(addr, r);
            if z != 6 {
                wr_r(cpu, z, Idx::Hl, r);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ED prefix
// ---------------------------------------------------------------------------

fn ed(cpu: &mut Z80, bus: &mut impl Bus) {
    let op = cpu.fetch(bus);
    let x = op >> 6;
    let y = (op >> 3) & 7;
    let z = op & 7;
    let p = y >> 1;
    let q = y & 1;

    match x {
        1 => match z {
            0 => {
                let port = cpu.bc();
                let v = bus.io_read(port);
                cpu.wz = port.wrapping_add(1);
                cpu.in_flags(v);
                if y != 6 {
                    wr_r(cpu, y, Idx::Hl, v);
                }
            }
            1 => {
                let port = cpu.bc();
                let v = if y == 6 { 0 } else { rd_r(cpu, y, Idx::Hl) };
                bus.io_write(port, v);
                cpu.wz = port.wrapping_add(1);
            }
            2 => {
                bus.contend(cpu.ir(), 7);
                let hl = cpu.hl();
                cpu.wz = hl.wrapping_add(1);
                let v = rp(cpu, p, Idx::Hl);
                let r = if q == 0 {
                    cpu.sbc16(hl, v)
                } else {
                    cpu.adc16(hl, v)
                };
                cpu.set_hl(r);
            }
            3 => {
                let nn = cpu.imm16(bus);
                cpu.wz = nn.wrapping_add(1);
                if q == 0 {
                    let v = rp(cpu, p, Idx::Hl);
                    cpu.write16(bus, nn, v);
                } else {
                    let v = cpu.read16(bus, nn);
                    set_rp(cpu, p, Idx::Hl, v);
                }
            }
            4 => cpu.neg(),
            5 => {
                // RETN (y == 1 is RETI, identical apart from the bus signal).
                cpu.iff1 = cpu.iff2;
                let addr = cpu.pop16(bus);
                cpu.pc = addr;
                cpu.wz = addr;
            }
            6 => {
                cpu.im = match y {
                    0 | 1 | 4 | 5 => 0,
                    2 | 6 => 1,
                    _ => 2,
                };
            }
            _ => match y {
                0 => {
                    bus.contend(cpu.ir(), 1);
                    cpu.i = cpu.a;
                }
                1 => {
                    bus.contend(cpu.ir(), 1);
                    cpu.r = cpu.a & 0x7f;
                    cpu.r7 = cpu.a & 0x80;
                }
                2 => {
                    bus.contend(cpu.ir(), 1);
                    cpu.a = cpu.i;
                    let iff2 = cpu.iff2;
                    let a = cpu.a;
                    cpu.f = (cpu.f & CF)
                        | super::tables::SZ53[a as usize]
                        | if iff2 { PF } else { 0 };
                    cpu.touched_flags();
                }
                3 => {
                    bus.contend(cpu.ir(), 1);
                    cpu.a = cpu.r_full();
                    let iff2 = cpu.iff2;
                    let a = cpu.a;
                    cpu.f = (cpu.f & CF)
                        | super::tables::SZ53[a as usize]
                        | if iff2 { PF } else { 0 };
                    cpu.touched_flags();
                }
                4 => {
                    // RRD
                    let hl = cpu.hl();
                    let v = bus.read(hl);
                    bus.contend(hl, 4);
                    bus.write(hl, (cpu.a << 4) | (v >> 4));
                    cpu.a = (cpu.a & 0xf0) | (v & 0x0f);
                    cpu.wz = hl.wrapping_add(1);
                    cpu.rxd_flags();
                }
                5 => {
                    // RLD
                    let hl = cpu.hl();
                    let v = bus.read(hl);
                    bus.contend(hl, 4);
                    bus.write(hl, (v << 4) | (cpu.a & 0x0f));
                    cpu.a = (cpu.a & 0xf0) | (v >> 4);
                    cpu.wz = hl.wrapping_add(1);
                    cpu.rxd_flags();
                }
                _ => {}
            },
        },
        2 if z <= 3 && y >= 4 => block(cpu, bus, y, z),
        // Everything else in the ED page behaves as two NOPs.
        _ => {}
    }
}

/// LDI/LDD/LDIR/LDDR, CPI/…, INI/…, OUTI/… (`y` 4..7, `z` 0..3).
fn block(cpu: &mut Z80, bus: &mut impl Bus, y: u8, z: u8) {
    let inc = y & 1 == 0; // y = 4,6 -> increment; y = 5,7 -> decrement
    let repeat = y >= 6;
    let delta: u16 = if inc { 1 } else { 0xffff };

    match z {
        0 => {
            // LDI / LDD / LDIR / LDDR
            let hl = cpu.hl();
            let de = cpu.de();
            let v = bus.read(hl);
            bus.write(de, v);
            bus.contend(de, 2);
            cpu.set_hl(hl.wrapping_add(delta));
            cpu.set_de(de.wrapping_add(delta));
            let bc = cpu.bc().wrapping_sub(1);
            cpu.set_bc(bc);
            cpu.ldx_flags(v, bc != 0);
            if repeat && bc != 0 {
                bus.contend(de.wrapping_add(delta), 5);
                cpu.pc = cpu.pc.wrapping_sub(2);
                cpu.wz = cpu.pc.wrapping_add(1);
            }
        }
        1 => {
            // CPI / CPD / CPIR / CPDR
            let hl = cpu.hl();
            let v = bus.read(hl);
            bus.contend(hl, 5);
            cpu.set_hl(hl.wrapping_add(delta));
            let bc = cpu.bc().wrapping_sub(1);
            cpu.set_bc(bc);
            cpu.cpx_flags(v, bc != 0);
            let matched = cpu.a == v;
            if repeat && bc != 0 && !matched {
                bus.contend(hl, 5);
                cpu.pc = cpu.pc.wrapping_sub(2);
                cpu.wz = cpu.pc.wrapping_add(1);
            } else {
                cpu.wz = cpu.wz.wrapping_add(delta);
            }
        }
        2 => {
            // INI / IND / INIR / INDR
            bus.contend(cpu.ir(), 1);
            let port = cpu.bc();
            let v = bus.io_read(port);
            cpu.wz = port.wrapping_add(delta);
            let hl = cpu.hl();
            bus.write(hl, v);
            cpu.b = cpu.b.wrapping_sub(1);
            cpu.set_hl(hl.wrapping_add(delta));
            let c_adj = if inc {
                cpu.c.wrapping_add(1)
            } else {
                cpu.c.wrapping_sub(1)
            };
            cpu.inoutx_flags(v, c_adj);
            if repeat && cpu.b != 0 {
                bus.contend(hl, 5);
                cpu.pc = cpu.pc.wrapping_sub(2);
            }
        }
        _ => {
            // OUTI / OUTD / OTIR / OTDR
            bus.contend(cpu.ir(), 1);
            let hl = cpu.hl();
            let v = bus.read(hl);
            cpu.b = cpu.b.wrapping_sub(1);
            let port = cpu.bc();
            bus.io_write(port, v);
            cpu.set_hl(hl.wrapping_add(delta));
            cpu.wz = port.wrapping_add(delta);
            let l = cpu.l;
            cpu.inoutx_flags(v, l);
            if repeat && cpu.b != 0 {
                bus.contend(cpu.bc(), 5);
                cpu.pc = cpu.pc.wrapping_sub(2);
            }
        }
    }
}
