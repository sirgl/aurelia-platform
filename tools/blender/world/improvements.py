"""Tile improvements of Classic (classic/docs/classic/improvements.json), one model each, plus
terrain variants of the farm.

Layout rule shared with resources.py: the yard (radius 0.30 around the tile anchor) stays
low (under 0.08) so the resource prop of an improved resource stands there; buildings of
the improvement keep to the ring 0.32..0.95. Fields reach from 0.32 to the hex edge.

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/improvements.py, which imports the
helpers it shares from here.
"""
import math

from geom import move, rot_y, rot_z, scale
import kit
from world_catalog import WorldModel

TAN30 = math.tan(math.radians(30.0))


def _model(slug, build, ground="upright", budget="improvement"):
    return WorldModel("improvement/" + slug, build, budget, ground)


# Shared parts -------------------------------------------------------------------------------
def _polar(r, deg):
    a = math.radians(deg)
    return (r * math.cos(a), r * math.sin(a))


def _beam(m, a, b, r, mat, sides=4):
    """Straight timber, pipe or rope between two points."""
    m.rod(a, b, r, r, mat, sides=sides)


def _spike(m, a, b, r, mat):
    """Three-sided cone from a to b: stakes, poles, flames (4 triangles)."""
    m.rod(a, b, r, 0.0, mat, sides=3)


def _band(m, k, r0, r1, z0, z1, mat, trim=0.0, slope=0.0):
    """Trapezoid slab along hex edge k (edge middle at 60k degrees) between apothems r0 < r1.

    `slope` pulls the top face in on both long sides (dikes, ramparts); `trim` shortens it.
    """
    th = math.radians(60 * k)
    ux, uy = math.cos(th), math.sin(th)

    def quad(a, b, z):
        pts = []
        for r, s in ((a, -1), (a, 1), (b, 1), (b, -1)):
            half = max(r * TAN30 - trim, 0.01)
            pts.append((ux * r - uy * s * half, uy * r + ux * s * half, z))
        return pts
    m.loft([quad(r0, r1, z0), quad(r0 + slope, r1 - slope, z1)], mat)


def _shrub(m, c, r, h, mat, u=None):
    """Faceted bush: a pentagonal bipyramid sunk a little into the ground."""
    m.ball((c[0], c[1], h * 0.45), (r, r, h * 0.55), mat, u=u or m.pick(5, 4, 4), v=2)


def _bale(m, c, r, length, mat, yaw=0.0):
    """Lying cylinder: bale, log, cloth roll, column drum."""
    with m.at(move(c[0], c[1], c[2] + r * 0.9), rot_z(yaw)):
        m.rod((-length / 2, 0, 0), (length / 2, 0, 0), r, r, mat, sides=m.pick(6, 5, 4))


def _logs(m, c, length, r, mat, yaw=0.0, count=3):
    """Small log pyramid: two logs below, one on top (or a single log)."""
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        bottom = 2 if count >= 2 else 1
        for i in range(bottom):
            y = (i - (bottom - 1) / 2) * r * 2.05
            m.rod((-length / 2, y, r * 0.9), (length / 2, y, r * 0.9), r, r, mat, sides=m.pick(5, 4, 3))
        if count >= 3:
            m.rod((-length * 0.45, 0, r * 2.6), (length * 0.45, 0, r * 2.6), r, r, mat, sides=m.pick(5, 4, 3))


def _figure(m, c, body="cloth", head="skin", h=0.11):
    """Tiny worker at game scale: a body post and a head (LOD0 only)."""
    if m.detail > 0:
        return
    x, y = c
    m.rod((x, y, -0.005), (x, y, h * 0.72), h * 0.17, h * 0.13, body, sides=4)
    m.octa((x, y, h * 0.86), h * 0.13, head)


def _lattice(m, c, half0, half1, h, mat, levels, r=0.008):
    """Steel or timber lattice tower (derrick): four legs, girts and front diagonals."""
    x, y, z = c
    if m.detail == 2:
        m.taper((x, y, z), (half0, half0), (half1, half1), h, mat)
        return

    def corner(sx, sy, t):
        hh = half0 + (half1 - half0) * t
        return (x + sx * hh, y + sy * hh, z + h * t)
    square = ((-1, -1), (1, -1), (1, 1), (-1, 1))
    for sx, sy in square:
        m.rod(corner(sx, sy, 0.0), corner(sx, sy, 1.0), r, r * 0.8, mat, sides=3)
    for i in range(1, levels + 1):
        t = i / (levels + 1)
        for (ax, ay), (bx, by) in zip(square, square[1:] + square[:1]):
            m.rod(corner(ax, ay, t), corner(bx, by, t), r * 0.6, r * 0.6, mat, sides=3)
        if m.detail == 0:
            t0 = (i - 1) / (levels + 1)
            m.rod(corner(-1, -1, t0), corner(1, -1, t), r * 0.5, r * 0.5, mat, sides=3)
            m.rod(corner(1, -1, t0), corner(1, 1, t), r * 0.5, r * 0.5, mat, sides=3)


# Farm -----------------------------------------------------------------------------------
FARM_CROPS = {
    "default": ("wheat", "leaf_light", "soil"),
    "grassland": ("leaf_light", "wheat", "soil"),
    "plains": ("wheat", "grass_dry", "soil"),
    "desert": ("leaf", "leaf_light", "sand"),
}


# Plots of a farm: (centre radius, centre direction, half axes, outline yaw, sides, row direction, crop index).
# They sit around the open yard but not in a ring: sizes differ, outlines are pebbles, every plot runs its rows
# in its own direction and a gap of bare ground (a path) or a hedge lies between neighbours.
FARM_PLOTS = [
    (0.58, -35.0, (0.34, 0.22), 35.0, 6, 12.0, 0),
    (0.58, 33.0, (0.26, 0.18), -25.0, 6, 100.0, 1),
    (0.60, 186.0, (0.31, 0.20), 65.0, 6, -28.0, 1),
    (0.56, 252.0, (0.25, 0.17), 5.0, 5, 62.0, 0),
]


