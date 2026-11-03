"""World wonders of Classic, renaissance to information (classic/docs/classic/wonders.json, kind world_wonder).
One landmark model each, drawn on a tile of the owning city's ring (wonders.py).

Every wonder stands on a ground slab (hex of radius about 0.86, z -0.04..0.015) and fills its
hex; the front (-y) shows the characteristic side. Small accessories are LOD0 only, LOD2 keeps
the silhouette in a few big solids.

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/wonders_late.py, which imports the
helpers it shares from here.
"""
import math

from geom import move, rot_x, rot_y, rot_z, scale
import kit
from wonders import wonder as _model


# Shared helpers ----------------------------------------------------------------------------
def _ground(m, mat, r=0.86, top=0.015):
    kit.hex_patch(m, r, mat, z0=-0.04, z1=top)


def _box(m, x, y, z, sx, sy, sz, mat, yaw=0.0):
    """Box with its base centred at (x, y, z); sx, sy, sz are full extents."""
    kit.block(m, (x, y, z), (sx, sy, sz), mat, yaw)


def _poly(m, pts, z0, z1, mat):
    kit.slab(m, pts, z0, z1, mat)


def _wall(m, a, b, h, thick, mat, step=0.0, z0=0.0, merlon=None):
    """Straight wall a -> b (2d points) from z0, with merlons every `step` at LOD0."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    yaw = math.degrees(math.atan2(dy, dx))
    _box(m, (a[0] + b[0]) / 2, (a[1] + b[1]) / 2, z0, length, thick, h, mat, yaw)
    if step > 0 and m.detail == 0:
        n = max(int(length / step), 2)
        for i in range(n):
            t = (i + 0.5) / n
            _box(m, a[0] + dx * t, a[1] + dy * t, z0 + h, length / n * 0.5, thick, 0.025, merlon or mat, yaw)


def _disc(m, c, r, depth, mat, sides=None):
    """Clock face / round plate facing -y."""
    m.rings([((c[0], c[1] - depth / 2, c[2]), r, r), ((c[0], c[1] + depth / 2, c[2]), r, r)], mat,
            sides=sides or m.pick(10, 6, 5), axis="y")


def _windows(m, x0, x1, y, z, n, w, h, mat="dark", depth=0.01):
    """Row of n small openings on a face at y (facing -y or +y)."""
    for i in range(n):
        x = x0 + (x1 - x0) * (i + 0.5) / n
        _box(m, x, y, z, w, depth, h, mat)


def _chhatri(m, c, r, h, mat="marble", dome=None):
    """Small domed kiosk: base drum and a dome (Mughal pavilions, minaret tops)."""
    kit.cylinder(m, c, r, h, mat, sides=m.pick(6, 5, 4))
    kit.dome(m, (c[0], c[1], c[2] + h), r * 1.15, dome or mat, h=r * 1.1, sides=m.pick(6, 5, 4),
             steps=m.pick(2, 2, 1))


def _cone(m, c, r, h, mat, sides=None):
    kit.cone(m, c, r, h, mat, sides=sides or m.pick(8, 6, 4))


# Big Ben -----------------------------------------------------------------------------------
def big_ben(m):
    _ground(m, "stone", 0.86)
    _poly(m, [(-0.74, -0.36), (0.74, -0.36), (0.74, -0.43), (0.0, -0.86), (-0.74, -0.43)], -0.03, 0.022,
          "water")
    if m.detail < 2:
        _box(m, 0.0, -0.33, -0.02, 1.48, 0.05, 0.05, "stone")
    _parliament(m)
    _clock_tower(m, 0.46, -0.12)


def _parliament(m):
    x, y, w, d, h = -0.18, 0.28, 0.96, 0.28, 0.22
    _box(m, x, y, 0.0, w, d, h, "limestone")
    kit.wedge_roof(m, (x, y, h), w, d, 0.07, "slate")
    if m.detail == 0:
        _windows(m, x - w / 2, x + w / 2, y - d / 2 - 0.003, 0.05, 12, 0.03, 0.12, "glass_dark")
        for i in range(11):
            _cone(m, (x - w / 2 + 0.04 + i * (w - 0.08) / 10, y - d / 2 + 0.01, h), 0.016, 0.07, "limestone",
                  sides=4)
    kit.cylinder(m, (-0.12, 0.3, 0.2), 0.06, 0.2, "limestone", sides=m.pick(8, 6, 5))
    _cone(m, (-0.12, 0.3, 0.4), 0.065, 0.2, "slate", sides=m.pick(8, 6, 5))
    _box(m, -0.62, 0.36, 0.0, 0.22, 0.22, 0.8, "limestone")
    if m.detail < 2:
        for sx in (-1, 1):
            for sy in (-1, 1):
                _cone(m, (-0.62 + sx * 0.095, 0.36 + sy * 0.095, 0.8), 0.022, 0.1, "limestone", sides=4)
        kit.flag(m, (-0.62, 0.36, 0.8), 0.14, size=0.08)
    if m.detail == 0:
        _windows(m, -0.66, -0.58, 0.249, 0.15, 1, 0.08, 0.5, "glass_dark")


def _clock_tower(m, x, y):
    _box(m, x, y, 0.0, 0.16, 0.16, 0.98, "limestone")
    if m.detail == 0:
        for ox in (-0.04, 0.04):
            _box(m, x + ox, y - 0.083, 0.1, 0.022, 0.006, 0.82, "ochre")
            _box(m, x + 0.083, y + ox, 0.1, 0.006, 0.022, 0.82, "ochre")
    _box(m, x, y, 0.98, 0.21, 0.21, 0.2, "limestone")
    if m.detail < 2:
        _disc(m, (x, y - 0.106, 1.08), 0.078, 0.012, "white")
        with m.at(move(x + 0.106, y, 1.08), rot_z(90)):
            _disc(m, (0.0, 0.0, 0.0), 0.078, 0.012, "white")
    if m.detail == 0:
        _box(m, x, y - 0.114, 1.08, 0.008, 0.004, 0.06, "black")
        _box(m, x + 0.02, y - 0.114, 1.075, 0.045, 0.004, 0.008, "black")
    _box(m, x, y, 1.18, 0.17, 0.17, 0.1, "limestone")
    if m.detail < 2:
        _box(m, x, y - 0.087, 1.2, 0.1, 0.006, 0.06, "dark")
    m.taper((x, y, 1.28), (0.1, 0.1), (0.008, 0.008), 0.36, "slate")
    if m.detail < 2:
        for sx in (-1, 1):
            for sy in (-1, 1):
                _cone(m, (x + sx * 0.095, y + sy * 0.095, 1.18), 0.022, 0.13, "gold", sides=4)
        m.rod((x, y, 1.62), (x, y, 1.76), 0.012, 0.0, "gold", sides=4)


# Brandenburg Gate --------------------------------------------------------------------------
def brandenburg_gate(m):
    _ground(m, "stone", 0.86)
    y = 0.05
    _box(m, 0.0, y, 0.0, 1.22, 0.34, 0.05, "limestone")
    cols = m.pick(6, 6, 4)
    for i in range(cols):
        x = -0.54 + 1.08 * i / (cols - 1)
        for oy in (-0.1, 0.1):
            kit.column(m, (x, y + oy, 0.05), 0.42, 0.036, "cream", capital=m.detail == 0, sides=m.pick(8, 6, 4))
        if m.detail < 2:
            _box(m, x, y, 0.05, 0.04, 0.2, 0.42, "limestone")
    _box(m, 0.0, y, 0.47, 1.2, 0.3, 0.09, "limestone")
    if m.detail == 0:
        _box(m, 0.0, y - 0.152, 0.5, 1.16, 0.006, 0.03, "cream")
    _box(m, 0.0, y, 0.56, 0.62, 0.26, 0.06, "limestone")
    _box(m, 0.0, y, 0.62, 0.36, 0.22, 0.05, "limestone")
    with m.at(move(0.0, y - 0.02, 0.67), scale(1.3)):
        _quadriga(m, 0.0, 0.0, 0.0)
    if m.detail == 0:
        for s in (-1, 1):
            kit.colonnade_temple(m, (s * 0.72, y + 0.06, 0.0), 0.16, 0.28, 0.24, "cream", "limestone", cols=(3, 4))


def _quadriga(m, x, y, z):
    if m.detail == 2:
        _box(m, x, y, z, 0.26, 0.16, 0.12, "copper_green")
        return
    for i in range(4):
        kit.quadruped(m, (x - 0.105 + 0.07 * i, y - 0.05, z), -90, "copper_green", length=0.13, height=0.06,
                      neck_up=0.7)
    _box(m, x, y + 0.05, z, 0.12, 0.06, 0.05, "copper_green")
    m.rod((x, y + 0.06, z + 0.04), (x, y + 0.06, z + 0.19), 0.022, 0.014, "copper_green", sides=m.pick(5, 4, 3))
    m.ball((x, y + 0.06, z + 0.21), 0.02, "copper_green", u=5, v=3)
    m.rod((x + 0.03, y + 0.05, z + 0.12), (x + 0.04, y + 0.03, z + 0.3), 0.006, 0.006, "copper_green", sides=3)
    if m.detail == 0:
        m.wheel((x + 0.06, y + 0.05, z + 0.03), 0.03, 0.01, "copper_green")
        m.wheel((x - 0.06, y + 0.05, z + 0.03), 0.03, 0.01, "copper_green")
        m.octa((x + 0.04, y + 0.03, z + 0.3), (0.03, 0.006, 0.03), "copper_green")


# CN Tower ----------------------------------------------------------------------------------
def cn_tower(m):
    _ground(m, "grass", 0.86)
    x, y = 0.12, 0.16
    kit.hex_patch(m, 0.3, "stone", z0=-0.03, z1=0.022, center=(x, y))
    for k in range(3):
        with m.at(move(x, y, 0.0), rot_z(90 + 120 * k)):
            m.taper((0.06, 0.0, -0.03), (0.085, 0.03), (0.012, 0.02), 1.05, "concrete", shift=(-0.055, 0.0))
    kit.cylinder(m, (x, y, -0.03), 0.065, 1.48, "concrete", sides=m.pick(6, 6, 4), r_top=0.036)
    sides = m.pick(12, 8, 6)
    m.rings([((x, y, 0.98), 0.06, 0.06), ((x, y, 1.08), 0.17, 0.17)], "concrete", sides=sides)
    m.rings([((x, y, 1.08), 0.172, 0.172), ((x, y, 1.15), 0.172, 0.172)], "glass_dark", sides=sides)
    m.rings([((x, y, 1.15), 0.17, 0.17), ((x, y, 1.21), 0.08, 0.08)], "white", sides=sides)
    m.rings([((x, y, 1.4), 0.04, 0.04), ((x, y, 1.43), 0.065, 0.065), ((x, y, 1.47), 0.065, 0.065),
             ((x, y, 1.5), 0.035, 0.035)], "glass_dark" if m.detail < 2 else "concrete", sides=m.pick(8, 6, 4))
    m.rod((x, y, 1.48), (x, y, 1.9), 0.024, 0.006, "white", sides=m.pick(6, 4, 3))
    if m.detail < 2:
        m.rod((x, y, 1.82), (x, y, 1.93), 0.008, 0.003, "red", sides=3)
    _skydome(m, -0.42, -0.28)
    if m.detail < 2:
        for i, (bx, by, h) in enumerate(((0.5, -0.32, 0.36), (0.62, 0.12, 0.28), (-0.5, 0.36, 0.32),
                                          (0.3, 0.6, 0.24))):
            _box(m, bx, by, 0.0, 0.16, 0.16, h, "glass_dark" if i % 2 else "sky_glass")


def _skydome(m, x, y):
    sides = m.pick(12, 8, 6)
    kit.cylinder(m, (x, y, 0.0), 0.3, 0.09, "white", sides=sides)
    kit.dome(m, (x, y, 0.09), 0.3, "silver", h=0.1, sides=sides, steps=m.pick(3, 2, 1))
    if m.detail == 0:
        _box(m, x, y - 0.29, 0.02, 0.18, 0.02, 0.05, "glass_dark")


# Hubble Space Telescope --------------------------------------------------------------------
def hubble_space_telescope(m):
    _ground(m, "concrete", 0.86)
    kit.steps(m, (-0.05, 0.0, -0.03), 0.42, 0.05, 3, "stone")
    m.rod((-0.05, 0.0, 0.1), (-0.05, 0.0, 0.55), 0.06, 0.04, "steel", sides=m.pick(6, 5, 4))
    _gantry(m, 0.52, 0.32, 1.05)
    with m.at(move(-0.05, 0.0, 0.66), rot_z(-20), rot_y(-28), rot_x(-55), scale(1.2)):
        _hubble(m)


def _hubble(m):
    sides = m.pick(10, 7, 5)
    m.rod((-0.4, 0.0, 0.0), (0.12, 0.0, 0.0), 0.13, 0.13, "silver", sides=sides)
    m.rod((0.12, 0.0, 0.0), (0.36, 0.0, 0.0), 0.15, 0.15, "aluminum", sides=sides)
    m.rod((0.36, 0.0, 0.0), (0.4, 0.0, 0.0), 0.12, 0.1, "dark", sides=sides)
    if m.detail < 2:
        m.rod((-0.41, 0.0, 0.0), (-0.38, 0.0, 0.0), 0.11, 0.11, "black", sides=sides)
        with m.at(move(-0.4, 0.0, 0.13), rot_y(-60)):
            m.rod((0.0, 0.0, 0.0), (0.0, 0.0, 0.015), 0.13, 0.13, "silver", sides=sides)
    for s in (-1, 1):
        m.rod((0.08, s * 0.12, 0.0), (0.08, s * 0.2, 0.0), 0.012, 0.012, "steel", sides=3)
        m.box((0.08, s * 0.39, 0.0), (0.25, 0.19, 0.008), "navy")
        if m.detail == 0:
            for t in (-0.12, 0.0, 0.12):
                m.box((0.08 + t, s * 0.39, 0.0), (0.004, 0.19, 0.01), "gold")
    if m.detail == 0:
        m.rod((0.0, 0.0, 0.13), (0.0, 0.0, 0.25), 0.008, 0.008, "steel", sides=3)
        kit.dome(m, (0.0, 0.0, 0.25), 0.06, "white", h=0.025, sides=6, steps=1)


def _gantry(m, x, y, h):
    s = 0.09
    sides = m.pick(4, 4, 3)
    for sx in (-1, 1):
        for sy in (-1, 1):
            m.rod((x + sx * s, y + sy * s, -0.03), (x + sx * s * 0.8, y + sy * s * 0.8, h), 0.016, 0.014, "red",
                  sides=sides)
    levels = m.pick(5, 3, 2)
    for i in range(levels):
        z = h * (i + 1) / levels
        f = 1 - 0.2 * (i + 1) / levels
        _box(m, x, y, z - 0.03, s * 2 * f + 0.03, s * 2 * f + 0.03, 0.03, "red")
        if m.detail == 0:
            z0 = h * i / levels
            f0 = 1 - 0.2 * i / levels
            m.rod((x - s * f0, y - s * f0, z0), (x + s * f, y - s * f, z - 0.03), 0.008, 0.008, "red", sides=3)
    if m.detail < 2:
        _box(m, x - 0.2, y - 0.12, 0.62, 0.3, 0.04, 0.03, "red", yaw=30)


# Uffizi ------------------------------------------------------------------------------------
def uffizi(m):
    _ground(m, "stone", 0.86)
    _poly(m, [(-0.76, -0.46), (0.76, -0.46), (0.62, -0.64), (-0.62, -0.64)], -0.03, 0.022, "water")
    for s in (-1, 1):
        _gallery_wing(m, s * 0.25, 0.04, s)
    _box(m, 0.0, -0.33, 0.0, 0.66, 0.14, 0.27, "plaster")
    kit.wedge_roof(m, (0.0, -0.33, 0.27), 0.66, 0.14, 0.05, "terracotta")
    if m.detail < 2:
        for ox in (-0.1, 0.0, 0.1):
            _box(m, ox, -0.403, 0.0, 0.06, 0.01, 0.12, "dark")
    if m.detail == 0:
        _windows(m, -0.3, 0.3, -0.403, 0.17, 6, 0.03, 0.05, "glass_dark")
    _palazzo_vecchio(m, -0.52, 0.34)


def _gallery_wing(m, x, y, side):
    length, depth, h = 0.86, 0.17, 0.27
    _box(m, x, y, 0.0, depth, length, h, "plaster")
    kit.wedge_roof(m, (x, y, h), length, depth, 0.05, "terracotta", yaw=90)
    if m.detail < 2:
        _box(m, x, y, 0.1, depth + 0.008, length + 0.008, 0.014, "stone")
    if m.detail == 0:
        face = x - side * (depth / 2 + 0.004)
        for i in range(8):
            yy = y - length / 2 + length * (i + 0.5) / 8
            _box(m, face, yy, 0.0, 0.01, 0.05, 0.085, "dark")
            _box(m, face, yy, 0.15, 0.01, 0.03, 0.05, "glass_dark")


def _palazzo_vecchio(m, x, y):
    _box(m, x, y, 0.0, 0.26, 0.26, 0.34, "sandstone")
    _box(m, x, y, 0.34, 0.29, 0.29, 0.05, "sandstone")
    if m.detail == 0:
        for k in range(4):
            with m.at(move(x, y, 0.39), rot_z(90 * k)):
                for i in range(4):
                    _box(m, -0.1 + i * 0.067, -0.13, 0.0, 0.03, 0.03, 0.025, "sandstone")
    tx, ty = x, y - 0.06
    _box(m, tx, ty, 0.39, 0.11, 0.11, 0.62, "sandstone")
    _box(m, tx, ty, 1.01, 0.15, 0.15, 0.06, "sandstone")
    if m.detail < 2:
        _disc(m, (tx, ty - 0.056, 0.82), 0.04, 0.008, "white")
        for sx in (-1, 1):
            for sy in (-1, 1):
                _box(m, tx + sx * 0.05, ty + sy * 0.05, 1.07, 0.02, 0.02, 0.1, "sandstone")
    _box(m, tx, ty, 1.17, 0.13, 0.13, 0.025, "sandstone")
    kit.dome(m, (tx, ty, 1.19), 0.045, "terracotta", h=0.08, sides=m.pick(6, 5, 4))
    m.rod((tx, ty, 1.26), (tx, ty, 1.36), 0.008, 0.003, "gold", sides=3)


# Red Fort ----------------------------------------------------------------------------------
def red_fort(m):
    _ground(m, "grass", 0.86)
    ring = kit.ring(8, 0.74, 22.5)
    sides = list(range(8))
    for k in sides:
        a, b = ring[k], ring[(k + 1) % 8]
        if k == 5:
            mid_a = (a[0] + (b[0] - a[0]) * 0.28, a[1] + (b[1] - a[1]) * 0.28)
            mid_b = (a[0] + (b[0] - a[0]) * 0.72, a[1] + (b[1] - a[1]) * 0.72)
            _wall(m, a, mid_a, 0.18, 0.06, "rock_red", step=0.07)
            _wall(m, mid_b, b, 0.18, 0.06, "rock_red", step=0.07)
        else:
            _wall(m, a, b, 0.18, 0.06, "rock_red", step=0.07)
    for x, y in ring:
        kit.cylinder(m, (x, y, -0.03), 0.075, 0.25, "rock_red", sides=m.pick(8, 6, 4))
        if m.detail < 2:
            _chhatri(m, (x, y, 0.22), 0.04, 0.05)
    _lahori_gate(m, 0.0, ring[5][1] + 0.02)
    if m.detail < 2:
        _box(m, 0.0, 0.18, 0.0, 0.5, 0.26, 0.06, "marble")
    _box(m, 0.0, 0.18, 0.0, 0.42, 0.2, 0.18, "marble")
    if m.detail == 0:
        _windows(m, -0.18, 0.18, 0.077, 0.06, 5, 0.04, 0.08, "sandstone")
    for s in (-1, 1) if m.detail < 2 else ():
        _chhatri(m, (s * 0.17, 0.18, 0.18), 0.045, 0.04)
    kit.dome(m, (0.0, 0.18, 0.18), 0.08, "marble", h=0.08, sides=m.pick(8, 6, 5))


def _lahori_gate(m, x, y):
    _box(m, x, y, 0.0, 0.34, 0.14, 0.36, "rock_red")
    if m.detail < 2:
        _box(m, x, y - 0.072, 0.0, 0.1, 0.01, 0.2, "dark")
        _box(m, x, y - 0.072, 0.2, 0.06, 0.01, 0.04, "dark")
    for s in (-1, 1):
        kit.cylinder(m, (x + s * 0.18, y, 0.0), 0.05, 0.46, "rock_red", sides=m.pick(8, 6, 4))
        if m.detail < 2:
            _chhatri(m, (x + s * 0.18, y, 0.46), 0.045, 0.05)
    if m.detail < 2:
        for i in range(m.pick(5, 3, 0)):
            _chhatri(m, (x - 0.12 + 0.24 * i / (m.pick(5, 3, 1) - 1), y, 0.36), 0.022, 0.03)


def models():
    return [
        _model("big_ben", big_ben, "upright", "land"),  # industrial
        _model("brandenburg_gate", brandenburg_gate, "upright", "land"),  # industrial
        _model("cn_tower", cn_tower, "upright", "land"),  # information
        _model("hubble_space_telescope", hubble_space_telescope, "upright", "land"),  # information
        _model("uffizi", uffizi, "upright", "land"),  # renaissance
        _model("red_fort", red_fort, "upright", "land"),  # renaissance
    ]
