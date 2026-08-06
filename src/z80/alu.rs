//! Arithmetic, logic and rotate operations, including the undocumented
//! F3/F5 flag results.

use super::flags::*;
use super::tables::{PARITY, SZ53, SZ53P};
use super::Z80;

impl Z80 {
    pub fn add8(&mut self, v: u8) {
        let a = self.a as u16;
        let r = a + v as u16;
        let res = r as u8;
        let half = ((self.a & 0x0f) + (v & 0x0f)) & 0x10;
        let ovf = (!(self.a ^ v) & (self.a ^ res) & 0x80) >> 5;
        self.f = SZ53[res as usize] | half | ovf | ((r >> 8) as u8 & CF);
        self.a = res;
        self.touched_flags();
    }

    pub fn adc8(&mut self, v: u8) {
        let c = (self.f & CF) as u16;
        let a = self.a as u16;
        let r = a + v as u16 + c;
        let res = r as u8;
        let half = ((self.a & 0x0f) + (v & 0x0f) + c as u8) & 0x10;
        let ovf = (!(self.a ^ v) & (self.a ^ res) & 0x80) >> 5;
        self.f = SZ53[res as usize] | half | ovf | ((r >> 8) as u8 & CF);
        self.a = res;
        self.touched_flags();
    }

    pub fn sub8(&mut self, v: u8) {
        let r = (self.a as u16).wrapping_sub(v as u16);
        let res = r as u8;
        let half = ((self.a & 0x0f).wrapping_sub(v & 0x0f)) & 0x10;
        let ovf = ((self.a ^ v) & (self.a ^ res) & 0x80) >> 5;
        self.f = SZ53[res as usize] | half | ovf | NF | ((r >> 8) as u8 & CF);
        self.a = res;
        self.touched_flags();
    }

    pub fn sbc8(&mut self, v: u8) {
        let c = (self.f & CF) as u16;
        let r = (self.a as u16).wrapping_sub(v as u16).wrapping_sub(c);
        let res = r as u8;
        let half = ((self.a & 0x0f).wrapping_sub(v & 0x0f).wrapping_sub(c as u8)) & 0x10;
        let ovf = ((self.a ^ v) & (self.a ^ res) & 0x80) >> 5;
        self.f = SZ53[res as usize] | half | ovf | NF | ((r >> 8) as u8 & CF);
        self.a = res;
        self.touched_flags();
    }

    pub fn and8(&mut self, v: u8) {
        self.a &= v;
        self.f = SZ53P[self.a as usize] | HF;
        self.touched_flags();
    }

    pub fn xor8(&mut self, v: u8) {
        self.a ^= v;
        self.f = SZ53P[self.a as usize];
        self.touched_flags();
    }

    pub fn or8(&mut self, v: u8) {
        self.a |= v;
        self.f = SZ53P[self.a as usize];
        self.touched_flags();
    }

    /// CP differs from SUB: F3/F5 come from the operand, not the result.
    pub fn cp8(&mut self, v: u8) {
        let r = (self.a as u16).wrapping_sub(v as u16);
        let res = r as u8;
        let half = ((self.a & 0x0f).wrapping_sub(v & 0x0f)) & 0x10;
        let ovf = ((self.a ^ v) & (self.a ^ res) & 0x80) >> 5;
        self.f = (SZ53[res as usize] & (SF | ZF))
            | (v & (F3 | F5))
            | half
            | ovf
            | NF
            | ((r >> 8) as u8 & CF);
        self.touched_flags();
    }

    pub fn inc8(&mut self, v: u8) -> u8 {
        let res = v.wrapping_add(1);
        self.f = (self.f & CF)
            | SZ53[res as usize]
            | if res & 0x0f == 0 { HF } else { 0 }
            | if res == 0x80 { PF } else { 0 };
        self.touched_flags();
        res
    }

    pub fn dec8(&mut self, v: u8) -> u8 {
        let res = v.wrapping_sub(1);
        self.f = (self.f & CF)
            | SZ53[res as usize]
            | if v & 0x0f == 0 { HF } else { 0 }
            | if res == 0x7f { PF } else { 0 }
            | NF;
        self.touched_flags();
        res
    }