def _farm_plots(seed, mirror):
    """Outlines (convex hulls) and row directions of the farm's plots, deterministic from the seed."""
    plots = []
    for k, (r, deg, (rx, ry), yaw, sides, rows, crop) in enumerate(FARM_PLOTS):
        jr = 1.0 + 0.10 * (kit.rand(seed, k) - 0.5)
        c = _polar(r * jr, deg + 8.0 * (kit.rand(seed, 10 + k) - 0.5))
        poly = kit.pebble(c, rx * jr, ry * jr, yaw + 25 * (kit.rand(seed, 20 + k) - 0.5), sides, seed * 7 + k)
        rows = rows + 14 * (kit.rand(seed, 30 + k) - 0.5)
        if mirror:
            poly = kit.convex_hull([(-x, y) for x, y in poly])
            rows = 180.0 - rows
            c = (-c[0], c[1])
        plots.append((poly, rows, crop, c))
    return plots


def farm(variant):
    crop_a, crop_b, soil = FARM_CROPS[variant]
    desert = variant == "desert"
    seed = {"default": 5, "grassland": 9, "plains": 14, "desert": 23}[variant]
    mirror = variant in ("grassland", "desert")

    def build(m):
        plots = _farm_plots(seed, mirror)
        for k, (poly, rows, crop, c) in enumerate(plots):
            mat = (crop_a, crop_b)[crop]
            kit.crop_plot(m, poly, rows, mat, pitch=0.078, height=0.04 if mat == "wheat" else 0.032, soil=soil)
        sign = -1.0 if mirror else 1.0
        if desert:
            _canal(m, sign)
        else:
            _hedge(m, sign)
        _farmstead(m, desert, sign)
    return build


def _hedge(m, sign):
    """A hedge of bushes between the two plots on the right, crooked like a real one."""
    if m.detail == 2:
        return
    n = 5 if m.detail == 0 else 3
    for i in range(n):
        t = i / (n - 1)
        x, y = 0.80 - 0.10 * t + 0.02 * math.sin(i * 2.1), 0.02 + 0.30 * t
        _shrub(m, (sign * x, y), 0.045 + 0.008 * (i % 2), 0.07, "leaf_dark" if i % 2 else "leaf")


def _canal(m, sign):
    """The irrigation channel of a flood-plain farm: a crooked ribbon of water from the edge to the yard."""
    if m.detail == 2:
        return
    path = [(sign * 0.80, 0.05), (sign * 0.64, 0.12), (sign * 0.50, 0.04), (sign * 0.38, 0.12)]
    for a, b in zip(path, path[1:]):
        dx, dy = b[0] - a[0], b[1] - a[1]
        ln = math.hypot(dx, dy)
        nx, ny = -dy / ln * 0.02, dx / ln * 0.02
        quad = [(a[0] + nx, a[1] + ny), (b[0] + nx, b[1] + ny), (b[0] - nx, b[1] - ny), (a[0] - nx, a[1] - ny)]
        kit.slab(m, kit.convex_hull(quad), -0.05, 0.016, "water")


def _farmstead(m, desert, sign):
    """Farmhouse, barn and haystack at the back, off-centre and turned to nothing in particular, with a bare
    path to the yard."""
    hx, hy = sign * -0.30, 0.70
    if m.detail == 2:
        kit.block(m, (hx, hy, -0.01), (0.17, 0.11, 0.13), "terracotta" if not desert else "mudbrick", 12 * sign)
        return
    if m.detail < 2:
        bend = (hx * 0.45 + sign * 0.06, 0.50)
        for a, b, wa, wb in (((hx * 0.9, hy - 0.07), bend, 0.05, 0.035), (bend, (0.0, 0.33), 0.035, 0.05)):
            dx, dy = b[0] - a[0], b[1] - a[1]
            ln = math.hypot(dx, dy)
            nx, ny = -dy / ln, dx / ln
            kit.slab(m, kit.convex_hull([(a[0] + nx * wa, a[1] + ny * wa), (b[0] + nx * wb, b[1] + ny * wb),
                                         (b[0] - nx * wb, b[1] - ny * wb), (a[0] - nx * wa, a[1] - ny * wa)]),
                     -0.05, 0.010, "soil")
    if desert:
        kit.house(m, (hx, hy, -0.01), 0.17, 0.11, 0.09, "mudbrick", "mudbrick", yaw=12 * sign, kind="flat")
        if m.detail == 0:
            kit.tree(m, (sign * 0.02, 0.78, 0.0), 0.26, "palm", leaf="leaf", seed=3)
            kit.tree(m, (sign * -0.55, 0.52, 0.0), 0.22, "palm", leaf="leaf", seed=5)
        return
    kit.house(m, (hx, hy, -0.01), 0.17, 0.11, 0.09, "red", "wood_dark", yaw=12 * sign, roof_h=0.08)
    kit.house(m, (hx + sign * 0.19, hy - 0.08, -0.01), 0.09, 0.07, 0.06, "plaster", "terracotta", yaw=-30 * sign,
              roof_h=0.05, door=None)
    kit.cone(m, (hx + sign * -0.12, hy - 0.10, -0.01), 0.045, 0.09, "wheat", sides=m.pick(7, 5, 4))
    if m.detail == 0:
        kit.tree(m, (sign * 0.28, 0.80, 0.0), 0.24, "fruit", seed=4)


# Mine -------------------------------------------------------------------------------------
def mine(m):
    # Adit in a rocky outcrop at the back left, with a timber portal.
    kit.rock(m, (-0.42, 0.42, -0.02), 0.26, "rock", seed=5, squash=0.8)
    kit.rock(m, (-0.18, 0.62, -0.02), 0.18, "rock_dark", seed=9, squash=0.7)
    with m.at(move(-0.30, 0.25, 0.0), rot_z(-35)):
        for s in (-1, 1):
            m.box((s * 0.06, 0, 0.07), (0.016, 0.016, 0.08), "wood")
        m.box((0, 0, 0.15), (0.085, 0.018, 0.016), "wood")
        if m.detail < 2:
            m.box((0, 0.01, 0.07), (0.048, 0.006, 0.07), "black")
    _headframe(m, (0.40, 0.42))
    # Spoil heap and an ore cart on its rails toward the yard.
    kit.rock(m, (-0.60, -0.12, -0.03), 0.16, "rock_dark", seed=21, squash=0.55)
    if m.detail < 2:
        with m.at(move(-0.20, 0.05, 0.0), rot_z(-35)):
            for s in (-1, 1):
                m.box((0.0, s * 0.03, 0.004), (0.16, 0.005, 0.006), "iron")
            m.box((0.05, 0, 0.045), (0.04, 0.03, 0.025), "wood_dark")
            m.box((0.05, 0, 0.074), (0.035, 0.025, 0.006), "rust")


