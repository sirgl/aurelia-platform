"""Siege engines, guns, vehicles, aircraft and spaceship parts. Built along +x (forward).

Crew figures are added at reduced detail so a whole engine stays inside its budget.
"""
from geom import move, rot_x, rot_y, rot_z, scale
import people

# Crewed engines are modeled 1.35x larger than people (catalog.ENGINE_SCALE) so a single
# engine reads at game scale; the crew is scaled back to human size.
ENGINE_SIZE = 1.35


def crew(m, look, at, facing=90):
    """A standing crew figure at reduced detail (never the close-up figure)."""
    if m.detail >= 2:
        return
    saved = m.detail
    m.detail = 1
    with m.at(move(*at), rot_z(facing), scale(1 / ENGINE_SIZE)):
        people.figure(m, look)
    m.detail = saved


def _frame(m, length, width, z, mat="wood", beam=0.035):
    for s in (-1, 1):
        m.box((0, s * width, z), (length, beam, beam), mat)
    for x in (-length * 0.8, length * 0.8):
        m.box((x, 0, z), (beam, width, beam * 0.8), mat)


# Siege engines -------------------------------------------------------------------
def catapult(m, crew_look):
    _frame(m, 0.42, 0.17, 0.16)
    for x in (-0.30, 0.30):
        for s in (-1, 1):
            m.wheel((x, s * 0.22, 0.14), 0.14, 0.04, "wood_dark", hub="iron", axis="y")
    for s in (-1, 1):
        m.rod((0.05, s * 0.16, 0.18), (0.10, s * 0.10, 0.52), 0.025, 0.02, "wood", sides=m.pick(4, 3, 3))
    m.box((0.10, 0, 0.52), (0.03, 0.13, 0.03), "wood")
    m.rod((-0.10, 0, 0.22), (-0.42, 0, 0.62), 0.03, 0.025, "wood", sides=m.pick(4, 3, 3))
    m.rings([((-0.46, 0, 0.60), 0.08, 0.08), ((-0.46, 0, 0.70), 0.10, 0.10)], "wood_dark", sides=m.pick(6, 4, 4))
    if m.detail < 2:
        m.ball((-0.46, 0, 0.72), 0.07, "stone", u=5, v=3)
        m.rod((-0.14, 0, 0.20), (-0.14, 0, 0.30), 0.06, 0.06, "rope", sides=5)
    crew(m, crew_look, (0.10, -0.40, 0.0), facing=110)
    crew(m, crew_look, (-0.40, 0.32, 0.0), facing=70)


def ballista(m, crew_look):
    _frame(m, 0.36, 0.16, 0.14)
    for x in (-0.24, 0.24):
        for s in (-1, 1):
            m.wheel((x, s * 0.21, 0.12), 0.12, 0.035, "wood_dark", hub="iron", axis="y")
    m.box((0.0, 0, 0.32), (0.32, 0.05, 0.04), "wood")
    m.box((0.18, 0, 0.36), (0.04, 0.10, 0.08), "wood_dark")
    for s in (-1, 1):
        m.rod((0.18, s * 0.06, 0.38), (0.06, s * 0.40, 0.42), 0.025, 0.015, "wood_dark", sides=m.pick(4, 3, 3))
        if m.detail < 2:
            m.rod((0.06, s * 0.40, 0.42), (-0.20, 0, 0.40), 0.006, 0.006, "rope", sides=3)
    m.rod((-0.22, 0, 0.40), (0.42, 0, 0.40), 0.012, 0.0, "iron", sides=3)
    for s in (-1, 1):
        m.rod((0.0, s * 0.12, 0.16), (0.0, s * 0.05, 0.30), 0.02, 0.02, "wood", sides=3)
    crew(m, crew_look, (-0.25, -0.38, 0.0), facing=100)


