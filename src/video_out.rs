//! Writing what the screen shows to a video file.
//!
//! The picture the emulator makes is a buffer of RGBA bytes a frame; an H.264
//! file is a great deal of arithmetic away from that, and none of it belongs in
//! an emulator. The frames are handed to `ffmpeg` instead, which is on most
//! machines that would want this and is a program rather than a dependency:
//! nothing is added to the build, and a machine without it is told so plainly
//! rather than being given a broken button.
//!
//! What goes into the file is the picture as the window shows it, effects and
//! all — the line structure, the composite colour, the dot crawl — because it
//! is the same buffer that becomes the texture. The curve of the glass is the
//! one thing that does not, since that is done by the shape the texture is
//! drawn on rather than to the pixels.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The arguments that put the sound and the picture together at the end.
///
/// Two files rather than one: ffmpeg takes the frames down its standard input,
/// which leaves nowhere for the sound to go while the recording is running. It
/// is written beside the picture as raw samples and the two are joined when the
/// recording stops, which costs a second of copying and no dropped frames.
pub fn mux_arguments(video: &Path, audio: &Path, rate: f64, to: &Path) -> Vec<String> {
    vec![
        "-y".into(),
        "-i".into(),
        video.to_string_lossy().to_string(),
        "-f".into(),
        "f32le".into(),
        "-ar".into(),
        format!("{rate:.0}"),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        audio.to_string_lossy().to_string(),
        // The picture is already encoded; only the sound needs doing.
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        "-shortest".into(),
        to.to_string_lossy().to_string(),
    ]
}

/// What to call the encoder, and what to ask it for.
pub const FFMPEG: &str = "ffmpeg";

/// What every file is written at, whatever the picture came in as.
///
/// A Spectrum's picture is 352 by 296 with the border on: nothing plays that
/// happily, and a file somebody wants to show somebody else is 1080p. The
/// picture is scaled up to fit and the rest is left black.
pub const OUT_W: usize = 1920;
pub const OUT_H: usize = 1080;

/// How big the picture is drawn inside the 1080p frame.
///
/// `pixel_aspect` is how tall a row of the buffer is against how wide a column
/// is, in the picture it stands for. It is 1 for the machine's own pixels and
/// a half with the set on, where every line of the picture is two rows of the
/// buffer — a line and the gap under it. Without that the televised picture
/// went into the file twice as tall as it should be.
///
/// Both sides come out even, since H.264 in yuv420p has no way to carry an odd
/// one.
pub fn fit(src_w: usize, src_h: usize, pixel_aspect: f64) -> (usize, usize) {
    let shown_h = (src_h as f64 * pixel_aspect).max(1.0);
    let scale = (OUT_W as f64 / src_w as f64).min(OUT_H as f64 / shown_h);
    let even = |v: f64| ((v.round() as usize).max(2)) & !1;
    (
        even(src_w as f64 * scale).min(OUT_W),
        even(shown_h * scale).min(OUT_H),
    )
}

/// The arguments that turn a stream of RGBA frames into an H.264 file.
///
/// `yuv420p` rather than anything better, because that is what every player
/// will show; `veryfast` because the emulator is running at the same time and
/// a dropped frame is worse than a larger file.
///
/// `smooth` picks how the scaling is done, and follows what the window is
/// doing: the machine's own pixels are squares and are kept as squares, and a
/// televised picture is softened, exactly as it is on screen.
pub fn arguments(
    width: usize,
    height: usize,
    pixel_aspect: f64,
    fps: f64,
    smooth: bool,
    to: &Path,
) -> Vec<String> {
    let (w, h) = fit(width, height, pixel_aspect);
    let filter = format!(
        "scale={w}:{h}:flags={},pad={OUT_W}:{OUT_H}:{}:{}:black",
        if smooth { "lanczos" } else { "neighbor" },
        (OUT_W - w) / 2,
        (OUT_H - h) / 2,
    );
    vec![
        "-y".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        "rgba".into(),
        "-video_size".into(),
        format!("{width}x{height}"),
        "-framerate".into(),
        format!("{fps:.3}"),
        "-i".into(),
        "-".into(),
        "-vf".into(),
        filter,
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "18".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        to.to_string_lossy().to_string(),
    ]
}

