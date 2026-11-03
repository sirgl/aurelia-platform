"""Ships of all eras. Built along +x (bow forward), waterline at z = 0.

The hull is a loft of convex sections; masts, sails and superstructure sit on the deck.
Sails and flags carry the owner color.
"""
from geom import move, rot_y, rot_z, scale
import people


def hull(m, length, beam, freeboard, draft, material="hull", bow=1.0, stern=0.55, sheer=0.0, deck=None):
    """Hull from stern (-x) to a pointed bow (+x). Returns the deck height."""
    half = length / 2
    stations = [(-half, stern, sheer), (-half * 0.5, 0.95, 0.0), (half * 0.3, 1.0, 0.0), (half * 0.75, 0.7, sheer * 0.5)]
    sections = []
    for x, w, lift in stations:
        b = beam * w
        top = freeboard + lift
        sections.append([(x, -b, top), (x, -b * 0.85, -draft * 0.6), (x, 0.0, -draft), (x, b * 0.85, -draft * 0.6),
                         (x, b, top)])
    sections.append([(half * bow, 0.0, freeboard + sheer)])
    m.loft(sections, material)
    if deck and m.detail < 2:
        m.loft([[(-half * 0.95, -beam * stern * 0.9, freeboard), (-half * 0.95, beam * stern * 0.9, freeboard),
                 (half * 0.7, beam * 0.65, freeboard), (half * 0.7, -beam * 0.65, freeboard)],
                [(-half * 0.95, -beam * stern * 0.9, freeboard + 0.01),
                 (-half * 0.95, beam * stern * 0.9, freeboard + 0.01),
                 (half * 0.7, beam * 0.65, freeboard + 0.01), (half * 0.7, -beam * 0.65, freeboard + 0.01)]], deck)
    return freeboard


def mast(m, x, height, z0, material="wood", r=0.022):
    m.rod((x, 0, z0), (x, 0, z0 + height), r, r * 0.7, material, sides=m.pick(5, 4, 3))


def square_sail(m, x, z0, z1, width, material="team", belly=0.05):
    """Square sail across the ship (spans y), bellied forward (+x)."""
    m.loft([[(x, -width, z1), (x, width, z1), (x + belly * 0.4, width * 0.9, z0), (x + belly * 0.4, -width * 0.9, z0)],
            [(x + belly, -width * 0.8, (z0 + z1) / 2 + 0.02), (x + belly, width * 0.8, (z0 + z1) / 2 + 0.02),
             (x + belly, width * 0.8, (z0 + z1) / 2 - 0.02), (x + belly, -width * 0.8, (z0 + z1) / 2 - 0.02)]],
           material)
    m.rod((x - 0.005, -width * 1.05, z1), (x - 0.005, width * 1.05, z1), 0.008, 0.008, "wood", sides=3)


def lateen_sail(m, x, z0, height, length, material="team"):
    """Triangular sail along the ship (spans x), slightly offset to the near side."""
    m.prism([(x - length * 0.5, z0), (x + length * 0.5, z0 + height * 0.15), (x - length * 0.15, z0 + height)],
            0.012, material)


def flag(m, x, z, material="team", size=0.12):
    m.rod((x, 0, z - 0.02), (x, 0, z + size * 0.9), 0.006, 0.006, "wood", sides=3)
    with m.at(move(x, 0, z + size * 0.9)):
        m.prism([(0.0, 0.0), (-size * 1.4, -size * 0.1), (-size * 1.3, -size * 0.7), (0.0, -size * 0.7)], 0.01,
                material)


