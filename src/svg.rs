//! A small SVG renderer, enough for the artwork in `designs/`.
//!
//! Nothing here is a general SVG implementation. It covers what the drawings
//! actually use — groups with matrix transforms, paths of straight lines and
//! cubic curves, rectangles, circles, solid and linear-gradient fills, and
//! strokes — and refuses to grow beyond that. The alternative was translating
//! the artwork into drawing code by hand, which meant it drifted from the
//! files every time they were re-exported.
//!
//! Shapes are flattened to polygons and filled by scanning them, sampling four
//! sub-rows per pixel and working out the exact horizontal spans, which is
//! enough anti-aliasing for artwork shown at a few hundred pixels across.

use std::collections::HashMap;

/// A rendered image, as RGBA rows from the top.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// An affine transform, in the same order as SVG's `matrix(a,b,c,d,e,f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn scale(sx: f32, sy: f32) -> Transform {
        Transform {
            a: sx,
            d: sy,
            ..Transform::IDENTITY
        }
    }

    pub fn translate(x: f32, y: f32) -> Transform {
        Transform {
            e: x,
            f: y,
            ..Transform::IDENTITY
        }
    }

    /// Turn by `radians` about a point.
    pub fn rotate_about(radians: f32, x: f32, y: f32) -> Transform {
        let (sin, cos) = radians.sin_cos();
        Transform::translate(x, y)
            .then(Transform {
                a: cos,
                b: sin,
                c: -sin,
                d: cos,
                e: 0.0,
                f: 0.0,
            })
            .then(Transform::translate(-x, -y))
    }

    /// Grow about a point, for a reel winding on.
    pub fn scale_about(factor: f32, x: f32, y: f32) -> Transform {
        Transform::translate(x, y)
            .then(Transform::scale(factor, factor))
            .then(Transform::translate(-x, -y))
    }

    /// `self` first, then `inner` — the order attributes nest in.
    pub fn then(self, inner: Transform) -> Transform {
        Transform {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// How much this transform scales lengths, on average: stroke widths are
    /// in the space they were written in.
    fn length_scale(&self) -> f32 {
        ((self.a * self.d - self.b * self.c).abs()).sqrt()
    }
}

/// Render `src` at `width` x `height`, mapping the document's viewBox onto it.
///
/// Groups whose `id` is in `skip` are left out, and `root` is applied before
/// everything else — which is how a reel is grown or a cog turned without
/// touching the file.
pub fn render(src: &str, width: usize, height: usize, skip: &[&str], root: Transform) -> Image {
    let mut canvas = Canvas::new(width, height);
    let view = view_box(src).unwrap_or((0.0, 0.0, width as f32, height as f32));
    let fit = Transform::scale(width as f32 / view.2, height as f32 / view.3)
        .then(Transform::translate(-view.0, -view.1))
        .then(root);
    let gradients = gradients(src);
    let mut state = vec![State {
        transform: fit,
        fill: Some(Paint::Solid([0, 0, 0, 255])),
        stroke: None,
        stroke_width: 1.0,
    }];
    walk(src, &mut canvas, &mut state, &gradients, skip);
    canvas.into_image()
}

/// The document's `viewBox`, as (x, y, width, height).
pub fn view_box(src: &str) -> Option<(f32, f32, f32, f32)> {
    let value = attribute(&tag_after(src, "<svg")?, "viewBox")?;
    let n: Vec<f32> = value
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
}

// ---------------------------------------------------------------------------
// painting
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Paint {
    Solid([u8; 4]),
    /// A linear gradient: the two ends in user space, and the stops along it.
    Linear {
        from: (f32, f32),
        to: (f32, f32),
        stops: Vec<(f32, [u8; 4])>,
    },
}

impl Paint {
    fn at(&self, x: f32, y: f32) -> [u8; 4] {
        match self {
            Paint::Solid(c) => *c,
            Paint::Linear { from, to, stops } => {
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let len2 = dx * dx + dy * dy;
                let t = if len2 == 0.0 {
                    0.0
                } else {
                    (((x - from.0) * dx + (y - from.1) * dy) / len2).clamp(0.0, 1.0)
                };
                let mut last = stops.first().copied().unwrap_or((0.0, [0, 0, 0, 255]));
                for stop in stops {
                    if t <= stop.0 {
                        let span = stop.0 - last.0;
                        let k = if span <= 0.0 {
                            0.0
                        } else {
                            (t - last.0) / span
                        };
                        let mut out = [0u8; 4];
                        for (i, channel) in out.iter_mut().enumerate() {
                            *channel = (last.1[i] as f32
                                + (stop.1[i] as f32 - last.1[i] as f32) * k)
                                as u8;
                        }
                        return out;
                    }
                    last = *stop;
                }
                last.1
            }
        }
    }
}

#[derive(Clone)]
struct State {
    transform: Transform,
    fill: Option<Paint>,
    stroke: Option<Paint>,
    stroke_width: f32,
}

struct Canvas {
    width: usize,
    height: usize,
    rgba: Vec<f32>,
    /// Coverage of the shape being drawn, one entry per pixel.
    coverage: Vec<f32>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Canvas {
        Canvas {
            width,
            height,
            rgba: vec![0.0; width * height * 4],
            coverage: vec![0.0; width * height],
        }
    }

    /// Fill a set of closed polygons, even-odd, in the given paint.
    fn fill(&mut self, polygons: &[Vec<(f32, f32)>], paint: &Paint, even_odd: bool) {
        self.coverage.iter_mut().for_each(|c| *c = 0.0);
        scan(
            polygons,
            self.width,
            self.height,
            even_odd,
            &mut self.coverage,
        );
        self.blend(paint);
    }

    /// Blend the coverage buffer into the image.
    fn blend(&mut self, paint: &Paint) {
        for y in 0..self.height {
            for x in 0..self.width {
                let c = self.coverage[y * self.width + x];
                if c <= 0.001 {
                    continue;
                }
                let src = paint.at(x as f32 + 0.5, y as f32 + 0.5);
                let alpha = (src[3] as f32 / 255.0) * c.min(1.0);
                let i = (y * self.width + x) * 4;
                for (k, channel) in src.iter().take(3).enumerate() {
                    self.rgba[i + k] = self.rgba[i + k] * (1.0 - alpha) + *channel as f32 * alpha;
                }
                self.rgba[i + 3] = self.rgba[i + 3] * (1.0 - alpha) + 255.0 * alpha;
            }
        }
    }

    fn into_image(self) -> Image {
        Image {
            width: self.width,
            height: self.height,
            rgba: self
                .rgba
                .iter()
                .map(|v| v.round().clamp(0.0, 255.0) as u8)
                .collect(),
        }
    }
}

/// Work out how much of each pixel a set of polygons covers.
fn scan(
    polygons: &[Vec<(f32, f32)>],
    width: usize,
    height: usize,
    even_odd: bool,
    coverage: &mut [f32],
) {
    /// Sub-rows sampled per pixel. Horizontal coverage is exact, so this only
    /// has to smooth the vertical direction.
    const SUB: usize = 4;

    let mut edges: Vec<((f32, f32), (f32, f32))> = Vec::new();
    for poly in polygons {
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            if a.1 != b.1 {
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return;
    }

    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for row in 0..height {
        for sub in 0..SUB {
            let y = row as f32 + (sub as f32 + 0.5) / SUB as f32;
            crossings.clear();
            for (a, b) in &edges {
                let (top, bottom, dir) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
                if y < top.1 || y >= bottom.1 {
                    continue;
                }
                let t = (y - top.1) / (bottom.1 - top.1);
                crossings.push((top.0 + (bottom.0 - top.0) * t, dir));
            }
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));

            let mut winding = 0;
            for i in 0..crossings.len().saturating_sub(1) {
                winding += if even_odd { 1 } else { crossings[i].1 };
                let inside = if even_odd {
                    winding % 2 != 0
                } else {
                    winding != 0
                };
                if !inside {
                    continue;
                }
                span(
                    crossings[i].0,
                    crossings[i + 1].0,
                    row,
                    width,
                    1.0 / SUB as f32,
                    coverage,
                );
            }
        }
    }
}

