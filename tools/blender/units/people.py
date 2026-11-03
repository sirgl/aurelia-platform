"""Human figures, headgear, hand-held gear and shields.

A standing figure is 1.0 tall, feet on z = 0, facing -y. `Look` describes the
figure; builders read only the fields they need, so new looks are data, not code.
"""
import math
from dataclasses import dataclass, field, replace

from geom import add, sub, mul, lerp, length, unit, cross, move, rot_x, rot_y, rot_z


@dataclass(frozen=True)
class Look:
    skin: str = "skin"
    body: str = "team"          # tunic / coat
    legs: str = "cloth_dark"
    boots: str = "leather"
    belt: str = "leather"
    armor: str = ""             # chest plate material, "" = none
    shoulders: str = ""         # pauldron material
    skirt: str = ""             # tunic skirt (pteruges, kilt, robe)
    robe: bool = False          # long robe to the ankles instead of legs
    cape: str = ""
    head: str = "bare"          # headgear kind, see HEADGEAR
    head_mat: str = "iron"
    plume: str = ""             # crest / plume material
    right: str = ""             # gear in the right hand, see GEAR
    left: str = ""              # shield or gear in the left hand
    back: str = ""              # quiver, pack, tank, radio
    bulk: float = 1.0           # torso width factor
    seated: bool = False        # rider pose
    extra: tuple = field(default_factory=tuple)  # small accessories: "beard", "bandolier", ...


def with_(look, **changes):
    return replace(look, **changes)


# Joint layout -----------------------------------------------------------------
HIP_Z = 0.47
SHOULDER = (0.135, 0.0, 0.745)
HEAD = (0.0, -0.005, 0.885)
HEAD_R = (0.072, 0.072, 0.082)

# Hand targets per pose: (left, right) in figure space.
POSES = {
    "idle": ((-0.17, -0.02, 0.48), (0.17, -0.02, 0.48)),
    "spear": ((-0.17, -0.13, 0.56), (0.19, -0.07, 0.62)),
    "pike": ((-0.06, -0.17, 0.62), (0.15, -0.05, 0.66)),
    "raise": ((-0.17, -0.13, 0.56), (0.21, -0.02, 0.98)),
    "bow": ((-0.30, -0.12, 0.74), (0.02, -0.06, 0.74)),
    "crossbow": ((-0.05, -0.22, 0.64), (0.08, -0.08, 0.62)),
    "port": ((-0.08, -0.15, 0.70), (0.13, -0.10, 0.48)),
    "shoulder": ((-0.06, -0.16, 0.62), (0.14, -0.04, 0.80)),
    "hip": ((-0.07, -0.20, 0.52), (0.13, -0.08, 0.50)),
    "staff": ((-0.16, -0.06, 0.50), (0.19, -0.10, 0.72)),
    "book": ((-0.07, -0.16, 0.60), (0.07, -0.16, 0.60)),
    "tool": ((-0.04, -0.15, 0.64), (0.14, -0.10, 0.80)),
    "rein": ((-0.08, -0.20, 0.62), (0.08, -0.20, 0.62)),
    "sling": ((-0.17, -0.10, 0.52), (0.20, 0.02, 1.00)),
}

GEAR_POSE = {
    "": "idle", "spear": "spear", "lance": "spear", "pike": "pike", "halberd": "pike",
    "club": "raise", "sword": "raise", "longsword": "raise", "axe": "raise", "katana": "raise",
    "kris": "raise", "sabre": "raise", "mace": "raise", "bow": "bow", "longbow": "bow",
    "crossbow": "crossbow", "repeater": "crossbow", "sling": "sling", "atlatl": "raise",
    "musket": "port", "rifle": "port", "smg": "hip", "assault": "port", "bazooka": "shoulder",
    "mg": "hip", "staff": "staff", "cross": "staff", "book": "book", "scroll": "book",
    "brush": "tool", "lute": "book", "hammer": "tool", "pick": "tool", "shovel": "tool",
    "banner": "staff", "pistol": "raise", "telescope": "raise", "compass": "book",
    "flask": "book", "coin": "book", "reins": "rein", "torch": "raise", "trumpet": "book",
}


def _bend(a, b, segment, hint):
    """Middle joint between a and b for two segments of `segment` length, bent toward hint."""
    d = sub(b, a)
    dist = max(length(d), 1e-6)
    mid = lerp(a, b, 0.5)
    out = math.sqrt(max(segment * segment - (dist / 2) ** 2, 0.0))
    axis = unit(d)
    h = sub(hint, mul(axis, sum(hint[i] * axis[i] for i in range(3))))
    if length(h) < 1e-6:
        return mid
    return add(mid, mul(unit(h), out))


