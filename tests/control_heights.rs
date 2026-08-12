//! Every control in a row is the same height.
//!
//! Nothing moves when the pointer arrives — that was checked by measuring the
//! same widgets hovered and not. What "the buttons jump on hover" actually was
//! is this: egui sizes a button to at least `interact_size` and a toggle to
//! its text plus padding, so a row came out at 18, 20 and 22 points with each
//! control sitting at a different height. The outline that appears under the
//! pointer was then two points off from its neighbours'.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn heights(harness: &Harness<'_, App>, labels: &[&str]) -> Vec<(String, f32, f32)> {
    labels
        .iter()
        .filter_map(|label| {
            let node = harness.get_all_by_label(label).next()?;
            let box_ = node.accesskit_node().bounding_box()?;
            Some((
                label.to_string(),
                (box_.y1 - box_.y0) as f32,
                box_.y0 as f32,
            ))
        })
        .collect()
}

fn same_height(measured: &[(String, f32, f32)], what: &str) {
    assert!(measured.len() >= 3, "only found {measured:?} in the {what}");
    let (_, first_height, first_top) = &measured[0];
    for (label, height, top) in measured {
        assert!(
            (height - first_height).abs() < 0.5,
            "in the {what}, {label} is {height} tall and {} is {first_height}: \
             a control of a different height sits at a different place in the \
             row, and its hover outline reads as the control jumping",
            measured[0].0
        );
        assert!(
            (top - first_top).abs() < 0.5,
            "in the {what}, {label} starts at y {top} and {} at y {first_top}",
            measured[0].0
        );
    }
}

fn app() -> App {
    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

#[test]
fn the_main_windows_controls_are_all_one_height() {
    let mut app = app();
    app.show_debugger = false;
    let mut harness = Harness::builder()
        .with_size([1600.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(4);

    same_height(
        &heights(
            &harness,
            &[
                "File",
                "▶ Run",
                "Late timing",
                "Reset",
                "Race the beam",
                "Overscan",
            ],
        ),
        "main window",
    );
}

#[test]
fn the_debuggers_controls_are_all_one_height() {
    let mut app = app();
    app.show_debugger = true;
    let mut harness = Harness::builder()
        .with_size([1600.0, 1100.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(4);

    same_height(
        &heights(
            &harness,
            &[
                "⤵ Step into",
                "⏭ Step over",
                "⤴ Step out",
                "↺ Reset",
                "Follow PC",
            ],
        ),
        "debugger",
    );
    same_height(
        &heights(&harness, &["Screen", "Beeper", "Interrupt", "ROM"]),
        "debugger's break row",
    );
}