/// Add coverage for one horizontal span, with partial pixels at each end.
fn span(x0: f32, x1: f32, row: usize, width: usize, weight: f32, coverage: &mut [f32]) {
    let (x0, x1) = (x0.max(0.0), x1.min(width as f32));
    if x1 <= x0 {
        return;
    }
    let first = x0.floor() as usize;
    let last = ((x1.ceil() as usize).min(width)).saturating_sub(1);
    for x in first..=last.min(width - 1) {
        let left = (x as f32).max(x0);
        let right = ((x + 1) as f32).min(x1);
        if right > left {
            coverage[row * width + x] += (right - left) * weight;
        }
    }
}

// ---------------------------------------------------------------------------
// walking the document
// ---------------------------------------------------------------------------

fn walk(
    src: &str,
    canvas: &mut Canvas,
    stack: &mut Vec<State>,
    gradients: &HashMap<String, Paint>,
    skip: &[&str],
) {
    let bytes = src.as_bytes();
    let mut i = 0;
    // Depth of a group being skipped, if any.
    let mut skipping: Option<usize> = None;
    let mut depth = 0usize;

    while i < bytes.len() {
        let Some(open) = src[i..].find('<').map(|p| p + i) else {
            break;
        };
        let Some(close) = src[open..].find('>').map(|p| p + open) else {
            break;
        };
        let tag = &src[open..=close];
        i = close + 1;

        if tag.starts_with("</") {
            if tag.starts_with("</g") {
                depth = depth.saturating_sub(1);
                if skipping == Some(depth) {
                    skipping = None;
                }
                if skipping.is_none() && stack.len() > 1 {
                    stack.pop();
                }
            }
            continue;
        }
        if tag.starts_with("<?") || tag.starts_with("<!") {
            continue;
        }

        let self_closing = tag.ends_with("/>");
        let name = tag_name(tag);

        if name == "g" {
            if skipping.is_none() {
                if attribute(tag, "id").is_some_and(|id| skip.contains(&id.as_str())) {
                    skipping = Some(depth);
                } else {
                    let mut next = stack.last().expect("a state is always present").clone();
                    apply_attributes(tag, &mut next, gradients);
                    stack.push(next);
                }
            }
            depth += 1;
            if self_closing {
                depth -= 1;
                if skipping == Some(depth) {
                    skipping = None;
                } else if stack.len() > 1 {
                    stack.pop();
                }
            }
            continue;
        }
        if skipping.is_some() {
            continue;
        }

        let mut state = stack.last().expect("a state is always present").clone();
        apply_attributes(tag, &mut state, gradients);
        match name.as_str() {
            "path" => {
                if let Some(d) = attribute(tag, "d") {
                    draw(canvas, &subpaths(&d, &state.transform), &state, tag);
                }
            }
            "rect" => {
                let n = |k: &str| attribute(tag, k).and_then(|v| v.parse::<f32>().ok());
                let (x, y) = (n("x").unwrap_or(0.0), n("y").unwrap_or(0.0));
                let (w, h) = (n("width").unwrap_or(0.0), n("height").unwrap_or(0.0));
                let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                let poly = corners
                    .iter()
                    .map(|(px, py)| state.transform.apply(*px, *py))
                    .collect();
                draw(canvas, &[poly], &state, tag);
            }
            "circle" => {
                let n = |k: &str| attribute(tag, k).and_then(|v| v.parse::<f32>().ok());
                let (cx, cy, r) = (
                    n("cx").unwrap_or(0.0),
                    n("cy").unwrap_or(0.0),
                    n("r").unwrap_or(0.0),
                );
                let steps = 64;
                let poly = (0..steps)
                    .map(|k| {
                        let a = k as f32 / steps as f32 * std::f32::consts::TAU;
                        state.transform.apply(cx + r * a.cos(), cy + r * a.sin())
                    })
                    .collect();
                draw(canvas, &[poly], &state, tag);
            }
            _ => {}
        }
    }
}

