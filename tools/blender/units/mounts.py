"""Mounts and riders: horse, elephant, camel, chariot. Built along +x (forward), feet on z = 0.

`ride(m, mount, rider_look, ...)` places a seated figure on the saddle facing +x.
"""
from geom import move, rot_z, rot_y
import people

# Saddle height (top of the back) per mount, used to seat riders.
SADDLE = {"horse": 0.74, "barded": 0.76, "elephant": 1.05, "camel": 0.98}


def _leg(m, hip, foot, r_top, r_bottom, mat, hoof="horse_dark", knee_dx=0.0):
    if m.detail == 0:
        knee = ((hip[0] + foot[0]) / 2 + knee_dx, hip[1], (hip[2] + foot[2]) / 2 + 0.02)
        m.rod(hip, knee, r_top, r_top * 0.75, mat, sides=5)
        m.rod(knee, foot, r_top * 0.7, r_bottom, mat, sides=5)
        m.box((foot[0] + 0.01, foot[1], 0.025), (r_bottom * 1.2, r_bottom * 1.1, 0.025), hoof)
    else:
        m.rod(hip, (foot[0], foot[1], 0.0), r_top, r_bottom, mat, sides=m.pick(5, 4, 3))


def horse(m, coat="horse", mane="horse_dark", cloth="team", barded=False, armor="iron"):
    """Horse about 0.95 long, back at z ~0.72. Walking stance, head up."""
    m.rings([((-0.40, 0, 0.62), 0.0, 0.0), ((-0.36, 0, 0.62), 0.11, 0.12), ((-0.14, 0, 0.62), 0.135, 0.15),
             ((0.12, 0, 0.64), 0.13, 0.15), ((0.28, 0, 0.68), 0.09, 0.11), ((0.31, 0, 0.70), 0.0, 0.0)],
            coat, axis="x", sides=m.pick(8, 6, 4))
    # Neck and head: the neck rises steeply so the silhouette reads as a horse at 20 px.
    m.rod((0.20, 0, 0.70), (0.40, 0, 0.98), (0.085, 0.07), (0.055, 0.05), coat, sides=m.pick(6, 4, 4))
    m.rod((0.38, 0, 1.00), (0.58, 0, 0.86), (0.06, 0.05), (0.04, 0.035), coat, sides=m.pick(6, 4, 3))
    if m.detail < 2:
        m.loft([[(0.18, -0.015, 0.78), (0.18, 0.015, 0.78), (0.15, 0.0, 0.84)],
                [(0.37, -0.015, 1.02), (0.37, 0.015, 1.02), (0.34, 0.0, 1.08)]], mane)
        m.rod((-0.38, 0, 0.66), (-0.50, 0, 0.36), 0.035, 0.015, mane, sides=m.pick(5, 4, 3))
    if m.detail == 0:
        for s in (-1, 1):
            m.rod((0.40, s * 0.025, 1.02), (0.39, s * 0.03, 1.09), 0.018, 0.0, coat, sides=3)
    # Walking legs: near-front forward, far-front back, diagonal pair for the hind legs.
    legs = [((0.20, -0.07, 0.56), (0.30, -0.07, 0.0)), ((0.20, 0.07, 0.56), (0.12, 0.07, 0.0)),
            ((-0.30, -0.07, 0.56), (-0.36, -0.07, 0.0)), ((-0.30, 0.07, 0.56), (-0.22, 0.07, 0.0))]
    for hip, foot in legs:
        _leg(m, hip, foot, 0.042, 0.022, coat, knee_dx=0.02 if hip[0] > 0 else -0.03)
    if barded:
        m.loft([[(-0.36, -0.155, 0.46), (-0.36, 0.155, 0.46), (-0.36, 0.155, 0.76), (-0.36, -0.155, 0.76)],
                [(0.22, -0.16, 0.48), (0.22, 0.16, 0.48), (0.22, 0.16, 0.78), (0.22, -0.16, 0.78)]], cloth)
        m.rod((0.38, 0, 1.00), (0.55, 0, 0.88), (0.065, 0.055), (0.045, 0.04), armor, sides=m.pick(6, 4, 3))
    else:
        m.loft([[(-0.20, -0.145, 0.56), (-0.20, 0.145, 0.56), (-0.20, 0.0, 0.775)],
                [(0.06, -0.15, 0.56), (0.06, 0.15, 0.56), (0.06, 0.0, 0.79)]], cloth)
        if m.detail == 0:
            m.box((-0.07, 0, 0.775), (0.10, 0.07, 0.02), "leather")


