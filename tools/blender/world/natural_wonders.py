"""Natural wonders of Classic (classic/docs/classic/natural_wonders.json): one recognisable model
each, covering its tile (radius up to 1.05, height up to 2.4).

Every wonder lays a terrain apron over most of the hex and builds its landform on it, so
the whole tile reads as the wonder. Mountains are stacks of jittered rings (`_massif`)
with one material per band: forest foot, rock, dark rock, snow cap.

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/natural_wonders.py, which imports the
helpers it shares from here.
"""
import math

from geom import move, rot_z
import kit
from world_catalog import WorldModel


def _model(slug, build, ground="upright", site_shape="pad"):
    return WorldModel("natural-wonder/" + slug, build, "natural-wonder", ground, site_shape=site_shape)


# Shared shapes ----------------------------------------------------------------------------
def _hex_reach(deg):
    """Distance from the anchor to the hex border in direction `deg` (edge midpoints at 0, 60, ...)."""
    d = ((deg + 30.0) % 60.0) - 30.0
    return kit.HEX_INNER / math.cos(math.radians(d))


def _polar(deg, r, z=0.0, c=(0.0, 0.0)):
    a = math.radians(deg)
    return (c[0] + math.cos(a) * r, c[1] + math.sin(a) * r, z)


def _apron(m, mat, inset=0.95, z1=0.012):
    """Terrain slab over the tile: the hex with chamfered corners, slightly inset."""
    pts = []
    for k in range(6):
        for off in (-8.0, 8.0):
            a = 90.0 + 60.0 * k + off
            pts.append(_polar(a, _hex_reach(a) * inset)[:2])
    kit.slab(m, pts, -0.04, z1, mat)


def _ellipse(c, rx, ry, n, yaw=0.0, seed=None, jit=0.0):
    pts = []
    ca, sa = math.cos(math.radians(yaw)), math.sin(math.radians(yaw))
    for i in range(n):
        a = math.tau * (i + 0.5) / n
        f = 1.0 if seed is None else 1.0 + (kit.rand(seed, i) - 0.5) * jit
        x, y = math.cos(a) * rx * f, math.sin(a) * ry * f
        pts.append((c[0] + x * ca - y * sa, c[1] + x * sa + y * ca))
    return pts


def _ring(c, r, z, n, seed, j, jit, zjit=0.0, elong=(1.0, 1.0)):
    pts = []
    for i in range(n):
        a = math.tau * (i + 0.5 + (kit.rand(seed, 100 * j + i) - 0.5) * 0.3) / n
        f = 1.0 + (kit.rand(seed, 100 * j + i + 40) - 0.5) * jit
        dz = (kit.rand(seed, 100 * j + i + 70) - 0.5) * zjit
        pts.append((c[0] + math.cos(a) * r * f * elong[0], c[1] + math.sin(a) * r * f * elong[1], z + dz))
    return pts


def _massif(m, c, r, h, layers, seed=0, sides=None, jit=0.15, elong=(1.0, 1.0), lean=(0.0, 0.0)):
    """Faceted mountain of stacked jittered rings, one material per band.

    layers: bottom-up (t, k, mat[, zjit]); t is the height fraction of the band top, k its
    radius fraction, zjit a vertical jitter of that ring (snow tongues). k == 0 ends in an
    apex, otherwise the last band ends in a flat top.
    """
    sides = sides or m.pick(11, 8, 6)
    prev = _ring(c, r, c[2] - 0.04, sides, seed, 0, jit, elong=elong)
    for j, layer in enumerate(layers, 1):
        t, k, mat = layer[:3]
        zjit = layer[3] if len(layer) > 3 else 0.0
        center = (c[0] + lean[0] * t, c[1] + lean[1] * t)
        if k <= 0.0:
            m.loft([prev, [(center[0], center[1], c[2] + h * t)]], mat)
            return
        ring = _ring(center, r * k, c[2] + h * t, sides, seed, j, jit, zjit * h, elong)
        m.loft([prev, ring], mat)
        prev = ring


def _foot_z(layers, r, h, d):
    """Surface height of a `_massif` at distance d from its axis (0 outside its foot)."""
    prof = [(0.0, 1.0)] + [(layer[0], layer[1]) for layer in layers]
    for (t0, k0), (t1, k1) in zip(prof, prof[1:]):
        if r * k1 <= d <= r * k0:
            return h * (t0 + (t1 - t0) * (r * k0 - d) / max(r * (k0 - k1), 1e-6))
    return 0.0


