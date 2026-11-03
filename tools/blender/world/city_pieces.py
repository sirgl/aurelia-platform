"""Pieces of the modular city kit (city_kit.py): per era a center and three houses, the
walls ring, building landmarks and national wonders. Every piece is a world model
(kit.py frame) placed by the city composition on a slot of the city tile.

Layout of a city tile (authoring frame, front = -y):
- Center: its mass stays inside radius 0.20 (the inner slots sit at 0.34 and hold pieces of
  radius 0.13); only narrow parts reach out to 0.28 toward the front and the back, where no
  slot is.
- House pieces: a small block of one to three buildings on a yard of radius ~0.12. Later eras
  get taller: ancient huts ~0.1, information towers up to ~0.6.
- Landmarks: main mass inside radius ~0.17 so neighbouring slots stay readable; low parts
  (quays, runways, tracks) may reach further.
- Walls: a hex ring with corner radius 0.815, outside the outer slots (0.60 + 0.13).

Ownership split: this file keeps the helpers and the models shared with lands (or generic);
the models only classic draws are in classic/tools/blender/world/city_pieces.py, which imports the
helpers it shares from here.
"""
import math

from geom import move, rot_z
import kit
from city_kit import KIT, STYLES
from world_catalog import WorldModel


# Shared parts --------------------------------------------------------------------------------
def _pad(m, half, mat, c=(0.0, 0.0), yaw=0.0, z1=0.008):
    """Yard / paving under a piece, sunk into the ground."""
    kit.patch(m, c, half, mat, yaw, z0=-0.04, z1=z1)


def _yard(m, mat):
    """Yard of a house piece (close-up only): keeps inside the house radius of 0.15."""
    if m.detail == 0:
        _pad(m, (0.105, 0.095), mat, z1=0.006)


def _front(m, x, y_front, z0, w, h, mat, depth=0.006):
    """Thin plate on a front (-y) face: door, window strip, timber post."""
    kit.block(m, (x, y_front - depth / 2 + 0.002, z0), (w, depth, h), mat)


def _door(m, x, y_front, w, h, mat="wood_dark"):
    if m.detail == 0:
        _front(m, x, y_front, -0.01, w, h + 0.01, mat)


def _band(m, x, y, z0, w, d, h, mat, grow=0.008):
    """Ring band around a box: floor line, cornice, window band."""
    kit.block(m, (x, y, z0), (w + grow, d + grow, h), mat)


def _gable(m, x, y, z, w, d, rise, mat, front=False, overhang=0.01):
    """Gable roof on a w x d block top; front=True turns the gable end toward the camera."""
    if front:
        kit.wedge_roof(m, (x, y, z), d, w, rise, mat, yaw=90, overhang=overhang)
    else:
        kit.wedge_roof(m, (x, y, z), w, d, rise, mat, overhang=overhang)


def _flag(m, x, y, z, h, mat="team", size=0.05):
    kit.flag(m, (x, y, z), h, mat, size=size)


def _columns(m, xs, y, z0, h, r, mat, sides=None):
    sides = sides or m.pick(5, 4, 3)
    for x in xs:
        kit.cylinder(m, (x, y, z0), r, h, mat, sides=sides)


def _square(cx, cy, z, hx, hy, deg=0.0):
    a = math.radians(deg)
    ca, sa = math.cos(a), math.sin(a)
    return [(cx + x * ca - y * sa, cy + x * sa + y * ca, z) for x, y in ((-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy))]


def _ellipse(rx, ry, n, phase=0.0):
    return [(rx * math.cos(phase + math.tau * k / n), ry * math.sin(phase + math.tau * k / n)) for k in range(n)]


def _floors(m, x, y, z0, w, d, h, count, band, frac=0.4):
    """`count` window bands around a block (one per floor)."""
    step = h / count
    for i in range(count):
        _band(m, x, y, z0 + step * (i + 0.5 - frac / 2), w, d, step * frac, band, grow=0.006)


def _tree(m, x, y, h, leaf="leaf", kind="ball"):
    if kind == "cypress":
        m.rings([((x, y, -0.01), h * 0.14, h * 0.14), ((x, y, h * 0.45), h * 0.18, h * 0.18), ((x, y, h), 0.0, 0.0)],
                leaf, sides=m.pick(5, 4, 3))
        return
    kit.cylinder(m, (x, y, -0.01), h * 0.07, h * 0.5, "bark", sides=3)
    m.ball((x, y, h * 0.68), (h * 0.34, h * 0.34, h * 0.32), leaf, u=m.pick(6, 4, 4), v=3)


def _smoke(m, x, y, z, r):
    if m.detail == 0:
        m.ball((x + r * 0.3, y, z + r), (r, r * 0.85, r * 0.8), "steam", u=5, v=3)
        m.ball((x + r * 1.4, y + r * 0.3, z + r * 2.4), (r * 1.25, r, r * 0.9), "steam", u=5, v=3)


def _figure(m, x, y, z, h, mat, arm_up=True, weapon="iron"):
    """Very small standing figure (statues): legs, body, head and a raised arm."""
    with m.at(move(x, y, z), kit.scale(h)):
        for s in (-1, 1):
            m.rod((s * 0.08, 0.0, 0.0), (s * 0.06, 0.0, 0.45), 0.06, 0.07, mat, sides=4)
        m.taper((0, 0, 0.42), (0.15, 0.09), (0.18, 0.1), 0.36, mat)
        m.ball((0, 0, 0.88), 0.1, mat, u=5, v=3)
        m.rod((-0.17, 0.0, 0.76), (-0.22, -0.05, 0.45), 0.045, 0.04, mat, sides=3)
        if arm_up:
            m.rod((0.17, 0.0, 0.76), (0.3, -0.04, 1.05), 0.045, 0.04, mat, sides=3)
            m.rod((0.3, -0.04, 1.0), (0.34, -0.06, 1.42), 0.025, 0.012, weapon, sides=3)
        else:
            m.rod((0.17, 0.0, 0.76), (0.22, -0.05, 0.45), 0.045, 0.04, mat, sides=3)


def _round_tower(m, x, y, r, h, mat, roof="slate", roof_h=None, sides=None):
    sides = sides or m.pick(7, 5, 4)
    kit.cylinder(m, (x, y, -0.02), r, h + 0.02, mat, sides=sides)
    if roof == "crenel":
        kit.cylinder(m, (x, y, h - 0.004), r * 1.15, 0.024, mat, sides=sides)
    elif roof:
        kit.cone(m, (x, y, h - 0.004), r * 1.25, roof_h or r * 2.4, roof, sides=sides)


def _merlons(m, x, y, z, w, d, count, mat, size=0.018):
    """Merlons along the front and back edges of a w x d top."""
    if m.detail != 0:
        return
    for i in range(count):
        px = x - w / 2 + w * (i + 0.5) / count
        for py in (y - d / 2, y + d / 2):
            kit.block(m, (px, py, z), (size, size * 0.8, size), mat)


# Houses: ancient (mud brick, thatch) -------------------------------------------------------------
def _hut(m, x, y, r, h, wall="mudbrick", roof="wheat", door=False):
    sides = m.pick(7, 4, 4)
    kit.cylinder(m, (x, y, -0.02), r, h + 0.02, wall, sides=sides)
    kit.cone(m, (x, y, h - 0.006), r * 1.3, r * 1.5, roof, sides=sides)
    if door:
        _door(m, x, y - r * 0.9, r * 0.55, h * 0.7)


def house_ancient_0(m):
    if m.detail == 2:
        _hut(m, -0.02, 0.02, 0.06, 0.05)
        return
    _yard(m, "soil")
    _hut(m, -0.05, 0.035, 0.044, 0.052, door=True)
    _hut(m, 0.055, 0.045, 0.036, 0.045, roof="grass_dry")
    _hut(m, 0.005, -0.055, 0.034, 0.042, door=True)


def house_ancient_1(m):
    """Flat-roofed mud-brick house with a roof room, a hut and a water jar."""
    kit.block(m, (-0.02, 0.025, -0.02), (0.13, 0.085, 0.09), "mudbrick")
    kit.block(m, (-0.045, 0.04, 0.068), (0.065, 0.055, 0.04), "sandstone")
    if m.detail == 2:
        return
    _yard(m, "soil")
    kit.block(m, (-0.02, 0.025, 0.066), (0.14, 0.095, 0.008), "wood")
    _door(m, 0.0, 0.025 - 0.0425, 0.025, 0.05)
    _hut(m, 0.07, -0.05, 0.032, 0.042)
    if m.detail == 0:
        kit.cylinder(m, (-0.07, -0.06, -0.01), 0.013, 0.035, "terracotta", sides=5, r_top=0.009)


