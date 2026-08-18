//! The tube: the curve in the glass, and the picture drawn onto it.
//!
//! A television's screen is part of a sphere, so a straight line drawn across
//! it bows outwards: the middle of each edge stands further out than the
//! corners, which are tucked back. What that looks like from in front is the
//! picture swelling in the middle — a barrel, not a pincushion.
//!
//! It is done by moving where the picture is *sampled from* rather than where
//! its corners are drawn. The quads stay in the rectangle the picture was
//! going to fill, and each one reads from a place pulled towards the middle by
//! the square of its distance from it, which is the same arithmetic a lens
//! does. Moving the corners instead — the first way this was written — pushes
//! them out past the edges and gives a pincushion, which is the shape a badly
//! adjusted monitor had and not the shape of a tube.

use egui::{Color32, Mesh, Pos2, Rect, TextureId, Vec2};

/// How far the glass bulges: how much of itself the middle of the picture
/// swells by, which on a set of the period is a few per cent.
pub const CURVE: f32 = 0.06;

/// How many quads across and down. Enough that the curve reads as a curve
/// rather than as a fan of triangles, and few enough to cost nothing.
const GRID: usize = 24;

/// Where a point of the picture is read from, in -1..1 from the middle.
///
/// The further out the point, the further out it reaches for its colour, so
/// what comes back is squeezed towards the edges and swollen in the middle.
/// The whole thing is then pulled in by however far the corners overshot, so
/// nothing is read from outside the picture: a tube shows all of the picture,
/// not the wall behind it.
pub fn sample_at(at: Vec2, curve: f32) -> Vec2 {
    let r2 = at.x * at.x + at.y * at.y;
    let corner = 1.0 + curve * 2.0;
    at * (1.0 + curve * r2) / corner
}

/// The picture as a mesh with the curve in it.
pub fn curved(tex: TextureId, rect: Rect, curve: f32) -> Mesh {
    let mut mesh = Mesh::with_texture(tex);
    for row in 0..=GRID {
        for col in 0..=GRID {
            let u = col as f32 / GRID as f32;
            let v = row as f32 / GRID as f32;
            let at = Vec2::new(u * 2.0 - 1.0, v * 2.0 - 1.0);
            let from = sample_at(at, curve);
            mesh.colored_vertex(
                Pos2::new(
                    rect.left() + u * rect.width(),
                    rect.top() + v * rect.height(),
                ),
                Color32::WHITE,
            );
            let last = mesh.vertices.len() - 1;
            mesh.vertices[last].uv = Pos2::new(from.x * 0.5 + 0.5, from.y * 0.5 + 0.5);
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
