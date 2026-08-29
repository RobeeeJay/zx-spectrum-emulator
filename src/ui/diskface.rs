//! The disk as a disk: the bits on it, drawn where they are.
//!
//! A ring per track, the way the tracks sit on a three-inch disk — track 0
//! outermost, because that is where the head parks and where the directory
//! lives — and the bits of each track around it, black for a nought and white
//! for a one, starting at the top and going round clockwise.
//!
//! Not every bit. A track is nine 512-byte sectors, which is 36,864 bits, and
//! a ring a few hundred pixels round cannot hold them: one bit is sampled for
//! each step around the ring, so what is drawn is the pattern of the data
//! rather than a transcript of it. The point is that a track full of $E5 looks
//! nothing like a track full of code, and an empty one nothing like either.
//!
//! Rasterised into a texture rather than drawn as shapes: fifty thousand line
//! segments a frame is not a thing to ask of a window that is also running a
//! Spectrum, and the picture only changes when the disk does.

use eframe::egui;

use crate::disk::Disk;

/// The rasterised platter, and what it was made from.
#[derive(Default)]
pub struct Platter {
    texture: Option<egui::TextureHandle>,
    /// The size it was drawn at, and the disk's revision when it was: either
    /// changing means drawing it again.
    drawn: Option<(usize, u64)>,
}

impl Platter {
    /// The picture, made again if the disk or the size has changed since.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        disk: &Disk,
        size: usize,
    ) -> &egui::TextureHandle {
        let wanted = (size, disk.revision);
        if self.drawn != Some(wanted) || self.texture.is_none() {
            let image = draw(disk, size);
            match &mut self.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ctx.load_texture("disk-platter", image, egui::TextureOptions::LINEAR))
                }
            }
            self.drawn = Some(wanted);
        }
        self.texture.as_ref().expect("just made")
    }
}

/// Where the tracks sit, as a fraction of the picture's half-width: the disk
/// itself, then the band the tracks are written in, then the hub.
const EDGE: f32 = 0.98;
const OUTER: f32 = 0.94;
const INNER: f32 = 0.30;
const HUB: f32 = 0.20;
const SPINDLE: f32 = 0.07;

/// How much of a track's width is the gap between it and the next.
///
/// Without one the rings run together and the picture is a field of noise
/// rather than forty tracks. A real disk has a guard band between tracks for
/// its own reasons, so this is not a lie about the disk either.
const GUARD: f32 = 0.32;

/// How wide a drawn bit is, in pixels.
///
/// One bit per pixel is what makes moiré: 36,864 bits round a ring a few
/// hundred pixels long beat against the pixels and come out as swirls. A bit
/// three pixels wide is a bit somebody can see.
const BIT_PIXELS: f32 = 3.0;

/// Which track a radius is in, and where across that track's own width it
/// sits — 0 at its outer edge, 1 at its inner one.
fn track_at(radius: f32, tracks: usize) -> Option<(usize, f32)> {
    if !(INNER..=OUTER).contains(&radius) || tracks == 0 {
        return None;
    }
    // Track 0 is the outermost, as it is on the disk.
    let through = (OUTER - radius) / (OUTER - INNER) * tracks as f32;
    let track = (through as usize).min(tracks - 1);
    Some((track, through - track as f32))
}

/// Rasterise the disk: black and white bits in rings, on a dark ground.
pub fn draw(disk: &Disk, size: usize) -> egui::ColorImage {
    let mut pixels = vec![egui::Color32::TRANSPARENT; size * size];
    let half = size as f32 / 2.0;
    let tracks = disk.tracks_per_side.max(1) as usize;

    // The bits of each track, laid out once: a track's sectors in the order
    // they sit on it, which is the order they are numbered in.
    let mut bits: Vec<Vec<u8>> = Vec::with_capacity(tracks);
    for track in 0..tracks {
        let bytes: Vec<u8> = match disk.track(track as u8, 0) {
            Some(track) => track
                .sectors
                .iter()
                .flat_map(|s| s.data.iter().copied())
                .collect(),
            None => Vec::new(),
        };
        bits.push(bytes);
    }

    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = (y as f32 + 0.5 - half) / half;
            let radius = (dx * dx + dy * dy).sqrt();
            let colour = if radius > EDGE {
                egui::Color32::TRANSPARENT
            } else if radius < SPINDLE {
                // The spindle hole.
                egui::Color32::from_rgb(0x08, 0x08, 0x0a)
            } else if radius < HUB {
                // The metal hub the drive grips.
                egui::Color32::from_rgb(0x50, 0x52, 0x58)
            } else {
                match track_at(radius, tracks) {
                    None => egui::Color32::from_rgb(0x1a, 0x18, 0x18),
                    // The guard band between one track and the next, which a
                    // real disk has for its own reasons and which is what
                    // makes forty rings read as forty rings.
                    Some((_, across)) if across > 1.0 - GUARD => {
                        egui::Color32::from_rgb(0x14, 0x13, 0x13)
                    }
                    Some((track, _)) => {
                        let data = &bits[track];
                        if data.is_empty() {
                            // A track the disk has not got: unformatted, and
                            // nothing is written there.
                            egui::Color32::from_rgb(0x12, 0x12, 0x14)
                        } else {
                            // Clockwise from the top, which is how a disk
                            // turns and how the sectors are numbered round it.
                            let angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
                            let turn =
                                angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                            // How many bits this ring has room for at a width
                            // somebody can see: fewer the further in it is.
                            let circumference = std::f32::consts::TAU * radius * half;
                            let steps = (circumference / BIT_PIXELS).clamp(48.0, 2048.0) as usize;
                            let step = ((turn * steps as f32) as usize).min(steps - 1);
                            // The same bit of every nth byte, rather than
                            // every nth bit. A stride that is not a whole
                            // number of bytes walks through the bits of a
                            // repeating pattern and turns it into noise: a
                            // track of the formatter's $E5 came out looking
                            // exactly like a track of code, which is the one
                            // thing this picture is for telling apart.
                            let stride = (data.len() / steps).max(1);
                            let byte = data[(step * stride).min(data.len() - 1)];
                            if byte & 0x80 != 0 {
                                egui::Color32::WHITE
                            } else {
                                egui::Color32::BLACK
                            }
                        }
                    }
                }
            };
            pixels[y * size + x] = colour;
        }
    }
    egui::ColorImage {
        size: [size, size],
        pixels,
        source_size: egui::vec2(size as f32, size as f32),
    }
}

/// Where a sector sits on the picture: the angles it covers, and the radii of
/// its track. Used to light the sectors that have just been read or written.
pub fn sector_wedge(
    tracks: usize,
    track: usize,
    sector: usize,
    sectors: usize,
) -> (std::ops::Range<f32>, std::ops::Range<f32>) {
    let band = (OUTER - INNER) / tracks.max(1) as f32;
    let outer = OUTER - band * track as f32;
    let inner = outer - band;
    let step = std::f32::consts::TAU / sectors.max(1) as f32;
    // From the top, clockwise, the same way the bits are laid down.
    let start = -std::f32::consts::FRAC_PI_2 + step * sector as f32;
    (start..start + step, inner..outer)
}