def house_ancient_2(m):
    """Thatched longhouse with a hut and a small granary jar."""
    kit.block(m, (0.0, 0.03, -0.02), (0.15, 0.075, 0.07), "mudbrick")
    _gable(m, 0.0, 0.03, 0.05, 0.15, 0.075, 0.075, "wheat", overhang=0.016)
    if m.detail == 2:
        return
    _yard(m, "soil")
    _door(m, 0.02, 0.03 - 0.0375, 0.025, 0.045)
    _hut(m, -0.06, -0.055, 0.03, 0.04, roof="grass_dry")
    if m.detail == 0:
        kit.cylinder(m, (0.065, -0.05, -0.01), 0.02, 0.045, "mudbrick", sides=5)
        kit.cone(m, (0.065, -0.05, 0.033), 0.028, 0.032, "wheat", sides=5)


# Houses: classical (white stucco, terracotta) --------------------------------------------------
def _stucco(m, x, y, w, d, h, roof="terracotta", wall="plaster", rise=None):
    kit.block(m, (x, y, -0.02), (w, d, h + 0.02), wall)
    kit.hip_roof(m, (x, y, h), w, d, rise or min(w, d) * 0.38, roof, overhang=0.008)


def house_classical_0(m):
    """Domus: main house and wing around a court, a cypress."""
    _stucco(m, 0.0, 0.035, 0.15, 0.075, 0.075)
    if m.detail == 2:
        return
    _yard(m, "limestone")
    _stucco(m, -0.055, -0.04, 0.055, 0.07, 0.06, wall="white")
    _door(m, 0.03, 0.035 - 0.0375, 0.025, 0.05)
    if m.detail == 0:
        _tree(m, 0.06, -0.05, 0.12, "leaf_dark", kind="cypress")
        _tree(m, 0.09, -0.02, 0.09, "leaf_dark", kind="cypress")


def house_classical_1(m):
    """Insula: two-storey block with a shop awning."""
    _stucco(m, -0.01, 0.02, 0.12, 0.1, 0.12, wall="cream")
    if m.detail == 2:
        return
    _yard(m, "limestone")
    _band(m, -0.01, 0.02, 0.058, 0.12, 0.1, 0.008, "terracotta")
    _stucco(m, 0.07, -0.045, 0.05, 0.05, 0.065)
    if m.detail == 0:
        m.loft([[(-0.06, -0.03, 0.05), (0.04, -0.03, 0.05), (0.04, -0.03, 0.056), (-0.06, -0.03, 0.056)],
                [(-0.06, -0.06, 0.035), (0.04, -0.06, 0.035), (0.04, -0.06, 0.039), (-0.06, -0.06, 0.039)]], "red")
        _door(m, -0.01, 0.02 - 0.05, 0.03, 0.035)


def house_classical_2(m):
    """Villa with a columned porch and a pediment toward the camera."""
    kit.block(m, (0.0, 0.03, -0.02), (0.12, 0.09, 0.1), "white")
    _gable(m, 0.0, 0.0, 0.08, 0.13, 0.15, 0.04, "terracotta", front=True)
    if m.detail == 2:
        return
    _yard(m, "limestone")
    kit.block(m, (0.0, -0.045, -0.02), (0.13, 0.06, 0.03), "marble")
    _columns(m, (-0.05, -0.017, 0.017, 0.05), -0.06, 0.01, 0.07, 0.008, "marble", sides=m.pick(5, 3, 3))
    if m.detail == 0:
        _tree(m, -0.09, 0.07, 0.11, "leaf_dark", kind="cypress")


# Houses: medieval (timber frame, steep roofs) ----------------------------------------------------
def _timber_house(m, x, y, w, d, h, roof, wall="plaster", posts=True, front=True, rise=None):
    """Ground floor, jettied upper floor with a dark beam line, steep roof."""
    rise = rise or (w if front else d) * 0.85
    if m.detail > 0:
        kit.block(m, (x, y, -0.02), (w, d, h + 0.02), wall)
        _gable(m, x, y, h, w, d, rise, roof, front=front)
        return
    kit.block(m, (x, y, -0.02), (w, d, h * 0.5 + 0.02), wall)
    kit.block(m, (x, y - 0.004, h * 0.5), (w + 0.01, d + 0.01, h * 0.5), wall)
    if m.detail == 0:
        _band(m, x, y - 0.004, h * 0.5, w + 0.01, d + 0.01, 0.008, "wood_dark", grow=0.004)
        if posts:
            for px in (x - w * 0.3, x + w * 0.3):
                _front(m, px, y - 0.004 - (d + 0.01) / 2, h * 0.5, 0.007, h * 0.5, "wood_dark")
    _gable(m, x, y - 0.004, h, w + 0.01, d + 0.01, rise, roof, front=front)


def house_medieval_0(m):
    if m.detail < 2:
        _yard(m, "stone")
    _timber_house(m, -0.045, 0.02, 0.065, 0.1, 0.12, "slate")
    if m.detail < 2:
        _timber_house(m, 0.035, 0.0, 0.06, 0.09, 0.1, "terracotta", wall="cream")


def house_medieval_1(m):
    if m.detail < 2:
        _yard(m, "stone")
    _timber_house(m, 0.0, 0.03, 0.14, 0.07, 0.1, "brick", front=False, rise=0.08)
    if m.detail < 2:
        kit.block(m, (0.07, -0.05, -0.02), (0.04, 0.04, 0.05), "wood")
        _gable(m, 0.07, -0.05, 0.03, 0.04, 0.04, 0.025, "wood_dark")


def house_medieval_2(m):
    if m.detail < 2:
        _yard(m, "stone")
    if m.detail == 2:
        _timber_house(m, 0.0, 0.0, 0.16, 0.09, 0.12, "slate", front=False)
        return
    if m.detail == 1:
        _timber_house(m, -0.03, 0.01, 0.1, 0.085, 0.13, "slate")
        _timber_house(m, 0.06, 0.015, 0.05, 0.085, 0.14, "slate")
        return
    _timber_house(m, -0.06, 0.01, 0.05, 0.085, 0.13, "slate", posts=False)
    _timber_house(m, 0.0, 0.0, 0.055, 0.085, 0.11, "terracotta", wall="cream", posts=False)
    _timber_house(m, 0.06, 0.015, 0.05, 0.085, 0.14, "slate", posts=False)


# Houses: renaissance (stone townhouses, tile roofs) ---------------------------------------------
def _townhouse(m, x, y, w, d, h, wall, roof="terracotta", windows=True, base="stone"):
    if m.detail > 0:
        kit.block(m, (x, y, -0.02), (w, d, h + 0.02), wall)
        kit.hip_roof(m, (x, y, h), w, d, 0.04, roof)
        return
    kit.block(m, (x, y, -0.02), (w, d, h * 0.3 + 0.02), base)
    kit.block(m, (x, y, h * 0.3), (w - 0.006, d - 0.006, h * 0.7), wall)
    if m.detail == 0:
        _band(m, x, y, h - 0.012, w, d, 0.012, "white", grow=0.01)
        if windows:
            _front(m, x, y - d / 2 + 0.003, h * 0.45, w * 0.7, h * 0.14, "glass_dark")
            _front(m, x, y - d / 2 + 0.003, h * 0.7, w * 0.7, h * 0.12, "glass_dark")
    kit.hip_roof(m, (x, y, h), w, d, min(w, d) * 0.32, roof, overhang=0.01)


def house_renaissance_0(m):
    """Row of three coloured townhouses."""
    if m.detail < 2:
        _yard(m, "stone")
    if m.detail > 0:
        _townhouse(m, 0.0, 0.0, 0.18, 0.09, 0.18, "cream")
        return
    _townhouse(m, -0.06, 0.0, 0.058, 0.09, 0.17, "cream", windows=False)
    _townhouse(m, 0.0, 0.005, 0.058, 0.09, 0.2, "pink", windows=False)
    _townhouse(m, 0.06, 0.0, 0.058, 0.09, 0.16, "ochre", windows=False)


def house_renaissance_1(m):
    """Palazzo with two window rows."""
    if m.detail < 2:
        _yard(m, "stone")
    _townhouse(m, 0.0, 0.01, 0.16, 0.12, 0.17, "sandstone")


def house_renaissance_2(m):
    if m.detail < 2:
        _yard(m, "stone")
    _townhouse(m, -0.035, 0.03, 0.09, 0.09, 0.2, "cream")
    if m.detail < 2:
        _townhouse(m, 0.06, -0.04, 0.07, 0.07, 0.14, "pink")


# Houses: industrial (brick rows, chimneys) ---------------------------------------------------------
def _chimneys(m, xs, y, z, h=0.04):
    for x in xs:
        kit.block(m, (x, y, z), (0.014, 0.022, h), "brick")


