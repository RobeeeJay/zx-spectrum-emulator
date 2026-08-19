//! The back buffer preview: a bitmap, not a screen.

use zx_rustrum::machine::Spectrum;
use zx_rustrum::screen::{render_bitmap_from, render_from, View, MONO_ATTR};

/// A buffer at $8000 with one cell of bitmap set, and rubbish where a screen
/// would keep its attributes.
fn machine_with_buffer() -> Spectrum {
    let mut spec = Spectrum::new();
    // The top-left cell: eight rows of alternating bits.
    for row in 0..8u16 {
        spec.bus.poke(0x8000 + (row << 8), 0b1010_1010);
    }
    // And every attribute byte set to red ink on green paper, which is what
    // would show if the preview read them.
    for at in 0x9800..0x9B00u16 {
        spec.bus.poke(at, 0x22);
    }
    spec
}

fn pixel(buf: &[u8], view: View, x: usize, y: usize) -> [u8; 4] {
    let i = (y * view.width() + x) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// Only the bitmap is read: the bytes past it are somebody else's variables,
/// and a preview that draws them as attributes is painted in their colours.
#[test]
fn the_preview_ignores_what_is_past_the_bitmap() {
    let spec = machine_with_buffer();
    let view = View::CROPPED;
    let mut out = vec![0u8; view.buffer_len()];
    render_bitmap_from(&spec.bus, view, 0x8000, &mut out, false);

    let (x0, y0) = (view.border_x, view.border_top);
    let ink = pixel(&out, view, x0, y0);
    let paper = pixel(&out, view, x0 + 1, y0);
    assert_eq!(
        ink,
        [255, 255, 255, 255],
        "a set bit is bright white, whatever the bytes behind the screen say"
    );
    assert_eq!(paper, [0, 0, 0, 255], "and a clear one is black");

    // The bitmap is still read, so the pattern is the program's.
    for x in 0..8 {
        let want = if x % 2 == 0 {
            [255, 255, 255, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(
            pixel(&out, view, x0 + x, y0),
            want,
            "bit {x} of the cell should be drawn from the bitmap"
        );
    }

    // Whereas reading it as a whole screen shows the rubbish: red on green.
    let mut as_screen = vec![0u8; view.buffer_len()];
    render_from(&spec.bus, view, 0x8000, &mut as_screen, false, false);
    assert_ne!(
        pixel(&as_screen, view, x0, y0),
        ink,
        "reading the attributes is what this preview stopped doing"
    );
    assert_eq!(
        MONO_ATTR, 0x47,
        "bright white ink on black paper, which is what a bitmap looks like"
    );
}

/// The window says what it is showing, so nobody reads the preview as a
/// screen that has lost its colours.
#[test]
fn the_window_says_it_is_showing_a_bitmap() {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use zx_rustrum::ui::{App, Roms};

    let mut app = App::with_roms(machine_with_buffer(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_tape = false;
    app.show_back_buffer = true;
    app.running = false;
    // A region to preview, as the detector would have found.
    app.spec.bus.tracker.manual = Some(zx_rustrum::tracker::Region {
        start: 0x8000,
        len: 6144,
    });
    let mut h: Harness<'_, App> = Harness::builder()
        .with_size([1400.0, 1000.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);
    assert!(
        h.get_all_by_label_contains("6144-byte bitmap")
            .next()
            .is_some(),
        "the window should say it is showing a bitmap"
    );

    // And what it drew is black and white: the attribute area of this buffer
    // holds red on green, which is what would show if it were read.
    let drawn = &h.state().back.pixels;
    assert!(!drawn.is_empty(), "the preview should have been drawn");
    let coloured = drawn
        .chunks(4)
        .filter(|p| {
            let (r, g, b) = (p[0], p[1], p[2]);
            !(r == g && g == b)
        })
        .count();
    assert_eq!(
        coloured, 0,
        "every pixel should be black or white: {coloured} are not"
    );
}