def trebuchet(m, crew_look):
    _frame(m, 0.40, 0.20, 0.06, beam=0.04)
    for s in (-1, 1):
        m.rod((-0.30, s * 0.20, 0.06), (0.0, s * 0.10, 0.95), 0.03, 0.025, "wood", sides=m.pick(4, 3, 3))
        m.rod((0.30, s * 0.20, 0.06), (0.0, s * 0.10, 0.95), 0.03, 0.025, "wood", sides=m.pick(4, 3, 3))
    m.rod((0.0, -0.14, 0.95), (0.0, 0.14, 0.95), 0.025, 0.025, "iron", sides=4)
    m.rod((0.22, 0, 0.82), (-0.55, 0, 1.40), 0.03, 0.02, "wood", sides=m.pick(4, 3, 3))
    m.box((0.24, 0, 0.66), (0.10, 0.09, 0.12), "wood_dark")
    if m.detail < 2:
        m.rod((-0.55, 0, 1.40), (-0.62, 0, 1.10), 0.006, 0.006, "rope", sides=3)
        m.ball((-0.62, 0, 1.08), 0.05, "stone", u=5, v=3)
    crew(m, crew_look, (0.40, -0.35, 0.0), facing=120)


def siege_tower(m, crew_look):
    m.taper((0, 0, 0.0), (0.26, 0.24), (0.20, 0.19), 1.15, "wood")
    m.taper((0, 0, 1.15), (0.24, 0.22), (0.24, 0.22), 0.08, "wood_dark")
    if m.detail < 2:
        for z in (0.40, 0.80):
            m.box((0, -0.235, z), (0.24, 0.012, 0.025), "wood_dark")
        m.loft([[(0.20, -0.17, 1.05), (0.20, 0.17, 1.05), (0.20, 0.17, 1.10), (0.20, -0.17, 1.10)],
                [(0.48, -0.17, 0.80), (0.48, 0.17, 0.80), (0.48, 0.17, 0.84), (0.48, -0.17, 0.84)]], "wood_dark")
    for x in (-0.18, 0.18):
        for s in (-1, 1):
            m.wheel((x, s * 0.26, 0.10), 0.10, 0.04, "wood_dark", axis="y")
    m.rod((-0.08, 0, 1.23), (-0.08, 0, 1.55), 0.012, 0.012, "wood", sides=3)
    with m.at(move(-0.08, 0, 1.52)):
        m.prism([(0.0, 0.0), (0.22, -0.02), (0.20, -0.14), (0.0, -0.16)], 0.012, "team")
    crew(m, crew_look, (0.0, -0.45, 0.0), facing=90)


def ram(m, crew_look):
    for x in (-0.25, 0.25):
        for s in (-1, 1):
            m.wheel((x, s * 0.22, 0.12), 0.12, 0.04, "wood_dark", axis="y")
    _frame(m, 0.34, 0.18, 0.16)
    m.loft([[(-0.40, -0.24, 0.26), (-0.40, 0.24, 0.26), (-0.40, 0.0, 0.62)],
            [(0.34, -0.24, 0.26), (0.34, 0.24, 0.26), (0.34, 0.0, 0.62)]], "fur")
    m.rod((-0.30, 0, 0.30), (0.62, 0, 0.30), 0.06, 0.06, "wood_dark", sides=m.pick(6, 4, 3))
    m.rod((0.62, 0, 0.30), (0.72, 0, 0.30), 0.075, 0.03, "iron", sides=m.pick(6, 4, 3))
    if m.detail < 2:
        m.box((0.0, 0, 0.64), (0.30, 0.02, 0.02), "team")
    crew(m, crew_look, (-0.45, -0.32, 0.0), facing=90)


def hwacha(m, crew_look):
    m.box((-0.05, 0, 0.26), (0.30, 0.14, 0.03), "wood")
    for s in (-1, 1):
        m.wheel((-0.05, s * 0.20, 0.20), 0.20, 0.04, "wood_dark", hub="iron", axis="y")
    m.rod((0.25, 0, 0.26), (0.50, 0, 0.18), 0.02, 0.02, "wood", sides=3)
    with m.at(move(-0.05, 0, 0.42), rot_y(-25)):
        m.box((0, 0, 0), (0.12, 0.17, 0.11), "red")
        if m.detail < 2:
            m.box((0.125, 0, 0), (0.01, 0.15, 0.09), "dark")
    crew(m, crew_look, (-0.40, -0.30, 0.0), facing=90)