/// Fill and then stroke one shape.
fn draw(canvas: &mut Canvas, polygons: &[Vec<(f32, f32)>], state: &State, tag: &str) {
    if polygons.is_empty() {
        return;
    }
    if let Some(paint) = &state.fill {
        let even_odd = style(tag, "fill-rule")
            .or_else(|| attribute(tag, "fill-rule"))
            .map(|r| r.trim() == "evenodd")
            .unwrap_or(true);
        canvas.fill(polygons, paint, even_odd);
    }
    if let Some(paint) = &state.stroke {
        let w = (state.stroke_width * state.transform.length_scale()).max(0.75);
        let mut quads = Vec::new();
        for poly in polygons {
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                if let Some(quad) = thick_segment(a, b, w) {
                    quads.push(quad);
                }
            }
        }
        // Each piece of the outline is filled on its own, so overlaps at the
        // joins do not cancel out the way an even-odd fill would make them.
        for quad in quads {
            canvas.fill(std::slice::from_ref(&quad), paint, false);
        }
    }
}

fn thick_segment(a: (f32, f32), b: (f32, f32), width: f32) -> Option<Vec<(f32, f32)>> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-6 {
        return None;
    }
    // Half a width past each end, so corners are filled in.
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy * width / 2.0, ux * width / 2.0);
    let (ex, ey) = (ux * width / 2.0, uy * width / 2.0);
    let (a, b) = ((a.0 - ex, a.1 - ey), (b.0 + ex, b.1 + ey));
    Some(vec![
        (a.0 + nx, a.1 + ny),
        (b.0 + nx, b.1 + ny),
        (b.0 - nx, b.1 - ny),
        (a.0 - nx, a.1 - ny),
    ])
}

