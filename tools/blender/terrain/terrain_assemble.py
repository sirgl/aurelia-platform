"""Terrain props (sea stacks) -> LOD meshes, checks and the "terrain" key of the platform's manifest.
No bpy: runs under python3 (check_catalogs.py) and inside Blender (export_terrain.py).

Terrain props are generic models: they belong to the platform only (no module draws them by a content
key), so there is a terrain section in platform/assets/models/manifest.json and nowhere else.
"""
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import parts  # noqa: E402

parts.use_library()
import sea_stack  # noqa: E402

LOD_NAMES = ("LOD0", "LOD1", "LOD2")
# Triangle budget per LOD and shape (max footprint radius, max height, lowest point).
BUDGET = [420, 220, 90]
SHAPE = [sea_stack.RADIUS, sea_stack.HEIGHT, 0.0]


def to_game(p):
    """Blender z-up -> glTF / game y-up (what the bake sees)."""
    return (p[0], p[2], -p[1])


def entry(variant, lods):
    pts = [to_game(p) for p in lods[0].points]
    lo = [round(min(p[i] for p in pts), 4) for i in range(3)]
    hi = [round(max(p[i] for p in pts), 4) for i in range(3)]
    radius = max((p[0] ** 2 + p[2] ** 2) ** 0.5 for p in pts)
    return {
        "id": f"terrain/sea-stack-{variant}",
        "file": f"terrain/sea-stack-{variant}.glb",
        "budget": "sea-stack",
        "bounds": {"min": lo, "max": hi},
        "radius": round(radius, 4),
        "materials": sorted(lods[0].materials),
        "lods": [{"node": name, "tris": len(m.triangles)} for name, m in zip(LOD_NAMES, lods)],
    }


def entry_problems(e):
    problems = []
    for lod, limit in zip(e["lods"], BUDGET):
        if lod["tris"] > limit:
            problems.append(f"{e['id']} {lod['node']}: {lod['tris']} tris > {limit}")
    if e["radius"] > SHAPE[0] + 1e-3 or e["bounds"]["max"][1] > SHAPE[1] + 1e-3:
        problems.append(f"{e['id']}: outside the sea-stack shape {SHAPE}")
    return problems


def build():
    """[(three LOD meshes, manifest entry)] of every sea stack variant."""
    out = []
    for variant in range(sea_stack.VARIANTS):
        lods = sea_stack.meshes(variant)
        out.append((lods, entry(variant, lods)))
    return out


def manifest_section(entries):
    return {
        "terrain": {
            "generator": "platform/tools/blender/export_terrain.py",
            "note": "Generated. Edit platform/tools/blender/terrain/*.py and run sh platform/tools/blender/export_all.sh.",
            "axes": "glTF y-up, model units; the coast props scale the footprint to their radius and stretch the height from the sea floor to their top",
            "budgets": {"sea-stack": BUDGET},
            "shapes": {"sea-stack": SHAPE},
            "models": entries,
        }
    }