    pub fn add16(&mut self, a: u16, b: u16) -> u16 {
        let r = a as u32 + b as u32;
        let res = r as u16;
        self.f = (self.f & (SF | ZF | PF))
            | (((a ^ res ^ b) >> 8) as u8 & HF)
            | ((r >> 16) as u8 & CF)
            | ((res >> 8) as u8 & (F3 | F5));
        self.touched_flags();
        res
    }

    pub fn adc16(&mut self, a: u16, b: u16) -> u16 {
        let c = (self.f & CF) as u32;
        let r = a as u32 + b as u32 + c;
        let res = r as u16;
        let ovf = ((!(a ^ b) & (a ^ res) & 0x8000) >> 13) as u8;
        self.f = SZ53[(res >> 8) as usize] & (SF | F3 | F5)
            | if res == 0 { ZF } else { 0 }
            | (((a ^ res ^ b) >> 8) as u8 & HF)
            | ovf
            | ((r >> 16) as u8 & CF);
        self.touched_flags();
        res
    }

    pub fn sbc16(&mut self, a: u16, b: u16) -> u16 {
        let c = (self.f & CF) as u32;
        let r = (a as u32).wrapping_sub(b as u32).wrapping_sub(c);
        let res = r as u16;
        let ovf = (((a ^ b) & (a ^ res) & 0x8000) >> 13) as u8;
        self.f = SZ53[(res >> 8) as usize] & (SF | F3 | F5)
            | if res == 0 { ZF } else { 0 }
            | (((a ^ res ^ b) >> 8) as u8 & HF)
            | ovf
            | NF
            | ((r >> 16) as u8 & CF);
        self.touched_flags();
        res
    }

    pub fn neg(&mut self) {
        let v = self.a;
        self.a = 0;
        self.sub8(v);
    }

    pub fn daa(&mut self) {
        let mut correction = 0u8;
        let mut carry = self.f & CF;
        if self.flag(HF) || (self.a & 0x0f) > 9 {
            correction |= 0x06;
        }
        if carry != 0 || self.a > 0x99 {
            correction |= 0x60;
            carry = CF;
        }
        let old = self.a;
        if self.flag(NF) {
            self.a = self.a.wrapping_sub(correction);
        } else {
            self.a = self.a.wrapping_add(correction);
        }
        let half = (old ^ self.a) & HF;
        self.f = SZ53P[self.a as usize] | half | carry | (self.f & NF);
        self.touched_flags();
    }

    pub fn cpl(&mut self) {
        self.a = !self.a;
        self.f = (self.f & (SF | ZF | PF | CF)) | HF | NF | (self.a & (F3 | F5));
        self.touched_flags();
    }

    pub fn scf(&mut self) {
        let yx = (self.q_prev() ^ self.f) | self.a;
        self.f = (self.f & (SF | ZF | PF)) | CF | (yx & (F3 | F5));
        self.touched_flags();
    }

    pub fn ccf(&mut self) {
        let yx = (self.q_prev() ^ self.f) | self.a;
        let c = self.f & CF;
        self.f =
            (self.f & (SF | ZF | PF)) | (if c != 0 { HF } else { 0 }) | (c ^ CF) | (yx & (F3 | F5));
        self.touched_flags();
    }

    // ---- rotates on A (fast forms, only carry + F3/F5 change) --------------

    pub fn rlca(&mut self) {
        self.a = self.a.rotate_left(1);
        self.f = (self.f & (SF | ZF | PF)) | (self.a & (F3 | F5 | CF));
        self.touched_flags();
    }

    pub fn rrca(&mut self) {
        let c = self.a & 1;
        self.a = self.a.rotate_right(1);
        self.f = (self.f & (SF | ZF | PF)) | (self.a & (F3 | F5)) | c;
        self.touched_flags();
    }

    pub fn rla(&mut self) {
        let c = self.f & CF;
        let newc = self.a >> 7;
        self.a = (self.a << 1) | c;
        self.f = (self.f & (SF | ZF | PF)) | (self.a & (F3 | F5)) | newc;
        self.touched_flags();
    }

    pub fn rra(&mut self) {
        let c = (self.f & CF) << 7;
        let newc = self.a & 1;
        self.a = (self.a >> 1) | c;
        self.f = (self.f & (SF | ZF | PF)) | (self.a & (F3 | F5)) | newc;
        self.touched_flags();
    }

    // ---- CB-prefixed shifts and rotates ------------------------------------