def _glints(m, pts, mat, r=0.022):
    if m.detail == 0:
        for p in pts:
            m.octa(p, (r, r, r * 1.3), mat)


# Barringer Crater -------------------------------------------------------------------------
def barringer_crater(m):
    """Wide impact crater: a raised rim (steep rock wall inside, gentle tan slope outside)
    around a dusty floor; the front rim is lower so the camera sees into the bowl."""
    _apron(m, "sand")
    n = m.pick(18, 12, 8)
    r_in = 0.44
    sections = []
    for i in range(n):
        deg = 360.0 * i / n
        back = (math.sin(math.radians(deg)) + 1.0) / 2.0
        crest = 0.16 + 0.16 * back + 0.05 * (kit.rand(3, i) - 0.5)
        r_c = 0.63 + 0.05 * (kit.rand(5, i) - 0.5)
        sections.append((deg, r_c, crest, _hex_reach(deg) * 0.97))
    for i in range(n):
        inner, outer, lip = [], [], []
        for deg, r_c, crest, r_out in (sections[i], sections[(i + 1) % n]):
            inner.append([_polar(deg, r_in, 0.0), _polar(deg, r_c, crest), _polar(deg, r_c, -0.04),
                          _polar(deg, r_in, -0.04)])
            outer.append([_polar(deg, r_c, -0.04), _polar(deg, r_c, crest), _polar(deg, r_out, -0.04)])
            lip.append([_polar(deg, r_c - 0.03, crest - 0.02), _polar(deg, r_c + 0.005, crest + 0.012),
                        _polar(deg, r_c + 0.06, crest - 0.03)])
        m.loft(inner, "rock")
        m.loft(outer, "sandstone")
        if m.detail < 2:
            m.loft(lip, "rock_dark")
    # Dusty floor with a few fallen boulders.
    kit.cylinder(m, (0.0, 0.0, -0.04), r_in + 0.05, 0.07, "khaki", sides=n)
    if m.detail < 2:
        kit.cylinder(m, (0.05, 0.06, -0.04), 0.2, 0.078, "sand", sides=m.pick(9, 6, 5))
        for i, (x, y) in enumerate(kit.scatter(31, m.pick(6, 3), 0.1, 0.36, 0.12)):
            kit.rock(m, (x, y, 0.03), 0.04 + 0.02 * kit.rand(7, i), "rock_dark", seed=40 + i, squash=0.6)
        # Ejecta boulders on the outer slope.
        for i, deg in enumerate((200, 250, 300, 340, 20, 160)[:m.pick(6, 3)]):
            p = _polar(deg, 0.82)
            kit.rock(m, (p[0], p[1], 0.02), 0.045, "rock", seed=60 + i, squash=0.6)


# Mt. Fuji -----------------------------------------------------------------------------------
FUJI = [(0.08, 0.72, "leaf_dark"), (0.26, 0.54, "slate"), (0.4, 0.42, "slate"), (0.52, 0.33, "slate", 0.2),
        (0.8, 0.2, "snow"), (1.0, 0.13, "snow")]


def mt_fuji(m):
    """Perfect concave volcanic cone: forest foot, blue-grey slopes, snow cap with streaks,
    cherry trees and a red torii in front."""
    _apron(m, "grass")
    c, r, h = (0.0, 0.1, 0.0), 0.9, 1.15
    _massif(m, c, r, h, FUJI, seed=11, sides=m.pick(14, 10, 7), jit=0.05)
    if m.detail == 2:
        return
    kit.cylinder(m, (c[0], c[1], h - 0.02), r * 0.06, 0.03, "rock_dark", sides=m.pick(7, 5))
    trees = [(-0.62, -0.5), (-0.45, -0.66), (0.42, -0.66), (0.6, -0.5), (-0.74, -0.2), (0.75, -0.18)]
    for i, (x, y) in enumerate(trees[:m.pick(6, 4)]):
        z = _foot_z(FUJI, r, h, math.hypot(x - c[0], y - c[1]))
        kit.tree(m, (x, y, z - 0.01), 0.15 + 0.03 * kit.rand(5, i), leaf="blossom" if i < 4 else "leaf_dark", seed=i)
    # Torii at the front foot.
    with m.at(move(0.0, -0.74, 0.0)):
        for s in (-1, 1):
            m.box((s * 0.05, 0.0, 0.055), (0.009, 0.009, 0.065), "lacquer")
        m.box((0.0, 0.0, 0.122), (0.08, 0.012, 0.009), "black")
        if m.detail == 0:
            m.box((0.0, 0.0, 0.098), (0.065, 0.007, 0.006), "lacquer")


