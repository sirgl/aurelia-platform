"""Pure-Python low-poly mesh builder for unit models. No bpy, fully deterministic.

Authoring frame (Blender-native): x = right, y = forward, z = up. Unit models face the
game camera, which looks along +y from the -y side, so the front of a model is -y.
A human figure is 1.0 tall; the manifest scale maps model units to hex units.

Every solid is closed and star-shaped about its centroid, so `Mesh.solid` orients each
face outward by checking it against that centroid. The bake (platform/crates/render/build.rs)
computes flat normals from winding, so outward winding is a hard requirement.

Colors are palette names (see PALETTE). The vertex color alpha is the team mask:
1.0 on "team" parts (owner color in the game), 0.0 everywhere else.
"""
import math

# Authored display-space albedo. Changing a value changes every model that uses it.
PALETTE = {
    "team": (1.0, 1.0, 1.0),
    "skin": (0.80, 0.58, 0.40),
    "skin_dark": (0.52, 0.34, 0.22),
    "hair": (0.22, 0.15, 0.10),
    "cloth": (0.86, 0.80, 0.64),
    "cloth_dark": (0.36, 0.33, 0.30),
    "linen": (0.93, 0.90, 0.80),
    "leather": (0.42, 0.27, 0.15),
    "fur": (0.55, 0.42, 0.28),
    "wood": (0.47, 0.32, 0.17),
    "wood_dark": (0.27, 0.18, 0.11),
    "rope": (0.70, 0.60, 0.40),
    "bronze": (0.78, 0.55, 0.25),
    "gold": (0.90, 0.72, 0.30),
    "iron": (0.46, 0.48, 0.50),
    "steel": (0.68, 0.72, 0.76),
    "dark": (0.16, 0.17, 0.18),
    "olive": (0.40, 0.42, 0.26),
    "khaki": (0.62, 0.56, 0.38),
    "field_grey": (0.44, 0.47, 0.45),
    "navy": (0.18, 0.22, 0.34),
    "white": (0.92, 0.92, 0.88),
    "red": (0.70, 0.16, 0.12),
    "green": (0.22, 0.42, 0.20),
    "feather": (0.20, 0.55, 0.45),
    "sail": (0.90, 0.86, 0.74),
    "hull": (0.36, 0.24, 0.14),
    "hull_grey": (0.50, 0.53, 0.56),
    "deck": (0.66, 0.52, 0.34),
    "glass": (0.30, 0.48, 0.60),
    "fire": (0.95, 0.55, 0.15),
    "horse": (0.48, 0.30, 0.18),
    "horse_dark": (0.24, 0.16, 0.11),
    "horse_light": (0.78, 0.70, 0.58),
    "elephant": (0.52, 0.50, 0.48),
    "camel": (0.76, 0.60, 0.40),
    "ivory": (0.94, 0.90, 0.78),
    "jaguar": (0.86, 0.62, 0.22),
    "stone": (0.60, 0.58, 0.54),
    "paper": (0.95, 0.93, 0.84),
    "purple": (0.42, 0.20, 0.48),
    "black": (0.08, 0.08, 0.09),
    # World objects (platform/tools/blender/world): ground cover, building stone, crops, minerals.
    "grass": (0.42, 0.58, 0.26),
    "grass_dry": (0.64, 0.62, 0.34),
    "wheat": (0.87, 0.72, 0.32),
    "soil": (0.49, 0.36, 0.20),
    "soil_dark": (0.32, 0.23, 0.14),
    "sand": (0.85, 0.75, 0.53),
    "water": (0.25, 0.51, 0.63),
    "water_deep": (0.12, 0.31, 0.46),
    "foam": (0.87, 0.93, 0.95),
    "snow": (0.95, 0.96, 0.98),
    "rock": (0.53, 0.51, 0.48),
    "rock_dark": (0.35, 0.33, 0.31),
    "rock_red": (0.67, 0.37, 0.23),
    "marble": (0.93, 0.92, 0.89),
    "limestone": (0.86, 0.78, 0.60),
    "plaster": (0.93, 0.87, 0.73),
    "mudbrick": (0.73, 0.57, 0.39),
    "brick": (0.63, 0.31, 0.21),
    "terracotta": (0.75, 0.39, 0.23),
    "slate": (0.31, 0.34, 0.39),
    "concrete": (0.71, 0.71, 0.69),
    "glass_dark": (0.21, 0.31, 0.39),
    "copper_green": (0.39, 0.67, 0.59),
    "leaf": (0.27, 0.47, 0.21),
    "leaf_dark": (0.17, 0.33, 0.16),
    "leaf_light": (0.49, 0.65, 0.27),
    "blossom": (0.89, 0.53, 0.63),
    "bark": (0.37, 0.26, 0.17),
    "lava": (0.96, 0.36, 0.11),
    "coral": (0.93, 0.51, 0.43),
    "coral_purple": (0.61, 0.37, 0.63),
    "coal": (0.15, 0.15, 0.16),
    "aluminum": (0.79, 0.81, 0.85),
    "uranium": (0.49, 0.81, 0.31),
    "gem_red": (0.81, 0.15, 0.27),
    "gem_blue": (0.21, 0.43, 0.87),
    "silver": (0.81, 0.83, 0.87),
    "salt": (0.97, 0.97, 0.95),
    "cotton": (0.98, 0.97, 0.94),
    "dye": (0.53, 0.23, 0.63),
    "spice": (0.81, 0.31, 0.11),
    "citrus": (0.99, 0.63, 0.13),
    "grape": (0.41, 0.19, 0.41),
    "cocoa": (0.49, 0.27, 0.15),
    "banana": (0.97, 0.87, 0.31),
    "cane": (0.65, 0.75, 0.37),
    "rust": (0.57, 0.31, 0.19),
    "wool": (0.95, 0.93, 0.87),
    "hide": (0.63, 0.43, 0.27),
    "bison": (0.31, 0.21, 0.14),
    "fish": (0.67, 0.73, 0.79),
    "whale": (0.31, 0.35, 0.41),
    "crab": (0.87, 0.33, 0.19),
    "steam": (0.93, 0.95, 0.97),
    "neon": (0.31, 0.83, 0.93),
    "sandstone": (0.80, 0.63, 0.43),
    "jade": (0.37, 0.67, 0.47),
    "turquoise": (0.31, 0.76, 0.73),
    "ochre": (0.81, 0.59, 0.26),
    "obsidian": (0.13, 0.12, 0.14),
    "ash": (0.46, 0.44, 0.43),
    "sky_glass": (0.56, 0.76, 0.86),
    "asphalt": (0.23, 0.23, 0.24),
    "roof_blue": (0.26, 0.39, 0.63),
    "cream": (0.96, 0.91, 0.79),
    "pink": (0.93, 0.71, 0.69),
    "lacquer": (0.62, 0.11, 0.09),
}


