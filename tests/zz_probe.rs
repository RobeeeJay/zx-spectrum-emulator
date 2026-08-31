use eframe::egui;
use zx_rustrum::machine::{Model, Spectrum};
use zx_rustrum::ui::{App, Roms};

const LAST_K: u16 = 0x5C08;

#[test]
fn probe() {
    let Ok(rom3) = std::fs::read("roms/plus3.rom") else {
        return;
    };
    let Ok(rom48) = std::fs::read("roms/48.rom") else {
        return;
    };
    for (label, model) in [("+3", Model::Plus3), ("48K", Model::Spectrum48)] {
        let roms = Roms {
            rom48: Some(rom48.clone()),
            rom_plus3: Some(rom3.clone()),
            ..Roms::default()
        };
        let mut app = App::with_roms(Spectrum::new(), String::new(), roms, None);
        app.show_ram_map = false;
        app.show_debugger = false;
        app.show_back_buffer = false;
        app.show_tape = false;
        app.running = true;
        app.switch_model(model);
        let mut h = egui_kittest::Harness::builder()
            .with_size([1400.0, 900.0])
            .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
        h.run_steps(3);
        // Let it settle, then reset as the button does.
        for _ in 0..400 {
            h.run_steps(1);
        }
        h.state_mut().spec.reset();
        h.state_mut().spec.bus.poke(LAST_K, 0);
        let mut seen = None;
        for frame in 0..600u32 {
            // Pressed the way the window presses it, every frame, so the
            // machine sees it held.
            h.key_press(egui::Key::Num1);
            h.run_steps(1);
            if h.state().spec.bus.peek_raw(LAST_K) != 0 {
                seen = Some(frame);
                break;
            }
        }
        eprintln!(
            "{label} in the app: key read after {seen:?} host frames; machine frame {}",
            h.state().spec.bus.frame
        );
    }
}