# Rock of Gibraltar ------------------------------------------------------------------------
GIBRALTAR = [(-0.84, 0.08), (-0.68, 0.22), (-0.45, 0.46), (-0.2, 0.72), (0.05, 0.9), (0.28, 1.12), (0.44, 1.36),
             (0.55, 1.42), (0.62, 1.32), (0.64, 1.26)]


def _gibraltar_section(x, zt, taper, jit):
    """Cross-section at x: scrub triangle on the lower west slope, limestone above it."""
    yr = 0.06
    yf, yb = -0.46 * taper, 0.34 * taper + 0.04
    front = (x, yr - 0.3 * taper, zt * (0.3 + 0.08 * jit))
    return {
        "scrub": [(x, yf, -0.04), front, (x, front[1], -0.04)],
        "rock": [(x, front[1], -0.04), front, (x, yr - (0.13 + 0.06 * jit) * taper, zt * 0.82), (x, yr, zt),
                 (x, yr + 0.07 * taper, zt * 0.86), (x, yb, -0.04)],
    }


def rock_of_gibraltar(m):
    """Long limestone wedge seen from the side: gentle scrubby west slope, sheer east face and
    the vertical north cliff at its high end; a white town at the foot."""
    _apron(m, "grass_dry")
    rows = GIBRALTAR if m.detail < 2 else GIBRALTAR[::3] + [GIBRALTAR[-1]]
    with m.at(rot_z(-26.0)):
        sections = []
        for i, (x, zt) in enumerate(rows):
            taper = min(1.0, 0.45 + (x + 0.84) * 1.6, 0.6 + (0.66 - x) * 6.0)
            sections.append(_gibraltar_section(x, zt + 0.04 * (kit.rand(9, i) - 0.5), taper, kit.rand(12, i) - 0.5))
        for a, b in zip(sections, sections[1:]):
            m.loft([a["rock"], b["rock"]], "limestone")
            m.loft([a["scrub"], b["scrub"]], "leaf")
        # Darker cliff band on the north face and a talus apron below it.
        last = sections[-1]["rock"]
        m.loft([[(p[0] + 0.004, p[1], p[2]) for p in last], [(p[0] + 0.02, p[1], p[2] * 0.97) for p in last]], "stone")
        if m.detail == 2:
            return
        kit.rock(m, (0.74, -0.02, -0.02), 0.13, "rock", seed=3, squash=0.6)
        for i, x in enumerate((-0.55, -0.35, -0.12, 0.12)[:m.pick(4, 2)]):
            kit.house(m, (x, -0.52, 0.0), 0.09, 0.07, 0.06, "plaster", "terracotta", kind="hip", yaw=0)
        for i, (x, y) in enumerate([(-0.7, -0.5), (0.3, -0.56), (0.5, -0.5)][:m.pick(3, 1)]):
            kit.tree(m, (x, y, 0.0), 0.12, "cypress", leaf="leaf_dark", seed=i)


