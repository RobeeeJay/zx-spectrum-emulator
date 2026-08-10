//! Snapshot loading: `.sna` (48K) and `.z80` (v1/v2/v3, 48K pages only).

use crate::machine::{Model, Spectrum};

/// Which machine a snapshot needs, without loading it. The caller can switch
/// models (and ROMs) before calling [`load`].
pub fn probe_model(path: &std::path::Path) -> Result<Model, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let kind = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    probe_model_bytes(&kind, &data)
}

/// The same, for a snapshot that is not a file of its own: the one inside an
/// RZX recording, for instance.
pub fn probe_model_bytes(kind: &str, data: &[u8]) -> Result<Model, String> {
    match kind {
        "sna" => Ok(if data.len() >= 131_103 {
            Model::Spectrum128
        } else {
            Model::Spectrum48
        }),
        "z80" => {
            if data.len() < 36 {
                return Ok(Model::Spectrum48);
            }
            if u16::from_le_bytes([data[6], data[7]]) != 0 {
                return Ok(Model::Spectrum48); // version 1 is always 48K
            }
            let ext_len = u16::from_le_bytes([data[30], data[31]]) as usize;
            let hw = data.get(34).copied().unwrap_or(0);
            Ok(model_from_hw(ext_len, hw))
        }
        other => Err(format!("unsupported snapshot type: .{other}")),
    }
}

/// The `.z80` hardware byte means different things in v2 and v3 headers.
fn model_from_hw(ext_len: usize, hw: u8) -> Model {
    if ext_len == 23 {
        // Version 2: 0/1 = 48K, 3/4 = 128K, 7 = +3.
        match hw {
            0..=2 => Model::Spectrum48,
            3..=4 => Model::Spectrum128,
            _ => Model::Plus3,
        }
    } else {
        // Version 3: 0-3 = 48K, 4-6 = 128K, 7/8 = +3.
        match hw {
            0..=3 => Model::Spectrum48,
            4..=6 => Model::Spectrum128,
            _ => Model::Plus3,
        }
    }
}

/// Write a flat 48K image ($4000-$FFFF) through the current paging.
fn write_48k_ram(spec: &mut Spectrum, data: &[u8]) {
    for (i, b) in data.iter().take(0xc000).enumerate() {
        spec.bus.poke(0x4000u16.wrapping_add(i as u16), *b);
    }
}

fn write_bank(spec: &mut Spectrum, bank: usize, data: &[u8]) {
    let base = (bank & 7) * 0x4000;
    let n = data.len().min(0x4000);
    spec.bus.ram[base..base + n].copy_from_slice(&data[..n]);
}

pub fn load(spec: &mut Spectrum, path: &std::path::Path) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "sna" => load_sna(spec, &data),
        "z80" => load_z80(spec, &data),
        other => Err(format!("unsupported snapshot type: .{other}")),
    }
}

pub fn load_sna(spec: &mut Spectrum, d: &[u8]) -> Result<(), String> {
    if d.len() < 49179 {
        return Err(format!("truncated .sna ({} bytes)", d.len()));
    }
    let is128 = d.len() >= 131_103;
    if is128 && spec.bus.model != Model::Spectrum128 {
        return Err("128K snapshot: switch the machine to 128K first".into());
    }
    let c = &mut spec.cpu;
    c.i = d[0];
    c.l_ = d[1];
    c.h_ = d[2];
    c.e_ = d[3];
    c.d_ = d[4];
    c.c_ = d[5];
    c.b_ = d[6];
    c.f_ = d[7];
    c.a_ = d[8];
    c.l = d[9];
    c.h = d[10];
    c.e = d[11];
    c.d = d[12];
    c.c = d[13];
    c.b = d[14];
    c.iy = u16::from_le_bytes([d[15], d[16]]);
    c.ix = u16::from_le_bytes([d[17], d[18]]);
    c.iff2 = d[19] & 0x04 != 0;
    c.iff1 = c.iff2;
    c.r = d[20] & 0x7f;
    c.r7 = d[20] & 0x80;
    c.f = d[21];
    c.a = d[22];
    c.sp = u16::from_le_bytes([d[23], d[24]]);
    c.im = d[25] & 3;
    spec.bus.border = d[26] & 7;
    spec.bus.border_start = spec.bus.border;

    if is128 {
        // The trailing header says which bank was at $C000, so page it in
        // before unpacking the flat part of the image.
        let tail = 27 + 0xc000;
        let pc = u16::from_le_bytes([d[tail], d[tail + 1]]);
        let port = d[tail + 2];
        spec.bus.paging_locked = false;
        spec.bus.write_paging(port);
        write_48k_ram(spec, &d[27..27 + 0xc000]);
        spec.cpu.pc = pc;

        // Then the remaining banks, in ascending order, skipping the three
        // that were already covered.
        let paged = (port & 0x07) as usize;
        let mut off = tail + 4;
        for bank in 0..8usize {
            if bank == 5 || bank == 2 || bank == paged {
                continue;
            }
            if off + 0x4000 > d.len() {
                break;
            }
            write_bank(spec, bank, &d[off..off + 0x4000]);
            off += 0x4000;
        }
    } else {
        write_48k_ram(spec, &d[27..27 + 0xc000]);
        // A 48K .sna resumes by executing a RET, so PC comes off the stack.
        let sp = spec.cpu.sp;
        let lo = spec.bus.mem(sp) as u16;
        let hi = spec.bus.mem(sp.wrapping_add(1)) as u16;
        spec.cpu.pc = (hi << 8) | lo;
        spec.cpu.sp = sp.wrapping_add(2);
    }
    spec.bus.tracker.reset();
    Ok(())
}

