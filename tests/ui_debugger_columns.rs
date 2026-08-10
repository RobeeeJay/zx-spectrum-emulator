//! The shape of the debugger's two panes: named columns in the listing, the
//! memory dump on the same pitch, and a layout that holds still.

use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;
use zx_rustrum::demo_rom::DEMO_ROM;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms};

fn app() -> App {
    let mut spec = Spectrum::new();
    spec.load_rom(&DEMO_ROM);
    let mut app = App::with_roms(spec, String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.show_debugger = true;
    app.running = false;
    app
}

fn harness<'a>() -> Harness<'a, App> {
    let mut h = Harness::builder()
        .with_size([1500.0, 1200.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app());
    h.run_steps(4);
    h
}

/// Every column of the listing says what it holds. Three of them were only
/// distinguishable by knowing the format of the line they came from.
#[test]
fn the_listing_names_all_five_columns() {
    let h = harness();
    let text = every_string(&h);
    for column in ["Label", "Address", "Value", "Instruction", "Comments"] {
        assert!(
            text.contains(&column.to_string()),
            "no heading for {column}: {text:?}"
        );
    }
}

/// Cells are left justified, so a short instruction starts where a long one
/// does. Centred text wanders about as the listing scrolls past instructions
/// of different lengths.
#[test]
fn the_columns_line_up_down_the_listing() {
    let h = harness();

    // The addresses on show: monospace, four hex digits, one per row.
    let addresses: Vec<_> = collect(&h, |value, _| {
        (value.len() == 4 && value.chars().all(|c| c.is_ascii_hexdigit())).then_some(())
    });
    assert!(
        addresses.len() > 8,
        "only found {} address cells",
        addresses.len()
    );

    let left = addresses[0].1.min.x;
    for (_, rect) in &addresses {
        assert!(
            (rect.min.x - left).abs() < 0.5,
            "an address starts at {} while the first is at {left}",
            rect.min.x
        );
    }
}

/// The dump and the listing are read together, so their lines sit on the same
/// pitch. The dump used to be laid out to the label's own height and came out
/// tighter than the listing beside it.
#[test]
fn the_memory_dump_is_on_the_same_pitch_as_the_listing() {
    let h = harness();

    let addresses = rows_of(&h, |value| {
        value.len() == 4 && value.chars().all(|c| c.is_ascii_hexdigit())
    });
    let dump = rows_of(&h, |value| {
        // A dump line is an address, eight bytes and eight characters.
        value.len() > 30 && value.split_whitespace().count() >= 9
    });

    let listing_pitch = pitch(&addresses);
    let dump_pitch = pitch(&dump);
    assert!(
        listing_pitch > 0.0 && dump_pitch > 0.0,
        "found {} listing rows and {} dump rows",
        addresses.len(),
        dump.len()
    );
    assert!(
        (listing_pitch - dump_pitch).abs() < 0.5,
        "the listing steps {listing_pitch} a row and the dump {dump_pitch}"
    );
}

/// Nothing moves between one frame and the next once the window has settled,
/// at whatever height it has been dragged to. Widths taken from the space
/// left over feed back through the scrollbar: the column takes what is free,
/// the row grows, the scrollbar appears, and the next frame has less space
/// than the last.
#[test]
fn the_listing_holds_still_at_any_height() {
    let mut h = harness();

    for height in [1200.0, 900.0, 640.0, 1100.0] {
        h.set_size(egui::vec2(1500.0, height));
        h.run_steps(4);
        let before = geometry(&h);
        h.run_steps(1);
        let after = geometry(&h);
        assert_eq!(
            before, after,
            "the listing moved between two frames at {height} points tall"
        );
    }
}

/// Every string in the window, from both the labels and the values, since a
/// plain label keeps its text in the value.
fn every_string(h: &Harness<'_, App>) -> Vec<String> {
    collect(h, |value, label| {
        Some(if value.is_empty() { label } else { value })
    })
    .into_iter()
    .map(|(text, _)| text)
    .collect()
}

/// The text and rectangle of every node the picker accepts.
fn collect<T>(
    h: &Harness<'_, App>,
    pick: impl Fn(String, String) -> Option<T> + Copy,
) -> Vec<(T, egui::Rect)> {
    fn walk<T>(
        node: &egui_kittest::Node<'_>,
        pick: impl Fn(String, String) -> Option<T> + Copy,
        out: &mut Vec<(T, egui::Rect)>,
    ) {
        let value = node
            .accesskit_node()
            .value()
            .unwrap_or_default()
            .to_string();
        let label = node
            .accesskit_node()
            .label()
            .unwrap_or_default()
            .to_string();
        // The root node has no rectangle of its own, so anything without one
        // is passed over rather than asked for it.
        if let Some(kept) = pick(value, label) {
            if let Some(box_) = node.accesskit_node().bounding_box() {
                out.push((
                    kept,
                    egui::Rect {
                        min: egui::pos2(box_.x0 as f32, box_.y0 as f32),
                        max: egui::pos2(box_.x1 as f32, box_.y1 as f32),
                    },
                ));
            }
        }
        for child in node.children() {
            walk(&child, pick, out);
        }
    }
    let mut found = Vec::new();
    walk(&h.root(), pick, &mut found);
    found
}

/// The rectangles of the rows whose text the test recognises.
fn rows_of(h: &Harness<'_, App>, matches: impl Fn(&str) -> bool + Copy) -> Vec<egui::Rect> {
    let mut rects: Vec<_> = collect(h, |value, _| matches(&value).then_some(()))
        .into_iter()
        .map(|(_, rect)| rect)
        .collect();
    rects.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
    rects
}

/// How far apart consecutive rows are, taken as the commonest step so a gap
/// between two panes does not count as one.
fn pitch(rows: &[egui::Rect]) -> f32 {
    let mut steps: Vec<f32> = rows.windows(2).map(|w| w[1].min.y - w[0].min.y).collect();
    steps.retain(|s| *s > 0.5);
    steps.sort_by(f32::total_cmp);
    steps.get(steps.len() / 2).copied().unwrap_or(0.0)
}

/// Where everything in the window is, rounded to a tenth of a point: what has
/// to be the same from one frame to the next.
fn geometry(h: &Harness<'_, App>) -> Vec<(i32, i32, i32, i32)> {
    collect(h, |_, _| Some(()))
        .into_iter()
        .map(|(_, r)| {
            let round = |v: f32| (v * 10.0).round() as i32;
            (
                round(r.min.x),
                round(r.min.y),
                round(r.max.x),
                round(r.max.y),
            )
        })
        .collect()
}

/// Nothing in the debugger reaches past the edge of the window it is drawn in.
///
/// The window is a fixed width, and there is no horizontal scrolling in it, so
/// anything wider than that is simply not there: the stack view went in beside
/// a register panel that turned out to be 300 points wider than the window,
/// and both it and the memory dump ended up off the right-hand edge.
#[test]
fn nothing_in_the_debugger_falls_off_the_right_hand_edge() {
    use zx_rustrum::ui::debugger;

    let mut h = Harness::builder()
        .with_size([debugger::WINDOW_W, 900.0])
        .build_ui_state(|ui, app: &mut App| debugger::ui(app, ui), app());
    h.run_steps(4);

    let mut overflowing: Vec<_> = collect(&h, |value, label| {
        Some(if value.is_empty() { label } else { value })
    })
    .into_iter()
    .filter(|(_, rect)| rect.max.x > debugger::WINDOW_W)
    .map(|(text, rect)| format!("{text:?} reaches {}", rect.max.x))
    .collect();
    overflowing.dedup();

    assert!(
        overflowing.is_empty(),
        "{} things are off the edge of a {}-point window: {overflowing:#?}",
        overflowing.len(),
        debugger::WINDOW_W
    );
}
