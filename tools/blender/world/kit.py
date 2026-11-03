"""Shared low-poly parts for world objects: improvements, resources, natural wonders,
world wonders and city pieces. Pure Python on top of units/geom.py (no bpy).

Authoring frame
- Hex units, Blender z-up: x right, y away from the camera, z up. The camera looks along
  +y from the -y side and 45-60 degrees down, so the front of a model is -y.
- One map tile is a pointy-top hex of outer radius 1.0: corners at (0, +-1) and
  (+-0.866, +-0.5); inner radius 0.866 (edge midpoints at angles 0, 60, ... degrees).
- z = 0 is the ground at the tile anchor. Foundations and field slabs reach a little
  below 0 (down to -0.06) so the model never floats on slightly uneven ground.
- A human is about 0.12-0.16 tall next to buildings (world scale, not unit scale).

Grounding (manifest `ground`, applied by the renderer from the final surface mesh)
- "upright": rigid; placed at the lowest ground under its footprint. Buildings, animals.
- "plane": sheared onto the ground plane fitted under the tile. Fields, terraces, pits.
- "water": floats at the water surface; parts below z = 0 are under water.

Every helper takes the `Mesh` as its first argument and honours `m.detail` (0 close-up,
1 reduced, 2 far marker): small accessories are skipped at detail 2 and simplified at 1.
Colors are palette names from geom.PALETTE; "team" is tinted with the owner color.
"""
import math

from geom import move, rot_z, rot_x, scale

SQRT3 = math.sqrt(3.0)
HEX_INNER = SQRT3 / 2.0


# Hash and layout ------------------------------------------------------------------------
def rand(seed, i=0):
    """Deterministic pseudo-random float in [0, 1) from integers (no global state)."""
    x = (seed * 0x9E3779B1 + i * 0x85EBCA77 + 0x165667B1) & 0xFFFFFFFF
    x ^= x >> 16
    x = (x * 0x7FEB352D) & 0xFFFFFFFF
    x ^= x >> 15
    x = (x * 0x846CA68B) & 0xFFFFFFFF
    x ^= x >> 16
    return (x & 0xFFFFFF) / float(0x1000000)


def hex_corners(r=1.0):
    """Corners of a pointy-top hex, counter-clockwise from the top (0, r)."""
    return [(r * math.cos(math.radians(90 + 60 * k)), r * math.sin(math.radians(90 + 60 * k))) for k in range(6)]


def ring(count, r, phase=0.0):
    """`count` points on a circle of radius r, starting at `phase` degrees."""
    return [(r * math.cos(math.radians(phase + 360.0 * k / count)),
             r * math.sin(math.radians(phase + 360.0 * k / count))) for k in range(count)]


def scatter(seed, count, r_min, r_max, min_gap, center=(0.0, 0.0), tries=40):
    """Deterministic points in an annulus, at least `min_gap` apart (fewer when crowded)."""
    pts = []
    for i in range(count * tries):
        if len(pts) == count:
            break
        a = rand(seed, 2 * i) * math.tau
        r = r_min + (r_max - r_min) * math.sqrt(rand(seed, 2 * i + 1))
        p = (center[0] + r * math.cos(a), center[1] + r * math.sin(a))
        if all(math.hypot(p[0] - q[0], p[1] - q[1]) >= min_gap for q in pts):
            pts.append(p)
    return pts


# Basic solids -----------------------------------------------------------------------------
def block(m, c, size, mat, yaw=0.0):
    """Box with its base centred at c = (x, y, z0); size = (sx, sy, sz) full extents."""
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        m.box((0, 0, size[2] / 2), (size[0] / 2, size[1] / 2, size[2] / 2), mat)


def slab(m, outline, z0, z1, mat):
    """Vertical prism of a convex outline [(x, y), ...] between z0 and z1."""
    m.loft([[(x, y, z0) for x, y in outline], [(x, y, z1) for x, y in outline]], mat)


def cylinder(m, c, r, h, mat, sides=None, r_top=None):
    """Upright cylinder / frustum, base centred at c = (x, y, z0)."""
    sides = sides or m.pick(8, 6, 5)
    top = r if r_top is None else r_top
    m.rings([((c[0], c[1], c[2]), r, r), ((c[0], c[1], c[2] + h), top, top)], mat, sides=sides)


