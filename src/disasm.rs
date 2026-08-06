//! Z80 disassembler, including the undocumented IXH/IXL and SLL opcodes.

const R: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const RP: [&str; 4] = ["BC", "DE", "HL", "SP"];
const RP2: [&str; 4] = ["BC", "DE", "HL", "AF"];
const CC: [&str; 8] = ["NZ", "Z", "NC", "C", "PO", "PE", "P", "M"];
const ALU: [&str; 8] = [
    "ADD A,", "ADC A,", "SUB ", "SBC A,", "AND ", "XOR ", "OR ", "CP ",
];
const ROT: [&str; 8] = ["RLC", "RRC", "RL", "RR", "SLA", "SRA", "SLL", "SRL"];

pub struct Insn {
    pub text: String,
    pub len: u8,
    pub bytes: Vec<u8>,
}

fn hex8(v: u8) -> String {
    format!("${:02X}", v)
}
fn hex16(v: u16) -> String {
    format!("${:04X}", v)
}

/// Disassemble one instruction at `addr`. `peek` supplies memory.
pub fn disasm<F: Fn(u16) -> u8>(peek: &F, addr: u16) -> Insn {
    let mut p = Cursor {
        peek,
        addr,
        len: 0,
        bytes: Vec::new(),
    };
    let text = p.instruction();
    Insn {
        text,
        len: p.len,
        bytes: p.bytes,
    }
}

struct Cursor<'a, F: Fn(u16) -> u8> {
    peek: &'a F,
    addr: u16,
    len: u8,
    bytes: Vec<u8>,
}

