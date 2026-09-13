//! A picture of the machine's keyboard: pressable, and lit by the real one.
//!
//! The layout and the legends are in `crate::keyboard`; what is here is the
//! drawing of them and the pointer, and the search that picks keys out by
//! what is written on them — with the shifts that reach it, since a word under
//! a key is no use to somebody who does not know it takes two.

use eframe::egui;
use egui::{Align2, Color32, FontId, Sense, Stroke};

use crate::keyboard::{self, Key};
use crate::ui::theme;
use crate::ui::App;

/// The gap between keys, at the size the keys are drawn.
const GAP: f32 = 6.0;

/// And the gap between rows, which is wider because the word printed under
/// each key goes in it — as it does on the case, in red.
const ROW_GAP: f32 = 13.0;

/// The face of a key, and the face of one that is down.
const FACE: Color32 = theme::CONTROL;
const LIT: Color32 = theme::AMBER;
/// A key the search found, and a shift it needs.
const FOUND: Color32 = theme::YELLOW;
const NEEDED: Color32 = theme::CYAN;

/// What the search says about a key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    None,
    Found,
    Needed,
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let now = std::time::Instant::now();
    let zx81 = app.on_zx81();
    let keys = keyboard::layout(zx81);

    ui.horizontal(|ui| {
        theme::group_label(ui, "Keyboard");
        ui.label(app.machine_name());
        if app.rzx.is_some() {
            ui.label(
                egui::RichText::new("a recording is supplying the keys")
                    .color(theme::DIM)
                    .small(),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.key_search)
                    .hint_text("find: print, cat, beep…")
                    .desired_width(170.0),
            )
            .on_hover_text(
                "Picks out the keys with that word on them, and the shifts it takes. \
                 Commas look for several at once.",
            );
        });
    });

    let found = keyboard::search(zx81, &app.key_search);
    let mut marks = [Mark::None; 40];
    for f in &found {
        for shift in f.shifts(zx81) {
            if marks[shift] == Mark::None {
                marks[shift] = Mark::Needed;
            }
        }
    }
    for f in &found {
        marks[f.key] = Mark::Found;
    }
    if !app.key_search.trim().is_empty() {
        // How to type each one, since the shifts lit on the picture say which
        // but not in what order.
        const SHOWN: usize = 4;
        let mut lines: Vec<String> = found.iter().take(SHOWN).map(|f| f.how(zx81)).collect();
        if found.is_empty() {
            lines.push("nothing on the keys says that".to_string());
        } else if found.len() > SHOWN {
            lines.push(format!("and {} more", found.len() - SHOWN));
        }
        ui.label(
            egui::RichText::new(lines.join("   ·   "))
                .small()
                .color(theme::DIM),
        );
    }
    ui.add_space(4.0);

    let area = ui.available_rect_before_wrap();
    let rects = keyboard::key_rects_with(area, GAP, ROW_GAP);
    let painter = ui.painter().clone();
    painter.rect_filled(area, 6.0, theme::CASE_DARK);

    for (i, key) in keys.iter().enumerate() {
        let rect = rects[i];
        let id = ui.id().with(("key", i));
        let response = ui.interact(rect, id, Sense::click_and_drag());
        // Picked out by the search, as far as anything reading the window can
        // tell: the key found, or a shift it needs.
        let marked = marks[i] != Mark::None;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, marked, key.main)
        });
        let legends: Vec<&str> = found
            .iter()
            .filter(|f| f.key == i)
            .map(|f| f.legend)
            .collect();

        // A shift stays down until the next key, so a shifted key can be
        // typed with one pointer; anything else is down while the pointer is.
        let shift = key.press.len() == 1 && is_shift(key.press[0]);
        if response.clicked() && shift {
            app.keys.latch(key.press[0].0, key.press[0].1);
        } else if response.is_pointer_button_down_on() || response.clicked() {
            for &(row, bit) in key.press {
                app.keys.press(row, bit, now);
            }
            // The shifts that were waiting for this key are pressed with it
            // rather than simply let go, or the machine would see the key on
            // its own: a press lasts a tenth of a second and the latch would
            // have gone by then.
            app.keys.take_latched(now);
        }

        let lit = key
            .press
            .iter()
            .all(|&(row, bit)| app.keys.is_lit(row, bit, now))
            || (shift && app.keys.latched(key.press[0].0, key.press[0].1));
        draw_key(
            &painter,
            rect,
            key,
            lit,
            response.hovered(),
            marks[i],
            &legends,
        );
        // Under the key rather than on it, which is where the machine prints
        // it: the extended-mode word a shift gives — CAT, FORMAT, INVERSE.
        if !key.under.is_empty() {
            painter.text(
                egui::pos2(rect.center().x, rect.bottom() + 1.0),
                Align2::CENTER_TOP,
                key.under,
                FontId::proportional((rect.height() * 0.20).clamp(6.0, 9.0)),
                if legends.contains(&key.under) {
                    FOUND
                } else {
                    theme::RED
                },
            );
        }
    }

    // A key that is lit goes out on its own, with nothing else moving.
    if app.keys.anything_lit(now) {
        ui.ctx().request_repaint_after(keyboard::MIN_PRESS);
    }
}

