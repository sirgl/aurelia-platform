"""World wonders of Classic (classic/docs/classic/wonders.json, kind world_wonder): one landmark
model each, drawn on a tile of the owning city's ring. Authored in wonders_early.py
(ancient to medieval) and wonders_late.py (renaissance to information).

Footprint: radius up to 0.95, height up to 2.0, LOD0 up to 2500 triangles. A wonder must
read at game scale (about 100 px across the tile): one strong silhouette first.
"""
from world_catalog import WorldModel


def wonder(slug, build, ground="upright", site="land"):
    return WorldModel("wonder/" + slug.replace("_", "-"), build, "wonder", ground, site=site)


def models():
    import wonders_early
    import wonders_late
    return wonders_early.models() + wonders_late.models()