def figure(m, look):
    """Full figure; returns hand positions (left, right) for gear placement."""
    pose = POSES[GEAR_POSE.get(look.right, "idle")]
    if look.left in SHIELDS and GEAR_POSE.get(look.right, "idle") in ("idle", "raise", "spear", "staff"):
        left_hand = (-0.17, -0.14, 0.56)
    else:
        left_hand = pose[0]
    right_hand = pose[1]
    _legs(m, look)
    _torso(m, look)
    _arms(m, look, left_hand, right_hand)
    _head(m, look)
    headgear(m, look)
    if look.back:
        back_item(m, look.back)
    gear(m, look.right, right_hand, "right", look)
    if look.left in SHIELDS:
        shield(m, look.left, left_hand)
    elif look.left:
        gear(m, look.left, left_hand, "left", look)
    return left_hand, right_hand


def _legs(m, look):
    if look.robe and not look.seated:
        m.rings([((0, 0.0, 0.02), 0.16, 0.12), ((0, 0.0, 0.30), 0.13, 0.095), ((0, 0, 0.52), 0.105, 0.075)],
                look.body if look.skirt == "" else look.skirt)
        if m.detail == 0:
            for x in (-0.06, 0.06):
                m.box((x, -0.06, 0.025), (0.03, 0.05, 0.025), look.boots)
        return
    if look.seated:
        joints = [((-0.07, 0, HIP_Z), (-0.13, -0.14, 0.40), (-0.14, -0.08, 0.17)),
                  ((0.07, 0, HIP_Z), (0.13, -0.14, 0.40), (0.14, -0.08, 0.17))]
    else:
        joints = [((-0.065, 0, HIP_Z), (-0.08, -0.05, 0.26), (-0.09, -0.04, 0.05)),
                  ((0.065, 0, HIP_Z), (0.085, 0.03, 0.26), (0.095, 0.06, 0.05))]
    for hip, knee, ankle in joints:
        if m.detail == 0:
            m.rod(hip, knee, 0.048, 0.038, look.legs)
            m.rod(knee, ankle, 0.037, 0.03, look.boots if look.boots != look.legs else look.legs)
            m.box(add(ankle, (0, -0.025, -0.02)), (0.032, 0.055, 0.026), look.boots)
        else:
            m.rod(hip, ankle, 0.045, 0.034, look.legs, sides=m.pick(6, 4, 3))


def _torso(m, look):
    b = look.bulk
    if m.detail == 2:
        # Marker: one tapered body block in the tunic color, armor and hips folded in.
        m.rings([((0, 0, 0.36 if look.skirt and not look.seated else 0.45), 0.12 * b, 0.085),
                 ((0, 0, 0.76), 0.13 * b, 0.075)], look.body, sides=4, phase=0.0)
        if look.cape:
            m.loft([[(-0.12 * b, 0.07, 0.76), (0.12 * b, 0.07, 0.76), (0.0, 0.09, 0.77)],
                    [(-0.17 * b, 0.14, 0.18), (0.17 * b, 0.14, 0.18), (0.0, 0.18, 0.20)]], look.cape)
        return
    m.rings([((0, 0, 0.43), 0.10 * b, 0.07), ((0, 0, 0.52), 0.10 * b, 0.068)], look.legs if not look.skirt else look.skirt,
            sides=m.pick(6, 4, 4))
    if look.skirt and not look.robe and not look.seated:
        m.rings([((0, 0, 0.33), 0.13 * b, 0.10), ((0, 0, 0.50), 0.105 * b, 0.075)], look.skirt, sides=m.pick(8, 5, 4))
    m.rings([((0, 0.0, 0.50), 0.098 * b, 0.066), ((0, -0.006, 0.66), 0.138 * b, 0.085),
             ((0, 0.0, 0.755), 0.125 * b, 0.072), ((0, 0.0, 0.79), 0.055, 0.05)], look.body)
    if look.armor:
        m.rings([((0, -0.008, 0.53), 0.104 * b, 0.072), ((0, -0.014, 0.67), 0.145 * b, 0.091),
                 ((0, -0.004, 0.75), 0.13 * b, 0.077)], look.armor, sides=m.pick(8, 5, 4))
    if look.belt and m.detail == 0:
        m.rings([((0, 0, 0.495), 0.104 * b, 0.071), ((0, 0, 0.525), 0.104 * b, 0.071)], look.belt, sides=6)
    if look.shoulders:
        for s in (-1, 1):
            m.ball((s * 0.135 * b, 0, 0.75), (0.06, 0.06, 0.045), look.shoulders, u=m.pick(6, 4, 4), v=3)
    if look.cape:
        m.loft([[(-0.12 * b, 0.07, 0.76), (0.12 * b, 0.07, 0.76), (0.0, 0.09, 0.77)],
                [(-0.17 * b, 0.14, 0.18), (0.17 * b, 0.14, 0.18), (0.0, 0.18, 0.20)]], look.cape)
    if "bandolier" in look.extra and m.detail == 0:
        with m.at(move(0, -0.075, 0.64), rot_y(40)):
            m.box((0, 0, 0), (0.018, 0.012, 0.15), "leather")
    if "wings" in look.extra and m.detail < 2:
        for s in (-1, 1):
            m.rod((s * 0.07, 0.09, 0.55), (s * 0.09, 0.16, 0.92), 0.02, 0.018, "wood", sides=3)
            m.rod((s * 0.09, 0.16, 0.92), (s * 0.04, 0.12, 1.22), 0.018, 0.0, "white", sides=3)
    if "beard" in look.extra and m.detail < 2:
        m.rings([((0, -0.06, 0.79), 0.05, 0.03), ((0, -0.075, 0.86), 0.06, 0.03)], "hair", sides=5)