fn is_shift(at: (usize, u8)) -> bool {
    at == (0, 0) || at == (7, 1)
}

fn draw_key(
    painter: &egui::Painter,
    rect: egui::Rect,
    key: &Key,
    lit: bool,
    hovered: bool,
    mark: Mark,
    legends: &[&str],
) {
    let face = if lit {
        LIT
    } else if hovered {
        theme::CONTROL_HOVER
    } else {
        FACE
    };
    painter.rect_filled(rect, 4.0, face);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, theme::EDGE),
        egui::StrokeKind::Inside,
    );
    // The search's marks go round the key, so a key can be found and lit at
    // once and still be read.
    let ring = match mark {
        Mark::None => None,
        Mark::Found => Some(FOUND),
        Mark::Needed => Some(NEEDED),
    };
    if let Some(colour) = ring {
        painter.rect_stroke(
            rect.expand(1.5),
            5.0,
            Stroke::new(2.5, colour),
            egui::StrokeKind::Outside,
        );
    }
    // The word that was found, in the ring's colour, wherever it is on the key.
    let found_in = |legend: &str, colour: Color32| {
        if !lit && legends.contains(&legend) {
            FOUND
        } else {
            colour
        }
    };

    let (ink, dim) = if lit {
        (theme::ON_LIT, theme::ON_LIT)
    } else {
        (theme::INK, theme::DIM)
    };
    // The words are small enough to be legible and no larger: the key is what
    // is being read, and a keyboard covered in words is what the machine's own
    // keys looked like.
    let big = (rect.height() * 0.34).clamp(8.0, 15.0);
    let small = (rect.height() * 0.20).clamp(6.0, 9.0);

    // The word above the key sits inside it at the top, since the case around
    // the keys is not drawn.
    if !key.over.is_empty() {
        painter.text(
            egui::pos2(rect.center().x, rect.top() + 2.0),
            Align2::CENTER_TOP,
            key.over,
            FontId::proportional(small),
            found_in(key.over, if lit { theme::ON_LIT } else { theme::GREEN }),
        );
    }
    let main_at = if key.main.len() > 2 {
        // A named key — ENTER, CAPS SHIFT — has the width and not the height.
        egui::pos2(rect.center().x, rect.center().y)
    } else {
        egui::pos2(rect.left() + rect.width() * 0.3, rect.center().y)
    };
    let main_size = if key.main.len() > 5 { small } else { big };
    painter.text(
        main_at,
        Align2::CENTER_CENTER,
        key.main,
        FontId::proportional(main_size),
        found_in(key.main, ink),
    );
    if !key.word.is_empty() {
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 2.0),
            Align2::CENTER_BOTTOM,
            key.word,
            FontId::proportional(small),
            found_in(key.word, dim),
        );
    }
    if !key.sym.is_empty() {
        painter.text(
            egui::pos2(rect.right() - 3.0, rect.center().y),
            Align2::RIGHT_CENTER,
            key.sym,
            FontId::proportional(small),
            found_in(key.sym, if lit { theme::ON_LIT } else { theme::RED }),
        );
    }
}