def oars(m, x0, x1, count, beam, z, material="wood"):
    if m.detail >= 2:
        return
    count = count if m.detail == 0 else max(2, count // 2)
    for i in range(count):
        x = x0 + (x1 - x0) * i / max(count - 1, 1)
        for s in (-1, 1):
            m.rod((x, s * beam * 0.9, z), (x - 0.04, s * (beam + 0.22), -0.02), 0.008, 0.008, material, sides=3)


def turret(m, x, z, length, color="hull_grey", barrels=2, back=False):
    d = -1 if back else 1
    m.rings([((x, 0, z), 0.09, 0.08), ((x, 0, z + 0.06), 0.075, 0.065)], color, sides=m.pick(8, 5, 4))
    if m.detail < 2:
        offsets = [0.0] if barrels == 1 else [-0.025, 0.025] if barrels == 2 else [-0.035, 0.0, 0.035]
        for o in offsets:
            m.rod((x + d * 0.06, o, z + 0.035), (x + d * length, o, z + 0.045), 0.012, 0.01, "dark", sides=3)


def funnel(m, x, z, h, r=0.05, color="dark", tilt=0):
    with m.at(move(x, 0, z), rot_y(tilt)):
        m.rings([((0, 0, 0), r, r * 0.8), ((0, 0, h), r, r * 0.8)], color, sides=m.pick(6, 5, 4))


def deckhouse(m, x, z, hx, hy, h, color):
    m.taper((x, 0, z), (hx, hy), (hx * 0.9, hy * 0.9), h, color)


# Classes ----------------------------------------------------------------------------
def trireme(m, sail="team", ram="bronze", hull_mat="hull", eye=True, banks=1, length=1.9, rig="square",
            tower=False):
    z = hull(m, length, 0.17, 0.12, 0.08, hull_mat, bow=1.12, stern=0.5, sheer=0.10, deck="deck")
    m.rod((length * 0.52, 0, 0.0), (length * 0.62, 0, -0.02), 0.04, 0.0, ram, sides=m.pick(5, 4, 3))
    for b in range(banks):
        oars(m, -length * 0.3, length * 0.3, 7, 0.17, z - 0.03 - b * 0.03)
    if rig == "square":
        mast(m, 0.05, 0.85, z)
        square_sail(m, 0.05, z + 0.30, z + 0.80, 0.36, sail)
    else:
        for x, h in ((0.35, 0.75), (-0.25, 0.65)):
            mast(m, x, h, z)
            lateen_sail(m, x, z + 0.15, h * 0.85, 0.6, sail)
    if tower:
        deckhouse(m, -0.35, z, 0.14, 0.13, 0.22, "wood")
        m.box((-0.35, 0, z + 0.23), (0.15, 0.14, 0.012), "team")
    if m.detail < 2:
        m.rod((-length * 0.5, 0, 0.16), (-length * 0.56, 0, 0.30), 0.03, 0.01, hull_mat, sides=3)
        if eye:
            for s in (-1, 1):
                m.box((length * 0.42, s * 0.12, 0.08), (0.03, 0.01, 0.02), "white")


def galleass(m, sail="team", length=2.3, stripe=""):
    z = hull(m, length, 0.22, 0.16, 0.09, "hull", bow=1.1, stern=0.6, sheer=0.08, deck="deck")
    if stripe and m.detail < 2:
        m.box((-0.05, 0, z - 0.05), (length * 0.36, 0.222, 0.025), stripe)
    oars(m, -0.7, 0.6, 8, 0.22, z - 0.04)
    deckhouse(m, -0.85, z, 0.18, 0.18, 0.14, "hull")
    for x, h in ((0.45, 0.80), (-0.25, 0.95)):
        mast(m, x, h, z)
        lateen_sail(m, x, z + 0.2, h * 0.8, 0.75, sail)
    if m.detail < 2:
        for x in (0.8, 0.95):
            m.rod((x, 0, z + 0.05), (x + 0.25, 0, z + 0.06), 0.02, 0.015, "bronze", sides=3)
        flag(m, -0.25, z + 0.95)


def caravel(m, sail="team", hull_mat="hull", rig="lateen"):
    z = hull(m, 1.8, 0.24, 0.20, 0.10, hull_mat, bow=1.12, stern=0.75, sheer=0.12, deck="deck")
    deckhouse(m, -0.68, z, 0.20, 0.20, 0.16, hull_mat)
    if rig == "lateen":
        for x, h in ((0.30, 0.95), (-0.25, 0.80)):
            mast(m, x, h, z)
            lateen_sail(m, x, z + 0.18, h * 0.8, 0.65, sail)
    else:
        for x, h in ((0.32, 0.95), (-0.18, 1.0)):
            mast(m, x, h, z)
            square_sail(m, x, z + 0.25, z + h - 0.10, 0.36, sail)
    flag(m, -0.25 if rig == "lateen" else -0.18, z + 0.95)


def turtle_ship(m):
    z = hull(m, 1.8, 0.26, 0.14, 0.10, "hull", bow=1.05, stern=0.75, deck=None)
    m.loft([[(-0.80, -0.26, z), (-0.80, 0.26, z), (-0.80, 0.0, z + 0.30)],
            [(0.70, -0.24, z), (0.70, 0.24, z), (0.70, 0.0, z + 0.28)]], "team")
    m.rings([((0.92, 0, z + 0.05), 0.08, 0.07), ((1.10, 0, z + 0.12), 0.05, 0.05)], "gold", axis="x",
            sides=m.pick(6, 4, 3))
    oars(m, -0.55, 0.45, 6, 0.26, z - 0.04)
    if m.detail < 2:
        for x in (-0.5, -0.1, 0.3):
            m.rod((x, 0, z + 0.20), (x, 0, z + 0.36), 0.03, 0.0, "iron", sides=4)
        mast(m, 0.0, 0.6, z + 0.25)


def frigate(m, sail="team", masts=3, hull_mat="hull", stripe="black"):
    z = hull(m, 2.5, 0.28, 0.26, 0.12, hull_mat, bow=1.15, stern=0.8, sheer=0.10, deck="deck")
    if m.detail < 2:
        hull_len = 2.5
        m.loft([[(-hull_len * 0.42, -0.282, z - 0.10), (-hull_len * 0.42, 0.282, z - 0.10),
                 (-hull_len * 0.42, 0.282, z - 0.04), (-hull_len * 0.42, -0.282, z - 0.04)],
                [(hull_len * 0.30, -0.282, z - 0.10), (hull_len * 0.30, 0.282, z - 0.10),
                 (hull_len * 0.30, 0.282, z - 0.04), (hull_len * 0.30, -0.282, z - 0.04)]], stripe)
    deckhouse(m, -1.0, z, 0.22, 0.24, 0.14, hull_mat)
    xs = (0.62, 0.05, -0.55) if masts == 3 else (0.40, -0.30)
    for i, x in enumerate(xs):
        h = 1.25 if i == 1 or masts == 2 else 1.05
        mast(m, x, h, z)
        square_sail(m, x, z + 0.25, z + h * 0.55, 0.40, sail)
        if m.detail == 0:
            square_sail(m, x, z + h * 0.60, z + h * 0.92, 0.30, sail)
    flag(m, xs[1], z + 1.25 if masts == 3 else z + 1.25)


def privateer(m, sail="team", hull_mat="wood_dark", pennant="black"):
    frigate(m, sail=sail, masts=2, hull_mat=hull_mat, stripe="red")
    if m.detail < 2:
        flag(m, -0.95, 0.40, pennant, 0.12)


def ironclad(m):
    z = hull(m, 2.0, 0.26, 0.10, 0.10, "iron", bow=1.05, stern=0.8, deck=None)
    m.taper((-0.10, 0, z), (0.55, 0.22), (0.45, 0.16), 0.18, "dark")
    funnel(m, -0.05, z + 0.18, 0.30, 0.06, "dark")
    if m.detail < 2:
        for s in (-1, 1):
            for x in (-0.3, 0.1):
                m.rod((x, s * 0.19, z + 0.10), (x, s * 0.26, z + 0.10), 0.02, 0.02, "black", sides=3)
        flag(m, -0.60, z + 0.20)
    m.box((0.0, 0, z + 0.185), (0.30, 0.08, 0.008), "team")


def destroyer(m, color="hull_grey"):
    z = hull(m, 2.4, 0.20, 0.14, 0.08, color, bow=1.18, stern=0.7, sheer=0.05, deck=None)
    deckhouse(m, 0.25, z, 0.20, 0.13, 0.20, color)
    deckhouse(m, 0.30, z + 0.20, 0.10, 0.08, 0.12, color)
    funnel(m, -0.15, z, 0.30, 0.055, color, tilt=-8)
    funnel(m, -0.40, z, 0.26, 0.055, color, tilt=-8)
    turret(m, 0.70, z, 0.30, color, barrels=1)
    turret(m, -0.80, z, 0.30, color, barrels=1, back=True)
    m.box((0.0, 0, z + 0.005), (0.85, 0.13, 0.006), "team")
    if m.detail < 2:
        mast(m, 0.20, 0.55, z + 0.30, "dark", r=0.012)


def battleship(m, color="hull_grey"):
    z = hull(m, 3.1, 0.30, 0.18, 0.12, color, bow=1.15, stern=0.75, sheer=0.04, deck=None)
    m.box((0.0, 0, z + 0.005), (1.15, 0.22, 0.006), "deck")
    turret(m, 0.95, z, 0.45, color, barrels=3)
    turret(m, 0.62, z + 0.07, 0.45, color, barrels=3)
    turret(m, -0.95, z, 0.45, color, barrels=3, back=True)
    deckhouse(m, 0.10, z, 0.30, 0.17, 0.20, color)
    deckhouse(m, 0.18, z + 0.20, 0.13, 0.10, 0.30, color)
    funnel(m, -0.25, z + 0.10, 0.32, 0.08, color, tilt=-6)
    if m.detail < 2:
        mast(m, 0.20, 0.45, z + 0.50, "dark", r=0.014)
        flag(m, -0.60, z + 0.30)
    m.box((0.20, 0, z + 0.505), (0.10, 0.08, 0.006), "team")


def submarine(m, color="dark", nuclear=False):
    length = 2.6 if nuclear else 2.0
    r = 0.16 if nuclear else 0.12
    m.rings([((-length / 2, 0, -0.02), 0.0, 0.0), ((-length * 0.38, 0, -0.02), r * 0.7, r * 0.7),
             ((length * 0.30, 0, -0.02), r, r), ((length / 2, 0, -0.02), r * 0.3, r * 0.3)], color, axis="x",
            sides=m.pick(8, 6, 4))
    m.taper((length * 0.10, 0, 0.02), (0.18, 0.05), (0.14, 0.04), 0.26 if nuclear else 0.22, color)
    m.box((length * 0.10, 0, 0.21 if not nuclear else 0.25), (0.14, 0.045, 0.01), "team")
    if m.detail < 2:
        m.box((length * 0.10, 0, 0.18 if not nuclear else 0.22), (0.04, 0.22, 0.008), color)
        m.box((-length / 2 + 0.04, 0, 0.0), (0.03, 0.006, 0.12), color)
        if not nuclear:
            m.rod((length * 0.12, 0, 0.24), (length * 0.12, 0, 0.36), 0.01, 0.01, "dark", sides=3)
            m.box((length * 0.30, 0, r * 0.98 - 0.02), (0.06, 0.03, 0.025), "hull_grey")


def carrier(m, color="hull_grey"):
    z = hull(m, 3.2, 0.30, 0.20, 0.10, color, bow=1.08, stern=0.95, deck=None)
    m.loft([[(-1.55, -0.40, z), (-1.55, 0.36, z), (1.50, 0.30, z), (1.50, -0.34, z)],
            [(-1.55, -0.40, z + 0.04), (-1.55, 0.36, z + 0.04), (1.50, 0.30, z + 0.04),
             (1.50, -0.34, z + 0.04)]], "dark")
    deckhouse(m, 0.10, z + 0.04, 0.22, 0.06, 0.26, color)
    m.box((0.10, 0, z + 0.31), (0.10, 0.05, 0.006), "team")
    if m.detail < 2:
        m.box((0.0, -0.05, z + 0.045), (1.4, 0.008, 0.002), "white")
        for x in (-0.9, -0.5, 0.6):
            with m.at(move(x, -0.12, z + 0.05), rot_z(15), scale(0.35)):
                jet_marker(m)


def jet_marker(m):
    """Tiny parked jet for carrier decks."""
    m.loft([[(0.4, 0.0, 0.0), (-0.3, -0.35, 0.0), (-0.3, 0.35, 0.0)],
            [(0.4, 0.0, 0.06), (-0.3, -0.35, 0.06), (-0.3, 0.35, 0.06)]], "steel")


def missile_cruiser(m, color="hull_grey"):
    z = hull(m, 2.6, 0.22, 0.16, 0.08, color, bow=1.15, stern=0.75, sheer=0.05, deck=None)
    deckhouse(m, 0.05, z, 0.32, 0.16, 0.22, color)
    deckhouse(m, 0.10, z + 0.22, 0.14, 0.11, 0.16, color)
    for x in (0.70, -0.75):
        m.box((x, 0, z + 0.06), (0.16, 0.12, 0.06), "dark")
    turret(m, 0.98, z, 0.22, color, barrels=1)
    m.box((0.0, 0, z + 0.005), (0.95, 0.14, 0.006), "team")
    if m.detail < 2:
        mast(m, 0.10, 0.40, z + 0.38, "dark", r=0.012)
        m.box((0.10, 0, z + 0.62), (0.03, 0.10, 0.04), "steel")


def work_boat(m, worker_look):
    """Fishing boat: a seated rower, a lateen sail and a net float."""
    z = hull(m, 1.5, 0.24, 0.12, 0.07, "hull", bow=1.12, stern=0.7, sheer=0.06, deck="deck")
    mast(m, 0.25, 0.75, z)
    lateen_sail(m, 0.25, z + 0.10, 0.62, 0.62, "team")
    saved = m.detail
    m.detail = max(m.detail, 1)
    if saved < 2:
        with m.at(move(-0.30, 0, z - 0.30), rot_z(90)):
            people.figure(m, people.with_(worker_look, seated=True, right=""))
        if saved == 0:
            m.rings([((0.20, -0.36, -0.01), 0.12, 0.09), ((0.20, -0.36, 0.03), 0.14, 0.11)], "rope", sides=6)
    m.detail = saved


def cargo_ship(m):
    """Ancient-era trading cog with bales and one square sail."""
    z = hull(m, 1.7, 0.27, 0.20, 0.10, "hull", bow=1.05, stern=0.85, sheer=0.10, deck="deck")
    mast(m, 0.05, 0.95, z)
    square_sail(m, 0.05, z + 0.30, z + 0.88, 0.40, "team")
    m.box((-0.45, 0, z + 0.08), (0.18, 0.14, 0.08), "linen")
    m.box((0.45, 0, z + 0.07), (0.14, 0.12, 0.07), "rope")
    if m.detail < 2:
        deckhouse(m, -0.75, z, 0.10, 0.18, 0.12, "hull")
        m.rings([((0.2, 0.0, z), 0.06, 0.06), ((0.2, 0.0, z + 0.14), 0.06, 0.06)], "wood_dark", sides=6)


def flagship(m):
    """Great Admiral: a gilded galleon with a large owner banner."""
    frigate(m, sail="sail", masts=3, hull_mat="hull", stripe="gold")
    if m.detail < 2:
        flag(m, 0.05, 1.55, "team", 0.24)