def cannon(m, crew_look):
    for s in (-1, 1):
        m.wheel((0.0, s * 0.19, 0.22), 0.22, 0.045, "wood_dark", hub="iron", axis="y")
    m.loft([[(-0.48, -0.05, 0.03), (-0.48, 0.05, 0.03), (-0.48, 0.05, 0.08), (-0.48, -0.05, 0.08)],
            [(0.06, -0.13, 0.20), (0.06, 0.13, 0.20), (0.06, 0.13, 0.30), (0.06, -0.13, 0.30)]], "wood")
    m.rod((-0.14, 0, 0.34), (0.48, 0, 0.40), 0.075, 0.05, "bronze", sides=m.pick(8, 5, 4))
    if m.detail < 2:
        m.rod((0.46, 0, 0.40), (0.50, 0, 0.40), 0.065, 0.065, "bronze", sides=m.pick(8, 5, 4))
    crew(m, crew_look, (-0.30, -0.36, 0.0), facing=90)
    crew(m, crew_look, (0.30, 0.34, 0.0), facing=120)


def field_gun(m, crew_look, barrel=0.80, shield=True, color="olive"):
    for s in (-1, 1):
        m.wheel((0.0, s * 0.21, 0.18), 0.18, 0.06, "dark", hub=color, axis="y")
        m.rod((0.0, s * 0.08, 0.16), (-0.62, s * 0.24, 0.02), 0.025, 0.02, color, sides=m.pick(4, 3, 3))
    m.box((0.02, 0, 0.30), (0.12, 0.08, 0.06), color)
    m.rod((-0.10, 0, 0.32), (barrel, 0, 0.40), 0.05, 0.03, color, sides=m.pick(6, 4, 3))
    if shield:
        m.loft([[(0.12, -0.24, 0.20), (0.12, 0.24, 0.20), (0.12, 0.22, 0.58), (0.12, -0.22, 0.58)],
                [(0.14, -0.24, 0.20), (0.14, 0.24, 0.20), (0.14, 0.22, 0.58), (0.14, -0.22, 0.58)]], color)
    crew(m, crew_look, (-0.40, -0.36, 0.0), facing=90)
    crew(m, crew_look, (-0.25, 0.38, 0.0), facing=70)


def aa_gun(m, crew_look):
    for a in (0, 90):
        with m.at(rot_z(a + 45)):
            m.box((0, 0, 0.03), (0.42, 0.04, 0.03), "olive")
    m.rings([((0, 0, 0.05), 0.15, 0.15), ((0, 0, 0.28), 0.12, 0.12)], "olive", sides=m.pick(8, 5, 4))
    m.box((0.0, 0, 0.36), (0.10, 0.10, 0.08), "olive")
    m.rod((0.0, 0, 0.38), (0.48, 0, 1.00), 0.04, 0.025, "dark", sides=m.pick(6, 4, 3))
    crew(m, crew_look, (-0.30, -0.35, 0.0), facing=100)


def gatling(m, crew_look):
    for s in (-1, 1):
        m.wheel((-0.05, s * 0.18, 0.18), 0.18, 0.04, "wood_dark", hub="iron", axis="y")
    m.loft([[(-0.50, -0.04, 0.03), (-0.50, 0.04, 0.03), (-0.50, 0.04, 0.07), (-0.50, -0.04, 0.07)],
            [(0.0, -0.10, 0.18), (0.0, 0.10, 0.18), (0.0, 0.10, 0.26), (0.0, -0.10, 0.26)]], "wood")
    m.rod((-0.15, 0, 0.32), (0.40, 0, 0.36), 0.065, 0.06, "bronze", sides=m.pick(8, 5, 4))
    if m.detail < 2:
        m.box((-0.05, 0, 0.42), (0.05, 0.03, 0.05), "iron")
    crew(m, crew_look, (-0.40, -0.30, 0.0), facing=90)


def mg_nest(m, crew_look):
    """Machine gun on a tripod with a kneeling gunner (crew figure)."""
    for a in (0, 120, 240):
        with m.at(rot_z(a)):
            m.rod((0.0, 0, 0.28), (0.22, 0, 0.0), 0.012, 0.012, "dark", sides=3)
    m.box((0.05, 0, 0.32), (0.16, 0.05, 0.06), "dark")
    m.rod((0.20, 0, 0.33), (0.58, 0, 0.35), 0.022, 0.018, "dark", sides=m.pick(5, 4, 3))
    if m.detail < 2:
        m.box((0.10, 0.10, 0.06), (0.12, 0.08, 0.06), "olive")
    crew(m, crew_look, (-0.35, 0.0, 0.0), facing=90)