/// Whether the encoder is there to be used.
pub fn available() -> bool {
    Command::new(FFMPEG)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A recording in progress.
pub struct Recording {
    child: Child,
    /// Where the finished file goes.
    pub path: PathBuf,
    /// The picture on its own, until the sound is put with it.
    video_path: PathBuf,
    /// The sound, as raw samples.
    audio_path: PathBuf,
    audio: Option<std::fs::File>,
    pub sample_rate: f64,
    /// What size the frames are. A recording is one size throughout: the
    /// encoder is told the size once, and a frame of another size cannot be
    /// put into the same file.
    pub width: usize,
    pub height: usize,
    pub frames: u64,
    pub samples: u64,
    /// What went wrong, if anything has. Kept rather than thrown, so the
    /// window can say so and stop rather than the recording dying quietly.
    pub failed: Option<String>,
}

impl Recording {
    /// Start `ffmpeg` writing to `path`.
    pub fn start(
        path: PathBuf,
        width: usize,
        height: usize,
        pixel_aspect: f64,
        fps: f64,
        smooth: bool,
        sample_rate: f64,
    ) -> Result<Recording, String> {
        let video_path = beside(&path, "video.mp4");
        let audio_path = beside(&path, "audio.f32");
        let child = Command::new(FFMPEG)
            .args(arguments(
                width,
                height,
                pixel_aspect,
                fps,
                smooth,
                &video_path,
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {FFMPEG}: {e}"))?;
        let audio = std::fs::File::create(&audio_path).ok();
        Ok(Recording {
            child,
            path,
            video_path,
            audio_path,
            audio,
            sample_rate,
            width,
            height,
            frames: 0,
            samples: 0,
            failed: None,
        })
    }

    /// Hand over the sound made since the last frame.
    pub fn sound(&mut self, samples: &[f32]) {
        let Some(file) = self.audio.as_mut() else {
            return;
        };
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if file.write_all(&bytes).is_err() {
            self.audio = None;
            return;
        }
        self.samples += samples.len() as u64;
    }

    /// Hand over one frame of RGBA.
    ///
    /// A frame of the wrong size is refused rather than written: the encoder
    /// was told the size at the start, and giving it a different one would
    /// shear the picture from there on. The window switching from cropped to
    /// overscan, or the set being switched on, is what changes it.
    pub fn frame(&mut self, pixels: &[u8], width: usize, height: usize) {
        if self.failed.is_some() {
            return;
        }
        if width != self.width || height != self.height {
            self.failed = Some(format!(
                "the picture changed size ({}x{} to {width}x{height})",
                self.width, self.height
            ));
            return;
        }
        let wanted = self.width * self.height * 4;
        if pixels.len() < wanted {
            self.failed = Some("the picture was short".into());
            return;
        }
        let Some(stdin) = self.child.stdin.as_mut() else {
            self.failed = Some("the encoder is not listening".into());
            return;
        };
        if let Err(e) = stdin.write_all(&pixels[..wanted]) {
            self.failed = Some(format!("could not write a frame: {e}"));
            return;
        }
        self.frames += 1;
    }

    /// Close the picture, put the sound with it, and clean up after both.
    ///
    /// Takes `&mut self` rather than `self`: the recording holds a child
    /// process and lets go of it when it is dropped, so it cannot be taken
    /// apart by moving its fields out.
    pub fn finish(&mut self) -> Result<(PathBuf, u64), String> {
        // Dropping stdin is what tells ffmpeg the stream has ended; without
        // it, waiting for the encoder waits for ever.
        drop(self.child.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|e| format!("waiting for {FFMPEG}: {e}"))?;
        drop(self.audio.take());
        if let Some(why) = self.failed.clone() {
            self.tidy_up();
            return Err(why);
        }
        if !status.success() {
            self.tidy_up();
            return Err(format!("{FFMPEG} gave up ({status})"));
        }

        // The sound and the picture, into one file.
        if self.samples > 0 {
            let muxed = Command::new(FFMPEG)
                .args(mux_arguments(
                    &self.video_path,
                    &self.audio_path,
                    self.sample_rate,
                    &self.path,
                ))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match muxed {
                Ok(status) if status.success() => {}
                _ => {
                    // The picture on its own is better than nothing at all,
                    // and better than a file that is not there.
                    let _ = std::fs::rename(&self.video_path, &self.path);
                    self.tidy_up();
                    return Err("the sound could not be put with the picture".into());
                }
            }
        } else {
            let _ = std::fs::rename(&self.video_path, &self.path);
        }
        self.tidy_up();
        Ok((self.path.clone(), self.frames))
    }

    /// Throw away the two halves once they are one file.
    fn tidy_up(&mut self) {
        let _ = std::fs::remove_file(&self.video_path);
        let _ = std::fs::remove_file(&self.audio_path);
    }
}

/// A working file beside the one being written, with the same stem.
fn beside(path: &Path, what: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "recording".into());
    path.with_file_name(format!(".{stem}.{what}"))
}

impl Drop for Recording {
    fn drop(&mut self) {
        // A recording dropped without being finished — the application
        // closing, say — still has to let go of the encoder's input, or the
        // process is left waiting on a pipe nobody will write to again.
        drop(self.child.stdin.take());
        let _ = self.child.wait();
        drop(self.audio.take());
    }
}
