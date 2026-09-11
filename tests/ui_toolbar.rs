//! The machine group on the main window's toolbar: the clock, the speed
//! beside it, and the 48K's two switches.

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use zx_rustrum::machine::Spectrum;
use zx_rustrum::ui::{App, Roms, CLOCK_MULTIPLES};

fn test_app() -> App {
    let roms = Roms {
        rom48: Some(vec![0x00; 0x4000]),
        rom128: Some(vec![0x00; 0x8000]),
        rom_plus3: Some(vec![0x00; 0x10000]),
        rom_zx81: Some(vec![0x00; 0x2000]),
    };
    let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    app
}

fn harness<'a>() -> Harness<'a, App> {
    Harness::builder()
        .with_size([1600.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), test_app())
}

/// Left and right edges of the control with this label.
fn span(h: &Harness<'_, App>, label: &str) -> (f64, f64) {
    let b = h
        .get_by_label(label)
        .accesskit_node()
        .bounding_box()
        .unwrap_or_else(|| panic!("{label:?} has no bounds"));
    (b.x0, b.x1)
}

/// The clock's button is as wide at 28MHz as at 3.5MHz, so choosing a clock
/// does not shove the speed and everything after it along the toolbar.
#[test]
fn the_clock_dropdown_keeps_its_width_whatever_it_says() {
    let mut h = harness();
    let mut seen = Vec::new();
    for mult in CLOCK_MULTIPLES {
        h.state_mut().clock_mult = *mult;
        h.run_steps(2);
        let clock = format!("{:.2}MHz  \u{25be}", 3.5 * mult);
        let (left, right) = span(&h, &clock);
        let (speed_left, _) = span(&h, "100%  \u{25be}");
        seen.push((clock, right - left, speed_left));
    }
    let (_, width, speed_at) = seen[0].clone();
    for (clock, w, s) in &seen {
        assert!(
            (w - width).abs() < 0.5,
            "{clock} is {w} wide where {} was {width}: {seen:?}",
            seen[0].0
        );
        assert!(
            (s - speed_at).abs() < 0.5,
            "at {clock} the speed moved to x {s} from {speed_at}"
        );
    }
}

/// The speed is the clock's neighbour now, and needs no heading: a
/// percentage beside a frequency says what it is.
#[test]
fn the_speed_follows_the_clock_without_a_heading() {
    let mut h = harness();
    h.run_steps(2);
    let (_, clock_right) = span(&h, "3.50MHz  \u{25be}");
    let (speed_left, _) = span(&h, "100%  \u{25be}");
    assert!(
        speed_left > clock_right && speed_left - clock_right < 20.0,
        "the speed starts at x {speed_left}, the clock ends at {clock_right}"
    );
    assert_eq!(
        h.query_all_by_label("Speed").count() + h.query_all_by_value("Speed").count(),
        0,
        "no Speed heading"
    );
}

/// On a 48K the two switches are short: Late and I2, with the full names in
/// what they say when hovered.
#[test]
fn the_48ks_switches_are_late_and_i2() {
    let mut h = harness();
    h.run_steps(2);
    assert!(h.query_by_label("Late").is_some(), "a Late switch");
    assert!(h.query_by_label("I2").is_some(), "an I2 switch");
    assert!(h.query_by_label("Late timing").is_none());
    assert!(h.query_by_label("Issue 2").is_none());
}