def rocket_truck(m, crew_look):
    truck(m, "olive")
    with m.at(move(-0.20, 0, 0.48), rot_y(-28)):
        m.box((0, 0, 0), (0.30, 0.17, 0.08), "olive")
        if m.detail < 2:
            m.box((0.305, 0, 0), (0.005, 0.15, 0.06), "dark")
    crew(m, crew_look, (0.15, -0.40, 0.0), facing=90)


def truck(m, color):
    m.box((0.30, 0, 0.32), (0.16, 0.18, 0.15), color)
    if m.detail < 2:
        m.box((0.465, 0, 0.36), (0.005, 0.14, 0.06), "glass")
    m.box((-0.15, 0, 0.24), (0.32, 0.19, 0.06), color)
    for x in (0.30, -0.05, -0.30):
        for s in (-1, 1):
            m.wheel((x, s * 0.19, 0.12), 0.12, 0.07, "dark", axis="y")


# Armored vehicles -------------------------------------------------------------------
def tracks(m, length, width, height, color="dark"):
    for s in (-1, 1):
        m.rings([((-length, s * width, height * 0.5), 0.0, 0.0),
                 ((-length + 0.02, s * width, height * 0.5), 0.06, height * 0.5),
                 ((length - 0.02, s * width, height * 0.5), 0.06, height * 0.5),
                 ((length, s * width, height * 0.5), 0.0, 0.0)], color, axis="x", sides=m.pick(8, 6, 4))


def landship(m, color="olive"):
    """WWI rhomboid tank with side sponsons."""
    for s in (-1, 1):
        with m.at(move(0, s * 0.20, 0)):
            m.prism([(-0.55, 0.10), (-0.38, 0.0), (0.42, 0.0), (0.62, 0.42), (0.40, 0.50), (-0.45, 0.42)], 0.10,
                    color)
            m.box((0.0, s * 0.08, 0.28), (0.12, 0.05, 0.08), color)
            if m.detail < 2:
                m.rod((0.05, s * 0.14, 0.28), (0.30, s * 0.14, 0.28), 0.02, 0.02, "dark", sides=3)
    m.box((-0.02, 0, 0.30), (0.42, 0.16, 0.16), color)
    if m.detail < 2:
        m.box((0.10, 0, 0.48), (0.10, 0.08, 0.04), color)


def tank(m, color="olive", turret="round", barrel=0.55, length=0.55, width=0.22, height=0.20, star=True):
    tracks(m, length, width, height)
    m.taper((0, 0, height * 0.55), (length - 0.04, width - 0.04), (length - 0.14, width - 0.06), height * 0.75,
            color)
    tz = height * 1.3
    if turret == "round":
        m.rings([((-0.04, 0, tz), 0.20, 0.17), ((-0.04, 0, tz + 0.14), 0.15, 0.13)], color, sides=m.pick(8, 6, 4))
    else:
        m.loft([[(-0.24, -0.17, tz), (-0.24, 0.17, tz), (0.18, 0.17, tz), (0.30, 0.0, tz), (0.18, -0.17, tz)],
                [(-0.22, -0.14, tz + 0.12), (-0.22, 0.14, tz + 0.12), (0.14, 0.14, tz + 0.12), (0.22, 0.0, tz + 0.12),
                 (0.14, -0.14, tz + 0.12)]], color)
    m.rod((0.10, 0, tz + 0.07), (0.10 + barrel, 0, tz + 0.08), 0.03, 0.022, "dark", sides=m.pick(6, 4, 3))
    if star and m.detail < 2:
        m.box((-0.04, 0, tz + 0.15), (0.06, 0.06, 0.012), "team")
    if m.detail < 2:
        for s in (-1, 1):
            m.box((-0.05, s * (width + 0.005), height * 0.98), (length * 0.6, 0.012, 0.02), "team")


