"""Civilian and great-person models: settler wagon, workers, caravan, religious units,
great people with a signature prop. Single models; people face the camera (-y).
"""
from geom import move, rot_z, rot_y
import mounts
import people
from people import Look

SETTLER = Look(body="cloth", legs="cloth_dark", head="wide", head_mat="wood", cape="team", right="staff")
WORKER = Look(body="team", legs="cloth_dark", head="cap", head_mat="cloth", right="pick", belt="leather")
WORKER_2 = Look(body="cloth", legs="cloth_dark", head="bare", right="shovel", skirt="", back="basket")


def person(m, look, at=(0, 0, 0), facing=20, detail=None):
    saved = m.detail
    if detail is not None:
        m.detail = max(m.detail, detail)
    with m.at(move(*at), rot_z(facing)):
        people.figure(m, look)
    m.detail = saved


def wagon(m, cover="linen", cloth="team"):
    """Covered wagon pulled by an ox, along +x."""
    m.box((-0.15, 0, 0.30), (0.32, 0.20, 0.05), "wood")
    for x in (-0.38, 0.08):
        for s in (-1, 1):
            m.wheel((x, s * 0.23, 0.18), 0.18, 0.04, "wood_dark", hub="iron", axis="y")
    profile = [(-0.20, 0.35), (-0.20, 0.56), (-0.12, 0.68), (0.0, 0.72), (0.12, 0.68), (0.20, 0.56), (0.20, 0.35)]
    m.loft([[(-0.44, y, z) for y, z in profile], [(0.14, y, z) for y, z in profile]], cover)
    if m.detail < 2:
        m.box((-0.15, -0.205, 0.40), (0.25, 0.01, 0.04), cloth)
        m.rod((0.15, 0, 0.30), (0.42, 0, 0.30), 0.015, 0.015, "wood", sides=3)
    # Ox: a horse body made stockier and darker reads as a draught animal.
    with m.at(move(0.62, 0, 0), rot_z(0)):
        m.rings([((-0.30, 0, 0.42), 0.0, 0.0), ((-0.26, 0, 0.42), 0.12, 0.13), ((0.10, 0, 0.44), 0.14, 0.15),
                 ((0.20, 0, 0.44), 0.0, 0.0)], "horse_light", axis="x", sides=m.pick(7, 5, 4))
        m.rod((0.16, 0, 0.46), (0.32, 0, 0.40), 0.07, 0.05, "horse_light", sides=m.pick(5, 4, 3))
        if m.detail < 2:
            for s in (-1, 1):
                m.rod((0.24, s * 0.04, 0.48), (0.26, s * 0.14, 0.56), 0.015, 0.0, "ivory", sides=3)
        for x in (0.08, -0.20):
            for s in (-1, 1):
                m.rod((x, s * 0.07, 0.36), (x + 0.02 * s, s * 0.07, 0.0), 0.035, 0.025, "horse_light",
                      sides=m.pick(4, 4, 3))


def settler(m):
    with m.at(rot_z(-30)):
        wagon(m)
    person(m, SETTLER, (0.08, -0.42, 0), 20, detail=1)
    person(m, people.with_(SETTLER, body="team", cape="", head="cap", right="", head_mat="cloth"),
           (-0.48, -0.15, 0), 15, detail=1)


def worker(m):
    person(m, WORKER, (-0.18, 0.06, 0), 25)
    person(m, WORKER_2, (0.22, -0.10, 0), 10, detail=1)


def caravan(m):
    with m.at(rot_z(-30)):
        mounts.camel(m)
        if m.detail < 2:
            for s in (-1, 1):
                m.box((-0.10, s * 0.16, 0.86), (0.10, 0.05, 0.09), "rope")
    person(m, Look(body="team", legs="cloth", head="turban", head_mat="linen", right="staff", robe=True),
           (0.12, -0.40, 0), 15, detail=1)


def missionary(m):
    person(m, Look(body="cloth_dark", skirt="cloth_dark", robe=True, head="hood", head_mat="cloth_dark",
                   right="cross", left="book", cape="team"), (0, 0, 0), 20)


def inquisitor(m):
    person(m, Look(body="red", skirt="red", robe=True, head="mitre", head_mat="red", right="staff", left="book",
                   cape="team"), (0, 0, 0), 20)


def prophet(m):
    person(m, Look(body="linen", skirt="linen", robe=True, head="bare", right="staff", cape="team",
                   extra=("beard",)), (0, 0, 0), 20)
    if m.detail < 2:
        m.rod((0.0, 0.0, 1.08), (0.0, 0.0, 1.10), 0.11, 0.11, "gold", sides=m.pick(10, 6, 5))


def great_person(m, look, prop=None):
    person(m, look, (0.0, 0.0, 0.0), 20)
    if prop and m.detail < 2:
        prop(m)


