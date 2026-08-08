//! The debugger's toolbar, and what happens when the machine stops at a
//! breakpoint.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use zx_rustrum::demo_rom::DEMO_ROM;
use zx_rustrum::machine::{Spectrum, Stop, FRAME_T};
use zx_rustrum::ui::{theme, App, Roms};

fn app() -> App {
    let mut spec = Spectrum::new();
    spec.load_rom(&DEMO_ROM);
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

/// Running into a breakpoint opens the debugger and asks for it to be brought
/// forward. Stopping is no use if the window showing where you stopped is
/// behind the emulator, or shut.
#[test]
fn a_breakpoint_brings_the_debugger_forward() {
    let mut app = app();
    app.running = true;
    app.show_debugger = false;

    // Somewhere the demo ROM comes back to: where it is after a few frames,
    // rather than the reset address it leaves once and never returns to.
    for _ in 0..4 {
        let _ = app.spec.run(FRAME_T);
    }
    let pc = app.cpu().pc;
    app.breakpoints_mut().push(pc);

    let mut hit = false;
    for _ in 0..8 {
        app.advance(1.0 / 60.0);
        if matches!(app.last_stop, Some(Stop::Breakpoint(_))) {
            hit = true;
            break;
        }
    }

    assert!(hit, "the machine never reached the breakpoint at ${pc:04X}");
    assert!(!app.running, "a breakpoint should stop the machine");
    assert!(app.show_debugger, "the debugger should be opened");
    assert!(
        app.dbg.raise,
        "the debugger should ask to be brought to the front"
    );
}

/// The window is a fixed width for the same reason the tape window is: the
/// disassembly and the panel beside it are columns, and they are unreadable
/// once they wrap. The width is held by viewport commands sent every frame,
/// as the tape window's is — a builder alone does not survive the window
/// being retired and rebuilt.
#[test]
fn the_debugger_window_is_held_to_a_fixed_width() {
    let source = include_str!("../src/ui/mod.rs");
    assert!(
        source.contains("self.fix_width(&ctx, debugger::WINDOW_W)"),
        "the debugger window does not fix its width"
    );
    assert!(
        !source.contains("[820.0, 780.0]"),
        "the debugger's size is written out again instead of using WINDOW_W"
    );
}

/// Every glyph in the toolbar has to exist in a font egui will actually reach
/// for. The step buttons used to carry arrows (U+2913, U+293C) that are in
/// none of the bundled fonts.
///
/// A missing glyph is not a missing character: epaint substitutes the
/// replacement character's own glyph, so the label still measures a sensible
/// width and only looks wrong on screen. What gives it away is the patch of
/// the font atlas it points at, which is the replacement's.
#[test]
fn the_toolbar_icons_are_in_the_fonts() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);

    let atlas_rects = std::cell::RefCell::new(Vec::new());
    let _ = ctx.run_ui(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let font = egui::TextStyle::Button.resolve(ui.style());
            let patch = |text: &str| {
                let galley = ui.painter().layout_no_wrap(
                    text.to_owned(),
                    font.clone(),
                    egui::Color32::WHITE,
                );
                let glyph = &galley.rows[0].glyphs[0];
                (glyph.uv_rect.min, glyph.uv_rect.max)
            };
            let missing = patch("\u{fffd}");
            for icon in button_icons().into_iter().chain(['\u{2913}', '\u{293c}']) {
                atlas_rects
                    .borrow_mut()
                    .push((icon, patch(&icon.to_string()), missing));
            }
        });
    });

    let drawn = atlas_rects.into_inner();
    assert!(drawn.len() > 4, "the icons were not laid out: {drawn:?}");
    let last_two = drawn.len() - 2;
    for (icon, patch, missing) in &drawn[..last_two] {
        assert_ne!(
            patch, missing,
            "no bundled font has {icon} (U+{:04X}), so it draws as a box",
            *icon as u32
        );
    }
    // The two the step buttons used to carry. They are still missing, which is
    // what shows the check above can tell a missing glyph from a present one.
    for (icon, patch, missing) in &drawn[last_two..] {
        assert_eq!(
            patch, missing,
            "{icon} turns out to be in a font after all, so this test proves nothing"
        );
    }
}

/// Pausing must not shuffle the buttons after it along. The label changes
/// width between "Run" and "Pause", so the button is sized for the longer of
/// the two whichever it is showing.
#[test]
fn the_run_button_keeps_its_width_when_paused() {
    let mut harness = Harness::builder()
        .with_size([1600.0, 800.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app());
    harness.run_steps(3);

    let running = harness.get_by_label(theme::RUN_LABEL).rect();
    harness.get_by_label(theme::RUN_LABEL).click();
    harness.run_steps(3);
    let paused = harness.get_by_label(theme::PAUSE_LABEL).rect();

    assert_eq!(
        running.width(),
        paused.width(),
        "the button is {} wide stopped and {} wide running",
        running.width(),
        paused.width()
    );
    assert_eq!(
        running.min.x, paused.min.x,
        "and it should not have moved either"
    );
}

/// The speed dropdown belongs to the main window. It was in the debugger as
/// well, setting the same field twice over.
#[test]
fn the_debugger_does_not_repeat_the_speed_control() {
    let source = include_str!("../src/ui/debugger.rs");
    assert!(
        !source.contains("speed_dropdown"),
        "the debugger draws a speed control of its own again"
    );
}

/// Stepping still works, so the relabelled buttons are still wired up.
#[test]
fn the_step_buttons_still_step() {
    let mut app = app();
    let _ = app.spec.run(FRAME_T);
    let before = app.cpu().pc;
    app.step_into();
    assert_ne!(app.cpu().pc, before, "step into did not move the PC");
}

/// Every symbol the buttons in the user interface are labelled with, read out
/// of the source, so a new button with an unrenderable icon is caught rather
/// than only the ones that were thought of when this was written.
fn button_icons() -> Vec<char> {
    let mut icons = Vec::new();
    for source in [
        include_str!("../src/ui/debugger.rs"),
        include_str!("../src/ui/mod.rs"),
        include_str!("../src/ui/theme.rs"),
        include_str!("../src/ui/tape.rs"),
    ] {
        for line in source.lines() {
            let line = line.trim_start();
            if line.starts_with("//") {
                continue;
            }
            if !["button(", "toggle_value(", "selectable_value("]
                .iter()
                .any(|call| line.contains(call))
            {
                continue;
            }
            // Whatever is between the quotes on the line: the labels here are
            // all written inline.
            let mut quoted = line.split('"').skip(1).step_by(2);
            if let Some(label) = quoted.next() {
                icons.extend(label.chars().filter(|c| !c.is_ascii()));
            }
        }
    }
    icons.sort_unstable();
    icons.dedup();
    assert!(!icons.is_empty(), "no button labels were found to check");
    icons
}