def cone(m, c, r, h, mat, sides=None):
    sides = sides or m.pick(8, 6, 4)
    m.rings([((c[0], c[1], c[2]), r, r), ((c[0], c[1], c[2] + h), 0.0, 0.0)], mat, sides=sides)


def dome(m, c, r, mat, h=None, sides=None, steps=None):
    """Half-ellipsoid dome on a circle of radius r at height c[2]; h = dome height."""
    h = r if h is None else h
    sides = sides or m.pick(10, 7, 5)
    steps = steps or m.pick(4, 3, 2)
    levels = []
    for j in range(steps):
        t = j / steps * math.pi / 2
        levels.append(((c[0], c[1], c[2] + math.sin(t) * h), math.cos(t) * r, math.cos(t) * r))
    levels.append(((c[0], c[1], c[2] + h), 0.0, 0.0))
    m.rings(levels, mat, sides=sides)


def pyramid(m, c, half, h, mat, top=0.0):
    """Square pyramid (or frustum with `top` half size), base centred at c."""
    m.taper((c[0], c[1], c[2]), (half, half), (top, top) if top > 0 else (1e-3, 1e-3), h, mat)


def steps(m, c, half, rise, count, mat, shrink=None, yaw=0.0):
    """Stacked square terraces (ziggurat, step pyramid, temple platform)."""
    shrink = half / (count + 1) if shrink is None else shrink
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        for i in range(count):
            s = half - i * shrink
            m.box((0, 0, rise * (i + 0.5)), (s, s, rise / 2), mat)


def wedge_roof(m, c, w, d, h, mat, yaw=0.0, overhang=0.012):
    """Gable roof prism: base w x d (full) at height c[2], ridge along x, rise h."""
    hw, hd = w / 2 + overhang, d / 2 + overhang
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        m.loft([[(-hw, -hd, 0.0), (-hw, hd, 0.0), (-hw, 0.0, h)],
                [(hw, -hd, 0.0), (hw, hd, 0.0), (hw, 0.0, h)]], mat)


def hip_roof(m, c, w, d, h, mat, yaw=0.0, overhang=0.012):
    hw, hd = w / 2 + overhang, d / 2 + overhang
    ridge = max(hw - hd, 0.0)
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        if ridge > 1e-3:
            m.loft([[(-hw, -hd, 0.0), (hw, -hd, 0.0), (hw, hd, 0.0), (-hw, hd, 0.0)],
                    [(-ridge, -0.002, h), (ridge, -0.002, h), (ridge, 0.002, h), (-ridge, 0.002, h)]], mat)
        else:
            m.taper((0, 0, 0), (hw, hd), (0.002, 0.002), h, mat)


def house(m, c, w, d, h, wall, roof, yaw=0.0, roof_h=None, kind="gable", door="wood_dark"):
    """Small building: walls w x d x h, roof on top. kind: gable | hip | flat | dome | cone."""
    roof_h = w * 0.45 if roof_h is None else roof_h
    block(m, c, (w, d, h), wall, yaw)
    top = (c[0], c[1], c[2] + h)
    if kind == "gable":
        wedge_roof(m, top, w, d, roof_h, roof, yaw)
    elif kind == "hip":
        hip_roof(m, top, w, d, roof_h, roof, yaw)
    elif kind == "flat":
        block(m, top, (w + 0.01, d + 0.01, 0.012), roof, yaw)
    elif kind == "dome":
        dome(m, top, min(w, d) * 0.45, roof, h=roof_h)
    elif kind == "cone":
        cone(m, top, max(w, d) * 0.62, roof_h, roof)
    if door and m.detail == 0:
        a = math.radians(yaw)
        dx, dy = math.sin(a) * (d / 2 + 0.003), -math.cos(a) * (d / 2 + 0.003)
        block(m, (c[0] + dx, c[1] + dy, c[2]), (min(w * 0.3, 0.035), 0.006, min(h * 0.6, 0.05)), door, yaw)


