"""World wonders of Classic, ancient to medieval (classic/docs/classic/wonders.json, kind world_wonder).
One landmark model each, drawn on a tile of the owning city's ring (wonders.py).

Every wonder stands on a hex plaza (radius 0.8-0.9, sunk to z -0.04) and keeps its parts
inside the tile hex; the front (-y) shows the characteristic facade. Shared private helpers
come first, then one section per wonder.

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/wonders_early.py, which imports the
helpers it shares from here.
"""
from geom import move
import kit
from people import Look
from wonders import wonder as _model


# Shared helpers -----------------------------------------------------------------------------
def _plaza(m, r, mat, top=0.012):
    """Hex ground slab under the wonder, sunk to z -0.04 so it never floats."""
    kit.hex_patch(m, r, mat, z0=-0.04, z1=top)


def _spaced(a, b, n):
    """n evenly spaced values from a to b, ends included."""
    if n == 1:
        return [(a + b) / 2]
    return [a + (b - a) * i / (n - 1) for i in range(n)]


def _column(m, x, y, z, h, r, mat, ionic=False):
    kit.cylinder(m, (x, y, z), r, h, mat, sides=m.pick(6, 4, 3))
    if m.detail == 0:
        if ionic:
            kit.block(m, (x, y, z + h - 0.008), (r * 4.4, r * 2.4, 0.014), mat)
        else:
            kit.block(m, (x, y, z + h - 0.008), (r * 3.0, r * 3.0, 0.012), mat)


def _ramp(m, x, w, y0, y1, z0, z1, mat):
    """Stair ramp of width w centred on x, rising toward +y from (y0, z0) to (y1, z1)."""
    with m.at(move(x, 0.0, 0.0)):
        m.prism([(y0, z0), (y1, z0), (y1, z1)], w, mat, axis="x")


# Great Library ------------------------------------------------------------------------------
def _scroll_rack(m, x, y):
    """Wooden shelf full of scrolls, open toward the camera."""
    kit.block(m, (x, y, 0.0), (0.22, 0.07, 0.2), "wood_dark")
    if m.detail == 0:
        for row in range(3):
            for col in range(4):
                sx = x - 0.075 + col * 0.05
                sz = 0.04 + row * 0.055
                m.rod((sx, y - 0.04, sz), (sx, y - 0.005, sz), 0.014, 0.014, "paper", sides=4)
    elif m.detail == 1:
        kit.block(m, (x, y - 0.03, 0.025), (0.19, 0.02, 0.15), "paper")


def _niche_wall(m, x0, x1, y, z0, rows, cols):
    """Grid of dark scroll niches with scroll ends, on a wall facing -y."""
    for row in range(rows):
        for col in range(cols):
            x = x0 + (x1 - x0) * (col + 0.5) / cols
            z = z0 + row * 0.075
            kit.block(m, (x, y - 0.004, z), (0.05, 0.01, 0.055), "wood_dark")
            if m.detail == 0:
                kit.block(m, (x, y - 0.008, z + 0.006), (0.04, 0.01, 0.03), "paper")


def great_library(m):
    """Hellenistic library of Alexandria: columned hall with scroll niches and scroll racks."""
    _plaza(m, 0.86, "stone", top=0.012)
    kit.block(m, (0, 0.22, -0.02), (1.04, 0.5, 0.07), "marble")
    kit.block(m, (0, 0.24, 0.05), (0.96, 0.4, 0.3), "limestone")
    kit.hip_roof(m, (0, 0.24, 0.35), 0.96, 0.4, 0.13, "terracotta")
    if m.detail < 2:
        _niche_wall(m, -0.46, -0.2, 0.04, 0.1, 3, 3)
        _niche_wall(m, 0.2, 0.46, 0.04, 0.1, 3, 3)
    with m.at(move(0, -0.1, 0.05)):
        kit.block(m, (0, 0.04, -0.07), (0.46, 0.34, 0.07), "marble")
        for x in _spaced(-0.19, 0.19, m.pick(6, 4, 2)):
            _column(m, x, -0.08, 0.0, 0.3, 0.02, "marble", ionic=True)
        kit.block(m, (0, 0.06, 0.3), (0.46, 0.32, 0.04), "marble")
        kit.wedge_roof(m, (0, 0.06, 0.34), 0.32, 0.46, 0.1, "terracotta", yaw=90)
        if m.detail < 2:
            kit.block(m, (0, 0.13, 0.0), (0.09, 0.012, 0.19), "dark")
    _ramp(m, 0.0, 0.3, -0.36, -0.2, -0.02, 0.05, "marble")
    _scroll_rack(m, -0.42, -0.36)
    _scroll_rack(m, 0.42, -0.36)
    if m.detail < 2:
        kit.statue(m, (0, -0.5, 0.0), 0.2, "bronze", "marble",
                   Look(body="bronze", legs="bronze", skin="bronze", boots="bronze", belt="bronze", robe=True,
                        right="scroll"))
        kit.person(m, (-0.22, -0.55, 0.0), 20, Look(body="linen", legs="linen", robe=True, right="scroll"), 0.11)
        kit.person(m, (0.25, -0.52, 0.0), -30, Look(body="cloth", legs="cloth", robe=True, right="scroll"), 0.11)