class Xf:
    """Affine transform: 3x3 matrix rows + translation."""

    __slots__ = ("m", "t")

    def __init__(self, m=((1, 0, 0), (0, 1, 0), (0, 0, 1)), t=(0.0, 0.0, 0.0)):
        self.m = m
        self.t = t

    def apply(self, p):
        m, t = self.m, self.t
        return (
            m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + t[0],
            m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + t[1],
            m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + t[2],
        )

    def __matmul__(self, other):
        a, b = self.m, other.m
        m = tuple(
            tuple(sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)) for i in range(3)
        )
        return Xf(m, self.apply(other.t))


def move(x=0.0, y=0.0, z=0.0):
    return Xf(t=(x, y, z))


def rot_x(deg):
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return Xf(((1, 0, 0), (0, c, -s), (0, s, c)))


def rot_y(deg):
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return Xf(((c, 0, s), (0, 1, 0), (-s, 0, c)))


def rot_z(deg):
    c, s = math.cos(math.radians(deg)), math.sin(math.radians(deg))
    return Xf(((c, -s, 0), (s, c, 0), (0, 0, 1)))


def scale(x, y=None, z=None):
    y = x if y is None else y
    z = x if z is None else z
    return Xf(((x, 0, 0), (0, y, 0), (0, 0, z)))


def add(a, b):
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def mul(a, k):
    return (a[0] * k, a[1] * k, a[2] * k)


