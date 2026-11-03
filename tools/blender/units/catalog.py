"""Unit model catalog of the platform: the building blocks every part uses (Model, the builders, the
budgets, the formation slots, the looks shared by several parts) and the shared and generic unit models:
the class models that more than one rules module draws, and the fallback "unit/unknown".

A model belongs to the part whose content keys draw it (integration/tools/models-split.py, catalogs.py):
the class models and unique variants only the classic module draws are in
classic/tools/blender/units/catalog.py, built with the same helpers. Content keys are not here: classic
maps "core:unit-class/<slug>" / "core:unit/<slug>" in its part.py, lands maps "lal:..." in its part.py.
Facing: people turn 20 degrees to the camera's right, long things point 35 degrees right.
"""
from dataclasses import dataclass, field

from geom import rot_z
import civilians
import machines
import mounts
import people
import ships
from people import Look

SCALE = 0.42          # model units (human = 1.0) -> hex units
ENGINE_SCALE = SCALE * machines.ENGINE_SIZE   # crewed engines: the engine reads bigger, crew stays human
SINGLE_SCALE = 0.52   # single civilians and great people stand out from the formations
MARKER_BOOST = 1.3    # far marker of a formation is one larger figure

# Triangle budgets per budget class: (LOD0, LOD1, LOD2) per figure or per single model.
# LOD0 is the game-scale model (R-07: 300-700 per foot figure), LOD2 the far marker.
BUDGETS = {
    "figure": (700, 320, 160),
    "mounted": (1000, 440, 240),
    "vehicle": (700, 320, 160),
    "siege": (1400, 640, 200),
    "ship": (1500, 650, 220),
    "aircraft": (800, 380, 200),
    "civilian": (1300, 600, 320),
    "great": (900, 420, 200),
}
FORMATION_LOD0_MAX = 3000   # whole formation at LOD0 (R-07: e.g. 6 x 500)
# The boat that a land unit is drawn as while it is afloat (manifest keys `<module>:embarked`).
EMBARKED_VESSEL = "unit/workboat"
LOD_PX = (14.0, 7.0)        # projected model height (px) below which LOD1 / marker is used

# Formation slots in hex units: (x, z, scale). Slot 0 (leader, near the centre) also
# carries the far marker. +z is toward the camera (south).
SLOTS = {
    1: [(0.0, 0.0, 1.0)],
    2: [(0.10, 0.12, 1.0), (-0.22, -0.20, 0.97)],
    3: [(0.04, 0.16, 1.0), (-0.30, -0.14, 0.97), (0.32, -0.20, 1.02)],
    4: [(0.02, 0.10, 1.0), (-0.32, -0.08, 0.97), (0.34, -0.06, 1.03), (0.06, -0.34, 0.99)],
}


@dataclass
class Model:
    id: str
    build: object
    budget: str
    category: str
    figures: int = 1
    contact: str = "ground"
    scale: float = SCALE
    variant_of: str = ""
    units: list = field(default_factory=list)


def person_build(look):
    def build(m):
        with m.at(rot_z(20)):
            people.figure(m, look)
    # What the model is, for rigs.py (animation rig and weapon of the manifest).
    build.art = {"kind": "person", "look": look, "yaw": 20}
    return build


def long_build(fn, *args, yaw=-35, **kwargs):
    def build(m):
        with m.at(rot_z(yaw)):
            fn(m, *args, **kwargs)
    build.art = {"kind": "long", "fn": fn, "args": args, "kwargs": kwargs, "yaw": yaw}
    return build


def rider_build(mount, rider, **mount_args):
    return long_build(mounts.ride, mount, rider, **mount_args)


def plain_build(fn, *args, **kwargs):
    def build(m):
        fn(m, *args, **kwargs)
    build.art = {"kind": "plain", "fn": fn, "args": args, "kwargs": kwargs, "yaw": 0}
    return build


def infantry(slug, look, figures=4, category="military"):
    return Model("unit/" + slug, person_build(look), "figure", category, figures)


def cavalry(slug, mount, rider, figures=3, **mount_args):
    return Model("unit/" + slug, rider_build(mount, rider, **mount_args), "mounted", "military", figures)


def engine(slug, fn, *args, figures=1, budget="siege", **kwargs):
    k = ENGINE_SCALE if budget == "siege" else SCALE
    return Model("unit/" + slug, long_build(fn, *args, **kwargs), budget, "military", figures, scale=k)