# Hanging Gardens ----------------------------------------------------------------------------
_GARDEN_TIERS = ((0.6, 0.48), (0.46, 0.36), (0.32, 0.25), (0.19, 0.15))


def _garden_tier(m, i, hx, hy, rise):
    z0, z1 = (-0.03 if i == 0 else i * rise), (i + 1) * rise
    kit.block(m, (0, 0, z0), (2 * hx, 2 * hy, z1 - z0), "mudbrick")
    kit.block(m, (0, 0, z1 - 0.012), (2 * hx + 0.02, 2 * hy + 0.02, 0.024), "leaf")
    if m.detail == 2:
        return
    count = m.pick(6, 3)
    for k in range(count):
        x = -hx + 2 * hx * (k + 0.5) / count
        if abs(x) < 0.07:
            continue
        drop = 0.06 + 0.06 * kit.rand(31 + i, k)
        m.taper((x, -hy - 0.012, z1 - drop), (0.03, 0.006), (0.045, 0.01), drop, "leaf_dark")
        if m.detail == 0:
            kit.block(m, (x + hx / count, -hy - 0.003, z0 + 0.03 if i == 0 else z0 + 0.02), (0.035, 0.008, 0.06),
                      "dark")
            kit.tree(m, (x, -hy + 0.035, z1), 0.08, "bush", leaf="leaf_light", seed=i * 7 + k)
    for s in (-1, 1):
        drop = 0.08
        m.taper((s * (hx + 0.012), -hy * 0.3, z1 - drop), (0.006, 0.04), (0.01, 0.05), drop, "leaf_dark")


def hanging_gardens(m):
    """Babylon: stepped terraces overflowing with greenery, palms on top, waterfalls down the front."""
    _plaza(m, 0.86, "sand", top=0.01)
    rise = 0.15
    for i, (hx, hy) in enumerate(_GARDEN_TIERS):
        _garden_tier(m, i, hx, hy, rise)
        if m.detail < 2:
            kit.block(m, (0, -hy - 0.008, i * rise + (0.0 if i else -0.01)), (0.07, 0.02, rise + 0.012), "water")
    if m.detail < 2:
        kit.water_pool(m, (0, -0.6), 0.11, rim="stone")
        kit.cylinder(m, (0, -0.52, 0.0), 0.06, 0.03, "foam", sides=6)
        for x, y in ((-0.1, 0.05), (0.1, -0.04), (0.0, 0.08)):
            kit.tree(m, (x, y, 4 * rise), 0.26, "palm", seed=int(x * 100) + 5)
        for i, (hx, hy) in enumerate(_GARDEN_TIERS[:3]):
            for s in (-1, 1):
                kit.tree(m, (s * (hx - 0.05), hy - 0.06, (i + 1) * rise), 0.16, "cypress", leaf="leaf_dark")
    else:
        kit.cone(m, (0, 0, 4 * rise), 0.14, 0.2, "leaf", sides=5)


def models():
    return [
        _model("great_library", great_library, "upright", "land"),  # ancient
        _model("hanging_gardens", hanging_gardens, "upright", "land"),  # classical
    ]