def house_industrial_0(m):
    """Terrace of brick row houses with chimney stacks."""
    kit.block(m, (0.0, 0.02, -0.02), (0.22, 0.08, 0.14), "brick")
    _gable(m, 0.0, 0.02, 0.12, 0.22, 0.08, 0.05, "slate")
    if m.detail == 2:
        return
    _yard(m, "rock_dark")
    _chimneys(m, (-0.074, 0.0, 0.074), 0.02, 0.14)
    if m.detail == 0:
        _front(m, 0.0, 0.02 - 0.04, 0.075, 0.2, 0.025, "glass_dark")
        for x in (-0.074, 0.0, 0.074):
            _front(m, x - 0.02, 0.02 - 0.04, -0.01, 0.016, 0.05, "white")
        _smoke(m, 0.074, 0.02, 0.19, 0.016)


def house_industrial_1(m):
    """Four-storey brick tenement with a flat roof and a water tank."""
    kit.block(m, (0.0, 0.02, -0.02), (0.14, 0.11, 0.24), "brick")
    if m.detail == 2:
        kit.block(m, (0.03, 0.04, 0.22), (0.03, 0.03, 0.04), "wood")
        return
    _yard(m, "rock_dark")
    _band(m, 0.0, 0.02, 0.21, 0.14, 0.11, 0.012, "limestone")
    if m.detail == 0:
        _band(m, 0.0, 0.02, 0.07, 0.14, 0.11, 0.008, "limestone")
        _floors(m, 0.0, 0.02, 0.08, 0.14, 0.11, 0.12, 2, "glass_dark", frac=0.35)
    kit.cylinder(m, (0.035, 0.04, 0.22), 0.018, 0.035, "wood", sides=m.pick(6, 4, 4))
    kit.cone(m, (0.035, 0.04, 0.255), 0.022, 0.02, "wood_dark", sides=m.pick(6, 4, 4))
    if m.detail == 0:
        _chimneys(m, (-0.04,), 0.05, 0.22)


def house_industrial_2(m):
    """Workshop with a sawtooth roof, a tall stack and a small house."""
    kit.block(m, (-0.02, 0.03, -0.02), (0.14, 0.1, 0.08), "brick")
    kit.cylinder(m, (0.075, 0.06, -0.02), 0.014, 0.3, "brick", sides=m.pick(6, 4, 4), r_top=0.011)
    if m.detail == 2:
        return
    _yard(m, "rock_dark")
    for i in range(2):
        y0 = 0.03 - 0.05 + 0.05 * i
        m.loft([[(-0.09, y0, 0.06), (-0.09, y0 + 0.05, 0.06), (-0.09, y0 + 0.05, 0.1)],
                [(0.05, y0, 0.06), (0.05, y0 + 0.05, 0.06), (0.05, y0 + 0.05, 0.1)]], "slate")
    _house_small(m, 0.07, -0.06, "brick", "slate")
    if m.detail == 0:
        _smoke(m, 0.075, 0.06, 0.29, 0.02)


def _house_small(m, x, y, wall, roof):
    kit.block(m, (x, y, -0.02), (0.05, 0.045, 0.07), wall)
    _gable(m, x, y, 0.05, 0.05, 0.045, 0.03, roof)


# Houses: modern (apartment blocks, flat roofs) ----------------------------------------------------------
def house_modern_0(m):
    """Slab apartment block with window bands."""
    kit.block(m, (0.0, 0.02, -0.02), (0.2, 0.085, 0.3), "cream")
    if m.detail == 2:
        kit.block(m, (0.05, 0.03, 0.28), (0.04, 0.04, 0.03), "concrete")
        return
    _yard(m, "asphalt")
    _floors(m, 0.0, 0.02, 0.04, 0.2, 0.085, 0.24, m.pick(5, 2, 1), "glass_dark")
    kit.block(m, (0.0, 0.02, 0.28), (0.21, 0.095, 0.012), "concrete")
    kit.block(m, (0.05, 0.03, 0.29), (0.04, 0.04, 0.03), "concrete")


def house_modern_1(m):
    """Art-deco setback tower."""
    kit.block(m, (0.0, 0.01, -0.02), (0.13, 0.12, 0.24), "limestone")
    kit.block(m, (0.0, 0.015, 0.22), (0.095, 0.09, 0.1), "limestone")
    if m.detail == 2:
        return
    _yard(m, "asphalt")
    kit.block(m, (0.0, 0.02, 0.32), (0.06, 0.06, 0.06), "limestone")
    if m.detail == 0:
        for x in (-0.035, 0.0, 0.035):
            _front(m, x, 0.01 - 0.06, 0.05, 0.016, 0.17, "glass_dark")
        for x in (-0.03, 0.03):
            _front(m, x, 0.015 - 0.045, 0.23, 0.014, 0.08, "glass_dark")
        kit.block(m, (0.0, 0.02, 0.38), (0.03, 0.03, 0.03), "copper_green")


def house_modern_2(m):
    """Two smaller blocks: a brick walk-up and a white block with balconies."""
    kit.block(m, (-0.045, 0.03, -0.02), (0.09, 0.1, 0.22), "plaster")
    kit.block(m, (0.06, -0.01, -0.02), (0.07, 0.08, 0.16), "brick")
    if m.detail == 2:
        return
    _yard(m, "asphalt")
    _floors(m, -0.045, 0.03, 0.03, 0.09, 0.1, 0.18, m.pick(3, 1, 1), "glass_dark", frac=0.35)
    kit.block(m, (-0.045, 0.03, 0.2), (0.095, 0.105, 0.01), "concrete")
    kit.block(m, (0.06, -0.01, 0.14), (0.075, 0.085, 0.01), "concrete")
    if m.detail == 0:
        _front(m, 0.06, -0.01 - 0.04, 0.05, 0.05, 0.02, "glass_dark")
        _front(m, 0.06, -0.01 - 0.04, 0.1, 0.05, 0.02, "glass_dark")
        _tree(m, 0.08, -0.08, 0.06, "leaf")


# Houses: atomic (concrete towers) -----------------------------------------------------------------
def house_atomic_0(m):
    """Concrete slab tower with glass bands on a low podium."""
    kit.block(m, (0.0, 0.03, -0.02), (0.16, 0.07, 0.46), "concrete")
    if m.detail == 2:
        kit.block(m, (0.0, -0.03, -0.02), (0.2, 0.08, 0.06), "concrete")
        return
    _yard(m, "concrete")
    kit.block(m, (0.0, -0.035, -0.02), (0.2, 0.07, 0.06), "glass_dark")
    _floors(m, 0.0, 0.03, 0.06, 0.16, 0.07, 0.38, m.pick(7, 2, 1), "glass_dark", frac=0.45)
    if m.detail == 0:
        kit.block(m, (0.0, -0.035, 0.04), (0.205, 0.075, 0.01), "concrete")


def house_atomic_1(m):
    """Round tower with stacked balcony rings."""
    sides = m.pick(7, 6, 5)
    kit.cylinder(m, (0.0, 0.0, -0.02), 0.05, 0.47, "concrete", sides=sides)
    if m.detail == 2:
        return
    _yard(m, "concrete")
    for i in range(m.pick(3, 1, 0)):
        kit.cylinder(m, (0.0, 0.0, 0.1 + 0.11 * i), 0.062, 0.05, "glass_dark", sides=sides)
    if m.detail == 0:
        kit.cylinder(m, (0.0, 0.0, 0.45), 0.058, 0.012, "concrete", sides=sides)
    if m.detail == 0:
        kit.block(m, (0.075, -0.07, -0.02), (0.05, 0.05, 0.06), "concrete")


def house_atomic_2(m):
    """Two towers: a glass curtain wall and a concrete slab."""
    kit.block(m, (-0.045, 0.03, -0.02), (0.08, 0.08, 0.4), "glass_dark")
    kit.block(m, (0.06, -0.03, -0.02), (0.075, 0.07, 0.3), "concrete")
    if m.detail == 2:
        return
    _yard(m, "concrete")
    _floors(m, -0.045, 0.03, 0.0, 0.08, 0.08, 0.38, m.pick(4, 2, 1), "concrete", frac=0.25)
    _floors(m, 0.06, -0.03, 0.04, 0.075, 0.07, 0.24, m.pick(4, 1, 1), "glass_dark", frac=0.45)
    if m.detail == 0:
        kit.block(m, (-0.045, 0.03, 0.38), (0.05, 0.05, 0.04), "concrete")


# Houses: information (glass towers, green roofs) ----------------------------------------------------
def _green_roof(m, x, y, z, w, d):
    kit.block(m, (x, y, z), (w + 0.006, d + 0.006, 0.012), "leaf_light")