def tower(m, c, r, h, mat, roof="cone", roof_mat="terracotta", sides=None, roof_h=None):
    """Round tower; roof: cone | crenel | dome | flat | None."""
    sides = sides or m.pick(8, 6, 5)
    cylinder(m, c, r, h, mat, sides=sides)
    top = (c[0], c[1], c[2] + h)
    if roof == "cone":
        cone(m, top, r * 1.2, roof_h or r * 2.2, roof_mat, sides=sides)
    elif roof == "dome":
        dome(m, top, r, roof_mat, h=roof_h or r, sides=sides)
    elif roof == "crenel":
        cylinder(m, top, r * 1.12, 0.02, mat, sides=sides)
        if m.detail == 0:
            for x, y in ring(sides, r * 1.02, 180.0 / sides):
                block(m, (top[0] + x, top[1] + y, top[2] + 0.02), (0.025, 0.025, 0.025), mat)
    elif roof == "flat":
        cylinder(m, top, r * 1.08, 0.015, roof_mat, sides=sides)


def column(m, base, h, r, mat, capital=True, sides=None):
    sides = sides or m.pick(6, 4, 3)
    cylinder(m, base, r, h, mat, sides=sides)
    if capital and m.detail == 0:
        block(m, (base[0], base[1], base[2] + h - 0.008), (r * 3.0, r * 3.0, 0.012), mat)


def colonnade_temple(m, c, w, d, h, mat="marble", roof="terracotta", cols=(6, 4), yaw=0.0, base_h=0.04):
    """Greek temple: stepped base, perimeter columns, entablature and a gable roof."""
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        m.box((0, 0, base_h / 2), (w / 2 + 0.03, d / 2 + 0.03, base_h / 2), mat)
        nx, ny = cols if m.detail < 2 else (2, 2)
        r = min(w / nx, d / ny) * 0.16
        pts = set()
        for i in range(nx):
            x = -w / 2 + r + (w - 2 * r) * i / max(nx - 1, 1)
            pts.add((round(x, 5), round(-d / 2 + r, 5)))
            pts.add((round(x, 5), round(d / 2 - r, 5)))
        for j in range(ny):
            y = -d / 2 + r + (d - 2 * r) * j / max(ny - 1, 1)
            pts.add((round(-w / 2 + r, 5), round(y, 5)))
            pts.add((round(w / 2 - r, 5), round(y, 5)))
        if m.detail == 0:
            m.box((0, 0, base_h + h * 0.45), (w / 2 - r * 3, d / 2 - r * 3, h * 0.45), mat)  # cella
        for x, y in sorted(pts):
            column(m, (x, y, base_h), h, r, mat, capital=m.detail == 0)
        m.box((0, 0, base_h + h + 0.012), (w / 2 + 0.008, d / 2 + 0.008, 0.012), mat)
        wedge_roof(m, (0, 0, base_h + h + 0.024), d, w, w * 0.18, roof, yaw=90, overhang=0.01)


def arch(m, c, w, h, depth, mat, yaw=0.0, opening="dark"):
    """Gate: two piers and a lintel; a dark recess suggests the opening."""
    with m.at(move(c[0], c[1], c[2]), rot_z(yaw)):
        pier = w * 0.22
        for s in (-1, 1):
            m.box((s * (w / 2 - pier / 2), 0, h * 0.5), (pier / 2, depth / 2, h * 0.5), mat)
        m.box((0, 0, h * 0.86), (w / 2, depth / 2, h * 0.14), mat)
        if opening and m.detail == 0:
            m.box((0, 0, h * 0.36), (w / 2 - pier, depth * 0.3, h * 0.36), opening)