def elephant(m, skin="elephant", cloth="team", tower=True):
    """War elephant about 1.3 long, back at z ~1.0."""
    m.rings([((-0.50, 0, 0.72), 0.0, 0.0), ((-0.44, 0, 0.72), 0.24, 0.26), ((-0.1, 0, 0.74), 0.30, 0.32),
             ((0.24, 0, 0.76), 0.27, 0.30), ((0.36, 0, 0.78), 0.0, 0.0)], skin, axis="x", sides=m.pick(8, 5, 4))
    m.ball((0.44, 0, 0.88), (0.17, 0.17, 0.19), skin, u=m.pick(7, 5, 4), v=m.pick(5, 4, 3))
    m.rod((0.56, 0, 0.84), (0.66, -0.0, 0.50), 0.07, 0.05, skin, sides=m.pick(6, 4, 3))
    m.rod((0.66, 0, 0.50), (0.62, -0.0, 0.18), 0.05, 0.035, skin, sides=m.pick(6, 4, 3))
    for s in (-1, 1):
        with m.at(move(0.40, s * 0.17, 0.92), rot_z(s * 25)):
            m.prism([(-0.12, 0.10), (0.05, 0.12), (0.06, -0.14), (-0.10, -0.10)], 0.025, skin, axis="x")
        if m.detail < 2:
            m.rod((0.55, s * 0.08, 0.76), (0.74, s * 0.10, 0.70), 0.025, 0.0, "ivory", sides=4)
    for x in (0.22, -0.32):
        for s in (-1, 1):
            m.rod((x, s * 0.16, 0.60), (x + (0.04 if s < 0 else -0.04), s * 0.16, 0.0), 0.085, 0.075, skin,
                  sides=m.pick(6, 4, 3))
    m.loft([[(-0.32, -0.33, 0.62), (-0.32, 0.33, 0.62), (-0.32, 0.0, 1.06)],
            [(0.20, -0.31, 0.64), (0.20, 0.31, 0.64), (0.20, 0.0, 1.07)]], cloth)
    if tower:
        m.box((-0.08, 0, 1.13), (0.17, 0.15, 0.09), "wood")
        if m.detail < 2:
            for x in (-0.22, 0.06):
                for s in (-1, 1):
                    m.rod((x, s * 0.13, 1.2), (x, s * 0.13, 1.36), 0.012, 0.012, "wood_dark", sides=3)
            m.box((-0.08, 0, 1.37), (0.17, 0.15, 0.015), "team")


def camel(m, coat="camel", cloth="team"):
    m.rings([((-0.38, 0, 0.80), 0.0, 0.0), ((-0.34, 0, 0.80), 0.10, 0.11), ((-0.08, 0, 0.82), 0.12, 0.14),
             ((0.16, 0, 0.82), 0.11, 0.13), ((0.26, 0, 0.84), 0.0, 0.0)], coat, axis="x", sides=m.pick(8, 6, 4))
    m.ball((-0.08, 0, 0.96), (0.13, 0.09, 0.10), coat, u=m.pick(6, 5, 4), v=m.pick(4, 3, 3))
    m.rod((0.20, 0, 0.82), (0.40, 0, 0.84), 0.06, 0.05, coat, sides=m.pick(5, 4, 3))
    m.rod((0.40, 0, 0.84), (0.44, 0, 1.10), 0.05, 0.04, coat, sides=m.pick(5, 4, 3))
    m.rod((0.42, 0, 1.12), (0.58, 0, 1.08), (0.05, 0.045), (0.03, 0.03), coat, sides=m.pick(5, 4, 3))
    legs = [((0.14, -0.06, 0.74), (0.24, -0.06, 0.0)), ((0.14, 0.06, 0.74), (0.06, 0.06, 0.0)),
            ((-0.30, -0.06, 0.74), (-0.36, -0.06, 0.0)), ((-0.30, 0.06, 0.74), (-0.22, 0.06, 0.0))]
    for hip, foot in legs:
        _leg(m, hip, foot, 0.04, 0.025, coat, hoof=coat)
    m.loft([[(-0.30, -0.13, 0.76), (-0.30, 0.13, 0.76), (-0.30, 0.0, 0.92)],
            [(0.10, -0.13, 0.76), (0.10, 0.13, 0.76), (0.10, 0.0, 0.92)]], cloth)


def ride(m, mount, rider, rider_detail=None, x=-0.06, **mount_args):
    """Mount plus a seated rider facing +x."""
    {"horse": horse, "barded": lambda mm, **a: horse(mm, barded=True, **a), "elephant": elephant,
     "camel": camel}[mount](m, **mount_args)
    seat = SADDLE[mount]
    saved = m.detail
    if rider_detail is not None:
        m.detail = max(m.detail, rider_detail)
    with m.at(move(x, 0, seat - people.HIP_Z + 0.03), rot_z(90)):
        people.figure(m, people.with_(rider, seated=True))
    m.detail = saved


def chariot(m, crew, archer, coat="horse"):
    """Two-wheeled chariot with one horse, a driver and an archer."""
    with m.at(move(0.30, 0, 0)):
        horse(m, coat=coat, cloth="team")
    m.rod((-0.22, 0, 0.36), (0.32, 0, 0.56), 0.016, 0.016, "wood", sides=m.pick(4, 3, 3))
    m.loft([[(-0.52, -0.19, 0.30), (-0.52, 0.19, 0.30), (-0.52, 0.19, 0.36), (-0.52, -0.19, 0.36)],
            [(-0.22, -0.19, 0.30), (-0.22, 0.19, 0.30), (-0.22, 0.19, 0.36), (-0.22, -0.19, 0.36)]], "wood")
    m.loft([[(-0.24, -0.19, 0.36), (-0.24, 0.19, 0.36), (-0.24, 0.17, 0.62), (-0.24, -0.17, 0.62)],
            [(-0.21, -0.19, 0.36), (-0.21, 0.19, 0.36), (-0.21, 0.17, 0.62), (-0.21, -0.17, 0.62)]], "team")
    for s in (-1, 1):
        m.wheel((-0.38, s * 0.23, 0.26), 0.26, 0.04, "wood_dark", hub="bronze", axis="y")
    saved = m.detail
    m.detail = 1 if saved == 0 else 2
    with m.at(move(-0.30, 0.08, 0.34), rot_z(90)):
        people.figure(m, crew)
    if saved < 2:
        with m.at(move(-0.44, -0.08, 0.34), rot_z(90)):
            people.figure(m, archer)
    m.detail = saved