def house_information_0(m):
    """Tall glass tower with steel bands and a roof garden."""
    kit.block(m, (0.0, 0.02, -0.02), (0.11, 0.1, 0.6), "sky_glass")
    if m.detail == 2:
        kit.block(m, (0.0, 0.02, 0.58), (0.11, 0.1, 0.012), "leaf_light")
        return
    _yard(m, "grass")
    _floors(m, 0.0, 0.02, 0.05, 0.11, 0.1, 0.5, m.pick(4, 1, 1), "steel", frac=0.12)
    _green_roof(m, 0.0, 0.02, 0.58, 0.11, 0.1)
    kit.block(m, (0.0, -0.06, -0.02), (0.18, 0.05, 0.05), "white")
    if m.detail == 0:
        _green_roof(m, 0.0, -0.06, 0.03, 0.18, 0.05)
        m.ball((0.02, 0.03, 0.61), (0.025, 0.025, 0.02), "leaf", u=5, v=3)


def house_information_1(m):
    """Slanted-crown glass tower beside a green-roofed low block."""
    sec = [_square(-0.03, 0.03, -0.02, 0.05, 0.05), _square(-0.03, 0.03, 0.42, 0.05, 0.05)]
    crown = [(p[0], p[1], 0.42 + (0.1 if p[1] > 0.03 else 0.0) + (0.04 if p[0] > -0.03 else 0.0)) for p in sec[1]]
    m.loft(sec + [crown], "glass")
    if m.detail == 2:
        return
    _yard(m, "grass")
    kit.block(m, (0.065, -0.04, -0.02), (0.07, 0.09, 0.12), "white")
    _front(m, 0.065, -0.04 - 0.045, 0.02, 0.06, 0.08, "sky_glass")
    _green_roof(m, 0.065, -0.04, 0.1, 0.07, 0.09)
    if m.detail == 0:
        _floors(m, -0.03, 0.03, 0.05, 0.1, 0.1, 0.32, 3, "steel", frac=0.1)
        _tree(m, -0.08, -0.07, 0.06, "leaf_light")


def house_information_2(m):
    """Green-roofed terraces stepping up to a mid-rise glass block."""
    kit.block(m, (0.0, 0.04, -0.02), (0.16, 0.07, 0.3), "glass")
    kit.block(m, (0.0, -0.035, -0.02), (0.16, 0.06, 0.13), "white")
    if m.detail == 2:
        return
    _yard(m, "grass")
    _green_roof(m, 0.0, 0.04, 0.29, 0.16, 0.07)
    _green_roof(m, 0.0, -0.035, 0.11, 0.16, 0.06)
    _front(m, 0.0, -0.035 - 0.03, 0.02, 0.14, 0.07, "sky_glass")
    if m.detail == 0:
        _floors(m, 0.0, 0.04, 0.13, 0.16, 0.07, 0.16, 2, "white", frac=0.15)
    if m.detail == 0:
        for x in (-0.05, 0.05):
            m.ball((x, -0.035, 0.135), (0.02, 0.02, 0.018), "leaf", u=5, v=3)


# Centers ----------------------------------------------------------------------------------------------
def _plaza(m, r, mat):
    if m.detail < 2:
        kit.hex_patch(m, r, mat, z0=-0.04, z1=0.014)


def center_ancient(m):
    """Plaza with an obelisk, a fire altar, the chief's thatched hall and huts."""
    _plaza(m, 0.21, "sand")
    kit.block(m, (0.0, 0.14, -0.02), (0.16, 0.08, 0.1), "mudbrick")
    _gable(m, 0.0, 0.14, 0.08, 0.16, 0.08, 0.09, "wheat", overhang=0.018)
    m.taper((0.0, 0.0, -0.02), (0.05, 0.05), (0.035, 0.035), 0.06, "stone")
    m.taper((0.0, 0.0, 0.04), (0.022, 0.022), (0.013, 0.013), 0.27, "sandstone")
    m.taper((0.0, 0.0, 0.31), (0.013, 0.013), (0.001, 0.001), 0.035, "gold")
    if m.detail == 2:
        return
    _door(m, 0.0, 0.1, 0.03, 0.06)
    _flag(m, 0.06, 0.14, 0.1, 0.1)
    kit.block(m, (0.0, -0.14, -0.02), (0.075, 0.06, 0.04), "stone")
    kit.cylinder(m, (0.0, -0.14, 0.02), 0.022, 0.022, "bronze", r_top=0.032, sides=m.pick(7, 5, 4))
    kit.cone(m, (0.0, -0.14, 0.04), 0.024, 0.07, "fire", sides=m.pick(6, 4, 4))
    _hut(m, -0.14, -0.06, 0.036, 0.048, door=True)
    _hut(m, 0.14, -0.06, 0.036, 0.048, roof="grass_dry", door=True)
    _hut(m, -0.12, 0.06, 0.03, 0.04)
    if m.detail == 0:
        for x, y in kit.ring(6, 0.075, 30.0):
            if y < 0.03:
                kit.cylinder(m, (x, y, -0.02), 0.009, 0.07, "rock", sides=4)
        kit.cylinder(m, (0.12, 0.06, -0.01), 0.022, 0.05, "mudbrick", sides=6)
        kit.cone(m, (0.12, 0.06, 0.035), 0.03, 0.035, "wheat", sides=6)


def center_classical(m):
    """Forum: colonnaded basilica, honorary column with a gilded statue, triumphal arch."""
    _plaza(m, 0.21, "limestone")
    # Basilica: podium, hall, clerestory, front colonnade.
    kit.block(m, (0.0, 0.1, -0.02), (0.22, 0.13, 0.045), "marble")
    kit.block(m, (0.0, 0.12, 0.02), (0.18, 0.09, 0.11), "plaster")
    _gable(m, 0.0, 0.12, 0.13, 0.18, 0.09, 0.05, "terracotta")
    if m.detail == 2:
        kit.cylinder(m, (0.11, -0.12, -0.02), 0.014, 0.32, "marble", sides=4)
        return
    kit.block(m, (0.0, 0.05, 0.13), (0.2, 0.03, 0.015), "marble")
    xs = [-0.09 + 0.036 * i for i in range(6)]
    _columns(m, xs, 0.05, 0.025, 0.105, 0.009, "marble", sides=m.pick(6, 4, 3))
    _gable(m, 0.0, 0.05, 0.145, 0.2, 0.03, 0.025, "terracotta", overhang=0.006)
    # Honorary column and statue.
    kit.block(m, (0.11, -0.12, -0.02), (0.04, 0.04, 0.05), "marble")
    kit.cylinder(m, (0.11, -0.12, 0.03), 0.013, 0.25, "marble", sides=m.pick(6, 4, 4))
    if m.detail == 0:
        _figure(m, 0.11, -0.12, 0.28, 0.07, "gold")
    # Triumphal arch.
    kit.arch(m, (-0.08, -0.15, -0.02), 0.1, 0.13, 0.035, "marble")
    kit.block(m, (-0.08, -0.15, 0.11), (0.1, 0.04, 0.025), "marble")
    if m.detail == 0:
        kit.block(m, (-0.08, -0.15, 0.135), (0.04, 0.02, 0.02), "bronze")
        _tree(m, 0.14, 0.0, 0.1, "leaf_dark", kind="cypress")
        _tree(m, -0.15, -0.0, 0.1, "leaf_dark", kind="cypress")


def center_medieval(m):
    """Stone church with a tall front spire, apse and a market well."""
    _plaza(m, 0.2, "stone")
    # Nave along y, front tower with spire, apse at the back.
    kit.block(m, (0.0, 0.05, -0.02), (0.1, 0.2, 0.16), "limestone")
    _gable(m, 0.0, 0.05, 0.14, 0.1, 0.2, 0.09, "slate", front=True)
    kit.block(m, (0.0, -0.08, -0.02), (0.07, 0.07, 0.3), "limestone")
    m.taper((0.0, -0.08, 0.28), (0.045, 0.045), (0.001, 0.001), 0.24, "slate")
    if m.detail == 2:
        return
    kit.block(m, (0.0, 0.07, -0.02), (0.18, 0.06, 0.13), "limestone")
    _gable(m, 0.0, 0.07, 0.11, 0.18, 0.06, 0.07, "slate")
    kit.cylinder(m, (0.0, 0.15, -0.02), 0.045, 0.14, "limestone", sides=m.pick(7, 5, 4))
    kit.cone(m, (0.0, 0.15, 0.12), 0.052, 0.07, "slate", sides=m.pick(7, 5, 4))
    kit.block(m, (0.0, -0.08, 0.27), (0.08, 0.08, 0.015), "stone")
    if m.detail == 0:
        _front(m, 0.0, -0.115, 0.0, 0.03, 0.07, "wood_dark")
        _front(m, 0.0, -0.115, 0.17, 0.022, 0.05, "dark")
        m.rod((0.0, -0.08, 0.52), (0.0, -0.08, 0.56), 0.004, 0.004, "gold", sides=3)
        kit.block(m, (-0.015, -0.08, 0.545), (0.03, 0.006, 0.006), "gold")
        kit.cylinder(m, (0.11, -0.12, -0.02), 0.025, 0.045, "stone", sides=6)
        kit.wedge_roof(m, (0.11, -0.12, 0.065), 0.06, 0.04, 0.03, "wood_dark")
        for s in (-1, 1):
            m.rod((0.11 + s * 0.022, -0.12, 0.02), (0.11 + s * 0.022, -0.12, 0.068), 0.003, 0.003, "wood", sides=3)
    _timber_house(m, -0.12, -0.08, 0.06, 0.07, 0.1, "terracotta")
    _timber_house(m, 0.12, 0.04, 0.06, 0.07, 0.11, "slate", wall="cream")


