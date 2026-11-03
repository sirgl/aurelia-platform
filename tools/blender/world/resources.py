"""Resource props of Classic (classic/docs/classic/resources.json): what the player sees on a tile
that holds the resource. Props stand in the yard (radius 0.30-0.40 around the tile anchor),
the improvement of the resource is drawn around them (improvements.py). Resources with
no map presence are listed in world_catalog.ICON_ONLY instead.

Budget class "resource": LOD0 <= 300, LOD1 <= 140, LOD2 <= 48 triangles. The far marker
(detail 2) keeps a few big solids that carry the colour and silhouette of the prop.
Land props are authored a little small and scaled up as a whole by `_model` (factor k).

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/resources.py, which imports the
helpers it shares from here.
"""
import math

from geom import move, rot_y, rot_z, scale
import kit
from world_catalog import WorldModel


def _model(slug, build, ground="upright", k=1.2):
    def scaled(m):
        with m.at(scale(k)):
            build(m)
    return WorldModel("resource/" + slug.replace("_", "-"), scaled, "resource", ground)


def _keep(m, i, counts):
    """True while item i is within the count kept at the current detail (LOD0, LOD1, LOD2)."""
    return i < counts[m.detail]


# Shared small parts ------------------------------------------------------------------------
def _crystal(m, x, y, h, r, mat, lean=0.0, yaw=0.0, sides=None):
    """Hexagonal crystal with a pointed tip, base sunk below the ground, leaning `lean` deg."""
    with m.at(move(x, y, -0.02), rot_z(yaw), rot_y(lean)):
        m.rings([((0, 0, -0.02), r, r), ((0, 0, h * 0.72), r, r), ((0, 0, h), 0.0, 0.0)], mat,
                sides=sides or m.pick(6, 5, 4))


# Animals --------------------------------------------------------------------------------
class Beast:
    """Proportions of a four-legged animal in units of its body length (facing +x)."""

    def __init__(self, leg, R, W, neck, neck_r, head, leg_r=0.05, rise=0.0, stride=0.07, neck_lod=1, sides=5,
                 cone_legs=False, lean=False):
        self.leg, self.R, self.W, self.sides = leg, R, W, sides
        self.neck, self.neck_r = neck, neck_r  # neck (dx, dz) from the shoulder
        self.head = head  # (length, radius, droop degrees, tip radius factor)
        self.leg_r, self.rise, self.stride = leg_r, rise, stride
        self.neck_lod = neck_lod  # highest detail level that still draws the neck (-1: no neck at all)
        self.cone_legs = cone_legs  # legs end in a point (4 triangles instead of 8)
        self.lean = lean  # three-sided neck and head (the herds: 8-10 triangles each instead of 16)


def _beast(m, b, coat, legs=None, face=None, posture=0.0):
    """Body, legs, neck and head; returns (shoulder, head base, head tip) for accessories.

    Called inside the animal's frame (see `_frame`): coordinates are in body lengths, the
    animal faces +x and its feet stand on z = 0. `posture` 0 holds the head up, 1 lowers it to the grass
    (the neck stretches down and forward and the head points at the ground); it is baked, the shader's
    graze motion moves the heads on top of it.
    """
    legs, face = legs or coat, face or coat
    zc = b.leg + b.R
    shoulder = (0.34, 0.0, zc + b.R * 0.35)
    hl, hr, droop, tip = b.head
    down = math.radians(min(droop + 40.0, 80.0))
    neck_dz = b.neck[1] + posture * ((0.03 + hl * math.sin(down)) - shoulder[2] - b.neck[1])
    hb = (shoulder[0] + b.neck[0] * (1.0 + 0.25 * posture), 0.0, shoulder[2] + neck_dz)
    a = math.radians(droop + (math.degrees(down) - droop) * posture)
    ht = (hb[0] + math.cos(a) * hl, 0.0, hb[2] - math.sin(a) * hl)
    if m.detail == 2:
        # Far marker: one block for body and legs, one cone for neck and head.
        top0, top1 = zc + b.R * 0.8, zc + b.R * (0.8 + b.rise)
        m.loft([[(-0.5, -b.W, -0.03), (-0.5, b.W, -0.03), (-0.5, b.W * 0.8, top0), (-0.5, -b.W * 0.8, top0)],
                [(0.45, -b.W, -0.03), (0.45, b.W, -0.03), (0.45, b.W * 0.8, top1), (0.45, -b.W * 0.8, top1)]], coat)
        m.rod(shoulder, ht, max(b.neck_r, hr) * 1.3, 0.0, face, sides=3)
        return shoulder, hb, ht
    if m.detail == 0:
        levels = [(-0.5, 0.0, 0.0), (-0.42, 0.85, 0.85), (0.0, 1.0, 1.0), (0.36, 0.92, 0.95 + b.rise * 0.6),
                  (0.5, 0.0, 0.0)]
    else:
        levels = [(-0.5, 0.0, 0.0), (-0.36, 0.9, 0.9), (0.3, 0.95, 1.0 + b.rise * 0.6), (0.5, 0.0, 0.0)]
    m.rings([((x, 0.0, zc + b.rise * b.R * max(x, 0.0)), b.W * kw, b.R * kr) for x, kw, kr in levels], coat,
            axis="x", sides=m.pick(b.sides, 4, 4))
    if m.detail <= b.neck_lod:
        m.rod(shoulder, hb, b.neck_r, b.neck_r * 0.8, coat, sides=3 if b.lean else m.pick(4, 3, 3))
    m.rod(hb, ht, hr, hr * tip, face, sides=3 if b.lean else m.pick(4, 3, 3))
    for i, (lx, s) in enumerate(((0.32, -1), (0.32, 1), (-0.33, -1), (-0.33, 1))):
        step = b.stride * (1 if i in (0, 3) else -1)
        hip = (lx, s * b.W * 0.55, zc - b.R * 0.3)
        foot = (lx + step, s * b.W * 0.55, -0.03)
        thin = 0.0 if (b.cone_legs or m.detail > 0) else b.leg_r * 0.55
        m.rod(hip, foot, b.leg_r, thin, legs, sides=3)
    return shoulder, hb, ht