# Old Faithful -----------------------------------------------------------------------------
def old_faithful(m):
    """Geyser: a towering steam column over a stepped white sinter mound with turquoise pools
    and orange mats, pines and a bison around it."""
    _apron(m, "grass")
    cx, cy = 0.0, 0.08
    sides = m.pick(12, 9, 7)
    m.rings([((cx, cy - 0.04, -0.04), 0.8, 0.72), ((cx, cy - 0.04, 0.024), 0.8, 0.72)], "ochre", sides=sides)
    tiers = [(0.66, 0.6, 0.05), (0.5, 0.46, 0.085), (0.34, 0.31, 0.12), (0.2, 0.18, 0.15)]
    for i, (rx, ry, z) in enumerate(tiers if m.detail < 2 else tiers[::2]):
        m.rings([((cx, cy, -0.04), rx, ry), ((cx, cy, z), rx * 0.97, ry * 0.97)], "cream" if i % 2 else "salt",
                sides=sides)
        if m.detail < 2 and i < 3:
            m.rings([((cx, cy, z - 0.02), rx * 0.88, ry * 0.88), ((cx, cy, z + 0.006), rx * 0.88, ry * 0.88)],
                    "turquoise", sides=sides)
    kit.cone(m, (cx, cy, 0.14), 0.1, 0.07, "salt", sides=m.pick(7, 5, 4))
    # Water jet and the steam column, widening and drifting with the wind.
    m.rod((cx, cy, 0.18), (cx + 0.01, cy, 0.78), 0.03, 0.06, "foam", sides=m.pick(6, 5, 4))
    # Puffs: (dx, dy, z, size); the first ones carry the column, the rest billow at the top.
    puffs = [(0.0, 0.0, 0.46, 0.1), (0.03, -0.02, 0.66, 0.14), (0.06, 0.0, 0.9, 0.18), (0.1, -0.02, 1.16, 0.22),
             (0.15, 0.0, 1.42, 0.24), (-0.07, 0.03, 1.06, 0.15), (0.24, 0.04, 1.3, 0.17), (0.0, 0.02, 1.4, 0.18),
             (0.3, -0.03, 1.54, 0.18), (0.12, 0.05, 1.6, 0.2), (-0.04, -0.06, 0.8, 0.12)]
    if m.detail == 2:
        puffs = [(0.0, 0.0, 0.6, 0.15), (0.06, 0.0, 1.0, 0.22), (0.14, 0.0, 1.4, 0.26)]
    for i, (dx, dy, z, s) in enumerate(puffs[:m.pick(11, 7, 3)]):
        m.ball((cx + dx, cy - 0.02 + dy, z), (s, s * 0.92, s * 0.88), "steam" if i % 3 else "white",
               u=m.pick(8, 6, 6), v=m.pick(5, 4, 4))
    if m.detail == 2:
        return
    pines = [(-0.78, 0.2), (-0.62, 0.52), (-0.3, 0.8), (0.3, 0.8), (0.62, 0.52), (0.78, 0.22), (-0.72, -0.3)]
    for i, (x, y) in enumerate(pines[:m.pick(7, 4)]):
        kit.tree(m, (x, y, 0.0), 0.2 + 0.05 * kit.rand(3, i), "conifer", leaf="leaf_dark", seed=i)
    kit.quadruped(m, (0.55, -0.55, 0.0), 160.0, "bison", length=0.13, height=0.06, hump=True, horns="ivory")


# Fountain of Youth ------------------------------------------------------------------------
def fountain_of_youth(m):
    """Sparkling turquoise pool in a ring of carved stones, fed by a little waterfall from a
    mossy rock, with a marble fountain in the middle and lush tropical plants."""
    _apron(m, "grass")
    pc, pr = (0.0, -0.14), 0.44
    m.rings([((pc[0], pc[1], -0.04), pr, pr), ((pc[0], pc[1], 0.05), pr, pr)], "turquoise", sides=m.pick(14, 10, 7))
    n = m.pick(12, 9, 6)
    for i in range(n):
        deg = 360.0 * (i + 0.5) / n
        p = _polar(deg, pr + 0.03, -0.02, pc)
        kit.block(m, p, (0.07, math.tau * (pr + 0.03) / n * 0.92, 0.1), "marble" if i % 2 else "stone", deg)
    # Mossy rock with the waterfall behind the pool.
    kit.rock(m, (0.0, 0.5, -0.02), 0.36, "rock", seed=4, squash=1.35)
    kit.rock(m, (-0.36, 0.44, -0.02), 0.24, "rock_dark", seed=7, squash=1.0)
    kit.rock(m, (0.36, 0.42, -0.02), 0.22, "rock", seed=9, squash=0.9)
    kit.dome(m, (0.0, 0.52, 0.42), 0.18, "leaf_light", h=0.07, sides=m.pick(7, 5, 4), steps=m.pick(2, 1))
    m.loft([[(-0.07, 0.28, 0.4), (0.07, 0.28, 0.4), (0.07, 0.31, 0.42), (-0.07, 0.31, 0.42)],
            [(-0.1, 0.17, 0.04), (0.1, 0.17, 0.04), (0.1, 0.22, 0.04), (-0.1, 0.22, 0.04)]], "sky_glass")
    if m.detail == 2:
        return
    m.ball((0.0, 0.17, 0.06), (0.11, 0.05, 0.035), "foam", u=7, v=3)
    # Marble fountain: basin on a column, a jet of water.
    kit.cylinder(m, (pc[0], pc[1], 0.0), 0.035, 0.14, "marble", sides=m.pick(6, 5))
    kit.cylinder(m, (pc[0], pc[1], 0.13), 0.1, 0.03, "marble", sides=m.pick(8, 6), r_top=0.12)
    m.rod((pc[0], pc[1], 0.15), (pc[0], pc[1], 0.27), 0.012, 0.03, "foam", sides=m.pick(5, 4))
    # Sparkles over the water.
    _glints(m, [(0.22, -0.3, 0.18), (-0.24, -0.06, 0.24), (0.12, 0.04, 0.32), (-0.14, -0.38, 0.14),
                (0.3, -0.04, 0.12), (-0.04, -0.24, 0.36)], "white", 0.022)
    _glints(m, [(-0.3, -0.26, 0.3), (0.2, -0.16, 0.38), (0.0, -0.44, 0.24)], "neon", 0.02)
    # Lush plants around the pool.
    for i, (x, y) in enumerate([(-0.62, 0.08), (0.64, 0.04), (-0.56, -0.5), (0.58, -0.5)]):
        kit.tree(m, (x, y, 0.0), 0.3 + 0.05 * kit.rand(2, i), "palm", leaf="leaf", seed=i)
    bushes = [(-0.62, -0.18, "leaf_light"), (0.66, -0.24, "leaf_dark"), (-0.2, -0.68, "blossom"),
              (0.24, -0.68, "leaf_light"), (-0.66, 0.38, "blossom"), (0.66, 0.36, "leaf_dark"), (0.0, -0.72, "pink")]
    for i, (x, y, leaf) in enumerate(bushes[:m.pick(7, 3)]):
        kit.tree(m, (x, y, 0.0), 0.1, "bush", leaf=leaf, seed=i)