/// Take the transform and paints off a tag.
fn apply_attributes(tag: &str, state: &mut State, gradients: &HashMap<String, Paint>) {
    if let Some(t) = attribute(tag, "transform").and_then(|v| parse_matrix(&v)) {
        state.transform = state.transform.then(t);
    }
    let get = |key: &str| style(tag, key).or_else(|| attribute(tag, key));
    let opacity = |key: &str| {
        get(key)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(1.0)
    };
    if let Some(fill) = get("fill") {
        state.fill = paint(&fill, gradients, opacity("fill-opacity"));
    }
    if let Some(stroke) = get("stroke") {
        state.stroke = paint(&stroke, gradients, opacity("stroke-opacity"));
    }
    if let Some(w) = get("stroke-width") {
        if let Ok(w) = w.trim().trim_end_matches("px").parse::<f32>() {
            state.stroke_width = w;
        }
    }
}

fn paint(value: &str, gradients: &HashMap<String, Paint>, opacity: f32) -> Option<Paint> {
    let value = value.trim();
    if value == "none" {
        return None;
    }
    if let Some(rest) = value.strip_prefix("url(#") {
        let id = rest.trim_end_matches(')');
        return gradients.get(id).cloned();
    }
    let mut c = colour(value)?;
    c[3] = (c[3] as f32 * opacity.clamp(0.0, 1.0)) as u8;
    Some(Paint::Solid(c))
}

fn colour(value: &str) -> Option<[u8; 4]> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("rgb(") {
        let n: Vec<u8> = rest
            .trim_end_matches(')')
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        return (n.len() == 3).then(|| [n[0], n[1], n[2], 255]);
    }
    if let Some(hex) = v.strip_prefix('#') {
        let n = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            6 => Some([(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]),
            _ => None,
        };
    }
    match v {
        "white" => Some([255, 255, 255, 255]),
        "black" => Some([0, 0, 0, 255]),
        _ => None,
    }
}

