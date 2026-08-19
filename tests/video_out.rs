//! Writing the picture out as a video file.

use std::path::{Path, PathBuf};
use zx_rustrum::video_out::{
    arguments, available, fit, mux_arguments, Recording, FFMPEG, OUT_H, OUT_W,
};

/// A scratch file that cleans up after itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "zx-video-{name}-{}-{:?}.mp4",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The encoder is told what it is being given and what to make of it: raw
/// RGBA at the picture's size and the machine's frame rate, out as H.264 in
/// the pixel format every player will show.
#[test]
fn the_encoder_is_told_what_it_is_being_given() {
    let args = arguments(352, 592, 0.5, 50.08, true, Path::new("/tmp/out.mp4"));
    let joined = args.join(" ");
    assert!(
        joined.contains("-f rawvideo") && joined.contains("-pixel_format rgba"),
        "raw frames in: {joined}"
    );
    assert!(
        joined.contains("-video_size 352x592"),
        "at the size the picture is, doubled in height by the set: {joined}"
    );
    assert!(
        joined.contains(&format!("pad={OUT_W}:{OUT_H}")),
        "and out at 1080p whatever came in: {joined}"
    );
    assert!(
        joined.contains("-framerate 50.080"),
        "and the machine's own frame rate, not a round fifty: {joined}"
    );
    assert!(
        joined.contains("-c:v libx264") && joined.contains("-pix_fmt yuv420p"),
        "H.264 out, in what every player will show: {joined}"
    );
    assert_eq!(
        args.last().map(String::as_str),
        Some("/tmp/out.mp4"),
        "and the file last, as ffmpeg takes it"
    );
}

/// A frame of the wrong size is refused rather than written.
///
/// The encoder was told the size once, at the start; a frame of another size
/// would shear the picture from there on. Switching the set on, or the view
/// from cropped to overscan, is what changes it.
#[test]
fn a_frame_of_the_wrong_size_stops_the_recording() {
    if !available() {
        eprintln!("no {FFMPEG} on the path; skipping");
        return;
    }
    let out = Scratch::new("wrong-size");
    let mut recording = Recording::start(out.0.clone(), 16, 16, 1.0, 50.0, false, 48_000.0)
        .expect("ffmpeg should have started");
    let frame = vec![0u8; 16 * 16 * 4];
    recording.frame(&frame, 16, 16);
    assert_eq!(recording.frames, 1, "the right size goes in");

    recording.frame(&frame, 16, 32);
    assert!(
        recording
            .failed
            .as_deref()
            .is_some_and(|why| why.contains("changed size")),
        "and the wrong size says so: {:?}",
        recording.failed
    );
    assert_eq!(recording.frames, 1, "rather than being written anyway");
}

/// Frames go in and a playable file comes out.
#[test]
fn what_goes_in_comes_out_as_a_file() {
    if !available() {
        eprintln!("no {FFMPEG} on the path; skipping");
        return;
    }
    let out = Scratch::new("frames");
    let (w, h) = (64usize, 48usize);
    let mut recording = Recording::start(out.0.clone(), w, h, 1.0, 50.0, false, 48_000.0)
        .expect("ffmpeg should have started");

    // Ten frames of a moving band, so the file has something to compress.
    for frame in 0..10u8 {
        let mut pixels = vec![0u8; w * h * 4];
        for (i, pixel) in pixels.chunks_mut(4).enumerate() {
            let y = (i / w) as u8;
            pixel[0] = y.wrapping_add(frame * 8);
            pixel[1] = 0x40;
            pixel[2] = 0x80;
            pixel[3] = 255;
        }
        recording.frame(&pixels, w, h);
    }
    assert_eq!(recording.frames, 10);

    let (path, frames) = recording.finish().expect("the encoder should be happy");
    assert_eq!(frames, 10);
    let written = std::fs::metadata(&path).expect("a file should have been written");
    assert!(written.len() > 0, "and it should have something in it");
}

