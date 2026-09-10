//! `.scr`: a screen on its own, which is the display file and its attributes
//! copied straight out of the machine.
//!
//! 6,144 bytes of pixels in the display file's own order — thirds, then the
//! line within the character row, then the row — followed by 768 bytes of
//! attributes, one per character cell. There is no header and no compression:
//! a `.scr` is what `$4000` to `$5AFF` held, and nothing else. That is why
//! loading one is a poke rather than a snapshot, and why the machine goes on
//! running underneath it.
//!
//! A 128K can have its screen in bank 7 rather than bank 5, and what a `.scr`
//! holds is whichever one the ULA was showing.

use crate::machine::Spectrum;

/// How long one is: 6,144 bytes of pixels and 768 of attributes.
pub const LEN: usize = 6912;

/// Whether a file is one, which for a format with no header is only its size.
pub fn is_scr(data: &[u8]) -> bool {
    data.len() == LEN
}

/// The screen the ULA is showing, as a `.scr`.
pub fn save(spec: &Spectrum) -> Vec<u8> {
    let bank = spec.bus.screen_bank();
    let mut out = Vec::with_capacity(LEN);
    for i in 0..LEN {
        out.push(spec.bus.ram[bank * 0x4000 + i]);
    }
    out
}

/// Put one on the screen the machine is showing.
///
/// This writes into the machine's memory, so a program that is drawing will
/// draw over it — which is the honest thing for it to do, since that is what
/// poking a screen into a running machine does.
pub fn load(spec: &mut Spectrum, data: &[u8]) -> Result<(), String> {
    if data.len() != LEN {
        return Err(format!(
            "a .scr is {LEN} bytes — 6,144 of pixels and 768 of attributes — and this is {}",
            data.len()
        ));
    }
    let bank = spec.bus.screen_bank();
    spec.bus.ram[bank * 0x4000..bank * 0x4000 + LEN].copy_from_slice(data);
    // What the ULA has already painted this frame is not what is in memory
    // any more, so the picture is taken again from the top.
    spec.bus.show_screen_now();
    Ok(())
}