def _arms(m, look, left, right):
    b = look.bulk
    for s, hand in ((-1, left), (1, right)):
        shoulder = (s * SHOULDER[0] * b, SHOULDER[1], SHOULDER[2])
        elbow = _bend(shoulder, hand, 0.17, (s * 0.8, 0.3, -1.0))
        if m.detail == 0:
            m.rod(shoulder, elbow, 0.036, 0.03, look.body)
            m.rod(elbow, hand, 0.03, 0.025, look.body)
            m.octa(hand, 0.028, look.skin)
        elif m.detail == 1:
            m.rod(shoulder, hand, 0.035, 0.026, look.body, sides=4)


def _head(m, look):
    if m.detail < 2:
        m.rod((0, 0, 0.77), (0, -0.003, 0.83), 0.034, 0.032, look.skin, sides=m.pick(5, 4, 3))
    m.ball(HEAD, HEAD_R, look.skin, u=m.pick(7, 5, 4), v=m.pick(5, 4, 3))
    if m.detail == 0:
        m.octa((0, HEAD[1] - HEAD_R[1] - 0.004, HEAD[2] - 0.01), (0.014, 0.014, 0.02), look.skin)


# Headgear ---------------------------------------------------------------------
def _dome(m, mat, r=1.0, lift=0.0, sides=None):
    hx, hy, hz = HEAD
    rr = (HEAD_R[0] + 0.014) * r
    m.rings([((hx, hy, hz + 0.005 + lift), rr, rr * 0.98), ((hx, hy, hz + 0.055 + lift), rr * 0.86, rr * 0.84),
             ((hx, hy, hz + 0.105 + lift), 0.0, 0.0)], mat, sides=sides or m.pick(8, 6, 4))


def _brim(m, mat, r, z, thick=0.012):
    m.rings([((HEAD[0], HEAD[1], z), r, r * 0.95), ((HEAD[0], HEAD[1], z + thick), r, r * 0.95)], mat,
            sides=m.pick(10, 6, 5))