def mech_infantry(m, color="olive"):
    tracks(m, 0.48, 0.20, 0.16)
    m.taper((0, 0, 0.10), (0.46, 0.20), (0.40, 0.18), 0.24, color, shift=(-0.02, 0))
    m.box((0.05, 0, 0.38), (0.10, 0.08, 0.05), color)
    m.rod((0.10, 0, 0.39), (0.45, 0, 0.40), 0.012, 0.012, "dark", sides=3)
    if m.detail < 2:
        for s in (-1, 1):
            m.box((-0.10, s * 0.205, 0.28), (0.30, 0.012, 0.03), "team")


def mobile_sam(m, color="olive"):
    tracks(m, 0.48, 0.20, 0.16)
    m.taper((0, 0, 0.10), (0.46, 0.20), (0.42, 0.18), 0.18, color)
    with m.at(move(-0.08, 0, 0.34), rot_y(-30)):
        for s in (-1, 1):
            m.rod((-0.25, s * 0.08, 0.06), (0.25, s * 0.08, 0.06), 0.045, 0.045, "white", sides=m.pick(6, 4, 3))
            m.rod((0.25, s * 0.08, 0.06), (0.36, s * 0.08, 0.06), 0.045, 0.0, "team", sides=m.pick(6, 4, 3))
        m.box((0, 0, 0.0), (0.22, 0.14, 0.02), color)


def robot(m):
    """Giant Death Robot: armored biped with shoulder cannons."""
    for s in (-1, 1):
        m.rod((0.0, s * 0.18, 0.62), (0.08, s * 0.20, 0.34), 0.08, 0.07, "steel", sides=m.pick(6, 4, 4))
        m.rod((0.08, s * 0.20, 0.34), (0.0, s * 0.22, 0.05), 0.07, 0.06, "dark", sides=m.pick(6, 4, 4))
        m.box((0.06, s * 0.22, 0.04), (0.14, 0.08, 0.04), "steel")
        m.rod((0.0, s * 0.30, 1.02), (0.30, s * 0.34, 0.80), 0.06, 0.05, "steel", sides=m.pick(6, 4, 4))
        m.rod((0.30, s * 0.34, 0.80), (0.62, s * 0.34, 0.84), 0.07, 0.06, "dark", sides=m.pick(6, 4, 4))
    m.taper((0, 0, 0.62), (0.18, 0.24), (0.22, 0.30), 0.42, "steel")
    m.box((0.08, 0, 1.08), (0.10, 0.12, 0.06), "dark")
    if m.detail < 2:
        m.box((0.185, 0, 1.08), (0.01, 0.08, 0.02), "fire")
        m.box((0.0, 0, 0.84), (0.23, 0.31, 0.04), "team")
        for s in (-1, 1):
            m.rod((-0.05, s * 0.18, 1.06), (0.40, s * 0.18, 1.12), 0.04, 0.03, "dark", sides=4)


# Aircraft -------------------------------------------------------------------------------
def _gear(m, x, y, z):
    if m.detail < 2:
        m.rod((x, y, z), (x, y, 0.04), 0.012, 0.012, "dark", sides=3)
        m.wheel((x, y, 0.04), 0.04, 0.025, "black", axis="y")


def triplane(m):
    m.rings([((-0.55, 0, 0.36), 0.0, 0.0), ((-0.45, 0, 0.36), 0.03, 0.04), ((0.20, 0, 0.30), 0.07, 0.08),
             ((0.30, 0, 0.30), 0.06, 0.07)], "red", axis="x", sides=m.pick(6, 4, 4))
    for z in (0.16, 0.32, 0.48):
        m.box((0.10, 0, z), (0.09, 0.42, 0.012), "team" if z == 0.48 else "red")
    if m.detail < 2:
        for s in (-1, 1):
            m.rod((0.10, s * 0.30, 0.16), (0.10, s * 0.30, 0.48), 0.008, 0.008, "wood", sides=3)
    m.box((-0.50, 0, 0.38), (0.05, 0.15, 0.01), "red")
    m.box((-0.52, 0, 0.44), (0.04, 0.008, 0.07), "team")
    m.box((0.32, 0, 0.30), (0.01, 0.16, 0.02), "wood_dark")
    _gear(m, 0.12, -0.10, 0.22)
    _gear(m, 0.12, 0.10, 0.22)