/// Nothing about this is in the build: the encoder is a program that may or
/// may not be there, and a machine without it is told rather than given a
/// button that does nothing.
#[test]
fn the_encoder_is_looked_for_rather_than_assumed() {
    // Whichever way this machine answers, it answers without panicking, and
    // starting a recording without it is an error rather than a crash.
    let there = available();
    if !there {
        let out = Scratch::new("missing");
        let started = Recording::start(out.0.clone(), 16, 16, 1.0, 50.0, false, 48_000.0);
        assert!(
            started.is_err(),
            "without the encoder there is nothing to start"
        );
    }
}

/// Every file is 1080p, and the picture inside it keeps its shape.
///
/// With the set on the buffer is twice as tall as the picture it stands for —
/// every line is a line and the gap under it — and written as though those
/// rows were square, the picture went into the file twice as tall as it
/// should be.
#[test]
fn the_picture_keeps_its_shape_at_1080p() {
    // The machine's own pixels: 352 by 296 with the border on.
    let plain = fit(352, 296, 1.0);
    assert_eq!(plain.1, OUT_H, "as tall as the frame");
    assert!(plain.0 <= OUT_W, "and no wider than it");
    let shape = plain.0 as f64 / plain.1 as f64;
    assert!(
        (shape - 352.0 / 296.0).abs() < 0.01,
        "the picture's own shape: {shape:.3}"
    );

    // The same picture with the set on, which is twice as many rows.
    let televised = fit(352, 592, 0.5);
    assert_eq!(
        televised, plain,
        "the televised picture is the same shape, not twice as tall"
    );

    // A frame wide enough to be limited by the width instead is fitted the
    // other way about.
    let wide = fit(1000, 100, 1.0);
    assert_eq!(wide.0, OUT_W, "wide pictures are limited by the width");
    assert!(wide.1 < OUT_H);

    // Both sides even, since yuv420p cannot carry an odd one.
    for (w, h) in [plain, televised, wide, fit(255, 191, 1.0)] {
        assert_eq!((w % 2, h % 2), (0, 0), "{w}x{h} should be even");
    }
}

/// How the scaling is done follows what the window is doing: the machine's own
/// pixels stay squares, and a televised picture is softened, as it is on
/// screen.
#[test]
fn the_scaling_matches_what_the_window_does() {
    let sharp = arguments(352, 296, 1.0, 50.0, false, Path::new("/tmp/a.mp4")).join(" ");
    assert!(sharp.contains("flags=neighbor"), "square pixels: {sharp}");
    let soft = arguments(352, 592, 0.5, 50.0, true, Path::new("/tmp/b.mp4")).join(" ");
    assert!(
        soft.contains("flags=lanczos"),
        "a tube has no edges: {soft}"
    );
}

/// What the window tells the encoder follows what the window is showing.
#[test]
fn the_window_describes_its_own_picture() {
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};
    use zx_rustrum::video_out::fit;

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.scale = 2.0;

    let (w, h, aspect, fps, smooth) = app.video_settings();
    assert_eq!(aspect, 1.0, "the machine's own pixels are square");
    assert!(!smooth, "and are kept as squares");
    assert!(
        (fps - 50.08).abs() < 0.01,
        "the machine's own frame rate: {fps}"
    );

    // With the set on, twice the rows and each standing for half the height.
    app.crt = true;
    let (cw, ch, caspect, _, csmooth) = app.video_settings();
    assert_eq!(cw, w, "the same width");
    assert_eq!(ch, h * 2, "twice the rows: a line and the gap under it");
    assert_eq!(caspect, 0.5, "each standing for half as much height");
    assert!(csmooth, "and softened, as a tube has no pixel edges");

    // So both come out the same shape in the file.
    assert_eq!(
        fit(w, h, aspect),
        fit(cw, ch, caspect),
        "the picture is the same shape with the set on as without it"
    );
}