def lerp(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def length(a):
    return math.sqrt(dot(a, a))


def unit(a):
    n = length(a)
    return (a[0] / n, a[1] / n, a[2] / n)


class Mesh:
    """Triangle soup with per-vertex palette colors and a transform stack.

    `detail` is the LOD being built: 0 = close-up, 1 = reduced, 2 = far marker.
    Builders ask `pick(a, b, c)` for detail-dependent values and skip small
    accessories when `detail` is high.
    """

    def __init__(self, detail=0):
        self.detail = detail
        self.points = []
        self.colors = []
        self.triangles = []
        self.materials = set()
        self._stack = [Xf()]

    # Transform stack -------------------------------------------------------
    def push(self, xf):
        self._stack.append(self._stack[-1] @ xf)

    def pop(self):
        self._stack.pop()

    class _Scope:
        def __init__(self, mesh, xf):
            self.mesh, self.xf = mesh, xf

        def __enter__(self):
            self.mesh.push(self.xf)

        def __exit__(self, *_):
            self.mesh.pop()

    def at(self, *xfs):
        """`with mesh.at(move(..), rot_z(..)):` composes left to right."""
        xf = Xf()
        for x in xfs:
            xf = xf @ x
        return Mesh._Scope(self, xf)

    def pick(self, *values):
        return values[min(self.detail, len(values) - 1)]

    # Core ------------------------------------------------------------------
    def solid(self, pts, faces, material):
        """Closed star-shaped solid: convex faces as vertex index loops."""
        if material not in PALETTE:
            raise KeyError("unknown material " + material)
        self.materials.add(material)
        xf = self._stack[-1]
        world = [xf.apply(p) for p in pts]
        centroid = mul(
            (sum(p[0] for p in world), sum(p[1] for p in world), sum(p[2] for p in world)),
            1.0 / len(world),
        )
        base = len(self.points)
        self.points.extend(world)
        self.colors.extend([material] * len(world))
        for face in faces:
            ring = [world[i] for i in face]
            normal = (0.0, 0.0, 0.0)
            for i in range(1, len(ring) - 1):
                normal = add(normal, cross(sub(ring[i], ring[0]), sub(ring[i + 1], ring[0])))
            mid = mul(
                (sum(p[0] for p in ring), sum(p[1] for p in ring), sum(p[2] for p in ring)),
                1.0 / len(ring),
            )
            if dot(normal, sub(mid, centroid)) < 0:
                face = list(reversed(face))
            for i in range(1, len(face) - 1):
                a, b, c = world[face[0]], world[face[i]], world[face[i + 1]]
                if length(cross(sub(b, a), sub(c, a))) > 1e-7:
                    self.triangles.append((base + face[0], base + face[i], base + face[i + 1]))

    # Primitives ------------------------------------------------------------
    def box(self, c, h, material):
        """Axis-aligned box, center c, half extents h."""
        x, y, z = c
        a, b, d = h
        pts = [
            (x + sx * a, y + sy * b, z + sz * d)
            for sz in (-1, 1)
            for sy in (-1, 1)
            for sx in (-1, 1)
        ]
        faces = [(0, 1, 3, 2), (4, 5, 7, 6), (0, 1, 5, 4), (2, 3, 7, 6), (0, 2, 6, 4), (1, 3, 7, 5)]
        self.solid(pts, faces, material)

    def taper(self, c, bottom, top, height, material, shift=(0.0, 0.0)):
        """Box from half extents `bottom` at z=c.z to `top` at z=c.z+height."""
        x, y, z = c
        pts = []
        for (hx, hy), dz, (ox, oy) in ((bottom, 0.0, (0, 0)), (top, height, shift)):
            for sy in (-1, 1):
                for sx in (-1, 1):
                    pts.append((x + ox + sx * hx, y + oy + sy * hy, z + dz))
        faces = [(0, 1, 3, 2), (4, 5, 7, 6), (0, 1, 5, 4), (2, 3, 7, 6), (0, 2, 6, 4), (1, 3, 7, 5)]
        self.solid(pts, faces, material)

    def rod(self, a, b, r0, r1, material, sides=None, phase=0.5):
        """Frustum from point a to b. r1 == 0 makes a cone. Radii may be (ru, rv)."""
        sides = sides or self.pick(6, 4, 3)
        axis = sub(b, a)
        n = unit(axis)
        seed = (0.0, 0.0, 1.0) if abs(n[2]) < 0.9 else (1.0, 0.0, 0.0)
        u = unit(cross(n, seed))
        v = cross(n, u)
        pts = []
        rings = []
        for center, r in ((a, r0), (b, r1)):
            ru, rv = (r, r) if not isinstance(r, tuple) else r
            if ru <= 0 and rv <= 0:
                rings.append([len(pts)])
                pts.append(center)
                continue
            ring = []
            for i in range(sides):
                t = (i + phase) * math.tau / sides
                ring.append(len(pts))
                pts.append(add(center, add(mul(u, math.cos(t) * ru), mul(v, math.sin(t) * rv))))
            rings.append(ring)
        self.solid(pts, _loft_faces(rings), material)

    def rings(self, levels, material, sides=None, axis="z", phase=0.5):
        """Loft of elliptical rings. levels: [(center, ra, rb)], ring plane normal to axis.

        A ring with zero radii is a single apex point.
        """
        sides = sides or self.pick(8, 5, 4)
        pts = []
        rings = []
        for center, ra, rb in levels:
            if ra <= 0 and rb <= 0:
                rings.append([len(pts)])
                pts.append(center)
                continue
            ring = []
            for i in range(sides):
                t = (i + phase) * math.tau / sides
                ca, cb = math.cos(t) * ra, math.sin(t) * rb
                offset = {"z": (ca, cb, 0.0), "x": (0.0, ca, cb), "y": (ca, 0.0, cb)}[axis]
                ring.append(len(pts))
                pts.append(add(center, offset))
            rings.append(ring)
        self.solid(pts, _loft_faces(rings), material)

    def loft(self, sections, material):
        """Loft of explicit convex sections (equal vertex counts, or a single apex point)."""
        pts = []
        rings = []
        for section in sections:
            ring = []
            for p in section:
                ring.append(len(pts))
                pts.append(p)
            rings.append(ring)
        self.solid(pts, _loft_faces(rings), material)

    def prism(self, outline, depth, material, axis="y"):
        """Convex outline extruded symmetrically along axis.

        axis "y": outline in (x, z), thickness along y (shields, sails seen from the front).
        axis "x": outline in (y, z), thickness along x (side plates, keels).
        axis "z": outline in (x, y), thickness along z (decks, plates).
        """
        h = depth * 0.5
        front, back = [], []
        for p, q in outline:
            if axis == "y":
                front.append((p, -h, q))
                back.append((p, h, q))
            elif axis == "x":
                front.append((-h, p, q))
                back.append((h, p, q))
            else:
                front.append((p, q, -h))
                back.append((p, q, h))
        self.loft([front, back], material)

    def ball(self, c, r, material, u=None, v=None):
        """Low-poly ellipsoid. r may be a float or (rx, ry, rz)."""
        rx, ry, rz = (r, r, r) if not isinstance(r, tuple) else r
        u = u or self.pick(8, 6, 4)
        v = v or self.pick(5, 4, 3)
        levels = [((c[0], c[1], c[2] - rz), 0.0, 0.0)]
        for j in range(1, v):
            phi = math.pi * j / v - math.pi / 2
            levels.append(((c[0], c[1], c[2] + math.sin(phi) * rz), math.cos(phi) * rx, math.cos(phi) * ry))
        levels.append(((c[0], c[1], c[2] + rz), 0.0, 0.0))
        self.rings(levels, material, sides=u)

    def octa(self, c, r, material):
        x, y, z = c
        a, b, d = (r, r, r) if not isinstance(r, tuple) else r
        pts = [(x - a, y, z), (x + a, y, z), (x, y - b, z), (x, y + b, z), (x, y, z - d), (x, y, z + d)]
        faces = [(0, 2, 5), (2, 1, 5), (1, 3, 5), (3, 0, 5), (2, 0, 4), (1, 2, 4), (3, 1, 4), (0, 3, 4)]
        self.solid(pts, faces, material)

    def wheel(self, c, r, width, material, hub=None, axis="x"):
        """Disc on an axle along x (side wheels) or y."""
        sides = self.pick(10, 7, 5)
        if axis == "x":
            a, b = (c[0] - width / 2, c[1], c[2]), (c[0] + width / 2, c[1], c[2])
        else:
            a, b = (c[0], c[1] - width / 2, c[2]), (c[0], c[1] + width / 2, c[2])
        self.rod(a, b, r, r, material, sides=sides)
        if hub and self.detail == 0:
            off = (width * 0.6, 0, 0) if axis == "x" else (0, width * 0.6, 0)
            self.rod(sub(c, off), add(c, off), r * 0.3, r * 0.3, hub, sides=5)


def _loft_faces(rings):
    faces = []
    first, last = rings[0], rings[-1]
    if len(first) > 2:
        faces.append(list(reversed(first)))
    if len(last) > 2:
        faces.append(list(last))
    for lower, upper in zip(rings, rings[1:]):
        if len(lower) == 1 and len(upper) == 1:
            continue
        if len(lower) == 1:
            n = len(upper)
            faces.extend([lower[0], upper[i], upper[(i + 1) % n]] for i in range(n))
        elif len(upper) == 1:
            n = len(lower)
            faces.extend([lower[i], lower[(i + 1) % n], upper[0]] for i in range(n))
        else:
            n = len(lower)
            faces.extend([lower[i], lower[(i + 1) % n], upper[(i + 1) % n], upper[i]] for i in range(n))
    return faces
