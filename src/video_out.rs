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

/// What to call the encoder, and what to ask it for.
pub const FFMPEG: &str = "ffmpeg";

/// The arguments that turn a stream of RGBA frames into an H.264 file.
///
/// `yuv420p` rather than anything better, because that is what every player
/// will show; `veryfast` because the emulator is running at the same time and
/// a dropped frame is worse than a larger file.
pub fn arguments(width: usize, height: usize, fps: f64, to: &Path) -> Vec<String> {
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
    pub path: PathBuf,
    /// What size the frames are. A recording is one size throughout: the
    /// encoder is told the size once, and a frame of another size cannot be
    /// put into the same file.
    pub width: usize,
    pub height: usize,
    pub frames: u64,
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
        fps: f64,
    ) -> Result<Recording, String> {
        let child = Command::new(FFMPEG)
            .args(arguments(width, height, fps, &path))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {FFMPEG}: {e}"))?;
        Ok(Recording {
            child,
            path,
            width,
            height,
            frames: 0,
            failed: None,
        })
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

    /// Close the file and wait for the encoder to finish with it.
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
        if let Some(why) = self.failed.clone() {
            return Err(why);
        }
        if !status.success() {
            return Err(format!("{FFMPEG} gave up ({status})"));
        }
        Ok((self.path.clone(), self.frames))
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        // A recording dropped without being finished — the application
        // closing, say — still has to let go of the encoder's input, or the
        // process is left waiting on a pipe nobody will write to again.
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}