def center_renaissance(m):
    """Domed town hall with a portico and side wings on a piazza with a fountain."""
    _plaza(m, 0.21, "stone")
    kit.block(m, (0.0, 0.06, -0.02), (0.26, 0.14, 0.17), "cream")
    kit.cylinder(m, (0.0, 0.06, 0.15), 0.06, 0.09, "cream", sides=m.pick(10, 7, 5))
    kit.dome(m, (0.0, 0.06, 0.24), 0.066, "terracotta", h=0.1, sides=m.pick(10, 7, 5), steps=m.pick(4, 3, 2))
    if m.detail == 2:
        return
    kit.block(m, (0.0, 0.06, -0.02), (0.27, 0.15, 0.06), "stone")
    _band(m, 0.0, 0.06, 0.155, 0.26, 0.14, 0.015, "white", grow=0.012)
    kit.hip_roof(m, (0.0, 0.06, 0.17), 0.26, 0.14, 0.04, "terracotta")
    kit.cylinder(m, (0.0, 0.06, 0.34), 0.012, 0.04, "white", sides=m.pick(6, 4, 4))
    kit.cone(m, (0.0, 0.06, 0.38), 0.016, 0.04, "gold", sides=m.pick(6, 4, 4))
    # Portico with pediment.
    _columns(m, (-0.05, -0.017, 0.017, 0.05), -0.03, 0.04, 0.1, 0.009, "white")
    kit.block(m, (0.0, -0.025, 0.14), (0.12, 0.04, 0.015), "white")
    _gable(m, 0.0, -0.025, 0.155, 0.12, 0.04, 0.035, "white", front=True, overhang=0.005)
    if m.detail == 0:
        kit.block(m, (0.0, -0.035, 0.0), (0.13, 0.06, 0.04), "stone")
        _front(m, 0.0, -0.01, 0.04, 0.03, 0.07, "wood_dark")
        for x in (-0.095, 0.095):
            _front(m, x, -0.01, 0.07, 0.04, 0.03, "glass_dark")
        kit.cylinder(m, (0.0, -0.16, -0.02), 0.045, 0.035, "white", sides=8)
        kit.cylinder(m, (0.0, -0.16, 0.015), 0.035, 0.004, "water", sides=8)
        kit.cylinder(m, (0.0, -0.16, 0.015), 0.008, 0.05, "white", sides=4)
        _flag(m, 0.1, 0.06, 0.2, 0.12)


def center_industrial(m):
    """Brick town hall with stone trim and a tall clock tower."""
    _plaza(m, 0.2, "stone")
    kit.block(m, (0.0, 0.08, -0.02), (0.26, 0.12, 0.16), "brick")
    kit.hip_roof(m, (0.0, 0.08, 0.14), 0.26, 0.12, 0.05, "slate")
    kit.block(m, (0.0, 0.0, -0.02), (0.075, 0.075, 0.48), "brick")
    m.taper((0.0, 0.0, 0.5), (0.045, 0.045), (0.001, 0.001), 0.15, "slate")
    if m.detail == 2:
        return
    kit.block(m, (0.0, 0.0, 0.42), (0.09, 0.09, 0.08), "limestone")
    _band(m, 0.0, 0.08, 0.14, 0.26, 0.12, 0.012, "limestone", grow=0.01)
    _band(m, 0.0, 0.0, 0.2, 0.075, 0.075, 0.012, "limestone", grow=0.01)
    _floors(m, 0.0, 0.08, 0.02, 0.26, 0.12, 0.12, m.pick(2, 1, 1), "glass_dark", frac=0.3)
    for y_face, axis_y in ((-0.047, True),):
        m.rod((0.0, y_face, 0.46), (0.0, y_face - 0.006, 0.46), 0.03, 0.03, "white", sides=m.pick(8, 6, 4))
    if m.detail == 0:
        for x in (-0.047, 0.047):
            m.rod((x, 0.0, 0.46), (x + (0.006 if x > 0 else -0.006), 0.0, 0.46), 0.03, 0.03, "white", sides=8)
        _front(m, 0.0, -0.0375, -0.01, 0.03, 0.07, "wood_dark")
        m.rod((0.0, 0.0, 0.64), (0.0, 0.0, 0.69), 0.004, 0.002, "gold", sides=3)
        for x in (-0.15, 0.15):
            m.rod((x, -0.1, -0.01), (x, -0.1, 0.1), 0.004, 0.004, "iron", sides=3)
            kit.block(m, (x, -0.1, 0.1), (0.014, 0.014, 0.018), "fire")
        kit.block(m, (0.0, -0.06, -0.02), (0.1, 0.05, 0.03), "limestone")


def center_modern(m):
    """City hall: a wide base and a setback tower with a pyramid top and a flag."""
    _plaza(m, 0.21, "concrete")
    kit.block(m, (0.0, 0.08, -0.02), (0.27, 0.13, 0.15), "cream")
    kit.block(m, (0.0, 0.03, -0.02), (0.11, 0.11, 0.52), "cream")
    kit.block(m, (0.0, 0.03, 0.5), (0.08, 0.08, 0.12), "cream")
    m.taper((0.0, 0.03, 0.62), (0.045, 0.045), (0.004, 0.004), 0.14, "copper_green")
    if m.detail == 2:
        return
    kit.block(m, (0.0, 0.03, 0.62), (0.06, 0.06, 0.04), "cream")
    _floors(m, 0.0, 0.08, 0.0, 0.27, 0.13, 0.14, m.pick(3, 1, 1), "glass_dark", frac=0.35)
    for x in (-0.03, 0.0, 0.03):
        _front(m, x, 0.03 - 0.055, 0.17, 0.014, 0.32, "glass_dark")
    if m.detail == 0:
        for x in (-0.02, 0.02):
            _front(m, x, 0.03 - 0.04, 0.52, 0.012, 0.09, "glass_dark")
        _flag(m, 0.0, 0.03, 0.76, 0.08, size=0.04)
        kit.block(m, (0.0, -0.05, -0.02), (0.08, 0.04, 0.06), "limestone")
        for x in (-0.13, 0.13):
            _tree(m, x, -0.1, 0.08, "leaf")
        _flag(m, -0.06, -0.15, 0.0, 0.16)
        _flag(m, 0.06, -0.15, 0.0, 0.16, "white")


def center_atomic(m):
    """International-style high-rise: dark glass tower with concrete floor bands on a plaza."""
    _plaza(m, 0.21, "concrete")
    kit.block(m, (0.0, 0.05, -0.02), (0.15, 0.12, 0.88), "glass_dark")
    kit.block(m, (0.0, 0.05, -0.02), (0.26, 0.16, 0.08), "concrete")
    if m.detail == 2:
        kit.block(m, (0.0, 0.05, 0.86), (0.1, 0.08, 0.06), "concrete")
        return
    _floors(m, 0.0, 0.05, 0.08, 0.15, 0.12, 0.78, m.pick(9, 4, 1), "concrete", frac=0.18)
    kit.block(m, (0.0, 0.05, 0.86), (0.155, 0.125, 0.012), "concrete")
    kit.block(m, (0.0, 0.06, 0.87), (0.08, 0.06, 0.05), "concrete")
    m.rod((0.03, 0.06, 0.92), (0.03, 0.06, 1.02), 0.004, 0.002, "steel", sides=3)
    _front(m, 0.0, 0.05 - 0.08, 0.0, 0.2, 0.05, "glass_dark")
    if m.detail == 0:
        kit.block(m, (0.0, 0.05, 0.06), (0.27, 0.17, 0.012), "concrete")
        kit.cylinder(m, (0.08, -0.15, -0.02), 0.04, 0.03, "concrete", sides=8)
        kit.cylinder(m, (0.08, -0.15, 0.01), 0.032, 0.004, "water", sides=8)
        for x in (-0.12, -0.05):
            _tree(m, x, -0.15, 0.07, "leaf")