impl<F: Fn(u16) -> u8> Cursor<'_, F> {
    fn next(&mut self) -> u8 {
        let v = (self.peek)(self.addr.wrapping_add(self.len as u16));
        self.len = self.len.saturating_add(1);
        self.bytes.push(v);
        v
    }
    fn next16(&mut self) -> u16 {
        let lo = self.next() as u16;
        let hi = self.next() as u16;
        (hi << 8) | lo
    }
    fn disp(&mut self) -> String {
        let d = self.next() as i8;
        if d < 0 {
            format!("-${:02X}", -(d as i16))
        } else {
            format!("+${:02X}", d)
        }
    }

    fn instruction(&mut self) -> String {
        let op = self.next();
        match op {
            0xdd => self.indexed("IX"),
            0xfd => self.indexed("IY"),
            0xed => self.ed(),
            0xcb => self.cb(),
            _ => self.base(op, "HL", None),
        }
    }

    fn indexed(&mut self, ix: &str) -> String {
        let op = self.next();
        match op {
            0xdd => self.indexed("IX"),
            0xfd => self.indexed("IY"),
            0xed => self.ed(),
            0xcb => {
                let d = self.disp();
                let op = self.next();
                let y = (op >> 3) & 7;
                let z = op & 7;
                let target = format!("({}{})", ix, d);
                match op >> 6 {
                    0 => {
                        if z == 6 {
                            format!("{} {}", ROT[y as usize], target)
                        } else {
                            format!("{} {},{}", ROT[y as usize], target, R[z as usize])
                        }
                    }
                    1 => format!("BIT {},{}", y, target),
                    2 => {
                        if z == 6 {
                            format!("RES {},{}", y, target)
                        } else {
                            format!("RES {},{},{}", y, target, R[z as usize])
                        }
                    }
                    _ => {
                        if z == 6 {
                            format!("SET {},{}", y, target)
                        } else {
                            format!("SET {},{},{}", y, target, R[z as usize])
                        }
                    }
                }
            }
            _ => self.base(op, ix, Some(ix)),
        }
    }

    /// `hl_name` replaces HL; when `index` is set, `(HL)` becomes `(IX+d)` and
    /// H/L become IXH/IXL.
    fn base(&mut self, op: u8, hl_name: &str, index: Option<&str>) -> String {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let p = (y >> 1) as usize;
        let q = y & 1;

        let reg = |c: u8, this: &mut Self| -> String {
            match (c, index) {
                (6, Some(ix)) => {
                    let d = this.disp();
                    format!("({}{})", ix, d)
                }
                (4, Some(ix)) => format!("{}H", ix),
                (5, Some(ix)) => format!("{}L", ix),
                _ => R[c as usize].to_string(),
            }
        };
        let rp = |i: usize| -> String {
            if i == 2 {
                hl_name.to_string()
            } else {
                RP[i].to_string()
            }
        };
        let rp2 = |i: usize| -> String {
            if i == 2 {
                hl_name.to_string()
            } else {
                RP2[i].to_string()
            }
        };

        match x {
            0 => match z {
                0 => match y {
                    0 => "NOP".into(),
                    1 => "EX AF,AF'".into(),
                    2 => {
                        let d = self.next() as i8;
                        let t = self
                            .addr
                            .wrapping_add(self.len as u16)
                            .wrapping_add(d as i16 as u16);
                        format!("DJNZ {}", hex16(t))
                    }
                    3 => {
                        let d = self.next() as i8;
                        let t = self
                            .addr
                            .wrapping_add(self.len as u16)
                            .wrapping_add(d as i16 as u16);
                        format!("JR {}", hex16(t))
                    }
                    _ => {
                        let d = self.next() as i8;
                        let t = self
                            .addr
                            .wrapping_add(self.len as u16)
                            .wrapping_add(d as i16 as u16);
                        format!("JR {},{}", CC[(y - 4) as usize], hex16(t))
                    }
                },
                1 => {
                    if q == 0 {
                        let nn = self.next16();
                        format!("LD {},{}", rp(p), hex16(nn))
                    } else {
                        format!("ADD {},{}", hl_name, rp(p))
                    }
                }
                2 => match (q, p) {
                    (0, 0) => "LD (BC),A".into(),
                    (0, 1) => "LD (DE),A".into(),
                    (0, 2) => {
                        let nn = self.next16();
                        format!("LD ({}),{}", hex16(nn), hl_name)
                    }
                    (0, _) => {
                        let nn = self.next16();
                        format!("LD ({}),A", hex16(nn))
                    }
                    (_, 0) => "LD A,(BC)".into(),
                    (_, 1) => "LD A,(DE)".into(),
                    (_, 2) => {
                        let nn = self.next16();
                        format!("LD {},({})", hl_name, hex16(nn))
                    }
                    (_, _) => {
                        let nn = self.next16();
                        format!("LD A,({})", hex16(nn))
                    }
                },
                3 => {
                    if q == 0 {
                        format!("INC {}", rp(p))
                    } else {
                        format!("DEC {}", rp(p))
                    }
                }
                4 => format!("INC {}", reg(y, self)),
                5 => format!("DEC {}", reg(y, self)),
                6 => {
                    let target = reg(y, self);
                    let n = self.next();
                    format!("LD {},{}", target, hex8(n))
                }
                _ => ["RLCA", "RRCA", "RLA", "RRA", "DAA", "CPL", "SCF", "CCF"][y as usize].into(),
            },
            1 => {
                if y == 6 && z == 6 {
                    "HALT".into()
                } else if z == 6 {
                    let src = reg(z, self);
                    format!("LD {},{}", R[y as usize], src)
                } else if y == 6 {
                    let dst = reg(y, self);
                    format!("LD {},{}", dst, R[z as usize])
                } else {
                    let src = reg(z, self);
                    format!("LD {},{}", reg(y, self), src)
                }
            }
            2 => format!("{}{}", ALU[y as usize], reg(z, self)),
            _ => match z {
                0 => format!("RET {}", CC[y as usize]),
                1 => {
                    if q == 0 {
                        format!("POP {}", rp2(p))
                    } else {
                        match p {
                            0 => "RET".into(),
                            1 => "EXX".into(),
                            2 => format!("JP ({})", hl_name),
                            _ => format!("LD SP,{}", hl_name),
                        }
                    }
                }
                2 => {
                    let nn = self.next16();
                    format!("JP {},{}", CC[y as usize], hex16(nn))
                }
                3 => match y {
                    0 => {
                        let nn = self.next16();
                        format!("JP {}", hex16(nn))
                    }
                    1 => unreachable!(),
                    2 => {
                        let n = self.next();
                        format!("OUT ({}),A", hex8(n))
                    }
                    3 => {
                        let n = self.next();
                        format!("IN A,({})", hex8(n))
                    }
                    4 => format!("EX (SP),{}", hl_name),
                    5 => "EX DE,HL".into(),
                    6 => "DI".into(),
                    _ => "EI".into(),
                },
                4 => {
                    let nn = self.next16();
                    format!("CALL {},{}", CC[y as usize], hex16(nn))
                }
                5 => {
                    if q == 0 {
                        format!("PUSH {}", rp2(p))
                    } else {
                        let nn = self.next16();
                        format!("CALL {}", hex16(nn))
                    }
                }
                6 => {
                    let n = self.next();
                    format!("{}{}", ALU[y as usize], hex8(n))
                }
                _ => format!("RST ${:02X}", y * 8),
            },
        }
    }

    fn cb(&mut self) -> String {
        let op = self.next();
        let y = (op >> 3) & 7;
        let z = (op & 7) as usize;
        match op >> 6 {
            0 => format!("{} {}", ROT[y as usize], R[z]),
            1 => format!("BIT {},{}", y, R[z]),
            2 => format!("RES {},{}", y, R[z]),
            _ => format!("SET {},{}", y, R[z]),
        }
    }

    fn ed(&mut self) -> String {
        let op = self.next();
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let p = (y >> 1) as usize;
        let q = y & 1;

        match x {
            1 => match z {
                0 => {
                    if y == 6 {
                        "IN (C)".into()
                    } else {
                        format!("IN {},(C)", R[y as usize])
                    }
                }
                1 => {
                    if y == 6 {
                        "OUT (C),0".into()
                    } else {
                        format!("OUT (C),{}", R[y as usize])
                    }
                }
                2 => {
                    if q == 0 {
                        format!("SBC HL,{}", RP[p])
                    } else {
                        format!("ADC HL,{}", RP[p])
                    }
                }
                3 => {
                    let nn = self.next16();
                    if q == 0 {
                        format!("LD ({}),{}", hex16(nn), RP[p])
                    } else {
                        format!("LD {},({})", RP[p], hex16(nn))
                    }
                }
                4 => "NEG".into(),
                5 => {
                    if y == 1 {
                        "RETI".into()
                    } else {
                        "RETN".into()
                    }
                }
                6 => format!("IM {}", [0, 0, 1, 2, 0, 0, 1, 2][y as usize]),
                _ => [
                    "LD I,A", "LD R,A", "LD A,I", "LD A,R", "RRD", "RLD", "NOP", "NOP",
                ][y as usize]
                    .into(),
            },
            2 if z <= 3 && y >= 4 => {
                const NAMES: [[&str; 4]; 4] = [
                    ["LDI", "CPI", "INI", "OUTI"],
                    ["LDD", "CPD", "IND", "OUTD"],
                    ["LDIR", "CPIR", "INIR", "OTIR"],
                    ["LDDR", "CPDR", "INDR", "OTDR"],
                ];
                NAMES[(y - 4) as usize][z as usize].into()
            }
            _ => format!("DB $ED,{}", hex8(op)),
        }
    }
}

/// Find an address at most `back` bytes before `pc` from which disassembling
/// forward lands exactly on `pc`, so the listing above the current
/// instruction is aligned with real opcode boundaries.
pub fn sync_start<F: Fn(u16) -> u8>(peek: &F, pc: u16, back: u16) -> u16 {
    for delta in (1..=back).rev() {
        let start = pc.wrapping_sub(delta);
        let mut off: u32 = 0;
        while off < delta as u32 {
            let insn = disasm(peek, start.wrapping_add(off as u16));
            off += insn.len.max(1) as u32;
        }
        if off == delta as u32 {
            return start;
        }
    }
    pc
}
