//! The tube: the curve in the glass, and the picture drawn onto it.
//!
//! A television's screen is part of a sphere, so straight lines bow outwards
//! and the corners sit further from the middle than the edges do. The picture
//! is drawn as a grid of quads with the curve applied to their corners rather
//! than to the pixels, which costs nothing and keeps the texture sampling
//! honest: the pixels are still the ULA's, they are just in a different place.

use egui::{Color32, Mesh, Pos2, Rect, TextureId, Vec2};

/// How far the glass bulges. The fraction of half the screen's width that a
/// corner is pushed out by, which on a set of the period is a few per cent.
pub const CURVE: f32 = 0.055;

/// How many quads across and down. Enough that the curve reads as a curve
/// rather than as a fan of triangles, and few enough to cost nothing.
const GRID: usize = 24;

/// Where a point of the flat picture lands on the curved one.
///
/// `at` is in -1..1 from the middle of the screen. The further out a point is,
/// the further out it goes: the classic barrel, which is what a sphere looks
/// like when it is drawn on a flat window.
pub fn bulge(at: Vec2, curve: f32) -> Vec2 {
    let r2 = at.x * at.x + at.y * at.y;
    at * (1.0 + curve * r2)
}

/// The picture as a mesh with the curve in it.
///
/// The rectangle is what the flat picture would have taken; the mesh fills a
/// little more than that at the corners, which is what a tube does.
pub fn curved(tex: TextureId, rect: Rect, curve: f32) -> Mesh {
    let mut mesh = Mesh::with_texture(tex);
    let half = rect.size() * 0.5;
    let middle = rect.center();
    for row in 0..=GRID {
        for col in 0..=GRID {
            let u = col as f32 / GRID as f32;
            let v = row as f32 / GRID as f32;
            let at = Vec2::new(u * 2.0 - 1.0, v * 2.0 - 1.0);
            let out = bulge(at, curve);
            mesh.colored_vertex(
                middle + Vec2::new(out.x * half.x, out.y * half.y),
                Color32::WHITE,
            );
            let last = mesh.vertices.len() - 1;
            mesh.vertices[last].uv = Pos2::new(u, v);
        }
    }
    let stride = GRID + 1;
    for row in 0..GRID {
        for col in 0..GRID {
            let i = (row * stride + col) as u32;
            let stride = stride as u32;
            mesh.add_triangle(i, i + 1, i + stride);
            mesh.add_triangle(i + 1, i + stride + 1, i + stride);
        }
    }
    mesh
}

/// The rectangle the curved picture actually covers, corners included.
///
/// What the bezel is drawn around: a bezel drawn to the flat picture would
/// have the bulge poking out through it.
pub fn covered(rect: Rect, curve: f32) -> Rect {
    let half = rect.size() * 0.5;
    let corner = bulge(Vec2::new(1.0, 1.0), curve);
    Rect::from_center_size(
        rect.center(),
        Vec2::new(half.x * corner.x * 2.0, half.y * corner.y * 2.0),
    )
}