    pub fn rlc(&mut self, v: u8) -> u8 {
        let res = v.rotate_left(1);
        self.f = SZ53P[res as usize] | (v >> 7);
        self.touched_flags();
        res
    }
    pub fn rrc(&mut self, v: u8) -> u8 {
        let res = v.rotate_right(1);
        self.f = SZ53P[res as usize] | (v & CF);
        self.touched_flags();
        res
    }
    pub fn rl(&mut self, v: u8) -> u8 {
        let res = (v << 1) | (self.f & CF);
        self.f = SZ53P[res as usize] | (v >> 7);
        self.touched_flags();
        res
    }
    pub fn rr(&mut self, v: u8) -> u8 {
        let res = (v >> 1) | ((self.f & CF) << 7);
        self.f = SZ53P[res as usize] | (v & CF);
        self.touched_flags();
        res
    }
    pub fn sla(&mut self, v: u8) -> u8 {
        let res = v << 1;
        self.f = SZ53P[res as usize] | (v >> 7);
        self.touched_flags();
        res
    }
    pub fn sra(&mut self, v: u8) -> u8 {
        let res = (v >> 1) | (v & 0x80);
        self.f = SZ53P[res as usize] | (v & CF);
        self.touched_flags();
        res
    }
    /// Undocumented: shift left, bit 0 set.
    pub fn sll(&mut self, v: u8) -> u8 {
        let res = (v << 1) | 1;
        self.f = SZ53P[res as usize] | (v >> 7);
        self.touched_flags();
        res
    }
    pub fn srl(&mut self, v: u8) -> u8 {
        let res = v >> 1;
        self.f = SZ53P[res as usize] | (v & CF);
        self.touched_flags();
        res
    }

    /// `BIT n,r`: F3/F5 come from the tested register.
    pub fn bit(&mut self, n: u8, v: u8) {
        let masked = v & (1 << n);
        self.f = (self.f & CF)
            | HF
            | (if masked == 0 { ZF | PF } else { 0 })
            | (masked & SF)
            | (v & (F3 | F5));
        self.touched_flags();
    }

    /// `BIT n,(HL)` and `BIT n,(IX+d)`: F3/F5 come from MEMPTR's high byte.
    pub fn bit_mem(&mut self, n: u8, v: u8, memptr_hi: u8) {
        let masked = v & (1 << n);
        self.f = (self.f & CF)
            | HF
            | (if masked == 0 { ZF | PF } else { 0 })
            | (masked & SF)
            | (memptr_hi & (F3 | F5));
        self.touched_flags();
    }

    /// Flags shared by RLD and RRD.
    pub fn rxd_flags(&mut self) {
        self.f = (self.f & CF) | SZ53P[self.a as usize];
        self.touched_flags();
    }

    /// Flags for IN r,(C).
    pub fn in_flags(&mut self, v: u8) {
        self.f = (self.f & CF) | SZ53P[v as usize];
        self.touched_flags();
    }

    /// Flags shared by all four block-copy instructions.
    pub fn ldx_flags(&mut self, transferred: u8, bc_nonzero: bool) {
        let n = self.a.wrapping_add(transferred);
        self.f = (self.f & (SF | ZF | CF))
            | (if bc_nonzero { PF } else { 0 })
            | (n & F3)
            | (if n & 0x02 != 0 { F5 } else { 0 });
        self.touched_flags();
    }

    /// Flags shared by all four block-compare instructions.
    pub fn cpx_flags(&mut self, value: u8, bc_nonzero: bool) {
        let res = self.a.wrapping_sub(value);
        let half = ((self.a & 0x0f).wrapping_sub(value & 0x0f)) & 0x10;
        let n = res.wrapping_sub(if half != 0 { 1 } else { 0 });
        self.f = (self.f & CF)
            | NF
            | half
            | (SZ53[res as usize] & (SF | ZF))
            | (if bc_nonzero { PF } else { 0 })
            | (n & F3)
            | (if n & 0x02 != 0 { F5 } else { 0 });
        self.touched_flags();
    }

    /// Flags shared by all eight block-I/O instructions.
    pub fn inoutx_flags(&mut self, value: u8, c_adj: u8) {
        let k = value as u16 + c_adj as u16;
        self.f = SZ53[self.b as usize]
            | (if value & 0x80 != 0 { NF } else { 0 })
            | (if k > 0xff { HF | CF } else { 0 })
            | PARITY[((k as u8 & 0x07) ^ self.b) as usize];
        self.touched_flags();
    }
}