def _headframe(m, c):
    """Timber headframe with its sheave wheel and the winch house beside it."""
    with m.at(move(c[0], c[1], 0.0)):
        if m.detail == 2:
            m.taper((0, 0, -0.02), (0.07, 0.06), (0.03, 0.025), 0.36, "wood_dark")
            kit.block(m, (0.16, 0.0, -0.01), (0.12, 0.10, 0.11), "wood")
            return
        for sx in (-1, 1):
            for sy in (-1, 1):
                m.rod((sx * 0.07, sy * 0.06, -0.02), (sx * 0.025, sy * 0.02, 0.34), 0.011, 0.008, "wood_dark",
                      sides=3)
        m.box((0, 0, 0.34), (0.04, 0.035, 0.012), "wood_dark")
        m.rod((0, -0.03, 0.37), (0, 0.03, 0.37), 0.045, 0.045, "iron", sides=m.pick(8, 5, 4))
        kit.house(m, (0.16, 0.0, -0.01), 0.12, 0.10, 0.07, "wood", "slate", kind="gable")


# Trading post -----------------------------------------------------------------------------
def trading_post(m):
    # Log trading hut with a thatched roof and a red awning over the counter.
    kit.house(m, (0.0, 0.66, -0.01), 0.34, 0.20, 0.12, "wood", "grass_dry", roof_h=0.12)
    m.loft([[(-0.17, 0.40, 0.085), (0.17, 0.40, 0.085), (0.17, 0.56, 0.13), (-0.17, 0.56, 0.13)],
            [(-0.17, 0.40, 0.097), (0.17, 0.40, 0.097), (0.17, 0.56, 0.142), (-0.17, 0.56, 0.142)]], "red")
    if m.detail < 2:
        for x in (-0.15, 0.15):
            _beam(m, (x, 0.42, -0.01), (x, 0.42, 0.09), 0.008, "wood_dark", sides=3)
        kit.block(m, (0.0, 0.47, -0.01), (0.26, 0.06, 0.055), "wood_dark")
        kit.flag(m, (0.17, 0.74, 0.17), 0.16, size=0.08)
    _market_stall(m, (-0.55, 0.16))
    _trade_goods(m)
    if m.detail == 0:
        _pack_camel(m, (-0.12, -0.56))


def _market_stall(m, c):
    x, y = c
    if m.detail < 2:
        for sx in (-1, 1):
            for sy in (-1, 1):
                _beam(m, (x + sx * 0.1, y + sy * 0.07, -0.01), (x + sx * 0.1, y + sy * 0.07, 0.12), 0.007,
                      "wood_dark", sides=3)
        kit.block(m, (x, y, -0.01), (0.2, 0.12, 0.055), "wood")
    kit.wedge_roof(m, (x, y, 0.12), 0.24, 0.17, 0.06, "linen", yaw=0, overhang=0.0)
    if m.detail == 0:
        kit.cone(m, (x - 0.05, y, 0.045), 0.035, 0.04, "spice", sides=5)
        _bale(m, (x + 0.05, y - 0.01, 0.045), 0.02, 0.08, "dye", yaw=90)


def _trade_goods(m):
    crates = [(0.48, 0.26, 0.0, 0.09, 10), (0.58, 0.16, 0.0, 0.08, -20), (0.50, 0.24, 0.09, 0.07, 30)]
    for x, y, z, s, yaw in crates[:m.pick(3, 2, 1)]:
        kit.crate(m, (x, y, z - 0.01 if z == 0 else z), s, "wood", yaw)
    if m.detail == 2:
        return
    for i, (x, y) in enumerate(((0.56, -0.08), (0.46, -0.20))[:m.pick(2, 1)]):
        _bale(m, (x, y, -0.01), 0.04, 0.11, "wool" if i == 0 else "cotton", yaw=70)
    if m.detail == 0:
        for x, y in ((0.34, -0.36), (0.42, -0.42)):
            kit.barrel(m, (x, y, -0.01), 0.035, 0.07, "wood_dark")


def _pack_camel(m, c):
    kit.quadruped(m, (c[0], c[1], 0.0), 10, "camel", length=0.2, height=0.1, hump=True, neck_up=0.55)
    for s in (-1, 1):
        kit.block(m, (c[0] - 0.01, c[1] + s * 0.05, 0.07), (0.06, 0.03, 0.05), "red" if s < 0 else "rope", 10)


# Lumber mill ------------------------------------------------------------------------------
def lumber_mill(m):
    # Mill house at the back right with its water wheel on the front wall over the race.
    kit.patch(m, (0.30, 0.43), (0.30, 0.045), "water", z1=0.008)
    kit.house(m, (0.30, 0.62, -0.01), 0.32, 0.22, 0.15, "wood", "slate", roof_h=0.12)
    if m.detail < 2:
        m.wheel((0.30, 0.48, 0.10), 0.11, 0.035, "wood_dark", hub="iron", axis="y")
        if m.detail == 0:
            for i in range(8):
                with m.at(move(0.30, 0.48, 0.10), rot_y(45 * i + 22.5)):
                    m.box((0, 0, 0.11), (0.014, 0.024, 0.016), "wood")
    else:
        m.rod((0.30, 0.46, 0.10), (0.30, 0.50, 0.10), 0.11, 0.11, "wood_dark", sides=5)
    kit.grove(m, [(-0.36, 0.66), (-0.12, 0.78)][:m.pick(2, 2, 1)], 0.34, "conifer", leaf="leaf_dark", seed=4)
    _saw_bench(m, (-0.50, 0.30))
    _logs(m, (-0.54, -0.10), 0.38, 0.042, "bark", yaw=20, count=m.pick(3, 3, 1))
    _logs(m, (0.04, -0.60), 0.36, 0.04, "bark", yaw=-5, count=m.pick(3, 2, 1))
    if m.detail < 2:
        for i in range(3):
            kit.block(m, (0.56 + 0.01 * i, -0.18, -0.01 + 0.018 * i), (0.22 - 0.03 * i, 0.09, 0.018), "deck", 30)
    if m.detail == 0:
        for x, y in ((-0.30, -0.42), (0.36, -0.46), (-0.70, 0.18)):
            kit.cylinder(m, (x, y, -0.01), 0.032, 0.04, "wood", sides=5)