/// The sound goes into the file with the picture.
///
/// ffmpeg takes the frames down its standard input, which leaves nowhere for
/// the sound to go while the recording is running: it is written beside the
/// picture as raw samples and the two are joined when the recording stops.
#[test]
fn the_sound_is_put_with_the_picture() {
    let args = mux_arguments(
        Path::new("/tmp/.a.video.mp4"),
        Path::new("/tmp/.a.audio.f32"),
        48_000.0,
        Path::new("/tmp/a.mp4"),
    );
    let joined = args.join(" ");
    assert!(
        joined.contains("-f f32le") && joined.contains("-ar 48000") && joined.contains("-ac 1"),
        "the samples as they were written: {joined}"
    );
    assert!(
        joined.contains("-c:v copy"),
        "the picture is already encoded and is not encoded again: {joined}"
    );
    assert!(joined.contains("-c:a aac"), "and the sound is: {joined}");
}

/// A recording with sound in it comes out as one file with both, and the
/// working files it was made from are cleaned up.
#[test]
fn a_recording_with_sound_leaves_one_file_behind() {
    if !available() {
        eprintln!("no {FFMPEG} on the path; skipping");
        return;
    }
    let out = Scratch::new("with-sound");
    let (w, h) = (32usize, 24usize);
    let mut recording = Recording::start(out.0.clone(), w, h, 1.0, 50.0, false, 48_000.0)
        .expect("ffmpeg should have started");
    let pixels = vec![0x40u8; w * h * 4];
    // A fifth of a second: ten frames and the samples that go with them.
    for frame in 0..10 {
        recording.frame(&pixels, w, h);
        let samples: Vec<f32> = (0..960)
            .map(|i| ((frame * 960 + i) as f32 / 40.0).sin() * 0.5)
            .collect();
        recording.sound(&samples);
    }
    assert_eq!(recording.samples, 9600, "a fifth of a second of sound");

    let (path, frames) = recording.finish().expect("the encoder should be happy");
    assert_eq!(frames, 10);
    assert!(
        std::fs::metadata(&path).is_ok_and(|m| m.len() > 0),
        "one file, with something in it"
    );
    // And nothing left beside it.
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    for working in [
        path.with_file_name(format!(".{stem}.video.mp4")),
        path.with_file_name(format!(".{stem}.audio.f32")),
    ] {
        assert!(
            !working.exists(),
            "the working file should have been cleaned up: {}",
            working.display()
        );
    }
}

/// A second of the file is a second of the machine, whatever the window is
/// doing.
///
/// The window repaints when the window system says so — sixty times a second
/// on this screen — and the machine draws fifty. Writing a frame per repaint
/// put sixty frames in the file for every fifty the machine drew and declared
/// them as fifty, so everything in it happened a fifth too slowly.
#[test]
fn the_file_gets_one_frame_per_machine_frame() {
    use zx_rustrum::ui::App;

    // Repainting faster than the machine draws: most repaints owe nothing.
    let mut due = 0.0;
    let mut written = 0u32;
    for _ in 0..60 {
        due += 50.0 / 60.0;
        let owed = App::video_frames_owed(due);
        due -= owed as f64;
        written += owed;
    }
    assert_eq!(
        written, 50,
        "sixty repaints of a machine drawing fifty frames is fifty frames"
    );

    // Repainting slower than it draws: a repaint can owe more than one.
    let mut due = 0.0;
    let mut written = 0u32;
    for _ in 0..25 {
        due += 50.0 / 25.0;
        let owed = App::video_frames_owed(due);
        due -= owed as f64;
        written += owed;
    }
    assert_eq!(written, 50, "and half as many repaints is still fifty");

    // A machine run flat out draws thousands, and the file is of what the
    // window showed rather than of every frame the machine got through.
    assert_eq!(App::video_frames_owed(2400.0), 4, "capped");
}