def wall_line(m, a, b, h, thick, mat, crenels=True):
    """Straight wall from a = (x, y, z0) to b along the ground, optional merlons."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = math.hypot(dx, dy)
    yaw = math.degrees(math.atan2(dy, dx))
    cx, cy = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
    block(m, (cx, cy, a[2]), (length, thick, h), mat, yaw)
    if crenels and m.detail == 0:
        n = max(int(length / 0.06), 2)
        for i in range(n):
            t = (i + 0.5) / n
            block(m, (a[0] + dx * t, a[1] + dy * t, a[2] + h), (length / n * 0.5, thick * 1.05, 0.02), mat, yaw)


def flag(m, c, h, mat="team", size=0.07, pole="wood_dark"):
    """Pole with a pennant at the top, facing the camera."""
    m.rod((c[0], c[1], c[2]), (c[0], c[1], c[2] + h), 0.006, 0.005, pole, sides=3)
    if m.detail < 2:
        with m.at(move(c[0], c[1], c[2] + h)):
            m.prism([(0.0, 0.0), (size, -size * 0.3), (0.0, -size * 0.6)], 0.006, mat)


# Ground cover -----------------------------------------------------------------------------
def patch(m, c, half, mat, yaw=0.0, z0=-0.05, z1=0.012):
    """Flat rectangular ground slab (field, yard, paving) slightly sunk into the ground."""
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        m.box((0, 0, (z0 + z1) / 2), (half[0], half[1], (z1 - z0) / 2), mat)


def hex_patch(m, r, mat, z0=-0.05, z1=0.01, center=(0.0, 0.0)):
    slab(m, [(center[0] + x, center[1] + y) for x, y in hex_corners(r)], z0, z1, mat)


def crop_rows(m, c, half, yaw, rows, mat, height=0.035, soil="soil", z_soil=0.012, along="x"):
    """Field of parallel ridges: a soil slab and `rows` triangular crop ridges."""
    if m.detail == 2:
        patch(m, c, half, mat, yaw, z1=z_soil + height * 0.5)
        return
    patch(m, c, half, soil, yaw, z1=z_soil)
    rows = rows if m.detail == 0 else max(rows // 2, 2)
    with m.at(move(c[0], c[1], 0.0), rot_z(yaw)):
        span, width = (half[0], half[1]) if along == "x" else (half[1], half[0])
        pitch = 2 * width / rows
        for i in range(rows):
            off = -width + pitch * (i + 0.5)
            w = pitch * 0.42
            if along == "x":
                m.loft([[(-span, off - w, z_soil), (-span, off + w, z_soil), (-span, off, z_soil + height)],
                        [(span, off - w, z_soil), (span, off + w, z_soil), (span, off, z_soil + height)]], mat)
            else:
                m.loft([[(off - w, -span, z_soil), (off + w, -span, z_soil), (off, -span, z_soil + height)],
                        [(off - w, span, z_soil), (off + w, span, z_soil), (off, span, z_soil + height)]], mat)


def convex_hull(pts):
    """Convex hull of (x, y) points, counter-clockwise (Andrew's monotone chain)."""
    pts = sorted(set((round(x, 6), round(y, 6)) for x, y in pts))
    if len(pts) < 3:
        return pts

    def turn(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and turn(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and turn(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def pebble(c, rx, ry, yaw, sides, seed, jitter=0.18):
    """A convex, irregular outline (a field, a pond, a trampled patch): an ellipse of `sides` points with
    jittered radii and angles, rotated by `yaw` degrees and centred at c. Never a regular polygon."""
    pts = []
    a0 = math.radians(yaw)
    for k in range(sides):
        t = math.tau * (k + (rand(seed, k) - 0.5) * 0.55) / sides
        f = 1.0 + (rand(seed, 40 + k) - 0.5) * 2 * jitter
        x, y = math.cos(t) * rx * f, math.sin(t) * ry * f
        pts.append((c[0] + x * math.cos(a0) - y * math.sin(a0), c[1] + x * math.sin(a0) + y * math.cos(a0)))
    return convex_hull(pts)


def point_in_poly(p, poly):
    """True when p is inside the convex counter-clockwise polygon."""
    for a, b in zip(poly, poly[1:] + poly[:1]):
        if (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) < 0:
            return False
    return True


def crop_plot(m, poly, yaw, mat, pitch=0.09, height=0.035, soil="soil", z_soil=0.012, rim=0.012):
    """An irregular field: a soil slab of the convex `poly` and ridges of `mat` that run along direction `yaw`
    (degrees, independent of the outline) and stop where the outline cuts them, so the edge of the plot is
    ragged and each plot reads in its own direction. LOD1 keeps every other ridge, the far marker the slab."""
    if m.detail == 2:
        slab(m, poly, -0.05, z_soil + height * 0.5, mat)
        return
    slab(m, poly, -0.05, z_soil, soil)
    a = math.radians(-yaw)
    ca, sa = math.cos(a), math.sin(a)
    local = [(x * ca - y * sa, x * sa + y * ca) for x, y in poly]  # rows run along local x
    lo, hi = min(p[1] for p in local), max(p[1] for p in local)
    pitch = pitch * (2.3 if m.detail == 1 else 1.0)
    count = max(int((hi - lo) / pitch), 1)
    step = (hi - lo) / count
    w = step * 0.42
    with m.at(rot_z(yaw)):
        for i in range(count):
            y = lo + step * (i + 0.5)
            xs = []
            for p, q in zip(local, local[1:] + local[:1]):
                if (p[1] - y) * (q[1] - y) < 0:
                    xs.append(p[0] + (q[0] - p[0]) * (y - p[1]) / (q[1] - p[1]))
            if len(xs) < 2:
                continue
            x0, x1 = min(xs) + rim, max(xs) - rim
            if x1 - x0 < 0.06:
                continue
            m.loft([[(x0, y - w, z_soil), (x0, y + w, z_soil), (x0, y, z_soil + height)],
                    [(x1, y - w, z_soil), (x1, y + w, z_soil), (x1, y, z_soil + height)]], mat)


def wander_fence(m, pts, h=0.075, mat="wood", post_mat="wood_dark", gaps=(), seed=0, rails=2, low=(), closed=True,
                 post_gap=0.26, front=None):
    """Post-and-rail fence along the polyline `pts` (closed by default) that does not look surveyed: posts at
    uneven spacing, a gap at each `gaps` entry (segment index, from t, to t) for a gate, `low` segments with a
    single rail (as has every segment not in `front`, when given). Rails and posts are three-sided timbers (8 triangles each). LOD1 keeps one rail and a post at
    every corner, the far marker draws nothing (the caller draws its own marker)."""
    if m.detail == 2:
        return
    n = len(pts)
    for i in range(n if closed else n - 1):
        a, b = pts[i], pts[(i + 1) % n]
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dy)
        spans = [(0.0, 1.0)]
        for gi, g0, g1 in gaps:
            if gi == i:
                spans = [(0.0, g0), (g1, 1.0)]
        for s0, s1 in spans:
            seg = (s1 - s0) * length
            if seg < 0.04:
                continue
            p0 = (a[0] + dx * s0, a[1] + dy * s0)
            p1 = (a[0] + dx * s1, a[1] + dy * s1)
            nrails = 1 if (i in low or m.detail == 1 or (front is not None and i not in front)) else rails
            for r in range(nrails):
                z = h * (0.84 - 0.4 * r)
                m.rod((p0[0], p0[1], z), (p1[0], p1[1], z), 0.011, 0.011, mat, sides=3)
            # Posts: uneven spacing, a darker post where a span starts (a corner is shared with the span before it)
            # and where it ends at a gap or at the open end of the line.
            k = 1 if m.detail == 1 else max(int(round(seg / post_gap)), 1)
            ts = sorted(min(max((j + (rand(seed + i, j) - 0.5) * 0.7) / k, 0.1), 0.9) for j in range(1, k))
            ends = [0.0] + ([1.0] if (s1 < 1.0 or (not closed and i == n - 2)) else [])
            for t in ends + ts:
                px, py = p0[0] + (p1[0] - p0[0]) * t, p0[1] + (p1[1] - p0[1]) * t
                top = h * (1.0 + 0.16 * (rand(seed, 90 + i * 11 + int(t * 9)) - 0.5))
                m.rod((px, py, -0.02), (px, py, top), 0.016, 0.012, post_mat if t in ends else mat, sides=3)


def gate(m, hinge, yaw, width, h=0.07, open_deg=70.0, mat="wood", post_mat="wood_dark"):
    """A field gate swung open about its hinge post (LOD0 and LOD1)."""
    if m.detail == 2:
        return
    a = math.radians(yaw + open_deg)
    tip = (hinge[0] + math.cos(a) * width, hinge[1] + math.sin(a) * width)
    for z in (h * 0.84, h * 0.46):
        m.rod((hinge[0], hinge[1], z), (tip[0], tip[1], z), 0.010, 0.010, mat, sides=3)
    if m.detail == 0:
        m.rod((hinge[0], hinge[1], h * 0.84), (tip[0], tip[1], h * 0.46), 0.008, 0.008, mat, sides=3)


def fence(m, pts, h=0.05, mat="wood", posts=True, closed=False):
    """Post-and-rail fence along a polyline of (x, y) ground points."""
    path = list(pts) + ([pts[0]] if closed else [])
    for a, b in zip(path, path[1:]):
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dy)
        yaw = math.degrees(math.atan2(dy, dx))
        block(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, h * 0.7), (length, 0.008, 0.008), mat, yaw)
        if m.detail == 0:
            block(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, h * 0.35), (length, 0.008, 0.008), mat, yaw)
        if posts and m.detail < 2:
            n = max(int(length / 0.12), 1)
            for i in range(n + (0 if closed else 1)):
                t = i / n
                block(m, (a[0] + dx * t, a[1] + dy * t, -0.02), (0.012, 0.012, h + 0.02), mat)