def center_information(m):
    """Iconic twisted glass tower on a podium with a green roof."""
    _plaza(m, 0.21, "concrete")
    kit.block(m, (0.0, 0.05, -0.02), (0.26, 0.2, 0.08), "white")
    count = m.pick(9, 5, 3)
    sections = []
    for i in range(count):
        t = i / (count - 1)
        half = 0.075 - 0.035 * t
        sections.append(_square(0.0, 0.04, 0.06 + 0.92 * t, half, half, 50.0 * t))
    m.loft(sections, "sky_glass")
    m.rod((0.0, 0.04, 0.97), (0.0, 0.04, 1.1), 0.01, 0.0, "steel", sides=m.pick(4, 3, 3))
    if m.detail == 2:
        return
    _green_roof(m, 0.0, 0.05, 0.06, 0.26, 0.2)
    for i in (2, 4, 6) if m.detail == 0 else (4,):
        t = i / 8
        half = 0.075 - 0.035 * t + 0.004
        z = 0.06 + 0.92 * t
        m.loft([_square(0.0, 0.04, z - 0.008, half, half, 50.0 * t), _square(0.0, 0.04, z + 0.008, half, half, 50.0 * t)],
               "steel")
    _front(m, 0.0, 0.05 - 0.1, 0.0, 0.22, 0.05, "sky_glass")
    if m.detail == 0:
        for x, y in ((-0.09, 0.12), (0.1, 0.11), (-0.1, -0.02), (0.1, -0.02)):
            m.ball((x, y, 0.1), (0.022, 0.022, 0.02), "leaf", u=5, v=3)
        for x in (-0.12, 0.12):
            _tree(m, x, -0.14, 0.08, "leaf_light")
        kit.block(m, (0.0, -0.15, -0.02), (0.06, 0.06, 0.03), "water")


# Landmarks ------------------------------------------------------------------------------------------
def palace(m):
    """Grand palace: wings around a forecourt, central dome, flag."""
    kit.block(m, (0.0, 0.05, -0.02), (0.28, 0.1, 0.13), "cream")
    kit.cylinder(m, (0.0, 0.05, 0.12), 0.05, 0.06, "cream", sides=m.pick(9, 6, 5))
    kit.dome(m, (0.0, 0.05, 0.18), 0.056, "copper_green", h=0.08, sides=m.pick(9, 6, 5), steps=m.pick(3, 2, 2))
    if m.detail == 2:
        kit.block(m, (0.0, -0.03, -0.02), (0.3, 0.07, 0.1), "cream")
        return
    kit.hip_roof(m, (0.0, 0.05, 0.13), 0.28, 0.1, 0.035, "slate")
    for s in (-1, 1):
        kit.block(m, (s * 0.12, -0.04, -0.02), (0.06, 0.1, 0.12), "cream")
        kit.hip_roof(m, (s * 0.12, -0.04, 0.12), 0.06, 0.1, 0.03, "slate")
    _band(m, 0.0, 0.05, 0.03, 0.28, 0.1, 0.01, "white")
    _columns(m, (-0.04, -0.013, 0.013, 0.04), -0.005, -0.01, 0.12, 0.008, "white")
    _gable(m, 0.0, -0.0, 0.11, 0.1, 0.03, 0.035, "white", front=True, overhang=0.004)
    _flag(m, 0.0, 0.05, 0.255, 0.1, size=0.05)
    if m.detail == 0:
        kit.block(m, (0.0, -0.04, -0.02), (0.1, 0.06, 0.03), "white")
        for s in (-1, 1):
            _front(m, s * 0.09, 0.0, 0.06, 0.05, 0.02, "glass_dark")
            _front(m, s * 0.12, -0.09, 0.05, 0.03, 0.04, "glass_dark")
            kit.dome(m, (s * 0.12, -0.04, 0.15), 0.02, "gold", h=0.03, sides=6, steps=2)
        kit.block(m, (0.0, -0.13, -0.02), (0.1, 0.04, 0.025), "leaf_dark")


def castle(m):
    """Stone keep inside a square curtain wall with four round corner towers."""
    half = 0.11
    kit.block(m, (0.0, 0.02, -0.02), (0.1, 0.1, 0.3), "stone")
    if m.detail == 2:
        kit.block(m, (0.0, 0.0, -0.02), (2 * half, 2 * half, 0.12), "stone")
        return
    for (x, y, w, d) in ((0, -half, 2 * half, 0.03), (0, half, 2 * half, 0.03), (-half, 0, 0.03, 2 * half),
                         (half, 0, 0.03, 2 * half)):
        kit.block(m, (x, y, -0.02), (w, d, 0.12), "stone")
    for x in (-half, half):
        for y in (-half, half):
            _round_tower(m, x, y, 0.032, 0.17, "stone", roof="slate", roof_h=0.07)
    kit.block(m, (0.0, 0.02, 0.28), (0.115, 0.115, 0.025), "rock")
    _flag(m, 0.0, 0.02, 0.3, 0.1, size=0.05)
    _front(m, 0.0, -half - 0.015, -0.01, 0.045, 0.08, "wood_dark")
    if m.detail == 0:
        _merlons(m, 0.0, 0.02, 0.305, 0.1, 0.1, 3, "rock")
        _merlons(m, 0.0, 0.0, 0.1, 0.14, 2 * half, 4, "stone")
        _front(m, 0.0, -0.03, 0.18, 0.02, 0.04, "dark")
        m.box((0.0, -half - 0.015, 0.11), (0.035, 0.022, 0.03), "stone")


def _basin(m, c, half, rim="stone"):
    """Sunken harbour basin: water slab with a stone rim at the back."""
    kit.block(m, (c[0], c[1], -0.04), (half[0] * 2, half[1] * 2, 0.044), "water")
    if m.detail < 2:
        kit.block(m, (c[0], c[1] + half[1] + 0.008, -0.04), (half[0] * 2, 0.016, 0.07), rim)


def harbor(m):
    """Stone quay with a wooden pier, a moored sailing boat, a wooden crane and a warehouse."""
    kit.block(m, (0.0, 0.08, -0.02), (0.26, 0.1, 0.05), "stone")
    _basin(m, (0.0, -0.06), (0.15, 0.06))
    kit.block(m, (0.02, 0.1, 0.03), (0.12, 0.06, 0.07), "wood")
    _gable(m, 0.02, 0.1, 0.1, 0.12, 0.06, 0.04, "terracotta")
    if m.detail == 2:
        return
    kit.block(m, (-0.06, -0.05, 0.02), (0.035, 0.12, 0.01), "deck")
    for y in (-0.1, -0.04):
        for x in (-0.075, -0.045):
            kit.cylinder(m, (x, y, -0.03), 0.005, 0.055, "wood_dark", sides=3)
    kit.boat(m, (0.06, -0.065, 0.0), 0.13, yaw=0.0)
    m.rod((-0.1, 0.06, 0.03), (-0.1, 0.06, 0.2), 0.008, 0.006, "wood_dark", sides=4)
    m.rod((-0.1, 0.06, 0.19), (-0.07, -0.03, 0.17), 0.006, 0.004, "wood_dark", sides=3)
    if m.detail == 0:
        m.rod((-0.07, -0.03, 0.17), (-0.07, -0.03, 0.07), 0.002, 0.002, "rope", sides=3)
        kit.crate(m, (-0.07, -0.03, 0.05), 0.022, "wood")
        kit.crate(m, (0.1, 0.05, 0.03), 0.025, "wood")
        kit.barrel(m, (0.12, 0.07, 0.03), 0.012, 0.03, "wood_dark")
        _front(m, 0.02, 0.07, 0.03, 0.03, 0.045, "wood_dark")


