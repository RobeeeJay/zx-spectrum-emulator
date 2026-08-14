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
