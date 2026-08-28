//! The tools that give back a picture: the screen, and memory read as
//! graphics. What they draw with is in `crate::mcp::picture`.

use crate::mcp::json::Json;
use crate::mcp::tools::{addr, count, count_in, flag, Reply, Session};

/// The screen. As a PNG for a model that can see one, and as a sketch of
/// character cells for one that cannot — with the attributes summarised,
/// since a Spectrum picture is half colour.
pub fn screen(session: &mut Session, args: &Json) -> Result<Reply, String> {
    let view = if flag(args, "border", false) {
        crate::screen::View::OVERSCAN
    } else {
        crate::screen::View::CROPPED
    };
    // The flash phase is the machine's own: sixteen frames on, sixteen off.
    let flash_on = (session.spec.bus.frame / 16) % 2 == 1;
    let sketch = crate::mcp::picture::sketch(&session.spec.bus);
    let colours = crate::mcp::picture::colours(&session.spec.bus);
    let words = format!(
        "The screen as it stands, frame {}.\n{colours}\n{sketch}",
        session.spec.bus.frame
    );
    if let Some(path) = args.get("path").and_then(|p| p.as_str()) {
        let png = crate::mcp::picture::png(&session.spec.bus, view, flash_on)?;
        std::fs::write(path, &png).map_err(|e| format!("{path}: {e}"))?;
        return Ok(Reply::Text(format!("{words}\nWritten to {path}")));
    }
    if flag(args, "image", true) {
        let png = crate::mcp::picture::png(&session.spec.bus, view, flash_on)?;
        return Ok(Reply::Picture { png, text: words });
    }
    Ok(Reply::Text(words))
}

/// Memory read as graphics: eight bytes to a character, as the Spectrum
/// stores its own. Point it at a candidate address and see whether
/// letters, sprites or rubbish come out.
pub fn graphics(session: &mut Session, args: &Json) -> Result<Reply, String> {
    let start = addr(args, "address")?;
    let count = count(args, "count", 16)?.min(256) as u16;
    let across = count_in(args, "across", 8, 1, 32)? as u16;
    let text = crate::mcp::picture::tiles(|a| session.spec.bus.peek_raw(a), start, count, across);
    let words = format!(
        "{count} characters' worth from ${start:04X}, {across} across. \
         Eight bytes a character, one bit a pixel, top row first.\n{text}"
    );
    if flag(args, "image", false) {
        let (w, h) = ((across as usize) * 8, count.div_ceil(across) as usize * 8);
        let mut rgba = vec![0u8; w * h * 4];
        for (i, pixel) in rgba.chunks_mut(4).enumerate() {
            let (x, y) = (i % w, i / w);
            let (tile_x, tile_y) = (x / 8, y / 8);
            let byte = session.spec.bus.peek_raw(
                start.wrapping_add((tile_y * across as usize + tile_x) as u16 * 8 + (y % 8) as u16),
            );
            let lit = byte & (0x80 >> (x % 8)) != 0;
            let value = if lit { 0xFF } else { 0x00 };
            pixel.copy_from_slice(&[value, value, value, 0xFF]);
        }
        let png = crate::mcp::picture::encode(&rgba, w, h)?;
        return Ok(Reply::Picture { png, text: words });
    }
    Ok(Reply::Text(words))
}