def _herd(m, seed, counts, spread, gap, draw, size=1.0, down=0.4):
    """A herd scattered over the tile: `counts` animals at (LOD0, LOD1, LOD2), at least `gap` apart inside a
    disc of radius `spread` (Poisson dart throwing, so never a clump and never a ring), each with its own size
    (+-12 %), heading and posture; about `down` of them graze with the head already at the grass. Animal i is
    at the same place at every LOD: the far marker keeps the first ones. `draw(m, x, y, yaw, L, i, posture)`."""
    spots = kit.scatter(seed, counts[0], 0.05, spread, gap, tries=120)
    for i, (x, y) in enumerate(spots[:counts[m.detail]]):
        yaw = 360.0 * kit.rand(seed, 300 + i)
        L = size * (0.88 + 0.24 * kit.rand(seed, 310 + i))
        draw(m, x, y, yaw, L, i, 0.9 if kit.rand(seed, 320 + i) < down else 0.0)


def _frame(m, x, y, yaw, L):
    return m.at(move(x, y, 0.0), rot_z(yaw), scale(L))


def _pair_cones(m, base, out, r, mat, detail_max=1):
    """Horns or ears: two cones mirrored in y from base (x, +-y, z) to base + out."""
    if m.detail > detail_max:
        return
    for s in (-1, 1):
        a = (base[0], s * base[1], base[2])
        m.rod(a, (a[0] + out[0], s * (base[1] + out[1]), a[2] + out[2]), r, 0.0, mat, sides=3)


def _tail(m, root, tip, r, mat, detail_max=0):
    if m.detail <= detail_max:
        m.rod(root, tip, r, 0.0, mat, sides=3)


HORSE = Beast(leg=0.62, R=0.17, W=0.14, neck=(0.2, 0.36), neck_r=0.085, head=(0.32, 0.075, 62, 0.6), leg_r=0.05,
              lean=True)


def horse(m, x, y, yaw, L, coat="horse", mane="horse_dark", posture=0.0):
    with _frame(m, x, y, yaw, L):
        sh, hb, ht = _beast(m, HORSE, coat, posture=posture)
        if m.detail < 2:
            m.rod((sh[0] - 0.04, 0, sh[2] + 0.06), (hb[0] - 0.02, 0, hb[2] + 0.06), 0.035, 0.03, mane, sides=3)
            _tail(m, (-0.48, 0, 0.9), (-0.6, 0, 0.42), 0.05, mane, detail_max=1)


# Minerals -----------------------------------------------------------------------------------
def iron(m):
    kit.rock(m, (-0.08, 0.08, -0.02), 0.2, "rock_red", seed=3, squash=0.95)
    kit.rock(m, (0.17, 0.14, -0.02), 0.13, "rust", seed=7, squash=0.9)
    if m.detail < 2:
        kit.rock(m, (-0.21, -0.15, -0.02), 0.1, "rust", seed=11)
        # Dark iron veins: thin plates cutting through the ore boulders.
        for x, y, z, yaw, tilt in ((-0.08, 0.06, 0.1, 30, 25), (-0.06, 0.1, 0.14, -60, -20), (0.17, 0.12, 0.08, 75, 30)):
            with m.at(move(x, y, z), rot_z(yaw), rot_y(tilt)):
                m.octa((0, 0, 0), (0.2, 0.01, 0.05), "dark")
    # Smelted iron bars stacked in front.
    with m.at(move(0.1, -0.15, 0.0), rot_z(-15)):
        for i, (bx, bz) in enumerate(((-0.035, -0.01), (0.035, -0.01), (0.0, 0.025))):
            if _keep(m, i, (3, 3, 1)):
                m.taper((bx, 0, bz), (0.03, 0.07), (0.022, 0.06), 0.035, "iron")