def headgear(m, look):
    kind, mat = look.head, look.head_mat
    hx, hy, hz = HEAD
    top = hz + HEAD_R[2]
    if kind == "bare":
        m.rings([((hx, hy + 0.01, hz + 0.01), 0.076, 0.076), ((hx, hy + 0.01, top + 0.012), 0.0, 0.0)], "hair",
                sides=m.pick(7, 5, 4))
    elif kind == "band":  # headband with feathers
        _dome(m, "hair", 0.98, -0.01)
        _brim(m, mat, 0.082, hz + 0.02, 0.02)
        for i, a in enumerate((-25, 0, 25) if m.detail == 0 else (0,)):
            with m.at(move(hx, hy + 0.03, top - 0.01), rot_y(a), rot_x(-12)):
                m.rod((0, 0, 0), (0, 0, 0.16), 0.016, 0.0, look.plume or "feather", sides=3)
    elif kind == "cone":
        m.rod((hx, hy, hz + 0.0), (hx, hy, top + 0.08), 0.088, 0.0, mat, sides=m.pick(8, 6, 4))
        _brim(m, mat, 0.09, hz - 0.005, 0.016)
    elif kind == "dome":
        _dome(m, mat)
        if look.plume:
            m.prism([(-0.01, top + 0.0), (0.01, top + 0.0), (0.01, top + 0.09), (-0.01, top + 0.09)], 0.16,
                    look.plume, axis="x")
    elif kind == "crest":  # Greek / Roman helmet with transverse or long crest
        _dome(m, mat)
        with m.at(move(hx, hy + 0.01, 0)):
            m.prism([(-0.09, top + 0.01), (0.09, top + 0.01), (0.07, top + 0.10), (-0.06, top + 0.12)], 0.035,
                    look.plume or "team", axis="x")
    elif kind == "kettle":
        _dome(m, mat, 0.95)
        _brim(m, mat, 0.13, hz + 0.02, 0.014)
    elif kind == "morion":
        _dome(m, mat, 0.95, 0.01)
        _brim(m, mat, 0.125, hz + 0.02, 0.014)
        m.prism([(-0.07, top), (0.07, top), (0.0, top + 0.08)], 0.016, mat, axis="x")
    elif kind == "kabuto":
        _dome(m, mat)
        m.rings([((hx, hy + 0.02, hz - 0.03), 0.13, 0.12), ((hx, hy + 0.01, hz + 0.04), 0.09, 0.085)], mat,
                sides=m.pick(8, 6, 4))
        if m.detail < 2:
            with m.at(move(hx, hy - 0.08, top - 0.01)):
                for s in (-1, 1):
                    m.rod((0, 0, 0), (s * 0.07, -0.01, 0.09), 0.01, 0.006, "gold", sides=3)
    elif kind == "horned":
        _dome(m, mat)
        for s in (-1, 1):
            m.rod((s * 0.07, hy, hz + 0.06), (s * 0.15, hy - 0.01, top + 0.06), 0.022, 0.0, "ivory", sides=4)
    elif kind == "tricorne":
        _dome(m, "black", 0.9, -0.01, sides=6)
        with m.at(move(hx, hy, hz + 0.045)):
            m.loft([[(-0.15, -0.06, 0), (0.15, -0.06, 0), (0.0, 0.15, 0)],
                    [(-0.13, -0.05, 0.05), (0.13, -0.05, 0.05), (0.0, 0.12, 0.05)]], "black")
    elif kind == "bicorne":
        _dome(m, "black", 0.9, -0.01, sides=6)
        with m.at(move(hx, hy, hz + 0.06)):
            m.prism([(-0.16, 0.0), (0.16, 0.0), (0.06, 0.08), (-0.06, 0.08)], 0.05, "black", axis="y")
    elif kind == "shako":
        m.rings([((hx, hy, hz + 0.0), 0.08, 0.08), ((hx, hy, top + 0.09), 0.088, 0.088)], mat, sides=m.pick(8, 6, 4))
        _brim(m, "black", 0.085, hz - 0.005, 0.012)
        if look.plume and m.detail < 2:
            m.rod((hx, hy - 0.05, top + 0.08), (hx, hy - 0.06, top + 0.15), 0.02, 0.012, look.plume, sides=4)
    elif kind == "kepi":
        m.rings([((hx, hy, hz + 0.01), 0.08, 0.08), ((hx, hy + 0.01, top + 0.04), 0.07, 0.072)], mat,
                sides=m.pick(8, 6, 4))
        if m.detail < 2:
            m.box((hx, hy - 0.085, hz + 0.02), (0.05, 0.03, 0.006), "black")
    elif kind == "stahl":  # flared steel helmet (WWI/WWII)
        _dome(m, mat, 1.06, -0.005)
        m.rings([((hx, hy + 0.01, hz - 0.035), 0.1, 0.1), ((hx, hy + 0.005, hz + 0.02), 0.088, 0.088)], mat,
                sides=m.pick(8, 6, 4))
    elif kind == "brodie":
        _dome(m, mat, 0.98, 0.0)
        _brim(m, mat, 0.13, hz + 0.03, 0.012)
    elif kind == "modern":
        _dome(m, mat, 1.08, -0.005)
        m.rings([((hx, hy + 0.01, hz - 0.02), 0.095, 0.095), ((hx, hy, hz + 0.03), 0.09, 0.09)], mat,
                sides=m.pick(8, 6, 4))
        if m.detail == 0:
            m.box((hx, hy - 0.08, hz + 0.035), (0.05, 0.02, 0.012), "dark")
    elif kind == "turban":
        m.ball((hx, hy, hz + 0.045), (0.088, 0.088, 0.06), mat, u=m.pick(8, 6, 4), v=m.pick(4, 3, 3))
        m.ball((hx, hy, hz + 0.09), (0.06, 0.06, 0.045), mat, u=m.pick(6, 5, 4), v=3)
    elif kind == "jaguar":
        m.ball((hx, hy + 0.01, hz + 0.02), (0.095, 0.095, 0.1), "jaguar", u=m.pick(8, 6, 4), v=m.pick(5, 4, 3))
        m.box((hx, hy - 0.1, hz + 0.06), (0.05, 0.04, 0.03), "jaguar")
        if m.detail < 2:
            for s in (-1, 1):
                m.rod((s * 0.05, hy, top + 0.0), (s * 0.06, hy, top + 0.05), 0.02, 0.0, "jaguar", sides=3)
    elif kind == "fur":
        m.rings([((hx, hy, hz + 0.0), 0.088, 0.088), ((hx, hy, top + 0.07), 0.095, 0.095)], mat,
                sides=m.pick(8, 6, 4))
        if look.plume and m.detail < 2:
            m.rod((hx + 0.04, hy - 0.04, top + 0.06), (hx + 0.05, hy - 0.05, top + 0.16), 0.016, 0.0, look.plume,
                  sides=4)
    elif kind == "hood":
        m.rings([((hx, hy + 0.01, hz - 0.06), 0.11, 0.1), ((hx, hy + 0.01, hz + 0.04), 0.095, 0.09),
                 ((hx, hy + 0.04, top + 0.06), 0.0, 0.0)], mat, sides=m.pick(7, 5, 4))
    elif kind == "mitre":
        _brim(m, mat, 0.08, hz + 0.03, 0.03)
        m.prism([(-0.065, top - 0.02), (0.065, top - 0.02), (0.05, top + 0.08), (0.0, top + 0.13),
                 (-0.05, top + 0.08)], 0.06, mat, axis="y")
    elif kind == "wide":  # wide-brim hat
        m.rings([((hx, hy, hz + 0.03), 0.08, 0.08), ((hx, hy, top + 0.04), 0.065, 0.065)], mat, sides=m.pick(7, 5, 4))
        _brim(m, mat, 0.15, hz + 0.03, 0.012)
    elif kind == "cap":
        m.rings([((hx, hy + 0.01, hz + 0.02), 0.08, 0.08), ((hx, hy + 0.02, top + 0.03), 0.04, 0.04)], mat,
                sides=m.pick(7, 5, 4))
    elif kind == "beret":
        _dome(m, "hair", 0.98, -0.01)
        with m.at(move(hx + 0.02, hy, top + 0.0), rot_y(-15)):
            m.rings([((0, 0, -0.01), 0.085, 0.085), ((0, 0, 0.02), 0.08, 0.08)], mat, sides=m.pick(8, 6, 4))
    elif kind == "pointed":  # Mongol / steppe fur-trimmed hat
        _brim(m, "fur", 0.09, hz + 0.02, 0.035)
        m.rod((hx, hy, hz + 0.05), (hx, hy, top + 0.10), 0.075, 0.0, mat, sides=m.pick(7, 5, 4))
    elif kind == "laurel":
        m.rings([((hx, hy + 0.01, hz + 0.01), 0.076, 0.076), ((hx, hy + 0.01, top + 0.012), 0.0, 0.0)], "hair",
                sides=m.pick(7, 5, 4))
        if m.detail < 2:
            _brim(m, "green", 0.08, hz + 0.04, 0.015)
    elif kind == "top":
        m.rings([((hx, hy, hz + 0.04), 0.07, 0.07), ((hx, hy, top + 0.12), 0.075, 0.075)], mat, sides=m.pick(7, 5, 4))
        _brim(m, mat, 0.11, hz + 0.04, 0.01)
    elif kind == "pilot":
        _dome(m, "leather", 1.0, -0.01)
        if m.detail == 0:
            m.box((hx, hy - 0.08, hz + 0.04), (0.06, 0.012, 0.02), "glass")
    elif kind == "crown":
        _dome(m, "hair", 0.98, -0.01)
        m.rings([((hx, hy, hz + 0.05), 0.085, 0.085), ((hx, hy, hz + 0.1), 0.09, 0.09)], "gold", sides=m.pick(8, 6, 4))
    else:
        raise KeyError("unknown headgear " + kind)


