//! What the emulator's own controls are meant to look like.

use eframe::egui;
use zx_rustrum::ui::theme;

/// Every slider the user reaches for is the one on the tape window's scope: a
/// green handle on a dark green track, with the part already dragged through
/// filled in. A slider added straight to a `Ui` gets egui's grey instead, and
/// the toolbar ends up with two kinds of control that do the same job.
#[test]
fn a_slider_is_green_on_a_dark_track() {
    let mut visuals = egui::Visuals::dark();
    theme::slider_visuals(&mut visuals);

    assert_eq!(
        visuals.selection.bg_fill,
        theme::GREEN,
        "the part behind the handle should fill in green"
    );
    assert!(
        visuals.slider_trailing_fill,
        "the trailing fill is what shows how far along the handle is"
    );
    for (state, widgets) in [
        ("hovered", &visuals.widgets.hovered),
        ("active", &visuals.widgets.active),
    ] {
        assert_eq!(
            widgets.bg_fill,
            theme::GREEN,
            "the {state} handle should light up, not match the track"
        );
    }
    let track = visuals.widgets.inactive.bg_fill;
    assert_ne!(track, theme::GREEN, "the track is not the handle's green");
    assert!(
        track.g() > track.r() && track.g() > track.b() && track.g() < 0x60,
        "the track should be a dark green, not {track:?}"
    );
}

/// The styling is scoped to the slider itself: it is applied to a copy of the
/// style inside a frame, so the controls beside it keep the case colours.
#[test]
fn styling_a_slider_leaves_the_rest_of_the_row_alone() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let before = ctx
        .style_of(egui::Theme::Dark)
        .visuals
        .widgets
        .inactive
        .bg_fill;

    let mut value = 0.5;
    let _ = ctx.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            theme::slider(ui, egui::Slider::new(&mut value, 0.0..=1.0));
            assert_eq!(
                ui.style().visuals.widgets.inactive.bg_fill,
                before,
                "the slider's green should not leak into the row it is on"
            );
        });
    });

    assert_eq!(
        ctx.style_of(egui::Theme::Dark)
            .visuals
            .widgets
            .inactive
            .bg_fill,
        before,
        "nor into the next frame"
    );
}

/// No slider goes straight to `Ui::add`. The look is only a standard if every
/// one of them goes through the theme, and the easiest way to lose it is to
/// add a slider somewhere new without noticing there is a helper. Reading the
/// source is crude, but it is the thing that actually went wrong: the toolbar
/// and the debug windows kept egui's grey for months while the tape window
/// had the green.
#[test]
fn every_slider_goes_through_the_theme() {
    let mut plain = Vec::new();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // Line breaks and indentation are `cargo fmt`'s business, so the
        // whitespace is taken out before looking for the call.
        let source: String = std::fs::read_to_string(&path)
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if source.contains("ui.add(egui::Slider::new") {
            plain.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(
        plain.is_empty(),
        "these draw a slider without theme::slider: {plain:?}"
    );
}