def _saw_bench(m, c):
    x, y = c
    kit.block(m, (x, y, -0.01), (0.26, 0.09, 0.05), "wood")
    if m.detail < 2:
        _bale(m, (x, y, 0.04), 0.026, 0.30, "bark")
        m.rod((x + 0.02, y - 0.006, 0.08), (x + 0.02, y + 0.006, 0.08), 0.06, 0.06, "steel", sides=m.pick(8, 5))


# Organic layout helpers -----------------------------------------------------------------------
def _hex_reach(deg):
    """Distance from the anchor to the hex edge in direction `deg` (corners at 30 + 60k degrees)."""
    d = ((deg + 30.0) % 60.0) - 30.0
    return (math.sqrt(3.0) / 2.0) / math.cos(math.radians(d))


def _wander_outline(seed, count, base, swing, limit=0.9, start=8.0):
    """A closed outline of `count` points around the anchor whose radius wanders between base - swing and
    base + swing, kept `limit` of the way to the hex edge: a pen or a field edge that is nobody's hexagon."""
    phase = rand_deg(seed, 99)
    pts = []
    for k in range(count):
        a = start + 360.0 * (k + (kit.rand(seed, k) - 0.5) * 0.5) / count
        r = base + swing * (kit.rand(seed, 20 + k) - 0.5) * 2 + 0.06 * math.sin(math.radians(2 * a + phase))
        pts.append(_polar(min(r, limit * _hex_reach(a)), a))
    return pts


def rand_deg(seed, i):
    return 360.0 * kit.rand(seed, i)


def _nearest_segment(pts, deg):
    """Index of the segment of the closed outline whose middle lies closest to direction `deg`."""
    best, at = 1e9, 0
    for i in range(len(pts)):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        mid = math.degrees(math.atan2(a[1] + b[1], a[0] + b[0]))
        diff = abs((mid - deg + 180.0) % 360.0 - 180.0)
        if diff < best:
            best, at = diff, i
    return at


def _trough(m, c, yaw, length=0.15):
    """A drinking trough: a dark timber box with a water surface (the water at LOD0 only)."""
    if m.detail == 2:
        return
    kit.block(m, (c[0], c[1], -0.01), (length, 0.05, 0.04), "wood_dark", yaw)
    if m.detail == 0:
        kit.block(m, (c[0], c[1], 0.026), (length - 0.025, 0.03, 0.006), "water", yaw)


def _haystack(m, c, r, h, mat="wheat"):
    kit.cone(m, (c[0], c[1], -0.01), r, h, mat, sides=m.pick(7, 5, 4))
    if m.detail == 0:
        kit.cone(m, (c[0], c[1], -0.01 + h * 0.92), r * 0.16, h * 0.2, "grass_dry", sides=4)


# Pasture ----------------------------------------------------------------------------------
PASTURE_SEED = 41


def pasture(m):
    """A wandering paddock: the fence follows a lobed outline of about 0.6-0.8, with two gates (one swung open),
    a stub of a pen by the shed, a trough, a pond and a trampled patch; haystacks and a tree stand outside it.
    The herd of the resource is drawn by resources.py and spreads across the whole paddock; the middle (the
    yard, 0.30) stays open. Nothing here is a hex edge."""
    seed = PASTURE_SEED
    outline = _wander_outline(seed, 11, 0.69, 0.085, limit=0.93)
    gate_a = _nearest_segment(outline, 252.0)
    gate_b = _nearest_segment(outline, 30.0)
    gaps = [(gate_a, 0.30, 0.62), (gate_b, 0.45, 0.72)]
    low = {_nearest_segment(outline, 190.0), _nearest_segment(outline, 330.0)}
    front = {i for i in range(len(outline)) if outline[i][1] + outline[(i + 1) % len(outline)][1] < 0.1}
    if m.detail == 2:
        # Far marker: the paddock as one low pale patch, a single rail on four corners, the shed.
        marker = kit.convex_hull(outline)
        kit.slab(m, marker, -0.05, 0.012, "leaf_light")
        for i in range(0, len(outline), 3):
            a, b = outline[i], outline[(i + 3) % len(outline)]
            m.rod((a[0], a[1], 0.06), (b[0], b[1], 0.06), 0.014, 0.014, "wood", sides=3)
        kit.block(m, _polar(0.58, 128) + (-0.01,), (0.2, 0.13, 0.13), "wood_dark", 20)
        return
    fence_pts = outline if m.detail == 0 else outline[::2] + ([] if len(outline) % 2 == 0 else [])
    if m.detail == 0:
        kit.wander_fence(m, fence_pts, h=0.08, gaps=gaps, seed=seed, low=low, front=front)
        for gi, g0, g1 in gaps[:1]:
            a, b = outline[gi], outline[(gi + 1) % len(outline)]
            yaw = math.degrees(math.atan2(b[1] - a[1], b[0] - a[0]))
            hinge = (a[0] + (b[0] - a[0]) * g0, a[1] + (b[1] - a[1]) * g0)
            kit.gate(m, hinge, yaw, math.hypot(b[0] - a[0], b[1] - a[1]) * (g1 - g0), h=0.07, open_deg=-70)
    else:
        kit.wander_fence(m, fence_pts, h=0.08, seed=seed, low=low)
    # The shed, off-centre at the back, and a lean-to hay store against it.
    sx, sy = _polar(0.58, 128)
    kit.house(m, (sx, sy, -0.01), 0.21, 0.13, 0.09, "wood", "wood_dark", yaw=22, roof_h=0.07)
    if m.detail == 0:
        kit.block(m, (sx + 0.13, sy - 0.07, -0.01), (0.07, 0.10, 0.05), "wood_dark", 22)
    _trough(m, _polar(0.52, 330), 62)
    hx, hy = _polar(0.52, 203)
    if m.detail == 0:
        kit.slab(m, kit.pebble((hx, hy), 0.11, 0.075, 25, 7, seed + 3), -0.05, 0.010, "water")
        kit.slab(m, kit.pebble(_polar(0.50, 255), 0.10, 0.07, 10, 6, seed + 5), -0.05, 0.007, "soil")
    elif m.detail == 1:
        kit.slab(m, kit.pebble((hx, hy), 0.11, 0.075, 25, 6, seed + 3), -0.05, 0.010, "water")
    # Outside the fence: haystacks, a tree, bushes and a rock break the hex outline.
    _haystack(m, _polar(0.80, 150), 0.07, 0.11)
    _haystack(m, _polar(0.76, 163), 0.05, 0.08)
    if m.detail < 2:
        kit.tree(m, _polar(0.84, 78) + (0.0,), 0.30, "broadleaf", seed=seed)
        _shrub(m, _polar(0.80, 288), 0.06, 0.07, "leaf")
    if m.detail == 0:
        _shrub(m, _polar(0.82, 18), 0.045, 0.06, "leaf_dark")
        kit.rock(m, _polar(0.80, 235) + (-0.01,), 0.05, "rock", seed=seed + 1)


