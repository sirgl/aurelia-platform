"""Tiny numpy rasterizer for unit contact sheets. Runs in Blender's Python or python3.

It shades exactly like assets/shaders/hex.wgsl (warm sun, cool sky, bounce, ACES),
so a sheet shows what the game shows, without terrain shadows and fog.
Input triangles are in game world space: y up, the camera looks along -z from +z.
"""
import math
import struct
import zlib

import numpy as np

AMBIENT = 0.32
TEAM_COLOR = (0.16, 0.34, 0.78)
BACKGROUND = (0.78, 0.74, 0.62)

# Game sun first, then three probes: from the right, back light, low grazing light.
LIGHTS = {
    "game": (-0.42, 0.80, 0.35),
    "right": (0.70, 0.60, 0.30),
    "back": (0.10, 0.55, -0.80),
    "low": (-0.90, 0.25, 0.30),
}


def _normalize(v):
    v = np.asarray(v, dtype=np.float64)
    return v / np.linalg.norm(v)


def shade(colors, normals, sun):
    """colors, normals: (n, 3). Returns display-space rgb (n, 3)."""
    lin = np.power(np.clip(colors, 0, 1), 2.2)
    diff = np.clip(normals @ sun, 0, None)[:, None]
    sun_col = np.array([1.0, 0.94, 0.84])
    sky = np.array([0.42, 0.52, 0.72]) * (AMBIENT * 1.5)
    ny = normals[:, 1:2]
    sky_amb = sky * (0.55 + 0.45 * ny)
    bounce = np.array([0.30, 0.26, 0.22]) * 0.18 * np.clip(0.5 - 0.5 * ny, 0, 1)
    e = lin * (sun_col * diff + sky_amb + bounce) * 0.85
    tm = e * (2.51 * e + 0.03) / (e * (2.43 * e + 0.59) + 0.14)
    return np.power(np.clip(tm, 0, 1), 1 / 2.2)


class View:
    """Orthographic view along the game camera direction (45 degrees down, from +z)."""

    def __init__(self, center, half_height, size, elevation=45.0):
        self.size = size
        a = math.radians(elevation)
        self.forward = _normalize((0.0, -math.sin(a), -math.cos(a)))
        self.right = np.array([1.0, 0.0, 0.0])
        self.up = np.cross(self.right, self.forward)
        self.center = np.asarray(center, dtype=np.float64)
        self.scale = size[1] / (2.0 * half_height)

    def project(self, p):
        d = p - self.center
        x = d @ self.right * self.scale + self.size[0] / 2
        y = -(d @ self.up) * self.scale + self.size[1] / 2
        z = d @ self.forward
        return np.stack([x, y, z], axis=-1)


def render(tris, colors, team, view, sun, supersample=2):
    """tris (n,3,3) world, colors (n,3), team (n,) 0/1. Returns uint8 (h, w, 3)."""
    w, h = view.size[0] * supersample, view.size[1] * supersample
    image = np.empty((h, w, 3))
    image[:] = BACKGROUND
    depth = np.full((h, w), np.inf)
    if len(tris) == 0:
        return _downsample(image, supersample)
    sun = _normalize(sun)
    e1 = tris[:, 1] - tris[:, 0]
    e2 = tris[:, 2] - tris[:, 0]
    normals = np.cross(e1, e2)
    normals /= np.linalg.norm(normals, axis=1, keepdims=True) + 1e-12
    base = np.where(team[:, None] > 0.5, np.array(TEAM_COLOR), colors)
    rgb = shade(base, normals, sun)
    scaled = View.__new__(View)
    scaled.__dict__.update(view.__dict__)
    scaled.size = (w, h)
    scaled.scale = view.scale * supersample
    screen = scaled.project(tris.reshape(-1, 3)).reshape(-1, 3, 3)
    for t in range(len(screen)):
        _fill(screen[t], rgb[t], image, depth)
    return _downsample(image, supersample)


def _fill(tri, rgb, image, depth):
    h, w = depth.shape
    x0 = max(int(math.floor(tri[:, 0].min())), 0)
    x1 = min(int(math.ceil(tri[:, 0].max())), w - 1)
    y0 = max(int(math.floor(tri[:, 1].min())), 0)
    y1 = min(int(math.ceil(tri[:, 1].max())), h - 1)
    if x0 > x1 or y0 > y1:
        return
    (ax, ay, az), (bx, by, bz), (cx, cy, cz) = tri
    area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
    if abs(area) < 1e-12:
        return
    xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
    w0 = ((bx - xs) * (cy - ys) - (by - ys) * (cx - xs)) / area
    w1 = ((cx - xs) * (ay - ys) - (cy - ys) * (ax - xs)) / area
    w2 = 1.0 - w0 - w1
    inside = (w0 >= -1e-6) & (w1 >= -1e-6) & (w2 >= -1e-6)
    z = w0 * az + w1 * bz + w2 * cz
    region = depth[y0 : y1 + 1, x0 : x1 + 1]
    closer = inside & (z < region)
    region[closer] = z[closer]
    image[y0 : y1 + 1, x0 : x1 + 1][closer] = rgb


def _downsample(image, k):
    h, w, _ = image.shape
    small = image.reshape(h // k, k, w // k, k, 3).mean(axis=(1, 3))
    return (np.clip(small, 0, 1) * 255 + 0.5).astype(np.uint8)


def write_png(path, pixels):
    """pixels: uint8 (h, w, 3)."""
    h, w, _ = pixels.shape
    raw = b"".join(b"\x00" + pixels[y].tobytes() for y in range(h))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


# 3x5 bitmap font for labels: rows top to bottom, 3 bits each.
_FONT = {
    "A": "010101111101101", "B": "110101110101110", "C": "011100100100011", "D": "110101101101110",
    "E": "111100110100111", "F": "111100110100100", "G": "011100101101011", "H": "101101111101101",
    "I": "111010010010111", "J": "001001001101010", "K": "101101110101101", "L": "100100100100111",
    "M": "101111111101101", "N": "110101101101101", "O": "010101101101010", "P": "110101110100100",
    "Q": "010101101110011", "R": "110101110101101", "S": "011100010001110", "T": "111010010010010",
    "U": "101101101101111", "V": "101101101101010", "W": "101101111111101", "X": "101101010101101",
    "Y": "101101010010010", "Z": "111001010100111", "0": "111101101101111", "1": "010110010010111",
    "2": "110001010100111", "3": "110001010001110", "4": "101101111001001", "5": "111100110001110",
    "6": "011100111101111", "7": "111001010010010", "8": "111101111101111", "9": "111101111001110",
    "-": "000000111000000", "/": "001001010100100", ".": "000000000000010", " ": "000000000000000",
    ":": "000010000010000", "_": "000000000000111",
}


def draw_text(pixels, x, y, text, color=(20, 20, 20), size=2):
    h, w, _ = pixels.shape
    for ch in text.upper():
        glyph = _FONT.get(ch, _FONT[" "])
        for row in range(5):
            for col in range(3):
                if glyph[row * 3 + col] == "1":
                    px, py = x + col * size, y + row * size
                    if 0 <= px < w - size and 0 <= py < h - size:
                        pixels[py : py + size, px : px + size] = color
        x += 4 * size
