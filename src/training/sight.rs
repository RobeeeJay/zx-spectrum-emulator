//! What the network sees: the picture, and only the picture.
//!
//! Built from `screen::render`, which draws what the ULA painted — the frame
//! a player saw, racing-the-beam effects and all — never from the display
//! file or anything else in memory. That is the rule the whole of training is
//! built around, and this is the one place it is kept.

use crate::machine::Spectrum;
use crate::screen::View;

/// How much of the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    /// The 256x192 display, no border.
    Display,
    /// A television's worth of border: 304x240.
    Television,
    /// All the border there is: 384x304.
    Overscan,
}

/// How the network sees a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sight {
    pub area: Area,
    /// 1, 2 or 4: the picture is averaged down by this much each way.
    pub shrink: usize,
    /// Red, green and blue, or one channel of brightness.
    pub colour: bool,
    /// How many frames, most recent last: one frame cannot say which way
    /// anything is moving.
    pub frames: usize,
}

impl Default for Sight {
    fn default() -> Self {
        Sight {
            area: Area::Display,
            shrink: 2,
            colour: false,
            frames: 4,
        }
    }
}

impl Sight {
    fn full(&self) -> (usize, usize) {
        match self.area {
            Area::Display => (256, 192),
            Area::Television => (View::CROPPED.width(), View::CROPPED.height()),
            Area::Overscan => (View::OVERSCAN.width(), View::OVERSCAN.height()),
        }
    }
    pub fn width(&self) -> usize {
        self.full().0 / self.shrink.max(1)
    }
    pub fn height(&self) -> usize {
        self.full().1 / self.shrink.max(1)
    }
    pub fn channels(&self) -> usize {
        if self.colour {
            3
        } else {
            1
        }
    }
    /// Bytes in one frame as the network sees it.
    pub fn frame_len(&self) -> usize {
        self.channels() * self.width() * self.height()
    }
    /// Bytes in a whole observation: the frames stacked.
    pub fn len(&self) -> usize {
        self.frame_len() * self.frames.max(1)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One frame as the network sees it, channel by channel, row by row.
pub fn look(spec: &Spectrum, sight: &Sight) -> Vec<u8> {
    let (view, crop) = match sight.area {
        Area::Display => (View::OVERSCAN, true),
        Area::Television => (View::CROPPED, false),
        Area::Overscan => (View::OVERSCAN, false),
    };
    let mut rgba = vec![0u8; view.buffer_len()];
    let flash_on = (spec.bus.frame / 16) % 2 == 1;
    crate::screen::render(&spec.bus, view, &mut rgba, flash_on);
    let (ox, oy) = if crop {
        (view.border_x, view.border_top)
    } else {
        (0, 0)
    };
    let (w, h, s) = (sight.width(), sight.height(), sight.shrink.max(1));
    let stride = view.width();
    let channels = sight.channels();
    let mut out = vec![0u8; sight.frame_len()];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 3];
            for dy in 0..s {
                for dx in 0..s {
                    let i = ((oy + y * s + dy) * stride + ox + x * s + dx) * 4;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += rgba[i + c] as u32;
                    }
                }
            }
            let n = (s * s) as u32;
            let [r, g, b] = sum.map(|t| t / n);
            if channels == 3 {
                for (c, v) in [r, g, b].into_iter().enumerate() {
                    out[c * w * h + y * w + x] = v as u8;
                }
            } else {
                out[y * w + x] = ((r * 299 + g * 587 + b * 114) / 1000) as u8;
            }
        }
    }
    out
}