# Fishing boats ----------------------------------------------------------------------------
def fishing_boats(m):
    boats = [((-0.56, 0.24), 0.48, 12, "sail"), ((0.52, 0.42), 0.44, 168, "red"), ((-0.20, -0.62), 0.40, -8, "linen")]
    for (x, y), length, yaw, sail in boats:
        _fishing_boat(m, (x, y), length, yaw, sail)
    # Net floats on a ring around the yard, with the float line between them.
    count = m.pick(12, 8, 4)
    pts = kit.ring(count, 0.40, 15)
    for i, (x, y) in enumerate(pts):
        m.octa((x, y, 0.004), (0.024, 0.024, 0.02), "red" if i % 2 == 0 else "white")
        if m.detail == 0:
            nx, ny = pts[(i + 1) % count]
            _beam(m, (x, y, 0.004), (nx, ny, 0.004), 0.004, "rope", sides=3)


def _fishing_boat(m, c, length, yaw, sail):
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        L, B = length / 2, length * 0.19
        m.loft([[(-L * 0.85, -B * 0.8, 0.045), (-L * 0.85, B * 0.8, 0.045), (-L * 0.75, 0.0, -0.025)],
                [(L * 0.1, -B, 0.05), (L * 0.1, B, 0.05), (L * 0.1, 0.0, -0.035)],
                [(L, 0.0, 0.075)]], "hull")
        if m.detail == 0:
            m.box((-L * 0.25, 0.0, 0.046), (L * 0.5, B * 0.6, 0.004), "deck")
            _figure(m, (-L * 0.55, 0.0), body="cloth_dark", h=0.09)
        if m.detail < 2:
            m.rod((0.0, 0.0, 0.03), (0.0, 0.0, length * 0.85), 0.006, 0.005, "wood_dark", sides=3)
        m.prism([(0.02, length * 0.14), (-length * 0.42, length * 0.16), (0.02, length * 0.82)], 0.005, sail)


# Oil well ---------------------------------------------------------------------------------
def oil_well(m):
    _lattice(m, (0.26, 0.56, -0.02), 0.12, 0.028, 0.78, "steel", levels=m.pick(4, 2, 0), r=0.01)
    kit.block(m, (0.26, 0.56, 0.74), (0.08, 0.08, 0.03), "red")
    if m.detail < 2:
        kit.block(m, (0.26, 0.56, -0.02), (0.30, 0.30, 0.04), "concrete")
    with m.at(move(-0.42, 0.24, 0.0), scale(1.35)):
        _pumpjack(m)
    _tank(m, (0.62, -0.06), 0.12, 0.16)
    if m.detail == 0:
        _tank(m, (0.40, -0.40), 0.075, 0.11)
        kit.patch(m, (-0.36, -0.20), (0.12, 0.08), "black", 20, z1=0.004)
        _beam(m, (-0.69, 0.24, 0.015), (-0.60, -0.04, 0.015), 0.012, "iron", sides=4)
        _beam(m, (-0.60, -0.04, 0.015), (0.50, -0.06, 0.015), 0.012, "iron", sides=4)


def _pumpjack(m):
    """Nodding donkey seen from the side: horsehead to the left, crank to the right."""
    m.box((0.0, 0.0, 0.0), (0.18, 0.05, 0.02), "concrete")
    for sy in (-1, 1):
        m.rod((0.0, sy * 0.045, 0.01), (0.0, 0.0, 0.21), 0.01, 0.008, "dark", sides=m.pick(4, 3, 3))
    m.box((-0.01, 0.0, 0.215), (0.16, 0.014, 0.016), "dark")
    with m.at(move(-0.17, 0.0, 0.215)):
        m.prism([(0.0, -0.06), (0.0, 0.05), (-0.035, 0.04), (-0.05, 0.0), (-0.035, -0.05)], 0.034, "red")
    m.box((0.13, 0.0, 0.07), (0.035, 0.035, 0.045), "iron")
    if m.detail < 2:
        m.rod((0.14, 0.0, 0.2), (0.13, 0.0, 0.08), 0.007, 0.007, "dark", sides=3)
        m.rod((-0.215, 0.0, 0.16), (-0.215, 0.0, 0.03), 0.005, 0.005, "steel", sides=3)
        kit.cylinder(m, (-0.215, 0.0, 0.0), 0.022, 0.04, "iron", sides=5)


def _tank(m, c, r, h):
    x, y = c
    kit.cylinder(m, (x, y, -0.02), r, h + 0.02, "white", sides=m.pick(10, 7, 5))
    kit.cone(m, (x, y, h), r * 1.03, r * 0.35, "steel", sides=m.pick(10, 7, 5))
    if m.detail == 0:
        kit.cylinder(m, (x, y, h * 0.55), r * 1.02, h * 0.16, "red", sides=10)