/// The sound written to the file is the sound the machine made.
///
/// ffmpeg's standard input is carrying the frames, so the samples cannot go
/// down it: they are kept as they are produced and written beside the picture.
/// Kept only while something is recording — the rest of the time it is a
/// buffer nobody reads.
#[test]
fn the_samples_are_kept_while_a_recording_is_running() {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use zx_rustrum::machine::Spectrum;

    let mut spec = Spectrum::new();
    spec.bus
        .audio
        .attach(Arc::new(Mutex::new(VecDeque::new())), 48_000.0);
    spec.bus.audio.volume = 1.0;

    // Nothing is kept until something asks for it.
    assert!(spec.bus.audio.tap.is_none(), "no tap to start with");
    for _ in 0..2 {
        spec.run(spec.bus.frame_t());
        spec.bus.audio_sync();
    }

    spec.bus.audio.tap = Some(Vec::new());
    // A frame of the beeper being hit, which is what a loading tone is.
    for step in 0..200 {
        spec.bus.audio.beeper = if step % 2 == 0 { 0.5 } else { 0.0 };
        spec.run(spec.bus.frame_t() / 200);
        spec.bus.audio_sync();
    }

    let kept = spec.bus.audio.tap.as_ref().expect("the tap is on");
    assert!(
        kept.len() > 800,
        "a frame of sound at 48kHz is a thousand samples: {}",
        kept.len()
    );
    assert!(
        kept.iter().any(|s| s.abs() > 0.01),
        "and the beeper should be in them"
    );
}

/// The switches that change the picture's size are held while a video is
/// being written, and the one that does not is left alone.
///
/// The encoder is told the frame size once, when the pipe opens, and slices a
/// headerless stream of bytes into frames by that number: a frame of another
/// size shears the picture from there on. The refusal in the recorder catches
/// it, but a switch that stops a recording is a worse thing to offer than one
/// that waits.
#[test]
fn the_switches_that_change_the_size_are_held_while_recording() {
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;
    use zx_rustrum::machine::Spectrum;
    use zx_rustrum::ui::{App, Roms};

    if !available() {
        eprintln!("no {FFMPEG} on the path; skipping");
        return;
    }

    let mut app = App::with_roms(Spectrum::new(), String::new(), Roms::default(), None);
    app.show_ram_map = false;
    app.show_debugger = false;
    app.show_back_buffer = false;
    app.show_tape = false;
    app.running = false;
    let mut h: Harness<'_, App> = Harness::builder()
        .with_size([1800.0, 900.0])
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(3);

    let disabled = |h: &Harness<'_, App>, label: &str| -> bool {
        h.get_by_label(label).accesskit_node().is_disabled()
    };
    assert!(!h.state().video_switches_held(), "nothing recording yet");
    for label in ["CRT", "Overscan"] {
        assert!(!disabled(&h, label), "{label} should be there to press");
    }

    // Recording: the three that decide the frame's size are held.
    let out = Scratch::new("held");
    let (w, h_px, aspect, fps, smooth) = h.state().video_settings();
    h.state_mut().video = Some(
        Recording::start(out.0.clone(), w, h_px, aspect, fps, smooth, 48_000.0)
            .expect("ffmpeg should have started"),
    );
    h.run_steps(3);
    assert!(h.state().video_switches_held());
    for label in ["CRT", "Overscan"] {
        assert!(
            disabled(&h, label),
            "{label} changes the picture's size and should be held"
        );
    }
    // Composite changes what the pixels are, not how many, so it stays live.
    assert!(
        !disabled(&h, "Composite") || !h.state().crt,
        "Composite is not one of the three: it is only greyed out because the \
         set is off"
    );

    // Stopped, and they are back.
    h.state_mut().stop_video();
    h.run_steps(3);
    assert!(!h.state().video_switches_held());
    for label in ["CRT", "Overscan"] {
        assert!(
            !disabled(&h, label),
            "{label} should be back after stopping"
        );
    }
}