def single(slug, build, budget, category):
    return Model("unit/" + slug, build, budget, category, scale=SINGLE_SCALE)


def vessel(slug, fn, *args, category="naval", **kwargs):
    return Model("unit/" + slug, long_build(fn, *args, yaw=-30, **kwargs), "ship", category, 1, "water")


# Shared looks ---------------------------------------------------------------------------
CREW_ANCIENT = Look(body="team", legs="cloth_dark", head="bare")
CREW_GUNPOWDER = Look(body="team", legs="cloth_dark", head="tricorne")
CREW_MODERN = Look(body="team", legs="olive", head="stahl", head_mat="olive")

WARRIOR = Look(body="leather", legs="leather", skirt="fur", head="bare", right="club", left="round")
SPEARMAN = Look(body="team", armor="leather", legs="cloth_dark", head="cone", head_mat="bronze", right="spear",
                left="round")
ARCHER = Look(body="team", legs="cloth_dark", head="cap", head_mat="leather", left="bow", back="quiver")
SWORDSMAN = Look(body="team", armor="iron", skirt="leather", head="dome", head_mat="iron", right="sword",
                 left="scutum")
MUSKET = Look(body="team", legs="cloth_dark", head="morion", head_mat="iron", right="musket", belt="leather")
RIFLE = Look(body="team", legs="navy", head="kepi", head_mat="navy", right="rifle", back="bedroll",
             extra=("bayonet",))


def class_models():
    """The shared class models by slug: what both rules modules draw."""
    c = {}

    def add(slug, model):
        c[slug] = model

    # Foot soldiers -------------------------------------------------------------
    add("warrior", infantry("warrior", WARRIOR))
    add("spearman", infantry("spearman", SPEARMAN))
    add("archer", infantry("archer", ARCHER))
    add("scout", infantry("scout", Look(body="team", legs="leather", head="cap", head_mat="leather", right="staff",
                                        back="bedroll"), figures=2))
    add("swordsman", infantry("swordsman", SWORDSMAN))
    add("musketman", infantry("musketman", MUSKET))
    add("rifleman", infantry("rifleman", RIFLE))
    # Mounted -----------------------------------------------------------------------
    horseman = Look(body="team", legs="cloth_dark", head="cone", head_mat="bronze", right="spear", left="round")
    add("horseman", cavalry("horseman", "horse", horseman))
    cavalry_look = Look(body="team", legs="navy", head="kepi", head_mat="navy", right="sabre")
    add("cavalry", cavalry("cavalry", "horse", cavalry_look))
    # Siege ----------------------------------------------------------------------------
    add("catapult", engine("catapult", machines.catapult, CREW_ANCIENT))
    add("cannon", engine("cannon", machines.cannon, CREW_GUNPOWDER))
    add("artillery", engine("artillery", machines.field_gun, CREW_MODERN, barrel=0.95))
    add("tank", engine("tank", machines.tank, figures=3, budget="vehicle"))
    add("mechanized-infantry", engine("mechanized-infantry", machines.mech_infantry, figures=2, budget="vehicle"))
    # Ships -----------------------------------------------------------------------------------
    add("trireme", vessel("trireme", ships.trireme))
    add("caravel", vessel("caravel", ships.caravel))
    add("frigate", vessel("frigate", ships.frigate))
    add("destroyer", vessel("destroyer", ships.destroyer))
    add("workboat", vessel("workboat", ships.work_boat, civilians.WORKER, category="civilian"))
    # Civilians, religion and great people ---------------------------------------------------
    add("settler", single("settler", plain_build(civilians.settler), "civilian", "civilian"))
    return c


def unknown_model():
    """Explicit runtime fallback for content keys without a manifest entry: a team banner."""
    def build(m):
        m.rod((0, 0, 0.0), (0, 0, 1.2), 0.02, 0.016, "wood", sides=m.pick(5, 4, 3))
        m.prism([(0.0, 1.15), (0.42, 1.12), (0.40, 0.78), (0.0, 0.75)], 0.02, "team")
        m.rings([((0, 0, 0.0), 0.16, 0.16), ((0, 0, 0.05), 0.12, 0.12)], "stone", sides=m.pick(8, 6, 4))
    return Model("unit/unknown", build, "figure", "military")