/// The gradients in `<defs>`, by id.
fn gradients(src: &str) -> HashMap<String, Paint> {
    let mut out = HashMap::new();
    let mut rest = src;
    while let Some(start) = rest.find("<linearGradient") {
        let Some(end) = rest[start..].find("</linearGradient>") else {
            break;
        };
        let block = &rest[start..start + end];
        rest = &rest[start + end..];
        let Some(id) = attribute(block, "id") else {
            continue;
        };
        let n = |k: &str| attribute(block, k).and_then(|v| v.parse::<f32>().ok());
        let t = attribute(block, "gradientTransform")
            .and_then(|v| parse_matrix(&v))
            .unwrap_or(Transform::IDENTITY);
        let from = t.apply(n("x1").unwrap_or(0.0), n("y1").unwrap_or(0.0));
        let to = t.apply(n("x2").unwrap_or(1.0), n("y2").unwrap_or(0.0));

        let mut stops = Vec::new();
        for stop in block.split("<stop").skip(1) {
            let offset = attribute(stop, "offset")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0);
            let c = style(stop, "stop-color")
                .and_then(|v| colour(&v))
                .unwrap_or([0, 0, 0, 255]);
            let a = style(stop, "stop-opacity")
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0);
            stops.push((offset, [c[0], c[1], c[2], (255.0 * a) as u8]));
        }
        stops.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
        out.insert(id, Paint::Linear { from, to, stops });
    }
    out
}

// ---------------------------------------------------------------------------
// path data
// ---------------------------------------------------------------------------