pub fn load_z80(spec: &mut Spectrum, d: &[u8]) -> Result<(), String> {
    if d.len() < 30 {
        return Err("truncated .z80".into());
    }
    let c = &mut spec.cpu;
    c.a = d[0];
    c.f = d[1];
    c.c = d[2];
    c.b = d[3];
    c.l = d[4];
    c.h = d[5];
    let pc_v1 = u16::from_le_bytes([d[6], d[7]]);
    c.sp = u16::from_le_bytes([d[8], d[9]]);
    c.i = d[10];
    c.r = d[11] & 0x7f;
    let mut byte12 = d[12];
    if byte12 == 0xff {
        byte12 = 1;
    }
    c.r7 = (byte12 & 0x01) << 7;
    spec.bus.border = (byte12 >> 1) & 7;
    spec.bus.border_start = spec.bus.border;
    let compressed_v1 = byte12 & 0x20 != 0;
    c.e = d[13];
    c.d = d[14];
    c.c_ = d[15];
    c.b_ = d[16];
    c.e_ = d[17];
    c.d_ = d[18];
    c.l_ = d[19];
    c.h_ = d[20];
    c.a_ = d[21];
    c.f_ = d[22];
    c.iy = u16::from_le_bytes([d[23], d[24]]);
    c.ix = u16::from_le_bytes([d[25], d[26]]);
    c.iff1 = d[27] != 0;
    c.iff2 = d[28] != 0;
    c.im = d[29] & 3;

    if pc_v1 != 0 {
        // Version 1: a single 48K block from offset 30.
        spec.cpu.pc = pc_v1;
        let body = &d[30..];
        let ram = if compressed_v1 {
            decompress(body, 0xc000)
        } else {
            body.to_vec()
        };
        write_48k_ram(spec, &ram);
        spec.bus.tracker.reset();
        return Ok(());
    }

    // Version 2/3: extended header then per-page blocks.
    if d.len() < 34 {
        return Err("truncated .z80 v2 header".into());
    }
    let ext_len = u16::from_le_bytes([d[30], d[31]]) as usize;
    spec.cpu.pc = u16::from_le_bytes([d[32], d[33]]);
    let hw = d.get(34).copied().unwrap_or(0);
    let want = model_from_hw(ext_len, hw);
    let is128 = want != Model::Spectrum48;
    if is128 && !spec.bus.model.has_paging() {
        return Err(format!(
            "{} snapshot: switch the machine to {} first",
            want.name(),
            want.name()
        ));
    }
    if want.has_plus3_paging() && !spec.bus.model.has_plus3_paging() {
        return Err("+3 snapshot: switch the machine to +2A or +3 first".into());
    }
    if is128 {
        spec.bus.paging_locked = false;
        // Byte 35 is the last value written to $7FFD, and on a +3 byte 86 is
        // the last value written to $1FFD.
        spec.bus.write_paging(d.get(35).copied().unwrap_or(0));
        if spec.bus.model.has_plus3_paging() {
            if let Some(v) = d.get(86).copied() {
                spec.bus.write_paging_1ffd(v);
            }
        }
    }

    let mut off = 32 + ext_len;
    while off + 3 <= d.len() {
        let blk_len = u16::from_le_bytes([d[off], d[off + 1]]) as usize;
        let page = d[off + 2];
        off += 3;
        let (raw, consumed) = if blk_len == 0xffff {
            (d[off..(off + 0x4000).min(d.len())].to_vec(), 0x4000)
        } else {
            let end = (off + blk_len).min(d.len());
            (decompress(&d[off..end], 0x4000), blk_len)
        };
        off += consumed;
        // On a 128K, pages 3..10 are RAM banks 0..7. On a 48K the three
        // pages map to the fixed banks at $8000, $C000 and $4000.
        let bank = if is128 {
            (3..=10).contains(&page).then(|| page as usize - 3)
        } else {
            match page {
                4 => Some(2),
                5 => Some(0),
                8 => Some(5),
                _ => None,
            }
        };
        if let Some(bank) = bank {
            write_bank(spec, bank, &raw);
        }
    }
    spec.bus.tracker.reset();
    Ok(())
}

/// The .z80 run-length scheme: `ED ED count byte`, terminated by `00 ED ED 00`.
fn decompress(src: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(max);
    let mut i = 0;
    while i < src.len() && out.len() < max {
        if i + 3 < src.len() && src[i] == 0xed && src[i + 1] == 0xed {
            let count = src[i + 2] as usize;
            let value = src[i + 3];
            if count == 0 {
                break;
            }
            for _ in 0..count {
                if out.len() >= max {
                    break;
                }
                out.push(value);
            }
            i += 4;
        } else {
            out.push(src[i]);
            i += 1;
        }
    }
    out.resize(max, 0);
    out
}