def biplane_bomber(m):
    m.rings([((-0.75, 0, 0.40), 0.0, 0.0), ((-0.6, 0, 0.40), 0.04, 0.05), ((0.30, 0, 0.36), 0.09, 0.10),
             ((0.42, 0, 0.36), 0.06, 0.07)], "khaki", axis="x", sides=m.pick(6, 4, 4))
    for z in (0.24, 0.52):
        m.box((0.10, 0, z), (0.13, 0.70, 0.014), "team" if z > 0.3 else "khaki")
    for s in (-1, 1):
        m.rings([((0.0, s * 0.25, 0.30), 0.05, 0.05), ((0.24, s * 0.25, 0.30), 0.06, 0.06)], "dark", axis="x",
                sides=m.pick(6, 4, 4))
        if m.detail < 2:
            m.rod((0.10, s * 0.50, 0.24), (0.10, s * 0.50, 0.52), 0.008, 0.008, "wood", sides=3)
    m.box((-0.70, 0, 0.42), (0.07, 0.22, 0.012), "khaki")
    m.box((-0.72, 0, 0.50), (0.05, 0.01, 0.09), "team")
    _gear(m, 0.10, -0.14, 0.26)
    _gear(m, 0.10, 0.14, 0.26)


def prop_fighter(m, body="field_grey", wing="team"):
    m.rings([((-0.50, 0, 0.30), 0.0, 0.0), ((-0.40, 0, 0.30), 0.035, 0.045), ((0.15, 0, 0.27), 0.075, 0.08),
             ((0.32, 0, 0.26), 0.06, 0.06)], body, axis="x", sides=m.pick(8, 5, 4))
    m.loft([[(0.20, -0.62, 0.22), (0.20, 0.62, 0.22), (0.02, 0.62, 0.22), (0.02, -0.62, 0.22)],
            [(0.20, -0.62, 0.245), (0.20, 0.62, 0.245), (0.02, 0.62, 0.245), (0.02, -0.62, 0.245)]], wing)
    m.box((-0.44, 0, 0.31), (0.05, 0.16, 0.01), body)
    m.box((-0.46, 0, 0.38), (0.04, 0.008, 0.07), body)
    if m.detail < 2:
        m.ball((0.06, 0, 0.34), (0.08, 0.04, 0.04), "glass", u=5, v=3)
        m.box((0.335, 0, 0.26), (0.008, 0.012, 0.13), "dark")
    _gear(m, 0.12, -0.12, 0.20)
    _gear(m, 0.12, 0.12, 0.20)


def heavy_bomber(m, body="hull_grey"):
    m.rings([((-0.85, 0, 0.40), 0.0, 0.0), ((-0.70, 0, 0.40), 0.05, 0.06), ((0.30, 0, 0.36), 0.10, 0.11),
             ((0.52, 0, 0.34), 0.07, 0.07), ((0.58, 0, 0.34), 0.0, 0.0)], body, axis="x", sides=m.pick(8, 6, 4))
    m.loft([[(0.24, -0.95, 0.30), (0.24, 0.95, 0.30), (0.02, 0.95, 0.30), (0.02, -0.95, 0.30)],
            [(0.24, -0.95, 0.33), (0.24, 0.95, 0.33), (0.02, 0.95, 0.33), (0.02, -0.95, 0.33)]], body)
    for y in (-0.62, -0.30, 0.30, 0.62):
        m.rings([((0.10, y, 0.28), 0.04, 0.04), ((0.32, y, 0.28), 0.045, 0.045)], "dark", axis="x",
                sides=m.pick(6, 4, 3))
    m.box((-0.78, 0, 0.42), (0.07, 0.30, 0.012), body)
    m.box((-0.80, 0, 0.55), (0.07, 0.01, 0.14), body)
    if m.detail < 2:
        for s in (-1, 1):
            m.box((0.13, s * 0.55, 0.335), (0.07, 0.07, 0.005), "team")
        m.box((-0.80, 0, 0.62), (0.05, 0.012, 0.05), "team")
    _gear(m, 0.12, -0.30, 0.28)
    _gear(m, 0.12, 0.30, 0.28)