# Fort -------------------------------------------------------------------------------------
def _bastion_outline(a, tip=0.86, shoulder=0.68, gorge=0.5):
    return [_polar(gorge, a - 15), _polar(shoulder, a - 21), _polar(tip, a), _polar(shoulder, a + 21),
            _polar(gorge, a + 15)]


def fort(m):
    """Vauban star fort: six arrowhead bastions joined by sloped curtain walls."""
    top = 0.11
    for j in range(6):
        a = 30 + 60 * j
        if m.detail == 2:
            pts = [_polar(0.5, a), _polar(0.66, a - 20), _polar(0.84, a), _polar(0.66, a + 20)]
        else:
            pts = _bastion_outline(a)
        _sloped_slab(m, pts, -0.04, top, "stone", 0.88)
        if m.detail == 0:
            _sloped_slab(m, pts, top - 0.004, top + 0.012, "grass", 0.8, base_shrink=0.88)
    for k in range(6):
        if m.detail == 2:
            _band(m, k, 0.45, 0.6, -0.04, top - 0.01, "stone", trim=0.06, slope=0.075)
            continue
        _band(m, k, 0.45, 0.6, -0.04, top - 0.01, "stone", trim=0.06, slope=0.03)
    if m.detail < 2:
        for a in (270, 210, 330, 30, 150)[:m.pick(5, 2)]:
            _cannon(m, _polar(0.74, a), a)
        kit.block(m, (*_polar(0.6, 300), -0.01), (0.07, 0.03, 0.06), "dark", 30)
        kit.flag(m, (*_polar(0.72, 90), 0.1), 0.26, size=0.11)


def _sloped_slab(m, pts, z0, z1, mat, shrink, base_shrink=1.0):
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)

    def ring(f, z):
        return [(cx + (x - cx) * f, cy + (y - cy) * f, z) for x, y in pts]
    m.loft([ring(base_shrink, z0), ring(shrink, z1)], mat)


def _cannon(m, c, a):
    x, y = c
    out = _polar(1.0, a)
    if m.detail == 0:
        kit.block(m, (x, y, 0.1), (0.05, 0.05, 0.03), "wood_dark", a)
    m.rod((x - out[0] * 0.04, y - out[1] * 0.04, 0.14), (x + out[0] * 0.09, y + out[1] * 0.09, 0.15), 0.018, 0.013,
          "black", sides=m.pick(5, 4))


# Customs house ----------------------------------------------------------------------------
def customs_house(m):
    """Merchant house with stepped gables, a gold emblem, a quay crane and stacked goods."""
    if m.detail < 2:
        kit.patch(m, (-0.06, 0.44), (0.42, 0.1), "limestone", z1=0.006)
    _gable_house(m, (0.10, 0.62), 0.32, 0.24, 0.26, "brick")
    _gable_house(m, (-0.26, 0.62), 0.26, 0.22, 0.2, "cream")
    if m.detail < 2:
        m.rod((0.10, 0.497, 0.17), (0.10, 0.48, 0.17), 0.05, 0.05, "gold", sides=m.pick(8, 6))
        kit.cone(m, (0.10, 0.62, 0.395), 0.025, 0.06, "gold", sides=4)
    _quay_crane(m, (0.60, 0.12))
    stacks = [(-0.52, 0.18, 0.0, 0.09, 15), (-0.42, 0.24, 0.0, 0.08, -10), (-0.48, 0.20, 0.09, 0.075, 40),
              (-0.36, -0.40, 0.0, 0.08, 5)]
    for x, y, z, s, yaw in stacks[:m.pick(4, 2, 1)]:
        kit.crate(m, (x, y, z - 0.01 if z == 0 else z), s, "wood", yaw)
    if m.detail < 2:
        for x, y in ((0.30, -0.48), (0.40, -0.40))[:m.pick(2, 1)]:
            kit.barrel(m, (x, y, -0.01), 0.04, 0.08, "wood_dark")
    if m.detail == 0:
        kit.block(m, (-0.58, -0.12, -0.01), (0.1, 0.07, 0.05), "wood_dark", 20)
        kit.block(m, (-0.58, -0.12, 0.04), (0.085, 0.055, 0.014), "gold", 20)
        for i, (x, y) in enumerate(((-0.44, -0.16), (-0.40, -0.10), (-0.47, -0.08))):
            kit.cylinder(m, (x, y, -0.01), 0.022, 0.03 + 0.015 * i, "gold", sides=6)


def _gable_house(m, c, w, d, h, wall):
    """Canal house: walls and a stepped gable facing the camera, windows in rows."""
    x, y = c
    kit.block(m, (x, y, -0.02), (w, d, h + 0.02), wall)
    for i in range(m.pick(3, 3, 2)):
        f = 1.0 - i / 3.0
        kit.block(m, (x, y, h + 0.045 * i), (w * f, d * 0.94, 0.045), "slate" if i == 0 and m.detail < 2 else wall)
    if m.detail == 0:
        for z in (0.04, h * 0.6):
            for sx in (-1, 1):
                kit.block(m, (x + sx * w * 0.25, y - d / 2 - 0.003, z), (w * 0.18, 0.006, h * 0.22), "glass_dark")


def _quay_crane(m, c):
    x, y = c
    kit.block(m, (x, y, -0.02), (0.12, 0.12, 0.05), "rock_dark")
    m.rod((x, y, 0.0), (x, y, 0.42), 0.02, 0.016, "wood_dark", sides=m.pick(4, 4, 3))
    tip = (x - 0.22, y - 0.22, 0.44)
    m.rod((x, y, 0.12), tip, 0.014, 0.01, "wood_dark", sides=m.pick(4, 3, 3))
    if m.detail < 2:
        _beam(m, (x, y, 0.42), tip, 0.005, "rope", sides=3)
        _beam(m, tip, (tip[0], tip[1], 0.2), 0.004, "rope", sides=3)
        kit.crate(m, (tip[0], tip[1], 0.13), 0.08, "wood", 20)


