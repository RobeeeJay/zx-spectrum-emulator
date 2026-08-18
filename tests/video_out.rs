//! Writing the picture out as a video file.

use std::path::{Path, PathBuf};
use zx_rustrum::video_out::{arguments, available, Recording, FFMPEG};

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
    let args = arguments(352, 592, 50.08, Path::new("/tmp/out.mp4"));
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
    let mut recording =
        Recording::start(out.0.clone(), 16, 16, 50.0).expect("ffmpeg should have started");
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
    let mut recording =
        Recording::start(out.0.clone(), w, h, 50.0).expect("ffmpeg should have started");

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
        let started = Recording::start(out.0.clone(), 16, 16, 50.0);
        assert!(
            started.is_err(),
            "without the encoder there is nothing to start"
        );
    }
}