def easel(m):
    with m.at(move(0.34, 0.10, 0), rot_z(-25)):
        for s in (-1, 1):
            m.rod((s * 0.12, 0.05, 0.0), (s * 0.04, 0.0, 0.85), 0.012, 0.012, "wood", sides=3)
        m.rod((0.0, 0.10, 0.0), (0.0, 0.0, 0.80), 0.012, 0.012, "wood", sides=3)
        m.box((0.0, -0.01, 0.58), (0.16, 0.012, 0.13), "paper")
        if m.detail == 0:
            m.box((0.0, -0.025, 0.60), (0.08, 0.004, 0.06), "team")


def globe(m):
    with m.at(move(0.32, 0.08, 0)):
        m.rod((0, 0, 0.0), (0, 0, 0.45), 0.02, 0.02, "wood", sides=3)
        m.box((0, 0, 0.02), (0.08, 0.08, 0.02), "wood")
        m.ball((0, 0, 0.56), 0.11, "glass", u=m.pick(8, 6, 4), v=m.pick(5, 4, 3))
        m.rod((-0.12, 0, 0.56), (0.12, 0, 0.56), 0.01, 0.01, "gold", sides=3)


def chest(m):
    with m.at(move(0.30, 0.0, 0), rot_z(10)):
        m.box((0, 0, 0.09), (0.12, 0.08, 0.09), "wood")
        m.box((0, 0, 0.19), (0.125, 0.085, 0.02), "gold")


def gearwheel(m):
    with m.at(move(0.32, 0.06, 0.0)):
        m.box((0, 0, 0.06), (0.10, 0.10, 0.06), "wood")
        m.rod((0, -0.02, 0.32), (0, 0.02, 0.32), 0.18, 0.18, "iron", sides=m.pick(10, 6, 5))
        if m.detail == 0:
            m.rod((0, -0.03, 0.32), (0, 0.03, 0.32), 0.05, 0.05, "bronze", sides=6)


def lectern(m):
    with m.at(move(0.30, 0.05, 0)):
        m.taper((0, 0, 0), (0.08, 0.08), (0.05, 0.05), 0.5, "wood")
        with m.at(move(0, 0, 0.56), rot_y(0)):
            m.box((0, 0, 0), (0.12, 0.09, 0.03), "wood_dark")
            m.box((0, -0.02, 0.035), (0.10, 0.06, 0.008), "paper")


def harp(m):
    with m.at(move(0.30, 0.0, 0)):
        m.rod((0, 0, 0.0), (0, 0, 0.62), 0.02, 0.02, "gold", sides=3)
        m.rod((0, 0, 0.62), (0.22, 0, 0.42), 0.02, 0.02, "gold", sides=3)
        m.rod((0.22, 0, 0.42), (0.05, 0, 0.02), 0.025, 0.02, "wood", sides=3)


def dig_site(m):
    with m.at(move(0.30, 0.0, 0)):
        m.box((0, 0, 0.08), (0.12, 0.09, 0.08), "wood")
        m.ball((0.0, 0.0, 0.2), (0.08, 0.06, 0.06), "stone", u=5, v=3)
        m.rod((0.14, 0.05, 0.0), (0.18, 0.08, 0.45), 0.012, 0.012, "wood", sides=3)


GREAT = {
    "artist": (Look(body="purple", legs="cloth_dark", head="beret", head_mat="red", right="brush", cape="team"),
               easel),
    "scientist": (Look(body="white", skirt="white", robe=True, head="bare", right="book", cape="team",
                       extra=("beard",)), globe),
    "merchant": (Look(body="team", legs="cloth", head="turban", head_mat="gold", right="coin", robe=True,
                      skirt="red"), chest),
    "engineer": (Look(body="leather", legs="cloth_dark", head="cap", head_mat="leather", right="hammer",
                      cape="team"), gearwheel),
    "writer": (Look(body="cloth_dark", skirt="cloth_dark", robe=True, head="laurel", right="scroll", cape="team"),
               lectern),
    "musician": (Look(body="team", legs="cloth_dark", head="bare", right="lute", cape="red"), harp),
    "archaeologist": (Look(body="team", legs="khaki", head="wide", head_mat="khaki", right="shovel",
                           back="pack", belt="leather"), dig_site),
    "merchant-of-venice": (Look(body="red", skirt="red", robe=True, head="cap", head_mat="red", right="coin",
                                cape="gold"), chest),
}


def great_general(m, khan=False):
    rider = Look(body="team", armor="steel" if not khan else "", legs="cloth_dark",
                 head="dome" if not khan else "pointed", head_mat="steel" if not khan else "red",
                 plume="red" if not khan else "", right="banner", cape="red" if not khan else "fur")
    with m.at(rot_z(-35)):
        mounts.ride(m, "horse", rider, coat="horse_light" if not khan else "horse_dark", cloth="team")