# Shields ----------------------------------------------------------------------
SHIELDS = ("round", "kite", "scutum", "hoplon", "hide", "tower", "buckler", "oval", "wicker")


def shield(m, kind, hand):
    """Shield held in the left hand, face toward -y (the camera)."""
    x, y, z = hand
    with m.at(move(x - 0.02, y - 0.035, z + 0.04), rot_z(12)):
        if kind in ("round", "hoplon", "buckler"):
            r = {"round": 0.15, "hoplon": 0.19, "buckler": 0.09}[kind]
            m.rod((0, 0.012, 0), (0, -0.012, 0), r, r, "team", sides=m.pick(10, 7, 6))
            rim = "bronze" if kind == "hoplon" else "wood"
            if m.detail == 0:
                m.rod((0, -0.012, 0), (0, -0.03, 0), r * 0.25, 0.0, rim, sides=5)
                m.rod((0, 0.02, 0), (0, 0.006, 0), r + 0.012, r + 0.012, rim, sides=10)
        elif kind == "kite":
            m.prism([(-0.12, 0.13), (0.12, 0.13), (0.12, -0.02), (0.0, -0.27), (-0.12, -0.02)], 0.025, "team")
            if m.detail == 0:
                m.prism([(-0.02, 0.12), (0.02, 0.12), (0.02, -0.22), (-0.02, -0.22)], 0.035, "linen")
        elif kind in ("scutum", "tower"):
            w, h = (0.12, 0.2) if kind == "scutum" else (0.13, 0.24)
            m.loft([[(-w, 0.02, -h), (w, 0.02, -h), (w * 0.8, -0.025, -h), (-w * 0.8, -0.025, -h)],
                    [(-w, 0.02, h), (w, 0.02, h), (w * 0.8, -0.025, h), (-w * 0.8, -0.025, h)]], "team")
            if m.detail == 0:
                m.rod((0, -0.02, 0), (0, -0.045, 0), 0.04, 0.02, "bronze", sides=5)
        elif kind in ("hide", "oval"):
            m.rings([((0, 0, -0.26), 0.0, 0.0), ((0, 0, -0.2), 0.08, 0.015), ((0, 0, 0.0), 0.12, 0.018),
                     ((0, 0, 0.2), 0.08, 0.015), ((0, 0, 0.26), 0.0, 0.0)], "team", sides=m.pick(8, 6, 4))
            if m.detail == 0 and kind == "hide":
                m.rings([((0, -0.01, -0.15), 0.04, 0.012), ((0, -0.01, 0.15), 0.04, 0.012)], "white", sides=4)
                m.rod((0, -0.03, -0.3), (0, -0.03, 0.32), 0.008, 0.008, "wood", sides=3)
        elif kind == "wicker":
            m.prism([(-0.13, -0.18), (0.13, -0.18), (0.14, 0.18), (-0.14, 0.18)], 0.03, "rope")
            if m.detail == 0:
                m.prism([(-0.05, -0.1), (0.05, -0.1), (0.05, 0.1), (-0.05, 0.1)], 0.04, "team")