# Holy site --------------------------------------------------------------------------------
def holy_site(m):
    """White stupa with a gold spire on stepped terraces; prayer flags strung to two poles."""
    with m.at(move(0.0, 0.58, 0.0), scale(1.25)):
        _stupa(m)
    _prayer_flags(m, (0.0, 0.58, 0.62))
    if m.detail < 2:
        kit.patch(m, (0.0, 0.26), (0.07, 0.06), "limestone", z1=0.008)
    if m.detail == 0:
        for sx in (-1, 1):
            _chorten(m, (sx * 0.46, -0.30))
        kit.tree(m, (0.56, 0.52, 0.0), 0.36, "broadleaf", leaf="leaf", seed=12)


def _stupa(m):
    kit.steps(m, (0.0, 0.0, -0.03), 0.24, 0.045, 2, "limestone", shrink=0.05)
    kit.cylinder(m, (0.0, 0.0, 0.06), 0.17, 0.04, "white", sides=m.pick(12, 8, 6))
    kit.dome(m, (0.0, 0.0, 0.1), 0.17, "white", h=0.16, sides=m.pick(12, 8, 6), steps=m.pick(4, 3, 2))
    kit.block(m, (0.0, 0.0, 0.25), (0.08, 0.08, 0.05), "gold")
    levels = [((0.0, 0.0, 0.3), 0.035, 0.035)]
    for i in range(m.pick(5, 3, 1)):
        z = 0.32 + i * 0.05
        r = 0.04 - i * 0.005
        levels += [((0.0, 0.0, z), r, r), ((0.0, 0.0, z + 0.03), r * 0.62, r * 0.62)]
    levels.append(((0.0, 0.0, 0.62), 0.0, 0.0))
    m.rings(levels, "gold", sides=m.pick(6, 5, 4))


def _prayer_flags(m, top):
    colors = ("roof_blue", "white", "red", "green", "banana")
    for sx in (-1, 1):
        pole = (sx * 0.62, 0.22)
        if m.detail < 2:
            _beam(m, (pole[0], pole[1], -0.02), (pole[0], pole[1], 0.24), 0.009, "wood", sides=3)
        if m.detail == 2:
            continue
        end = (pole[0], pole[1], 0.23)
        _beam(m, top, end, 0.003, "rope", sides=3)
        n = m.pick(5, 2)
        for i in range(n):
            t = (i + 1) / (n + 1)
            p = (top[0] + (end[0] - top[0]) * t, top[1] + (end[1] - top[1]) * t, top[2] + (end[2] - top[2]) * t - 0.004)
            with m.at(move(*p)):
                m.prism([(-0.024, 0.0), (0.024, 0.0), (0.0, -0.05)], 0.006, colors[(i + sx) % 5])


def _chorten(m, c):
    x, y = c
    kit.block(m, (x, y, -0.02), (0.1, 0.1, 0.05), "limestone")
    kit.dome(m, (x, y, 0.03), 0.045, "white", h=0.05, sides=6, steps=2)
    _spike(m, (x, y, 0.07), (x, y, 0.14), 0.012, "gold")


# City ruins -------------------------------------------------------------------------------
def city_ruins(m):
    """Broken walls with jagged tops, a few standing columns and toppled drums in the grass."""
    _broken_wall(m, (-0.70, 0.20), (-0.18, 0.70), (0.24, 0.18, 0.07, 0.22), "stone")
    _broken_wall(m, (0.22, 0.74), (0.66, 0.36), (0.14, 0.2, 0.1), "brick")
    for i, (x, y, h) in enumerate(((0.50, 0.02, 0.30), (0.62, -0.16, 0.18), (0.38, 0.18, 0.30))):
        kit.cylinder(m, (x, y, -0.02), 0.03, h + 0.02, "marble", sides=m.pick(6, 5, 4))
        if m.detail == 0 and h > 0.25:
            kit.block(m, (x, y, h), (0.08, 0.08, 0.025), "marble")
    if m.detail < 2:
        with m.at(move(0.44, 0.10, 0.0), rot_z(-55)):
            m.box((0.0, 0.0, 0.33), (0.15, 0.03, 0.022), "marble")
        for i, (x, y, yaw) in enumerate(((-0.34, -0.50, 10), (-0.18, -0.56, 18))):
            _bale(m, (x, y, -0.015), 0.03, 0.11, "marble", yaw=yaw)
        kit.patch(m, (-0.05, -0.38), (0.12, 0.08), "limestone", 12, z1=0.006)
        kit.patch(m, (0.18, -0.48), (0.08, 0.07), "limestone", -20, z1=0.006)
    if m.detail == 0:
        for x, y, s in ((-0.56, -0.18, 0.06), (0.04, -0.66, 0.05), (0.66, 0.16, 0.05)):
            kit.rock(m, (x, y, -0.02), s, "stone", seed=int(x * 100), squash=0.6)
        for x, y in ((-0.30, 0.50), (0.42, 0.56)):
            _shrub(m, (x, y), 0.07, 0.1, "leaf")


def _broken_wall(m, a, b, heights, mat):
    n = len(heights)
    dx, dy = (b[0] - a[0]) / n, (b[1] - a[1]) / n
    yaw = math.degrees(math.atan2(dy, dx))
    seg = math.hypot(dx, dy)
    for i, h in enumerate(heights):
        if m.detail == 2 and h < 0.1:
            continue
        cx, cy = a[0] + dx * (i + 0.5), a[1] + dy * (i + 0.5)
        kit.block(m, (cx, cy, -0.02), (seg * 1.02, 0.05, h + 0.02), mat, yaw)
        if m.detail == 0 and h > 0.15:
            kit.block(m, (cx + dx * 0.2, cy + dy * 0.2, h), (seg * 0.4, 0.05, 0.03), mat, yaw)