def jet(m, body="hull_grey"):
    m.rings([((-0.60, 0, 0.30), 0.04, 0.05), ((0.10, 0, 0.30), 0.08, 0.07), ((0.40, 0, 0.30), 0.04, 0.04),
             ((0.58, 0, 0.30), 0.0, 0.0)], body, axis="x", sides=m.pick(8, 5, 4))
    with m.at(move(0, 0, 0.28)):
        m.prism([(0.20, -0.08), (0.20, 0.08), (-0.25, 0.55), (-0.42, 0.55), (-0.42, -0.55), (-0.25, -0.55)],
                0.025, body, axis="z")
    for s in (-1, 1):
        with m.at(move(-0.50, s * 0.08, 0.36), rot_x(s * 15)):
            m.prism([(-0.08, -0.02), (0.08, -0.02), (-0.04, 0.16), (-0.10, 0.16)], 0.012, body)
    m.loft([[(-0.48, -0.25, 0.30), (-0.48, 0.25, 0.30), (-0.60, 0.25, 0.30), (-0.60, -0.25, 0.30)],
            [(-0.48, -0.25, 0.31), (-0.48, 0.25, 0.31), (-0.60, 0.25, 0.31), (-0.60, -0.25, 0.31)]], body)
    if m.detail < 2:
        m.ball((0.24, 0, 0.36), (0.10, 0.04, 0.04), "glass", u=5, v=3)
        for s in (-1, 1):
            m.box((-0.30, s * 0.38, 0.285), (0.06, 0.06, 0.006), "team")
    _gear(m, 0.20, 0.0, 0.24)
    _gear(m, -0.25, -0.12, 0.24)
    _gear(m, -0.25, 0.12, 0.24)


def stealth(m):
    # Flying wing: the outline is concave, so each half is its own convex slab.
    for s in (-1, 1):
        half = [(0.45, 0.0), (-0.10, s * 0.95), (-0.30, s * 0.80), (-0.20, s * 0.40), (-0.25, 0.0)]
        m.loft([[(x, y, 0.30) for x, y in half], [(x, y, 0.35) for x, y in half]], "dark")
    m.loft([[(0.30, -0.12, 0.34), (0.30, 0.12, 0.34), (-0.15, 0.20, 0.34), (-0.15, -0.20, 0.34)],
            [(0.20, -0.08, 0.42), (0.20, 0.08, 0.42), (-0.05, 0.12, 0.40), (-0.05, -0.12, 0.40)]], "dark")
    if m.detail < 2:
        m.box((0.24, 0, 0.42), (0.05, 0.05, 0.008), "team")
    _gear(m, 0.20, 0.0, 0.30)
    _gear(m, -0.10, -0.20, 0.30)
    _gear(m, -0.10, 0.20, 0.30)


def helicopter(m, body="olive"):
    """Hovering gunship: contact 'hover', the lowest point is above the ground."""
    m.rings([((-0.15, 0, 0.42), 0.08, 0.10), ((0.12, 0, 0.42), 0.12, 0.14), ((0.30, 0, 0.42), 0.0, 0.0)], body,
            axis="x", sides=m.pick(8, 6, 4))
    m.rod((-0.15, 0, 0.46), (-0.72, 0, 0.52), 0.05, 0.025, body, sides=m.pick(5, 4, 3))
    m.box((-0.72, 0, 0.60), (0.04, 0.008, 0.08), body)
    if m.detail < 2:
        m.ball((0.20, 0, 0.46), (0.09, 0.07, 0.07), "glass", u=6, v=3)
        for s in (-1, 1):
            m.box((0.02, s * 0.18, 0.40), (0.07, 0.06, 0.02), body)
            m.rod((-0.03, s * 0.24, 0.38), (0.20, s * 0.24, 0.38), 0.025, 0.025, "dark", sides=4)
            m.box((0.0, s * 0.10, 0.24), (0.22, 0.012, 0.012), "dark")
    m.rod((0.0, 0, 0.56), (0.0, 0, 0.62), 0.03, 0.03, "dark", sides=4)
    for a in (20, 110):
        with m.at(move(0, 0, 0.63), rot_z(a)):
            m.box((0, 0, 0), (0.62, 0.025, 0.006), "dark")
    m.box((-0.10, 0, 0.50), (0.10, 0.10, 0.012), "team")