# Hand-held gear -----------------------------------------------------------------
def gear(m, kind, hand, side, look):
    if not kind:
        return
    x, y, z = hand
    if kind in ("spear", "lance", "pike", "halberd"):
        top = {"spear": 1.30, "lance": 1.45, "pike": 1.75, "halberd": 1.55}[kind]
        if kind in ("pike", "halberd"):
            base, tip = (x + 0.06, y + 0.18, 0.06), (x - 0.14, y - 0.32, top)
        else:
            base, tip = (x, y + 0.02, 0.02), (x, y - 0.02, top)
        shaft_end = lerp(base, tip, 0.9)
        m.rod(base, shaft_end, 0.014, 0.012, "wood", sides=m.pick(4, 3, 3))
        head = "steel" if look.head_mat in ("steel",) else "iron" if kind != "spear" else "bronze"
        m.rod(shaft_end, tip, 0.03, 0.0, head, sides=m.pick(4, 3, 3))
        if kind == "halberd" and m.detail < 2:
            with m.at(move(*lerp(base, tip, 0.86))):
                m.prism([(0.0, -0.05), (0.09, -0.03), (0.09, 0.04), (0.0, 0.05)], 0.015, "steel")
    elif kind in ("sword", "longsword", "katana", "kris", "sabre"):
        blade = {"sword": 0.30, "longsword": 0.45, "katana": 0.42, "kris": 0.26, "sabre": 0.38}[kind]
        with m.at(move(x, y, z), rot_y(-15)):
            m.box((0, 0, 0.0), (0.05 if kind != "katana" else 0.025, 0.014, 0.012), "gold" if kind == "kris" else "iron")
            m.box((0, 0, -0.04), (0.012, 0.012, 0.04), "leather")
            w = 0.024 if kind != "longsword" else 0.03
            m.prism([(-w, 0.01), (w, 0.01), (w * 0.7, blade), (0.0, blade + 0.05), (-w * 0.7, blade)], 0.012,
                    "steel")
    elif kind in ("club", "mace", "axe"):
        with m.at(move(x, y, z), rot_y(-20)):
            m.rod((0, 0, -0.06), (0, 0, 0.30), 0.016, 0.03 if kind == "club" else 0.014, "wood", sides=m.pick(5, 4, 3))
            if kind == "club":
                m.ball((0, 0, 0.30), 0.05, "wood_dark", u=m.pick(6, 4, 4), v=3)
            elif kind == "mace":
                m.ball((0, 0, 0.30), 0.045, "iron", u=m.pick(6, 4, 4), v=3)
            else:
                m.prism([(0.0, 0.20), (0.11, 0.17), (0.12, 0.32), (0.0, 0.29)], 0.015, "steel")
    elif kind in ("bow", "longbow"):
        # The limb bulges outward (-x) so the curve reads from the camera.
        h = min(0.40 if kind == "bow" else 0.56, z - 0.06)
        bend = 0.11
        with m.at(move(x, y, z), rot_y(-12)):
            arc = m.pick(5, 3, 2)
            pts = []
            for i in range(arc + 1):
                t = i / arc * 2 - 1
                pts.append((-bend * (1 - t * t), 0, h * t))
            for a, b in zip(pts, pts[1:]):
                m.rod(a, b, 0.016, 0.016, "wood_dark", sides=3)
            if m.detail < 2:
                m.rod((0.0, 0, -h), (0.0, 0, h), 0.005, 0.005, "linen", sides=3)
    elif kind in ("crossbow", "repeater"):
        with m.at(move(x, y, z)):
            m.box((0, -0.05, 0), (0.025, 0.16, 0.025), "wood")
            m.loft([[(-0.20, -0.18, -0.01), (0.20, -0.18, -0.01), (0.0, -0.15, -0.01)],
                    [(-0.20, -0.18, 0.015), (0.20, -0.18, 0.015), (0.0, -0.15, 0.015)]], "wood_dark")
            if kind == "repeater" and m.detail < 2:
                m.box((0, -0.08, 0.06), (0.03, 0.06, 0.04), "wood")
    elif kind == "sling":
        if m.detail < 2:
            m.rod((x, y, z), (x - 0.08, y + 0.03, z + 0.16), 0.007, 0.007, "rope", sides=3)
            m.ball((x - 0.08, y + 0.03, z + 0.17), 0.025, "stone", u=4, v=3)
    elif kind == "atlatl":
        with m.at(move(x, y, z), rot_y(-30)):
            m.rod((0, 0, -0.05), (0, 0, 0.25), 0.012, 0.012, "wood", sides=3)
            m.rod((0, -0.02, -0.25), (0, -0.02, 0.55), 0.008, 0.0, "wood_dark", sides=3)
    elif kind in ("musket", "rifle", "assault", "smg"):
        n = {"musket": 0.72, "rifle": 0.66, "assault": 0.52, "smg": 0.40}[kind]
        stock = "wood" if kind in ("musket", "rifle") else "dark"
        a = (x + 0.04, y + 0.02, z - 0.10) if kind != "smg" else (x + 0.05, y + 0.05, z - 0.02)
        dirn = unit(sub(look_target(kind, hand), a))
        b = add(a, mul(dirn, n))
        m.rod(a, lerp(a, b, 0.35), 0.03, 0.022, stock, sides=4)
        m.rod(lerp(a, b, 0.33), b, 0.014, 0.012, "dark" if kind != "musket" else "iron", sides=m.pick(4, 3, 3))
        if kind in ("musket", "rifle") and look.extra and "bayonet" in look.extra and m.detail < 2:
            m.rod(b, add(b, mul(dirn, 0.14)), 0.01, 0.0, "steel", sides=3)
        if kind == "assault" and m.detail < 2:
            mid = lerp(a, b, 0.45)
            m.box(add(mid, (0, 0, -0.05)), (0.012, 0.02, 0.04), "dark")
    elif kind == "bazooka":
        a, b = (x + 0.02, y + 0.30, z + 0.0), (x - 0.06, y - 0.42, z + 0.05)
        m.rod(a, b, 0.04, 0.04, "olive", sides=m.pick(6, 4, 3))
    elif kind == "mg":
        a = (x, y + 0.08, z)
        b = (x - 0.08, y - 0.45, z + 0.04)
        m.rod(a, lerp(a, b, 0.4), 0.05, 0.05, "dark", sides=m.pick(6, 4, 3))
        m.rod(lerp(a, b, 0.4), b, 0.018, 0.018, "dark", sides=3)
    elif kind in ("staff", "cross", "banner", "torch"):
        top = {"staff": 1.15, "cross": 1.2, "banner": 1.45, "torch": 1.1}[kind]
        m.rod((x, y, 0.02), (x, y, top), 0.014, 0.012, "wood", sides=m.pick(4, 3, 3))
        if kind == "cross":
            m.box((x, y, top - 0.02), (0.012, 0.012, 0.07), "gold")
            m.box((x, y, top + 0.0), (0.05, 0.012, 0.012), "gold")
        elif kind == "banner":
            with m.at(move(x, y, top - 0.04)):
                m.prism([(0.0, 0.0), (0.30, -0.02), (0.28, -0.22), (0.0, -0.25)], 0.012, "team")
        elif kind == "torch":
            m.rod((x, y, top), (x, y, top + 0.08), 0.03, 0.0, "fire", sides=4)
        elif kind == "staff" and m.detail == 0:
            m.octa((x, y, top), 0.03, "gold")
    elif kind in ("book", "scroll", "compass", "coin", "flask"):
        cx, cy, cz = lerp(hand, POSES["book"][0], 0.5)
        if kind == "book":
            m.box((cx, cy - 0.01, cz + 0.02), (0.08, 0.02, 0.06), "red")
            if m.detail == 0:
                m.box((cx, cy - 0.032, cz + 0.02), (0.07, 0.004, 0.05), "paper")
        elif kind == "scroll":
            m.rod((cx - 0.09, cy - 0.01, cz + 0.02), (cx + 0.09, cy - 0.01, cz + 0.02), 0.03, 0.03, "paper",
                  sides=m.pick(6, 4, 3))
        elif kind == "compass":
            m.rod((cx, cy - 0.01, cz), (cx, cy - 0.04, cz), 0.05, 0.05, "gold", sides=m.pick(8, 5, 4))
        elif kind == "coin":
            m.ball((cx, cy - 0.03, cz), (0.06, 0.05, 0.06), "leather", u=m.pick(6, 4, 4), v=3)
        else:
            m.ball((cx, cy - 0.03, cz), (0.04, 0.04, 0.05), "glass", u=m.pick(6, 4, 4), v=3)
    elif kind == "lute":
        cx, cy, cz = lerp(hand, POSES["book"][0], 0.5)
        with m.at(move(cx, cy - 0.03, cz), rot_y(-40)):
            m.ball((0, 0, 0), (0.08, 0.035, 0.1), "wood", u=m.pick(7, 5, 4), v=3)
            m.box((0, 0, 0.17), (0.015, 0.01, 0.09), "wood_dark")
    elif kind == "trumpet":
        cx, cy, cz = lerp(hand, POSES["book"][0], 0.5)
        m.rod((cx, cy, cz + 0.2), (cx, cy - 0.25, cz + 0.22), 0.012, 0.05, "gold", sides=m.pick(6, 4, 3))
    elif kind in ("brush", "hammer", "pick", "shovel"):
        with m.at(move(x, y, z), rot_y(-35)):
            m.rod((0, 0, -0.28), (0, 0, 0.30), 0.014, 0.014, "wood", sides=m.pick(4, 3, 3))
            if kind == "hammer":
                m.box((0, 0, 0.30), (0.07, 0.03, 0.035), "iron")
            elif kind == "pick":
                m.prism([(-0.14, 0.27), (0.0, 0.33), (0.14, 0.27), (0.0, 0.30)], 0.02, "iron")
            elif kind == "shovel":
                m.box((0, 0, -0.33), (0.05, 0.012, 0.07), "iron")
            else:
                m.rod((0, 0, 0.30), (0, 0, 0.36), 0.02, 0.0, "red", sides=4)
    elif kind == "pistol":
        m.box((x, y - 0.06, z), (0.015, 0.06, 0.02), "dark")
    elif kind == "telescope":
        m.rod((x, y, z), (x - 0.02, y - 0.25, z + 0.1), 0.025, 0.018, "gold", sides=m.pick(6, 4, 3))
    elif kind == "reins":
        pass
    else:
        raise KeyError("unknown gear " + kind)


