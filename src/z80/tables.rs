//! Const lookup tables for flag generation.

use super::flags::*;

const fn parity(mut v: u8) -> bool {
    let mut bits = 0u8;
    let mut i = 0;
    while i < 8 {
        bits += v & 1;
        v >>= 1;
        i += 1;
    }
    bits & 1 == 0
}

/// S, Z, F5, F3 derived from the value itself.
pub const SZ53: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let v = i as u8;
        let mut f = v & (SF | F5 | F3);
        if v == 0 {
            f |= ZF;
        }
        t[i] = f;
        i += 1;
    }
    t
};

/// SZ53 plus the parity/overflow bit set from parity.
pub const SZ53P: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut f = SZ53[i];
        if parity(i as u8) {
            f |= PF;
        }
        t[i] = f;
        i += 1;
    }
    t
};

/// PF only, from parity of the value.
pub const PARITY: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        t[i] = if parity(i as u8) { PF } else { 0 };
        i += 1;
    }
    t
};