def water_pool(m, c, r, mat="water", z=0.008, sides=None, rim=None):
    """Flat pool (lake, pond, spring) with an optional rim ring."""
    sides = sides or m.pick(10, 7, 6)
    m.rings([((c[0], c[1], -0.04), r, r), ((c[0], c[1], z), r, r)], mat, sides=sides)
    if rim and m.detail < 2:
        m.rings([((c[0], c[1], -0.04), r * 1.15, r * 1.15), ((c[0], c[1], z - 0.004), r * 1.12, r * 1.12)], rim,
                sides=sides)


# Natural forms ---------------------------------------------------------------------------
def rock(m, c, r, mat="rock", seed=0, squash=0.7):
    """Irregular faceted boulder: a jittered octahedron-ish solid, base sunk below c[2]."""
    rx, ry, rz = (r, r * (0.8 + 0.3 * rand(seed, 1)), r * squash)
    pts = []
    n = m.pick(6, 5, 4)
    for k in range(n):
        a = math.tau * (k + rand(seed, k + 3) * 0.4) / n
        f = 0.8 + 0.35 * rand(seed, k + 11)
        pts.append((c[0] + math.cos(a) * rx * f, c[1] + math.sin(a) * ry * f, c[2] - rz * 0.25))
    for k in range(n):
        a = math.tau * (k + 0.5 + rand(seed, k + 23) * 0.3) / n
        f = 0.55 + 0.3 * rand(seed, k + 31)
        pts.append((c[0] + math.cos(a) * rx * f, c[1] + math.sin(a) * ry * f, c[2] + rz * (0.55 + 0.3 * rand(seed, k))))
    top = (c[0] + (rand(seed, 41) - 0.5) * r * 0.3, c[1] + (rand(seed, 43) - 0.5) * r * 0.3, c[2] + rz * 1.05)
    lower = [list(range(n)), list(range(n, 2 * n))]
    m.loft([[pts[i] for i in lower[0]], [pts[i] for i in lower[1]], [top]], mat)


