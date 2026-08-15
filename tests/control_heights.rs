//! Every control in a row is the same height, and nothing moves under the
//! pointer.
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

    // The machine row and the video row beneath it: each sits on one line,
    // and every control in both is the same height, whichever line it is on.
    let machine = heights(&harness, &["File", "▶ Run", "Late timing", "Reset"]);
    let video = heights(
        &harness,
        &["Race the Beam", "Cursor Beam", "Overscan", "Next frame"],
    );
    same_height(&machine, "main window's machine row");
    same_height(&video, "main window's video row");
    assert!(
        (machine[0].1 - video[0].1).abs() < 0.5,
        "the two rows of the main window are {} and {} tall",
        machine[0].1,
        video[0].1
    );
    assert!(
        video[0].2 > machine[0].2,
        "the video row should be under the machine row, not at y {}",
        video[0].2
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

/// Hovering a control must not move anything, including itself.
///
/// egui leaves the frame off a selectable button while it is unselected and
/// the pointer is elsewhere, and puts it back when the pointer arrives. The
/// stroke is a point on each side, so every toggle grew by two as the pointer
/// crossed it and shoved the rest of the row along. Found by hovering the
/// middle of every button in turn and diffing the position of every widget in
/// the window, which is how it should stay found.
fn nothing_moves_when_hovered(mut app: App, size: [f32; 2], what: &str) {
    app.running = false;
    let mut harness = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(4);
    let away = every_widget(&harness);

    let buttons: Vec<(String, [f32; 4])> = away
        .iter()
        .filter(|(name, _)| name.contains("Button|"))
        .cloned()
        .collect();
    assert!(
        buttons.len() > 5,
        "only {} buttons found in the {what} — the sweep is not sweeping",
        buttons.len()
    );

    for (who, rect) in buttons {
        let at = egui::pos2((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at));
        harness.run_steps(3);
        for (name, now) in every_widget(&harness) {
            let Some((_, was)) = away.iter().find(|(other, _)| *other == name) else {
                continue;
            };
            assert!(
                (now[0] - was[0]).abs() < 0.4 && (now[1] - was[1]).abs() < 0.4,
                "in the {what}, hovering {who} moved {name} from {was:?} to {now:?}"
            );
            assert!(
                (now[2] - now[0] - (was[2] - was[0])).abs() < 0.4,
                "in the {what}, hovering {who} made {name} {} wide instead of {}",
                now[2] - now[0],
                was[2] - was[0]
            );
        }
    }
}

/// Every widget in the window, named so the same one can be found again after
/// the pointer has moved. The index is part of the name because a window holds
/// dozens of unlabelled text runs, and matching those by their text alone pairs
/// up whichever two happen to be blank.
fn every_widget(harness: &Harness<'_, App>) -> Vec<(String, [f32; 4])> {
    harness
        .root()
        .children_recursive()
        .enumerate()
        .filter_map(|(index, node)| {
            let node = node.accesskit_node();
            let box_ = node.bounding_box()?;
            Some((
                format!(
                    "#{index} {:?}|{}|{}",
                    node.role(),
                    node.label().unwrap_or_default(),
                    node.value().unwrap_or_default()
                ),
                [
                    box_.x0 as f32,
                    box_.y0 as f32,
                    box_.x1 as f32,
                    box_.y1 as f32,
                ],
            ))
        })
        .collect()
}

#[test]
fn nothing_in_the_main_window_moves_under_the_pointer() {
    let mut app = app();
    app.show_debugger = false;
    nothing_moves_when_hovered(app, [1600.0, 900.0], "main window");
}

#[test]
fn nothing_in_the_debugger_moves_under_the_pointer() {
    let mut app = app();
    app.show_debugger = true;
    nothing_moves_when_hovered(app, [1600.0, 1100.0], "debugger");
}

/// The debugger's panels do not change width with what they are showing.
///
/// HALTED appears at the end of the flags row while the machine is halted,
/// which is once a frame for a program waiting on the interrupt. The panel
/// sizes itself to its contents, so a word appearing there widened it and
/// shoved the stack, the labels and the screen along beside it — twice a
/// frame, back and forth.
#[test]
fn the_halted_flag_does_not_move_the_panels_beside_it() {
    let panels = |halted: bool| -> Vec<(String, [f32; 4])> {
        let mut app = app();
        app.show_debugger = true;
        app.spec.cpu.halted = halted;
        let mut harness = Harness::builder()
            .with_size([1600.0, 1100.0])
            .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
        harness.run_steps(4);
        every_widget(&harness)
            .into_iter()
            .filter(|(name, _)| {
                ["Stack", "Labels", "Data", "Screen"]
                    .iter()
                    .any(|panel| name.ends_with(panel))
            })
            .collect()
    };

    let running = panels(false);
    let halted = panels(true);
    assert!(
        !running.is_empty(),
        "the panels beside the registers should be on show"
    );
    for (name, was) in &running {
        let Some((_, now)) = halted.iter().find(|(other, _)| other == name) else {
            continue;
        };
        assert!(
            (now[0] - was[0]).abs() < 0.5,
            "{name} sits at x {} while running and {} while halted",
            was[0],
            now[0]
        );
    }
}
