//! The window icon: the artwork, decoded.

/// The icon in the repository is a PNG the emulator can read, and it comes out
/// as RGBA of the size the file says.
///
/// The icon is decoded at startup rather than built into the binary as pixels,
/// so a file that has been re-exported and no longer decodes would cost the
/// icon silently. This checks the one in the repository decodes.
#[test]
fn the_icon_decodes_to_rgba() {
    let data = std::fs::read("icon.png").expect("the icon should be in the repository");
    let decoder = png::Decoder::new(std::io::Cursor::new(&data));
    let mut reader = decoder.read_info().expect("it should be a PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a known size")];
    let info = reader.next_frame(&mut buffer).expect("it should decode");

    assert_eq!(info.width, info.height, "an icon is square");
    assert!(
        info.width >= 128,
        "and big enough for a dock: {}x{}",
        info.width,
        info.height
    );
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => panic!("the icon is {other:?}, which the emulator does not turn into RGBA"),
    };
    assert_eq!(
        info.buffer_size(),
        (info.width * info.height) as usize * channels,
        "every pixel accounted for"
    );
}

/// The icon is shaped the way macOS shapes one: the artwork inside a rounded
/// square with clear space around it, so it sits in the dock at the same
/// visual size as every other icon rather than as a full-bleed square.
#[test]
fn the_icon_is_shaped_like_a_macos_icon() {
    use zx_rustrum::appicon::shaped;

    // A solid square of artwork, so anything transparent in the result is the
    // shaping rather than the picture.
    let (width, height) = (64u32, 64u32);
    let art: Vec<u8> = (0..width * height)
        .flat_map(|_| [0xC0, 0x40, 0x20])
        .collect();
    let size = 512u32;
    let icon = shaped(&art, width, height, size);
    assert_eq!(icon.len(), (size * size * 4) as usize);

    let alpha = |x: u32, y: u32| icon[((y * size + x) * 4 + 3) as usize];
    let colour = |x: u32, y: u32| {
        let at = ((y * size + x) * 4) as usize;
        [icon[at], icon[at + 1], icon[at + 2]]
    };

    // The middle is the artwork, solid.
    assert_eq!(alpha(size / 2, size / 2), 255, "the middle should be solid");
    assert_eq!(colour(size / 2, size / 2), [0xC0, 0x40, 0x20]);

    // The corners are clear: both the margin Apple leaves and the rounding.
    for (x, y) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
        assert_eq!(alpha(x, y), 0, "the corner at ({x}, {y}) should be clear");
    }

    // There is a margin all the way round, so the artwork does not reach the
    // edge of the canvas.
    let margin = (size as f32 * (1024.0 - 824.0) / 2.0 / 1024.0) as u32;
    assert_eq!(
        alpha(size / 2, margin / 2),
        0,
        "the space above the artwork should be clear"
    );
    assert_eq!(
        alpha(size / 2, margin + 4),
        255,
        "and the artwork should start just inside it"
    );

    // The edge is drawn smoothly rather than as a staircase: all the way round
    // the shape there are pixels that are neither in nor out.
    let part = (0..size * size)
        .filter(|i| {
            let a = icon[(i * 4 + 3) as usize];
            a > 0 && a < 255
        })
        .count();
    assert!(
        part > size as usize,
        "the edge should be drawn smoothly, and only {part} pixels are part-way \
         in"
    );
}