def peak(m, c, r, h, mat="rock", snow=None, snow_frac=0.3, seed=0, sides=None, lean=(0.0, 0.0)):
    """Faceted mountain cone with jittered ridges and an optional snow cap."""
    sides = sides or m.pick(9, 7, 5)
    levels = []
    for j, (t, k) in enumerate(((0.0, 1.0), (0.45, 0.62), (1.0 - snow_frac, 0.3))):
        ring_pts = []
        for i in range(sides):
            a = math.tau * (i + 0.5) / sides
            f = k * (0.85 + 0.3 * rand(seed, i + 7 * j))
            ring_pts.append((c[0] + lean[0] * t + math.cos(a) * r * f,
                             c[1] + lean[1] * t + math.sin(a) * r * f, c[2] + h * t - (0.04 if j == 0 else 0.0)))
        levels.append(ring_pts)
    tip = (c[0] + lean[0], c[1] + lean[1], c[2] + h)
    m.loft([levels[0], levels[1], levels[2]], mat)
    if snow and m.detail < 2:
        m.loft([[(p[0], p[1], p[2] - 0.004) for p in levels[2]], [tip]], snow)
    else:
        m.loft([levels[2], [tip]], mat)


def tree(m, c, h, kind="broadleaf", leaf="leaf", trunk="bark", seed=0):
    """Tree with its base at c. kind: broadleaf | conifer | palm | bush | cypress | fruit."""
    x, y, z = c
    if kind == "conifer":
        cylinder(m, (x, y, z - 0.01), h * 0.05, h * 0.25, trunk, sides=m.pick(5, 4, 3))
        tiers = m.pick(3, 2, 1)
        for i in range(tiers):
            base = z + h * (0.18 + 0.24 * i)
            r = h * (0.32 - 0.07 * i)
            cone(m, (x, y, base), r, h * (0.5 - 0.06 * i), leaf, sides=m.pick(7, 5, 4))
    elif kind == "palm":
        lean = (rand(seed, 1) - 0.5) * h * 0.4
        top = (x + lean, y, z + h)
        m.rod((x, y, z - 0.01), top, h * 0.045, h * 0.03, trunk, sides=m.pick(5, 4, 3))
        fronds = m.pick(6, 4, 3)
        for i in range(fronds):
            a = math.tau * (i + rand(seed, 3)) / fronds
            tip = (top[0] + math.cos(a) * h * 0.42, top[1] + math.sin(a) * h * 0.42, top[2] - h * 0.18)
            m.loft([[(top[0], top[1] - 0.004, top[2] + 0.01), (top[0], top[1] + 0.004, top[2] + 0.01),
                     (top[0] + math.cos(a + 1.2) * h * 0.06, top[1] + math.sin(a + 1.2) * h * 0.06, top[2])],
                    [tip]], leaf)
    elif kind == "bush":
        m.ball((x, y, z + h * 0.45), (h * 0.6, h * 0.55, h * 0.5), leaf, u=m.pick(6, 5, 4), v=m.pick(4, 3, 3))
    elif kind == "cypress":
        cylinder(m, (x, y, z - 0.01), h * 0.04, h * 0.15, trunk, sides=3)
        m.rings([((x, y, z + h * 0.1), h * 0.12, h * 0.12), ((x, y, z + h * 0.45), h * 0.16, h * 0.16),
                 ((x, y, z + h), 0.0, 0.0)], leaf, sides=m.pick(6, 5, 4))
    else:
        cylinder(m, (x, y, z - 0.01), h * 0.06, h * 0.45, trunk, sides=m.pick(5, 4, 3))
        m.ball((x, y, z + h * 0.68), (h * 0.36, h * 0.34, h * 0.32), leaf, u=m.pick(7, 5, 4), v=m.pick(4, 3, 3))
        if kind == "fruit" and m.detail == 0:
            for i in range(4):
                a = math.tau * (i + rand(seed, 9)) / 4
                m.octa((x + math.cos(a) * h * 0.3, y + math.sin(a) * h * 0.28, z + h * 0.62), h * 0.06, trunk)