HORSE_COATS = (("horse", "horse_dark"), ("horse_light", "horse_dark"), ("hide", "horse_dark"))


def horses(m):
    """Three horses spread over the tile, two grazing; their size and heading differ."""
    def draw(m, x, y, yaw, L, i, posture):
        coat, mane = HORSE_COATS[i % len(HORSE_COATS)]
        horse(m, x, y, yaw, L, coat, mane, posture)
    _herd(m, 61, (3, 2, 2), 0.40, 0.42, draw, size=0.28)


def coal(m):
    kit.peak(m, (0.0, 0.08, 0.0), 0.24, 0.2, "coal", seed=4, sides=m.pick(7, 6, 5))
    for i, (x, y, r) in enumerate(((-0.22, -0.12, 0.07), (0.2, -0.1, 0.06), (0.02, -0.22, 0.05))):
        if _keep(m, i, (3, 3, 1)):
            kit.rock(m, (x, y, -0.01), r, "coal", seed=20 + i)
    if m.detail < 2:
        # A wooden barrow of coal at the foot of the heap.
        with m.at(move(0.22, 0.12, 0.0), rot_z(-30)):
            m.taper((0, 0, 0.02), (0.05, 0.035), (0.07, 0.05), 0.06, "wood")
            m.taper((0, 0, 0.075), (0.06, 0.042), (0.03, 0.02), 0.03, "coal")
            if m.detail == 0:
                m.wheel((0.07, 0, 0.025), 0.025, 0.012, "wood_dark", axis="y")
                for s in (-1, 1):
                    m.rod((-0.06, s * 0.03, 0.06), (-0.14, s * 0.035, 0.05), 0.008, 0.008, "wood_dark", sides=3)


def oil(m):
    # Black seep pool with an iridescent sheen, a tar mound and oil barrels at the rim.
    sides = m.pick(9, 7, 6)
    if m.detail < 2:
        m.rings([((0.0, 0.04, -0.04), 0.27, 0.22), ((0.0, 0.04, 0.008), 0.27, 0.22)], "slate", sides=sides)
    m.rings([((0.0, 0.04, -0.04), 0.23, 0.18), ((0.0, 0.04, 0.014), 0.23, 0.18)], "black", sides=sides)
    kit.dome(m, (-0.06, 0.08, 0.0), 0.09, "black", h=0.1, sides=m.pick(7, 5, 4), steps=m.pick(3, 2, 1))
    if m.detail == 0:
        for x, y, r in ((0.08, -0.02, 0.06), (-0.02, 0.17, 0.045)):
            m.rings([((x, y, 0.012), r, r * 0.6), ((x, y, 0.019), r, r * 0.6)], "glass", sides=5)
        for x, y, r in ((0.1, 0.1, 0.03), (-0.14, -0.04, 0.025)):
            kit.dome(m, (x, y, 0.01), r, "obsidian", h=r * 0.8, sides=5, steps=2)
    for i, (x, y) in enumerate(((0.25, -0.1), (0.3, 0.02), (-0.28, -0.08))):
        if _keep(m, i, (3, 2, 1)):
            kit.barrel(m, (x, y, -0.01), 0.045, 0.13, "dark")
            if m.detail == 0 and i < 2:
                kit.cylinder(m, (x, y, 0.08), 0.048, 0.018, "red", sides=6)


def aluminum(m):
    kit.rock(m, (-0.04, 0.08, -0.02), 0.21, "rock_red", seed=12, squash=0.6)
    if m.detail < 2:
        kit.rock(m, (0.2, -0.08, -0.02), 0.1, "rust", seed=13)
    crystals = ((-0.06, 0.04, 0.32, 0.05, 0, 0), (0.06, 0.1, 0.24, 0.04, 22, 30), (-0.16, 0.12, 0.22, 0.038, -24, 70),
                (-0.02, -0.1, 0.17, 0.035, 18, -60), (0.12, -0.04, 0.14, 0.03, 30, -20))
    for i, (x, y, h, r, lean, yaw) in enumerate(crystals):
        if _keep(m, i, (5, 4, 2)):
            _crystal(m, x, y, h, r, "aluminum", lean, yaw)


def models():
    return [
        _model("iron", iron, k=1.15),
        _model("horses", horses, "plane"),
        _model("coal", coal),
        _model("oil", oil),
        _model("aluminum", aluminum),
    ]