# Lake Victoria ----------------------------------------------------------------------------
def lake_victoria(m):
    """Great lake filling the tile: sandy shore, deep blue centre, wooded islands, papyrus,
    sailing fishing boats and trees in the corners."""
    _apron(m, "grass")
    n = m.pick(16, 11, 8)
    c = (0.0, 0.02)
    kit.slab(m, _ellipse(c, 0.83, 0.79, n), -0.04, 0.016, "sand")
    kit.slab(m, _ellipse(c, 0.75, 0.71, n), -0.04, 0.021, "water")
    kit.slab(m, _ellipse((0.02, 0.06), 0.42, 0.38, n), -0.04, 0.025, "water_deep")
    islands = [(0.3, 0.2, 0.13), (-0.28, 0.34, 0.1), (-0.1, -0.12, 0.07)]
    for i, (x, y, r) in enumerate(islands if m.detail < 2 else islands[:1]):
        kit.cylinder(m, (x, y, -0.02), r * 1.15, 0.05, "sand", sides=m.pick(8, 6, 5))
        kit.dome(m, (x, y, 0.025), r, "leaf", h=r * 0.55, sides=m.pick(8, 6, 5), steps=m.pick(2, 1))
        if m.detail < 2:
            kit.tree(m, (x + r * 0.2, y, 0.03 + r * 0.4), 0.12 + r * 0.4, "broadleaf", leaf="leaf_dark", seed=i)
    if m.detail == 2:
        return
    kit.boat(m, (-0.32, -0.36, 0.021), 0.17, yaw=10)
    kit.boat(m, (0.36, -0.3, 0.021), 0.15, yaw=-160)
    if m.detail == 0:
        kit.boat(m, (0.05, 0.5, 0.021), 0.13, yaw=200)
    # Papyrus at the shore and trees in the corners of the tile.
    for i, deg in enumerate((200.0, 225.0, -20.0, 40.0, 140.0)[:m.pick(5, 2)]):
        p = _polar(deg, 0.74, 0.0, c)
        for k in range(m.pick(4, 2)):
            a = math.radians(deg + 90 * k)
            tip = (p[0] + math.cos(a) * 0.03, p[1] + math.sin(a) * 0.03, 0.12 + 0.03 * kit.rand(i, k))
            m.rod((p[0], p[1], 0.0), tip, 0.008, 0.002, "cane", sides=3)
    for i, deg in enumerate((30.0, 90.0, 150.0, 210.0, 270.0, 330.0)):
        p = _polar(deg, 0.9)
        kit.tree(m, (p[0], p[1], 0.0), 0.18, "palm" if i % 2 else "broadleaf", leaf="leaf_dark", seed=i)


def models():
    return [
        _model("the-barringer-crater", barringer_crater, "upright", "crater"),
        _model("mt-fuji", mt_fuji, "upright"),
        _model("rock-of-gibraltar", rock_of_gibraltar, "upright"),
        _model("old-faithful", old_faithful, "upright"),
        _model("fountain-of-youth", fountain_of_youth, "upright"),
        _model("lake-victoria", lake_victoria, "upright"),
    ]
