#!/usr/bin/env python3
"""Draw the application icon: a dark screen with the Spectrum's four colour
bars and a big ZX, rendered at 1024x1024 as PNG and as a multi-size ICO.

Deliberately dependency-free — no Pillow — so it runs anywhere Python does.
Shapes are supersampled 3x and box-filtered down, which is enough
anti-aliasing for an icon.
"""

import struct
import zlib
from pathlib import Path

SIZE = 1024
SS = 3  # supersampling factor
W = SIZE * SS

BG = (0x14, 0x16, 0x1A)
INK = (0xF2, 0xF3, 0xF5)
BARS = [(0xD8, 0x00, 0x00), (0xD8, 0xD8, 0x00), (0x00, 0xD8, 0x00), (0x00, 0xD8, 0xD8)]


def rounded(x, y, w, radius):
    """Inside the rounded square covering the whole canvas?"""
    left, top, right, bottom = 0.0, 0.0, float(w), float(w)
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, top + radius), bottom - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2


def thick_segment(x, y, x0, y0, x1, y1, half):
    """Distance from a point to a line segment, capped as a thick stroke."""
    dx, dy = x1 - x0, y1 - y0
    length2 = dx * dx + dy * dy
    t = 0.0 if length2 == 0 else ((x - x0) * dx + (y - y0) * dy) / length2
    t = min(max(t, 0.0), 1.0)
    px, py = x0 + t * dx, y0 + t * dy
    return (x - px) ** 2 + (y - py) ** 2 <= half * half


def render():
    """RGBA pixels of the icon, top row first."""
    scale = W / 1024.0
    radius = 190 * scale
    stroke = 52 * scale

    # The Z and the X, as stroke skeletons in 1024-space.
    z = [(300, 300, 640, 300), (640, 300, 300, 620), (300, 620, 640, 620)]
    x = [(700, 300, 940, 620), (940, 300, 700, 620)]
    letters = [(x0 * scale, y0 * scale, x1 * scale, y1 * scale) for x0, y0, x1, y1 in z + x]

    # Four colour bars along the bottom, sheared into a parallelogram so the
    # icon has some movement to it.
    bar_top, bar_bottom = 720 * scale, 900 * scale
    bar_w, gap = 150 * scale, 40 * scale
    bar_left = 226 * scale
    shear = 58 * scale

    rows = []
    for py in range(W):
        y = py + 0.5
        row = []
        for px in range(W):
            px_ = px + 0.5
            if not rounded(px_, y, W, radius):
                row.append((0, 0, 0, 0))
                continue
            colour = BG
            if bar_top <= y <= bar_bottom:
                # Shear: higher up the bar, further right.
                offset = (bar_bottom - y) / (bar_bottom - bar_top) * shear
                for i, bar in enumerate(BARS):
                    start = bar_left + i * (bar_w + gap) + offset
                    if start <= px_ <= start + bar_w:
                        colour = bar
                        break
            if any(thick_segment(px_, y, *seg, stroke) for seg in letters):
                colour = INK
            row.append((*colour, 255))
        rows.append(row)
    return rows


def downsample(rows, factor):
    out = []
    n = factor * factor
    for y in range(0, len(rows), factor):
        line = []
        for x in range(0, len(rows[0]), factor):
            r = g = b = a = 0
            for dy in range(factor):
                for dx in range(factor):
                    pr, pg, pb, pa = rows[y + dy][x + dx]
                    # Premultiply so transparent pixels do not darken edges.
                    r += pr * pa
                    g += pg * pa
                    b += pb * pa
                    a += pa
            if a == 0:
                line.append((0, 0, 0, 0))
            else:
                line.append((r // a, g // a, b // a, a // n))
        out.append(line)
    return out


def png_bytes(rows):
    width, height = len(rows[0]), len(rows)
    raw = b"".join(
        b"\x00" + bytes(v for pixel in row for v in pixel) for row in rows
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def resize(rows, size):
    """Nearest-neighbour is fine here: every target size divides 1024 evenly
    apart from rounding, and the source is already anti-aliased."""
    src = len(rows)
    return [
        [rows[y * src // size][x * src // size] for x in range(size)] for y in range(size)
    ]


def ico_bytes(images):
    """ICO holding PNG-compressed images, which Windows has taken since Vista."""
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, blobs = b"", b""
    for size, blob in images:
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset
        )
        blobs += blob
        offset += len(blob)
    return header + entries + blobs


def main():
    here = Path(__file__).parent
    full = downsample(render(), SS)
    (here / "icon.png").write_bytes(png_bytes(full))
    ico = [(s, png_bytes(resize(full, s))) for s in (16, 32, 48, 64, 128, 256)]
    (here / "icon.ico").write_bytes(ico_bytes(ico))
    print(f"wrote {here/'icon.png'} and {here/'icon.ico'}")


if __name__ == "__main__":
    main()