def missile(m, length=0.9, radius=0.07, body="white", tip="team", stand=True):
    """Missile raised 35 degrees on its launcher; the tail clears the ground."""
    lift = 0.10 + length * 0.5 * 0.574 + radius * 1.3
    with m.at(move(0, 0, lift), rot_y(-35)):
        m.rings([((-length / 2, 0, 0), radius, radius), ((length * 0.32, 0, 0), radius, radius),
                 ((length / 2, 0, 0), 0.0, 0.0)], body, axis="x", sides=m.pick(8, 6, 4))
        m.rings([((length * 0.30, 0, 0), radius * 1.02, radius * 1.02), ((length / 2 - 0.005, 0, 0), 0.0, 0.0)],
                tip, axis="x", sides=m.pick(8, 6, 4))
        if m.detail < 2:
            for a in (0, 90):
                with m.at(rot_x(a)):
                    m.box((-length / 2 + 0.06, 0, 0), (0.06, 0.008, radius * 2.4), "dark")
    if stand:
        m.box((-0.05, 0, 0.06), (0.30, 0.16, 0.06), "olive")
        m.rod((0.0, 0, 0.10), (0.0, 0, lift - radius), 0.03, 0.03, "dark", sides=4)


def bomb(m):
    m.box((0, 0, 0.08), (0.36, 0.18, 0.03), "olive")
    for x in (-0.25, 0.25):
        for s in (-1, 1):
            m.wheel((x, s * 0.20, 0.06), 0.06, 0.03, "dark", axis="y")
    m.ball((0.0, 0, 0.30), (0.34, 0.20, 0.20), "khaki", u=m.pick(10, 6, 5), v=m.pick(6, 4, 3))
    m.box((-0.38, 0, 0.30), (0.05, 0.14, 0.14), "team")


# Spaceship parts on a transport sled -------------------------------------------------------
def sled(m):
    m.box((0, 0, 0.08), (0.50, 0.22, 0.04), "steel")
    for x in (-0.36, 0.0, 0.36):
        for s in (-1, 1):
            m.wheel((x, s * 0.23, 0.06), 0.06, 0.04, "dark", axis="y")


def ss_booster(m):
    sled(m)
    m.rings([((-0.46, 0, 0.28), 0.14, 0.14), ((0.30, 0, 0.28), 0.14, 0.14), ((0.50, 0, 0.28), 0.0, 0.0)], "white",
            axis="x", sides=m.pick(10, 6, 5))
    m.rings([((-0.48, 0, 0.28), 0.15, 0.15), ((-0.40, 0, 0.28), 0.15, 0.15)], "team", axis="x", sides=m.pick(10, 6, 5))


def ss_cockpit(m):
    sled(m)
    m.rings([((-0.30, 0, 0.30), 0.18, 0.18), ((0.10, 0, 0.30), 0.18, 0.18), ((0.42, 0, 0.30), 0.0, 0.0)], "white",
            axis="x", sides=m.pick(10, 6, 5))
    if m.detail < 2:
        m.box((0.18, 0, 0.42), (0.06, 0.08, 0.03), "glass")
    m.box((-0.10, 0, 0.485), (0.08, 0.08, 0.008), "team")


def ss_engine(m):
    sled(m)
    m.rings([((0.40, 0, 0.30), 0.12, 0.12), ((-0.05, 0, 0.30), 0.16, 0.16)], "steel", axis="x",
            sides=m.pick(10, 6, 5))
    m.rings([((-0.05, 0, 0.30), 0.08, 0.08), ((-0.45, 0, 0.30), 0.21, 0.21)], "dark", axis="x",
            sides=m.pick(10, 6, 5))
    m.box((0.20, 0, 0.45), (0.10, 0.06, 0.02), "team")


def ss_stasis(m):
    sled(m)
    m.rings([((-0.40, 0, 0.30), 0.0, 0.0), ((-0.36, 0, 0.30), 0.17, 0.17), ((0.36, 0, 0.30), 0.17, 0.17),
             ((0.40, 0, 0.30), 0.0, 0.0)], "white", axis="x", sides=m.pick(10, 6, 5))
    m.box((0.0, -0.17, 0.32), (0.22, 0.012, 0.07), "glass")
    m.box((0.0, 0, 0.475), (0.22, 0.06, 0.006), "team")