def grove(m, pts, h, kind="broadleaf", leaf="leaf", trunk="bark", seed=0):
    """Trees at the given (x, y) points with deterministic size variation."""
    for i, (x, y) in enumerate(pts):
        tree(m, (x, y, 0.0), h * (0.85 + 0.3 * rand(seed, i)), kind, leaf, trunk, seed + i)


# Animals and people ---------------------------------------------------------------------
def quadruped(m, c, yaw, body, length=0.16, height=0.08, legs=None, head=None, horns=None, hump=False,
              neck_up=0.3):
    """Generic grazing animal facing +x before `yaw`: cattle, sheep, deer, bison, goats."""
    legs = legs or body
    head = head or body
    L, H = length, height
    with m.at(move(*c), rot_z(yaw)):
        m.ball((0.0, 0.0, H * 1.05), (L * 0.5, L * 0.22, H * 0.42), body, u=m.pick(7, 5, 4), v=m.pick(4, 3, 3))
        if hump:
            m.ball((L * 0.18, 0.0, H * 1.4), (L * 0.22, L * 0.18, H * 0.38), body, u=m.pick(6, 4, 4), v=3)
        hx, hz = L * 0.55, H * (1.15 + neck_up)
        m.rod((L * 0.32, 0.0, H * 1.15), (hx, 0.0, hz), L * 0.09, L * 0.07, body, sides=m.pick(5, 4, 3))
        m.ball((hx + L * 0.05, 0.0, hz - H * 0.05), (L * 0.11, L * 0.07, L * 0.07), head, u=m.pick(5, 4, 4), v=3)
        if m.detail < 2:
            for lx in (L * 0.32, -L * 0.32):
                for s in (-1, 1):
                    m.rod((lx, s * L * 0.1, H * 0.85), (lx, s * L * 0.1, 0.0), L * 0.045, L * 0.03, legs, sides=3)
        if horns and m.detail == 0:
            for s in (-1, 1):
                m.rod((hx + L * 0.02, s * L * 0.04, hz + L * 0.04), (hx - L * 0.04, s * L * 0.14, hz + L * 0.14),
                      L * 0.02, 0.0, horns, sides=3)