# Ancient ruins (goody hut) ----------------------------------------------------------------
def ancient_ruins(m):
    """Overgrown stone circle with a trilithon arch at the back and ivy on the stones."""
    _trilithon(m, (0.0, 0.62))
    stones = [(200, 0.6, 0.26), (238, 0.6, 0.2), (302, 0.6, 0.22), (340, 0.6, 0.28), (20, 0.62, 0.3),
              (160, 0.62, 0.28)]
    for i, (a, r, h) in enumerate(stones[:m.pick(6, 6, 4)]):
        x, y = _polar(r, a)
        with m.at(move(x, y, 0.0), rot_z(a + 90)):
            m.taper((0, 0, -0.03), (0.065, 0.045), (0.045, 0.032), h + 0.03, "stone", shift=(0.0, 0.008 * (i % 3 - 1)))
            if m.detail < 2:
                m.taper((0, 0, h - 0.004), (0.047, 0.034), (0.03, 0.022), 0.03, "leaf")
            if m.detail == 0:
                m.box((0.012, -0.046, h * 0.4), (0.05, 0.006, h * 0.32), "leaf_dark")
    if m.detail < 2:
        x, y = _polar(0.6, 272)
        with m.at(move(x, y, 0.0), rot_z(20)):
            m.box((0.0, 0.0, 0.025), (0.13, 0.05, 0.04), "stone")
        for x, y, s in ((-0.66, 0.34, 0.11), (0.68, 0.30, 0.1), (0.30, 0.74, 0.09), (-0.30, -0.66, 0.08)):
            _shrub(m, (x, y), s, s * 1.5, "leaf_dark" if x < 0 else "leaf")
    if m.detail == 0:
        kit.tree(m, (-0.36, 0.74, 0.0), 0.4, "broadleaf", leaf="leaf_dark", seed=2)
        kit.patch(m, (0.0, 0.0), (0.1, 0.07), "stone", 15, z1=0.03)


def _trilithon(m, c):
    x, y = c
    for sx in (-1, 1):
        m.taper((x + sx * 0.14, y, -0.03), (0.06, 0.055), (0.05, 0.045), 0.42, "stone")
    m.box((x + 0.01, y, 0.43), (0.23, 0.06, 0.04), "stone")
    if m.detail < 2:
        m.box((x - 0.06, y, 0.475), (0.12, 0.045, 0.008), "leaf")
    if m.detail == 0:
        m.box((x - 0.14, y - 0.057, 0.18), (0.055, 0.006, 0.15), "leaf_dark")
        m.box((x + 0.08, y - 0.062, 0.42), (0.1, 0.006, 0.035), "leaf_dark")


# Terrace farm -----------------------------------------------------------------------------
TERRACE_SEED = 17
TERRACE_RINGS = [(0.50, 0.05, "leaf_light"), (0.67, 0.10, "wheat"), (0.83, 0.15, "leaf_light")]


def _terrace_rings(seed=TERRACE_SEED, segments=9):
    """Radius of each ring edge at each of `segments` uneven angles: [(angle, [r_yard, r1, r2, r3]), ...]. The
    edges wander, so the terraces read as contours of a hollow, not as three hexagons."""
    cuts = [360.0 * (k + 0.55 * (kit.rand(seed, k) - 0.5)) / segments + 14.0 for k in range(segments)]
    out = []
    for k, deg in enumerate(cuts):
        reach = 0.93 * _hex_reach(deg)
        radii = [0.32 + 0.025 * kit.rand(seed, 40 + k)]
        for i, (base, _, _) in enumerate(TERRACE_RINGS):
            radii.append(min(base + 0.07 * (kit.rand(seed, 50 + 8 * i + k) - 0.5) + 0.03 * math.sin(math.radians(2 * deg + 60 * i)),
                             reach - 0.02 * (2 - i)))
        out.append((deg, radii))
    return out


def terrace_farm(m):
    """Inca terraces in the manner of Moray: three uneven rings stepping up from the yard, stone retaining walls
    with green crop beds on top, a few steps and a hut at the back. The ring edges follow no hexagon."""
    cuts = _terrace_rings()
    n = len(cuts)
    for i, (_, h0, crop) in enumerate(TERRACE_RINGS):
        if m.detail == 1 and i == 0:
            continue
        for k in range(n):
            # The inner rings are broken: where the hollow is shallow the ground stays open.
            if kit.rand(TERRACE_SEED, 70 + 9 * i + k) > (0.55, 0.8, 1.0)[i]:
                continue
            h = h0 * (0.85 + 0.3 * kit.rand(TERRACE_SEED, 100 + 9 * i + k))
            (a0, r0), (a1, r1) = cuts[k], cuts[(k + 1) % n]
            inner = [_polar(r0[i], a0), _polar(r1[i], a1)]
            outer = [_polar(r1[i + 1], a1), _polar(r0[i + 1], a0)]
            if m.detail == 2:
                if i == 2:
                    kit.slab(m, inner + outer, -0.04, h, crop)
                continue
            if m.detail > 0:
                kit.slab(m, inner + outer, -0.04, h, crop)
                continue
            # LOD0: the stone wall, then a crop bed set back from the wall on top of it.
            kit.slab(m, inner + outer, -0.04, h - 0.012, "rock")
            cx, cy = sum(p[0] for p in inner + outer) / 4, sum(p[1] for p in inner + outer) / 4
            bed = [(cx + (p[0] - cx) * 0.93 + (p[0] - cx) * 0.0, cy + (p[1] - cy) * 0.93) for p in inner + outer]
            kit.slab(m, bed, h - 0.014, h, crop)
    if m.detail == 0:
        # Stone stairs up the front, a short flight cut across the rings.
        for i in range(3):
            x, y = _polar(0.40 + 0.17 * i, 272)
            kit.block(m, (x, y, -0.02), (0.09, 0.06, 0.03 + 0.05 * i), "stone", 0)
    if m.detail < 2:
        kit.house(m, _polar(0.80, 98) + (0.12,), 0.12, 0.08, 0.07, "stone", "grass_dry", yaw=-8, roof_h=0.07)


def models():
    farms = [_model("farm", farm("default"))]
    for variant in ("grassland", "plains", "desert"):
        farms.append(WorldModel(f"improvement/farm-{variant}", farm(variant), "improvement", "plane"))
    farms[0].ground = "plane"
    others = [
        _model("mine", mine),
        _model("trading-post", trading_post),
        _model("lumber-mill", lumber_mill),
        _model("pasture", pasture, "plane"),
        _model("fishing-boats", fishing_boats, "water"),
        _model("oil-well", oil_well),
        _model("fort", fort, "plane"),
        _model("customs-house", customs_house),
        _model("holy-site", holy_site),
        _model("city-ruins", city_ruins),
        _model("ancient-ruins", ancient_ruins),
        _model("terrace-farm", terrace_farm, "plane"),
    ]
    return farms + others