/// Flatten a `d` attribute into closed polygons in device space.
fn subpaths(d: &str, transform: &Transform) -> Vec<Vec<(f32, f32)>> {
    /// Segments a curve is chopped into. The artwork's curves are small, and
    /// this is cheap next to the scan that follows.
    const CURVE_STEPS: usize = 12;

    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    let mut cursor = (0.0f32, 0.0f32);
    let mut start = cursor;

    let mut numbers: Vec<f32> = Vec::new();
    let mut command = ' ';
    let chars = d.char_indices();
    let mut pending = String::new();

    let flush = |command: char,
                 numbers: &mut Vec<f32>,
                 cursor: &mut (f32, f32),
                 start: &mut (f32, f32),
                 current: &mut Vec<(f32, f32)>,
                 out: &mut Vec<Vec<(f32, f32)>>| {
        let relative = command.is_lowercase();
        match command.to_ascii_uppercase() {
            'M' => {
                for pair in numbers.chunks_exact(2) {
                    let p = if relative {
                        (cursor.0 + pair[0], cursor.1 + pair[1])
                    } else {
                        (pair[0], pair[1])
                    };
                    if current.len() > 1 {
                        out.push(std::mem::take(current));
                    } else {
                        current.clear();
                    }
                    *cursor = p;
                    *start = p;
                    current.push(transform.apply(p.0, p.1));
                }
            }
            'L' => {
                for pair in numbers.chunks_exact(2) {
                    let p = if relative {
                        (cursor.0 + pair[0], cursor.1 + pair[1])
                    } else {
                        (pair[0], pair[1])
                    };
                    *cursor = p;
                    current.push(transform.apply(p.0, p.1));
                }
            }
            'H' | 'V' => {
                for n in numbers.iter() {
                    let p = match command.to_ascii_uppercase() {
                        'H' if relative => (cursor.0 + n, cursor.1),
                        'H' => (*n, cursor.1),
                        _ if relative => (cursor.0, cursor.1 + n),
                        _ => (cursor.0, *n),
                    };
                    *cursor = p;
                    current.push(transform.apply(p.0, p.1));
                }
            }
            'C' => {
                for six in numbers.chunks_exact(6) {
                    let at = |i: usize| {
                        if relative {
                            (cursor.0 + six[i], cursor.1 + six[i + 1])
                        } else {
                            (six[i], six[i + 1])
                        }
                    };
                    let (c1, c2, end) = (at(0), at(2), at(4));
                    let p0 = *cursor;
                    for step in 1..=CURVE_STEPS {
                        let t = step as f32 / CURVE_STEPS as f32;
                        let u = 1.0 - t;
                        let x = u * u * u * p0.0
                            + 3.0 * u * u * t * c1.0
                            + 3.0 * u * t * t * c2.0
                            + t * t * t * end.0;
                        let y = u * u * u * p0.1
                            + 3.0 * u * u * t * c1.1
                            + 3.0 * u * t * t * c2.1
                            + t * t * t * end.1;
                        current.push(transform.apply(x, y));
                    }
                    *cursor = end;
                }
            }
            'Z' => {
                *cursor = *start;
                if current.len() > 1 {
                    out.push(std::mem::take(current));
                } else {
                    current.clear();
                }
            }
            _ => {}
        }
        numbers.clear();
    };

    for (_, ch) in chars {
        if ch.is_ascii_alphabetic() {
            if !pending.is_empty() {
                if let Ok(n) = pending.parse::<f32>() {
                    numbers.push(n);
                }
                pending.clear();
            }
            if command != ' ' {
                flush(
                    command,
                    &mut numbers,
                    &mut cursor,
                    &mut start,
                    &mut current,
                    &mut out,
                );
            }
            command = ch;
            continue;
        }
        let starts_number = ch == '-' && !pending.is_empty() && !pending.ends_with(['e', 'E']);
        if ch == ',' || ch.is_whitespace() || starts_number {
            if !pending.is_empty() {
                if let Ok(n) = pending.parse::<f32>() {
                    numbers.push(n);
                }
                pending.clear();
            }
            if starts_number {
                pending.push(ch);
            }
            continue;
        }
        pending.push(ch);
    }
    if !pending.is_empty() {
        if let Ok(n) = pending.parse::<f32>() {
            numbers.push(n);
        }
    }
    if command != ' ' {
        flush(
            command,
            &mut numbers,
            &mut cursor,
            &mut start,
            &mut current,
            &mut out,
        );
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

fn parse_matrix(value: &str) -> Option<Transform> {
    let inner = value.trim().strip_prefix("matrix(")?.strip_suffix(')')?;
    let n: Vec<f32> = inner
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (n.len() == 6).then(|| Transform {
        a: n[0],
        b: n[1],
        c: n[2],
        d: n[3],
        e: n[4],
        f: n[5],
    })
}

// ---------------------------------------------------------------------------
// scraps of XML
// ---------------------------------------------------------------------------

fn tag_name(tag: &str) -> String {
    tag.trim_start_matches('<')
        .split([' ', '>', '/', '\n'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn tag_after(src: &str, open: &str) -> Option<String> {
    let start = src.find(open)?;
    let end = src[start..].find('>')? + start;
    Some(src[start..=end].to_string())
}

/// The value of an attribute, if the tag has one.
fn attribute(tag: &str, key: &str) -> Option<String> {
    let mut from = 0;
    while let Some(found) = tag[from..].find(key) {
        let at = from + found;
        let before = tag[..at].chars().last();
        let after = tag[at + key.len()..].trim_start();
        from = at + key.len();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == ':') {
            continue; // part of a longer name
        }
        if let Some(rest) = after.strip_prefix('=') {
            let rest = rest.trim_start();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                continue;
            }
            let rest = &rest[1..];
            let end = rest.find(quote)?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// One property out of a `style="..."` attribute.
fn style(tag: &str, key: &str) -> Option<String> {
    let style = attribute(tag, "style")?;
    for part in style.split(';') {
        let (name, value) = part.split_once(':')?;
        if name.trim() == key {
            return Some(value.trim().to_string());
        }
    }
    None
}