def seaport(m):
    """Concrete docks, a steel gantry crane, containers and a steamship."""
    kit.block(m, (0.0, 0.09, -0.02), (0.3, 0.1, 0.05), "concrete")
    _basin(m, (0.0, -0.06), (0.16, 0.07), rim="concrete")
    _steamship(m, 0.02, -0.065, 0.24)
    if m.detail == 2:
        return
    for sx in (-1, 1):
        m.rod((0.05 + sx * 0.035, 0.08, 0.03), (0.05 + sx * 0.035, 0.08, 0.27), 0.007, 0.007, "steel", sides=4)
        m.rod((0.05 + sx * 0.035, 0.12, 0.03), (0.05 + sx * 0.035, 0.12, 0.27), 0.007, 0.007, "steel", sides=4)
    kit.block(m, (0.05, 0.05, 0.27), (0.08, 0.12, 0.03), "citrus")
    kit.block(m, (0.05, 0.08, 0.3), (0.04, 0.05, 0.03), "white")
    m.rod((0.05, -0.0, 0.29), (0.05, -0.12, 0.29), 0.008, 0.006, "citrus", sides=4)
    for i, (x, mat) in enumerate(((-0.1, "red"), (-0.06, "roof_blue"), (-0.1, "green"), (-0.06, "ochre"))):
        kit.block(m, (x, 0.1, 0.03 + 0.03 * (i // 2)), (0.035, 0.07, 0.03), mat)


def _steamship(m, x, y, length):
    L = length / 2
    B = length * 0.13
    m.loft([[(x - L, y - B, 0.045), (x - L, y + B, 0.045), (x - L * 0.9, y, -0.03)],
            [(x + L * 0.5, y - B, 0.05), (x + L * 0.5, y + B, 0.05), (x + L * 0.5, y, -0.035)],
            [(x + L, y, 0.065)]], "hull_grey")
    kit.block(m, (x - L * 0.3, y, 0.045), (L * 0.6, B * 1.4, 0.04), "white")
    kit.cylinder(m, (x - L * 0.1, y, 0.08), 0.014, 0.07, "red", sides=m.pick(6, 4, 3))
    if m.detail == 0:
        kit.block(m, (x - L * 0.35, y, 0.085), (L * 0.3, B * 1.1, 0.025), "white")
        kit.cylinder(m, (x - L * 0.1, y, 0.14), 0.015, 0.012, "black", sides=6)
        _smoke(m, x - L * 0.1, y, 0.16, 0.018)


def factory(m):
    """Brick hall with a sawtooth roof, tall chimneys with smoke and a water tank."""
    kit.block(m, (0.0, 0.03, -0.02), (0.22, 0.16, 0.1), "brick")
    for x, h in ((0.07, 0.48), (0.12, 0.4)):
        kit.cylinder(m, (x, 0.1, -0.02), 0.02, h, "brick", sides=m.pick(6, 5, 4), r_top=0.015)
    if m.detail == 2:
        return
    for i in range(3):
        y0 = 0.03 - 0.08 + 0.054 * i
        m.loft([[(-0.11, y0, 0.08), (-0.11, y0 + 0.054, 0.08), (-0.11, y0 + 0.054, 0.14)],
                [(0.11, y0, 0.08), (0.11, y0 + 0.054, 0.08), (0.11, y0 + 0.054, 0.14)]], "slate")
        if m.detail == 0:
            kit.block(m, (0.0, y0 + 0.052, 0.085), (0.2, 0.004, 0.045), "glass")
    for x, h in ((0.07, 0.48), (0.12, 0.4)):
        _band(m, x, 0.1, h - 0.06, 0.03, 0.03, 0.015, "white", grow=0.008)
        _smoke(m, x, 0.1, h - 0.02, 0.03)
    kit.block(m, (-0.13, -0.06, -0.02), (0.07, 0.07, 0.13), "brick")
    kit.hip_roof(m, (-0.13, -0.06, 0.11), 0.07, 0.07, 0.03, "slate")
    if m.detail == 0:
        _front(m, 0.0, 0.03 - 0.08, 0.02, 0.18, 0.025, "glass_dark")
        _front(m, -0.03, 0.03 - 0.08, -0.01, 0.04, 0.06, "wood_dark")
        kit.cylinder(m, (-0.1, 0.11, 0.1), 0.025, 0.05, "rust", sides=6)
        for sx in (-1, 1):
            m.rod((-0.1 + sx * 0.02, 0.11, -0.02), (-0.1 + sx * 0.02, 0.11, 0.1), 0.003, 0.003, "iron", sides=3)


def university(m):
    """Gothic college: long hall with a steep roof, gate tower with a tall spire, pinnacles."""
    kit.block(m, (0.0, 0.03, -0.02), (0.24, 0.08, 0.13), "sandstone")
    _gable(m, 0.0, 0.03, 0.11, 0.24, 0.08, 0.08, "slate")
    kit.block(m, (0.0, -0.01, -0.02), (0.06, 0.06, 0.25), "sandstone")
    m.taper((0.0, -0.01, 0.23), (0.034, 0.034), (0.001, 0.001), 0.2, "slate")
    if m.detail == 2:
        return
    _band(m, 0.0, -0.01, 0.22, 0.06, 0.06, 0.015, "limestone", grow=0.012)
    for s in (-1, 1):
        kit.block(m, (s * 0.11, -0.03, -0.02), (0.06, 0.06, 0.1), "sandstone")
        _gable(m, s * 0.11, -0.03, 0.08, 0.06, 0.06, 0.06, "slate", front=True)
    if m.detail == 0:
        for x in (-0.07, -0.045, 0.045, 0.07):
            _front(m, x, 0.03 - 0.04, 0.03, 0.01, 0.06, "glass_dark")
        _front(m, 0.0, -0.04, -0.01, 0.025, 0.07, "wood_dark")
        _front(m, 0.0, -0.04, 0.12, 0.02, 0.06, "glass")
        for x in (-0.12, 0.12):
            m.taper((x, 0.03, 0.11), (0.008, 0.008), (0.001, 0.001), 0.08, "sandstone")
        kit.block(m, (0.0, -0.13, -0.02), (0.22, 0.06, 0.016), "grass")


def _ring_segments(m, rx, ry, n, thick, height_at, mat, z0=-0.02, scale_r=1.0):
    pts = _ellipse(rx * scale_r, ry * scale_r, n)
    for k in range(n):
        a, b = pts[k], pts[(k + 1) % n]
        mid_angle = math.atan2((a[1] + b[1]) / 2, (a[0] + b[0]) / 2)
        h = height_at(mid_angle)
        if h <= 0:
            continue
        kit.block(m, ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, z0),
                  (math.hypot(b[0] - a[0], b[1] - a[1]) + 0.006, thick, h - z0),
                  mat, math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])))


def arena(m):
    """Oval colosseum: tall arcaded back wall, broken lower front, sand floor."""
    rx, ry = 0.17, 0.13
    n = m.pick(14, 10, 8)

    def outer(angle):
        return 0.07 + 0.08 * max(math.sin(angle), 0.0)

    if m.detail == 2:
        m.rings([((0.0, 0.0, -0.02), rx, ry), ((0.0, 0.0, 0.12), rx, ry)], "limestone", sides=8)
        m.rings([((0.0, 0.0, 0.1), rx * 0.7, ry * 0.7), ((0.0, 0.0, 0.122), rx * 0.7, ry * 0.7)], "sand", sides=8)
        return
    m.rings([((0.0, 0.0, -0.03), rx * 0.62, ry * 0.62), ((0.0, 0.0, 0.012), rx * 0.62, ry * 0.62)], "sand",
            sides=m.pick(10, 7, 6))
    _ring_segments(m, rx, ry, n, 0.03, outer, "limestone")
    if m.detail == 0:
        _ring_segments(m, rx, ry, n, 0.04, lambda a: 0.045 + 0.03 * max(math.sin(a), 0.0), "stone", scale_r=0.8)
    if m.detail == 0:
        pts = _ellipse(rx + 0.0165, ry + 0.0165, n)
        for k in range(n):
            a, b = pts[k], pts[(k + 1) % n]
            mid = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
            if mid[1] > 0.02:
                continue
            kit.block(m, (mid[0], mid[1], 0.012), (math.hypot(b[0] - a[0], b[1] - a[1]) * 0.6, 0.006, 0.035), "dark",
                      math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])))
        _flag(m, -0.1, 0.1, 0.14, 0.06, "red", size=0.035)
        _flag(m, 0.1, 0.1, 0.14, 0.06, "red", size=0.035)


def military_base(m):
    """Arched hangar, radar dish, barracks, flag."""
    kit.patch(m, (0.0, 0.0), (0.16, 0.14), "asphalt", z0=-0.04, z1=0.008)
    m.prism([(0.0, -0.02), (0.07, -0.02), (0.07, 0.03), (0.05, 0.07), (0.0, 0.085), (-0.05, 0.07), (-0.07, 0.03),
             (-0.07, -0.02)], 0.18, "olive", axis="x")
    if m.detail == 2:
        kit.block(m, (-0.1, -0.09, -0.02), (0.05, 0.05, 0.08), "khaki")
        return
    with m.at(move(0.0, 0.03, 0.0)):
        pass
    _front(m, -0.091, 0.0, -0.01, 0.006, 0.065, "dark")
    m.rod((-0.11, -0.09, -0.02), (-0.11, -0.09, 0.14), 0.007, 0.006, "steel", sides=4)
    m.rings([((-0.11, -0.095, 0.13), 0.004, 0.004), ((-0.11, -0.11, 0.16), 0.045, 0.045)], "white",
            sides=m.pick(8, 6, 4), axis="z")
    kit.block(m, (0.1, -0.1, -0.02), (0.1, 0.05, 0.07), "khaki")
    _gable(m, 0.1, -0.1, 0.05, 0.1, 0.05, 0.02, "olive")
    _flag(m, 0.02, -0.12, 0.0, 0.22, size=0.05)
    if m.detail == 0:
        with m.at(move(0.11, 0.1, 0.0)):
            kit.block(m, (0.0, 0.0, -0.01), (0.06, 0.035, 0.025), "olive")
            kit.block(m, (0.0, 0.0, 0.015), (0.03, 0.025, 0.015), "olive")
            m.rod((0.0, 0.0, 0.022), (0.0, -0.05, 0.022), 0.003, 0.003, "olive", sides=3)
        for x in (-0.06, 0.06):
            kit.block(m, (x, -0.01, 0.02), (0.006, 0.08, 0.004), "white")