def herd(m, pts, yaw_seed, body, **kwargs):
    for i, (x, y) in enumerate(pts):
        quadruped(m, (x, y, 0.0), 360.0 * rand(yaw_seed, i), body, **kwargs)


def person(m, c, yaw=0.0, look=None, height=0.14):
    """A small standing figure (units/people.figure scaled to world size)."""
    import people
    if m.detail >= 2:
        return
    look = look or people.Look(body="cloth", legs="cloth_dark", head="bare")
    saved = m.detail
    m.detail = max(m.detail, 1)
    with m.at(move(*c), rot_z(yaw), scale(height)):
        people.figure(m, look)
    m.detail = saved


def boat(m, c, length, yaw=0.0, hull="hull", sail="sail", mast=True):
    """Small open boat (fishing, ferries); hull crosses the waterline at z = 0."""
    with m.at(move(*c), rot_z(yaw)):
        L, B = length / 2, length * 0.17
        m.loft([[(-L * 0.85, -B * 0.8, 0.04), (-L * 0.85, B * 0.8, 0.04), (-L * 0.75, 0.0, -0.02)],
                [(L * 0.15, -B, 0.045), (L * 0.15, B, 0.045), (L * 0.15, 0.0, -0.03)],
                [(L, 0.0, 0.06)]], hull)
        if mast and m.detail < 2:
            m.rod((0.0, 0.0, 0.03), (0.0, 0.0, length * 0.75), 0.005, 0.004, "wood_dark", sides=3)
            m.prism([(0.02, length * 0.12), (length * 0.32, length * 0.18), (0.02, length * 0.7)], 0.004, sail)


# Props ------------------------------------------------------------------------------------
def crate(m, c, s, mat="wood", yaw=0.0):
    block(m, c, (s, s, s), mat, yaw)


def barrel(m, c, r, h, mat="wood"):
    cylinder(m, c, r, h, mat, sides=m.pick(6, 5, 4))


def log_pile(m, c, length, count, mat="bark", yaw=0.0, r=0.018):
    with m.at(move(*c), rot_z(yaw)):
        k = 0
        row = 0
        while k < count:
            n = max(count - k, 1) if row > 0 else min(count, 3)
            n = min(n, 3 - row) if row < 3 else 1
            for i in range(n):
                y = (i - (n - 1) / 2) * r * 2.1
                m.rod((-length / 2, y, r + row * r * 1.8), (length / 2, y, r + row * r * 1.8), r, r, mat,
                      sides=m.pick(5, 4, 3))
                k += 1
            row += 1
            if row > 3:
                break


def tent(m, c, r, h, mat="linen", yaw=0.0, sides=None):
    sides = sides or m.pick(6, 5, 4)
    with m.at(move(*c), rot_z(yaw)):
        m.rings([((0, 0, -0.01), r, r), ((0, 0, h), 0.0, 0.0)], mat, sides=sides)


def chimney(m, c, r, h, mat="brick", smoke=True):
    cylinder(m, c, r, h, mat, sides=m.pick(6, 5, 4), r_top=r * 0.85)
    if smoke and m.detail == 0:
        m.ball((c[0] + r * 0.6, c[1], c[2] + h + r * 1.6), (r * 1.6, r * 1.3, r * 1.2), "steam", u=5, v=3)


def statue(m, c, h, mat="bronze", plinth="marble", look=None):
    """Figure on a plinth: a landmark at small scale."""
    block(m, c, (h * 0.35, h * 0.35, h * 0.3), plinth)
    if m.detail < 2:
        import people
        look = look or people.Look(body=mat, legs=mat, skin=mat, boots=mat, belt=mat, head="bare", right="staff")
        saved = m.detail
        m.detail = max(saved, 1)
        with m.at(move(c[0], c[1], c[2] + h * 0.3), scale(h * 0.7)):
            people.figure(m, look)
        m.detail = saved
    else:
        cylinder(m, (c[0], c[1], c[2] + h * 0.3), h * 0.08, h * 0.65, mat, sides=4)