def look_target(kind, hand):
    """Muzzle direction targets for long guns: port arms diagonal up-left, smg forward."""
    if kind == "smg":
        return (hand[0] - 0.05, hand[1] - 0.6, hand[2] + 0.05)
    return (hand[0] - 0.55, hand[1] - 0.15, hand[2] + 0.75)


def back_item(m, kind):
    if m.detail == 2:
        return
    if kind == "quiver":
        m.rod((0.06, 0.09, 0.50), (-0.05, 0.10, 0.86), 0.035, 0.04, "leather", sides=m.pick(5, 4, 3))
        if m.detail == 0:
            for dx in (-0.015, 0.015):
                m.rod((-0.04 + dx, 0.10, 0.84), (-0.07 + dx, 0.10, 0.95), 0.006, 0.006, "feather", sides=3)
    elif kind == "pack":
        m.box((0, 0.11, 0.62), (0.09, 0.05, 0.11), "khaki")
    elif kind == "bedroll":
        m.rod((-0.12, 0.10, 0.75), (0.12, 0.10, 0.75), 0.045, 0.045, "cloth", sides=m.pick(6, 4, 3))
        m.box((0, 0.10, 0.60), (0.08, 0.04, 0.1), "leather")
    elif kind == "radio":
        m.box((0, 0.11, 0.62), (0.08, 0.05, 0.12), "olive")
        if m.detail == 0:
            m.rod((0.05, 0.12, 0.72), (0.08, 0.13, 1.1), 0.005, 0.003, "dark", sides=3)
    elif kind == "chute":
        m.box((0, 0.11, 0.64), (0.10, 0.06, 0.12), "khaki")
    elif kind == "basket":
        m.rings([((0, 0.12, 0.48), 0.08, 0.06), ((0, 0.13, 0.78), 0.11, 0.08)], "rope", sides=m.pick(6, 4, 4))
    elif kind == "jetpack":
        for s in (-1, 1):
            m.rod((s * 0.05, 0.12, 0.45), (s * 0.05, 0.12, 0.78), 0.045, 0.045, "steel", sides=m.pick(6, 4, 3))
    else:
        raise KeyError("unknown back item " + kind)