def broadcast_tower(m):
    """Red-and-white lattice radio mast with cross braces and a small transmitter hut."""
    top = 0.95
    base_r, top_r = 0.085, 0.01
    legs = kit.ring(3, 1.0, 90.0)
    sections = m.pick(5, 3, 1)

    def leg_point(i, t):
        r = base_r + (top_r - base_r) * t
        return (legs[i][0] * r, legs[i][1] * r + 0.02, top * t - (0.02 if t == 0 else 0.0))

    for s in range(sections):
        t0, t1 = s / sections, (s + 1) / sections
        mat = "red" if s % 2 == 0 else "white"
        for i in range(3):
            m.rod(leg_point(i, t0), leg_point(i, t1), 0.011 * (1 - t0) + 0.005, 0.011 * (1 - t1) + 0.005, mat,
                  sides=3)
    if m.detail == 2:
        kit.block(m, (0.0, -0.08, -0.02), (0.07, 0.05, 0.06), "concrete")
        return
    levels = m.pick(6, 4, 0)
    for j in range(1, levels + 1):
        t = j / (levels + 1)
        for i in range(3):
            a, b = leg_point(i, t), leg_point((i + 1) % 3, t)
            m.rod(a, b, 0.0035, 0.0035, "red" if j % 2 else "white", sides=3)
            if m.detail == 0:
                m.rod(leg_point(i, t - 1 / (levels + 1)), b, 0.003, 0.003, "steel", sides=3)
    m.rod((0.0, 0.02, top - 0.02), (0.0, 0.02, top + 0.12), 0.004, 0.002, "red", sides=3)
    kit.block(m, (0.0, -0.08, -0.02), (0.08, 0.05, 0.07), "concrete")
    if m.detail == 0:
        m.ball((0.0, 0.02, top + 0.12), 0.008, "fire", u=4, v=3)
        m.rings([((0.0, 0.0, 0.45), 0.004, 0.004), ((0.0, -0.025, 0.47), 0.03, 0.03)], "white", sides=6)
        _front(m, 0.0, -0.105, -0.01, 0.02, 0.045, "steel")


def _stall(m, x, y, yaw, awning):
    with m.at(move(x, y, 0.0), rot_z(yaw)):
        m.box((0.0, 0.0, 0.015), (0.03, 0.015, 0.025), "wood")
        if m.detail == 0:
            for sx in (-1, 1):
                m.rod((sx * 0.028, 0.015, -0.01), (sx * 0.028, 0.015, 0.075), 0.003, 0.003, "wood_dark", sides=3)
            m.box((0.0, 0.0, 0.043), (0.026, 0.012, 0.006), "citrus" if awning != "citrus" else "grape")
        m.loft([[(-0.035, 0.022, 0.075), (0.035, 0.022, 0.075), (0.035, 0.022, 0.081), (-0.035, 0.022, 0.081)],
                [(-0.035, -0.03, 0.055), (0.035, -0.03, 0.055), (0.035, -0.03, 0.061), (-0.035, -0.03, 0.061)]],
               awning)


def market(m):
    """Open market: a timber market hall and stalls with coloured awnings round it."""
    kit.patch(m, (0.0, 0.0), (0.16, 0.14), "stone", z0=-0.04, z1=0.008)
    kit.block(m, (0.0, 0.06, 0.07), (0.12, 0.08, 0.02), "wood")
    kit.hip_roof(m, (0.0, 0.06, 0.09), 0.12, 0.08, 0.05, "terracotta")
    if m.detail == 2:
        kit.block(m, (0.0, 0.06, -0.02), (0.1, 0.06, 0.09), "wood_dark")
        return
    for x in (-0.05, 0.05):
        for y in (0.03, 0.09):
            kit.cylinder(m, (x, y, -0.01), 0.006, 0.08, "wood_dark", sides=m.pick(4, 3, 3))
    stalls = ((-0.1, -0.06, 15, "red"), (-0.03, -0.09, 0, "linen"), (0.05, -0.09, 0, "roof_blue"),
              (0.12, -0.05, -15, "citrus"), (-0.13, 0.04, 40, "green"))
    for x, y, yaw, awning in stalls if m.detail == 0 else stalls[:3]:
        _stall(m, x, y, yaw, awning)
    if m.detail == 0:
        kit.crate(m, (0.12, 0.06, -0.01), 0.025, "wood")
        kit.barrel(m, (0.12, 0.1, -0.01), 0.012, 0.03, "wood_dark")
        m.ball((0.0, 0.06, 0.012), (0.02, 0.015, 0.015), "citrus", u=5, v=3)
        m.ball((-0.03, 0.05, 0.012), (0.018, 0.015, 0.014), "wheat", u=5, v=3)


def national_national_college(m):
    """Domed academy: a long hall, a columned portico and a green dome on a drum."""
    kit.block(m, (0.0, 0.04, -0.02), (0.24, 0.12, 0.13), "limestone")
    kit.cylinder(m, (0.0, 0.04, 0.11), 0.05, 0.06, "limestone", sides=m.pick(10, 7, 5))
    kit.dome(m, (0.0, 0.04, 0.17), 0.055, "copper_green", h=0.075, sides=m.pick(10, 7, 5), steps=m.pick(3, 2, 2))
    if m.detail == 2:
        return
    kit.hip_roof(m, (0.0, 0.04, 0.13), 0.24, 0.12, 0.03, "slate")
    _columns(m, (-0.045, -0.015, 0.015, 0.045), -0.04, 0.0, 0.11, 0.008, "white")
    kit.block(m, (0.0, -0.035, 0.11), (0.11, 0.04, 0.015), "white")
    _gable(m, 0.0, -0.035, 0.125, 0.11, 0.04, 0.035, "white", front=True, overhang=0.004)
    kit.block(m, (0.0, -0.04, -0.02), (0.12, 0.06, 0.02), "white")
    if m.detail == 0:
        kit.cylinder(m, (0.0, 0.04, 0.24), 0.01, 0.03, "white", sides=5)
        kit.cone(m, (0.0, 0.04, 0.27), 0.012, 0.03, "gold", sides=5)
        for x in (-0.09, 0.09):
            _front(m, x, 0.04 - 0.06, 0.04, 0.04, 0.05, "glass_dark")
        kit.block(m, (0.0, -0.13, -0.02), (0.2, 0.03, 0.03), "leaf_dark")


# Catalog ----------------------------------------------------------------------------------------------
HOUSES = {
    "ancient": (house_ancient_0, house_ancient_1, house_ancient_2),
    "classical": (house_classical_0, house_classical_1, house_classical_2),
    "medieval": (house_medieval_0, house_medieval_1, house_medieval_2),
    "renaissance": (house_renaissance_0, house_renaissance_1, house_renaissance_2),
    "industrial": (house_industrial_0, house_industrial_1, house_industrial_2),
    "modern": (house_modern_0, house_modern_1, house_modern_2),
    "atomic": (house_atomic_0, house_atomic_1, house_atomic_2),
    "information": (house_information_0, house_information_1, house_information_2),
}
CENTERS = {
    "ancient": center_ancient, "classical": center_classical, "medieval": center_medieval,
    "renaissance": center_renaissance, "industrial": center_industrial, "modern": center_modern,
    "atomic": center_atomic, "information": center_information,
}
LANDMARK_BUILDERS = {
    "city/palace": palace, "city/castle": castle, "city/harbor": harbor, "city/seaport": seaport,
    "city/factory": factory, "city/university": university, "city/arena": arena,
    "city/military-base": military_base, "city/broadcast-tower": broadcast_tower, "city/market": market,
    "city/national-national-college": national_national_college,
}


def models():
    out = []
    for style in STYLES:
        out.append(WorldModel(KIT["centers"][style], CENTERS[style], "city-center"))
        for house, build in zip(KIT["houses"][style], HOUSES[style]):
            out.append(WorldModel(house, build, "city-house"))
    for model_id, build in LANDMARK_BUILDERS.items():
        out.append(WorldModel(model_id, build, "city-landmark", ground="upright"))
    return out
