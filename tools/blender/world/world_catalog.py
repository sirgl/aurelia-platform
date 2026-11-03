"""World object catalog: every map object of Classic keyed by its content id.

Model ids are "<kind>/<slug>" with kind improvement | resource | natural-wonder | wonder |
city. A model does not know the content keys that draw it: every module part keeps its own key table
(classic/tools/blender/world/keys.py, lands/tools/blender/part.py). A key may carry a variant qualifier
after "@": "core:improvement/farm@core:terrain/plains" is the farm on plains; the renderer tries the
qualified key first and falls back to the plain key.

Content without map presence is listed explicitly by the module part (icon_only, no_map_presence) so the
coverage check in build.rs can tell a decision from a gap.
"""
from dataclasses import dataclass, field

# Triangle budgets (LOD0, LOD1, LOD2 far marker) per budget class.
BUDGETS = {
    "improvement": (600, 260, 120),
    "resource": (300, 140, 48),
    "natural-wonder": (3000, 1200, 400),
    "wonder": (2500, 1000, 300),
    "city-center": (800, 340, 100),
    "city-house": (160, 70, 24),
    "city-walls": (900, 360, 120),
    "city-landmark": (500, 220, 64),
}
# One city tile at LOD0: center + walls + landmarks + houses in the worst case.
CITY_TILE_LOD0_MAX = 6000

# Shape limits per budget class: (max footprint radius from the anchor, max height,
# lowest allowed point). Hex units; the hex has outer radius 1.0, inner 0.866.
SHAPES = {
    "improvement": (1.0, 0.9, -0.08),
    "resource": (0.62, 0.6, -0.08),
    "natural-wonder": (1.05, 2.4, -0.25),
    "wonder": (0.95, 2.0, -0.25),
    "city-center": (0.30, 1.4, -0.08),
    "city-house": (0.15, 1.0, -0.08),
    "city-walls": (0.86, 0.45, -0.08),
    "city-landmark": (0.26, 1.2, -0.25),
}

# Projected size in pixels (max of height and footprint radius) below which LOD1 / the
# far marker is drawn.
LOD_PX = {
    "improvement": (34.0, 12.0),
    "resource": (22.0, 9.0),
    "natural-wonder": (40.0, 14.0),
    "wonder": (40.0, 14.0),
    "city-center": (20.0, 8.0),
    "city-house": (14.0, 6.0),
    "city-walls": (36.0, 12.0),
    "city-landmark": (18.0, 7.0),
}

GROUNDS = ("upright", "plane", "water")
# How the terrain surface prepares the tile under a natural wonder: a level pad, a bowl with
# a low rim, or the sea floor lifted close under the surface (render: scene/terrain/surface/sites.rs).
SITE_SHAPES = ("pad", "crater", "reef")


@dataclass
class WorldModel:
    id: str
    build: object
    budget: str
    ground: str = "upright"
    # Wonders: "land" sits on a land tile of the city ring, "water" on a water tile.
    site: str = "land"
    # Natural wonders: the ground the surface shapes under the model (SITE_SHAPES).
    site_shape: str = "pad"
    note: str = ""
    tags: tuple = field(default_factory=tuple)

    @property
    def kind(self):
        return self.id.split("/", 1)[0]


def all_models():
    """Every world model of the platform (shared and generic ones) in a stable order."""
    import city_kit
    import improvements
    import natural_wonders
    import resources
    import wonders

    models = []
    for module in (improvements, resources, natural_wonders, wonders, city_kit):
        models += module.models()
    ids = [m.id for m in models]
    duplicates = sorted({i for i in ids if ids.count(i) > 1})
    if duplicates:
        raise SystemExit(f"duplicate world model ids: {duplicates}")
    return sorted(models, key=lambda m: m.id)


def city_kit_data():
    import city_kit
    return city_kit.KIT
