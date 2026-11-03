"""World catalogs -> LOD meshes, checks, coverage against the part's documents and the manifest
section of one part. No bpy: runs under python3 (contact sheets, check_catalogs.py) and inside Blender
(export).

A part's world models are declared in <part>/tools/blender/world/*.py (platform: shared and generic ones,
classic: the models only classic draws), its content keys in the part's part.py / world/keys.py. A part is
checked together with the platform's models it can see; the platform alone on its own.
"""
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import parts  # noqa: E402

parts.use_library()
import catalogs  # noqa: E402
import world_catalog as catalog  # noqa: E402
from geom import Mesh  # noqa: E402

LOD_NAMES = ("LOD0", "LOD1", "LOD2")
SCALE = 1.0  # world models are authored in hex units


def build_lod(model, detail):
    mesh = Mesh(detail)
    model.build(mesh)
    return mesh


def to_game(point):
    """Blender z-up hex units -> glTF / game y-up (what the bake sees)."""
    return (point[0] * SCALE, point[2] * SCALE, -point[1] * SCALE)


def bounds(mesh):
    pts = [to_game(p) for p in mesh.points]
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    return lo, hi


def radius(mesh):
    return max(math.hypot(p[0], p[1]) for p in mesh.points)


def manifest_entry(model, meshes):
    lo, hi = bounds(meshes[0])
    materials = sorted(set().union(*(mesh.materials for mesh in meshes)))
    size = max(hi[1], radius(meshes[0]))
    slug = model.id.split("/", 1)[1]
    return {
        "id": model.id,
        "file": f"world/{model.kind}/{slug}.glb",
        "kind": model.kind,
        "budget": model.budget,
        "ground": model.ground,
        "site": model.site,
        "site_shape": model.site_shape,
        "scale": SCALE,
        "pivot": [0.0, 0.0, 0.0],
        "bounds": {"min": [round(v, 4) for v in lo], "max": [round(v, 4) for v in hi]},
        "radius": round(radius(meshes[0]), 4),
        "size": round(size, 4),
        "materials": materials,
        "team_mask": "team" in materials,
        "lods": [{"node": name, "tris": len(mesh.triangles)} for name, mesh in zip(LOD_NAMES, meshes)],
        "lod_px": list(catalog.LOD_PX[model.budget]),
    }


def check(entry):
    """Same rules as platform/crates/render/build/world.rs, so a bad model fails before export."""
    problems = []
    name = entry["id"]
    limits = catalog.BUDGETS[entry["budget"]]
    tris = [lod["tris"] for lod in entry["lods"]]
    for lod, t, limit in zip(LOD_NAMES, tris, limits):
        if t > limit:
            problems.append(f"{name} {lod}: {t} tris > {limit} ({entry['budget']})")
    if not tris[0] >= tris[1] >= tris[2] > 0:
        problems.append(f"{name}: LOD triangle counts must be positive and not grow {tris}")
    max_r, max_h, low = catalog.SHAPES[entry["budget"]]
    if entry["radius"] > max_r:
        problems.append(f"{name}: footprint radius {entry['radius']:.3f} > {max_r}")
    top, bottom = entry["bounds"]["max"][1], entry["bounds"]["min"][1]
    if top > max_h:
        problems.append(f"{name}: height {top:.3f} > {max_h}")
    if bottom < low:
        problems.append(f"{name}: lowest point {bottom:.3f} < {low}")
    if entry["ground"] == "water":
        if not bottom < 0.0 < top:
            problems.append(f"{name}: water model must cross the waterline ({bottom:.3f}..{top:.3f})")
    elif bottom > 0.02:
        problems.append(f"{name}: floats, lowest point {bottom:.3f} (must reach the ground)")
    if entry["ground"] not in catalog.GROUNDS:
        problems.append(f"{name}: ground {entry['ground']} not in {catalog.GROUNDS}")
    if entry["site_shape"] not in catalog.SITE_SHAPES:
        problems.append(f"{name}: site_shape {entry['site_shape']} not in {catalog.SITE_SHAPES}")
    return problems


def lod_bounds(entry, lods):
    """Reduced LODs stay within LOD0 bounds + 0.05 + 10% of the model size (as build.rs)."""
    lo, hi = entry["bounds"]["min"], entry["bounds"]["max"]
    tolerance = 0.05 + 0.1 * entry["size"]
    problems = []
    for name, mesh in zip(LOD_NAMES[1:], lods[1:]):
        l, h = bounds(mesh)
        if any(l[k] < lo[k] - tolerance or h[k] > hi[k] + tolerance for k in range(3)):
            problems.append(f"{entry['id']} {name}: bounds leave LOD0 bounds by more than {tolerance:.3f}")
    return problems


def kit_problems(kit, by_id, landmarks):
    """The city kit's pieces exist with the right budget; `landmarks`: building key -> entry of the
    part(s) being checked."""
    problems = []

    def model(model_id, kind):
        if model_id not in by_id:
            problems.append(f"city kit: {model_id} is not a model")
        elif by_id[model_id].budget != kind:
            problems.append(f"city kit: {model_id} must use budget {kind}")

    for style in kit["styles"]:
        model(kit["centers"][style], "city-center")
        for house in kit["houses"][style]:
            model(house, "city-house")
    for building, piece in landmarks.items():
        model(piece["model"], "city-walls" if piece["slot"] == "ring" else "city-landmark")
    if len(kit["slots"]) < kit["houses_by_population"][-1][1]:
        problems.append("city kit: fewer slots than the largest house count")
    return problems


def city_worst_case(kit, entries, landmarks):
    """Largest LOD0 triangle count of one city tile, per style. `landmarks`: the building keys of
    the part(s) being checked."""
    tris = {e["id"]: e["lods"][0]["tris"] for e in entries}
    rings = [tris[p["model"]] for p in landmarks.values() if p["slot"] == "ring"]
    slotted = sorted((tris[p["model"]] for p in landmarks.values() if p["slot"] != "ring"), reverse=True)
    max_houses = kit["houses_by_population"][-1][1]
    worst = {}
    for style in kit["styles"]:
        house = max(tris[h] for h in kit["houses"][style])
        best = 0
        for landmarks in range(kit["max_landmarks"] + 1):
            houses = min(max_houses, len(kit["slots"]) - landmarks)
            total = tris[kit["centers"][style]] + sum(rings) + sum(slotted[:landmarks]) + houses * house
            best = max(best, total)
        worst[style] = best
    return worst


def manifest_section(part, entries, cat, kit):
    """The "world" key of the part's manifest (see manifest_io for the order of its keys)."""
    if part == parts.PLATFORM:
        return {
            "world": {
                "generator": "platform/tools/blender/export_world.py",
                "note": "Generated. Edit platform/tools/blender/world/*.py and run sh platform/tools/blender/export_all.sh.",
                "axes": "glTF y-up, hex units; front faces +z (the camera); pivot = tile anchor on the ground",
                "budgets": {name: list(v) for name, v in sorted(catalog.BUDGETS.items())},
                "shapes": {name: list(v) for name, v in sorted(catalog.SHAPES.items())},
                "city_tile_lod0_max": catalog.CITY_TILE_LOD0_MAX,
                "models": entries,
                "city_kit": kit,
            }
        }
    return {
        "world": {
            "models": entries,
            "keys": dict(sorted(cat.keys.items())),
            "icon_only": dict(sorted(cat.icon_only.items())),
            "no_map_presence": dict(sorted(cat.no_map_presence.items())),
            "city_kit": {"landmarks": cat.landmarks},
        }
    }


def summary_line(entry):
    tris = "/".join(str(lod["tris"]) for lod in entry["lods"])
    return (f"{entry['id']:<44} {entry['budget']:<14} {entry['ground']:<7} tris {tris:<14} "
            f"h {entry['bounds']['max'][1]:.2f} r {entry['radius']:.2f}")


def build_entries(models, cache=None):
    """{model id: (three LOD meshes, manifest entry)} for `models`, reusing `cache` (same shape)."""
    cache = {} if cache is None else cache
    for model in models:
        if model.id not in cache:
            lods = [build_lod(model, d) for d in range(3)]
            cache[model.id] = (lods, manifest_entry(model, lods))
    return cache


def build_part(cats, part, only=None, module=None, cache=None):
    """(models, meshes per model id, entries, problems) of the part's own world models straight from the
    catalog. The checks run on the part together with the platform models it can see (the platform alone
    on its own). `only`: id substrings to keep; `module`: build just that authoring module of the part
    (no scope checks), so one module can be iterated while another is mid-edit. `cache`: see build_entries,
    shared between parts so the platform is built once."""
    cache = {} if cache is None else cache
    cat = cats.part[part]
    if module:
        mod = parts.load(part, f"world/{module}") if part != parts.PLATFORM else __import__(module)
        models = sorted(mod.models(), key=lambda m: m.id)
    else:
        models = cat.world
    chosen = [m for m in models if any(o in m.id for o in only)] if only else models
    build_entries(chosen, cache)
    meshes, entries, problems = {}, [], []
    for model in chosen:
        lods, entry = cache[model.id]
        problems += check(entry)
        problems += lod_bounds(entry, lods)
        meshes[model.id] = lods
        entries.append(entry)
    if module or only:
        return chosen, meshes, entries, problems
    visible = cats.visible_world_models(part)
    build_entries(visible, cache)
    visible_entries = [cache[m.id][1] for m in visible]
    by_id = {m.id: m for m in visible}
    kit = catalogs.platform_kit()
    landmarks = cats.visible_landmarks(part)
    problems += kit_problems(kit, by_id, landmarks)
    extra = getattr(cat.mod, "world_problems", None)
    if extra is not None:
        problems += extra(by_id, cat.keys, cat.landmarks)
    for style, total in city_worst_case(kit, visible_entries, landmarks).items():
        if total > catalog.CITY_TILE_LOD0_MAX:
            problems.append(f"city tile ({style}) worst case {total} tris > {catalog.CITY_TILE_LOD0_MAX}")
    return chosen, meshes, entries, problems


if __name__ == "__main__":
    args = sys.argv[1:]
    only = parts.positional(args)
    module = next((a.split("=", 1)[1] for a in args if a.startswith("--module=")), None)
    cats = catalogs.Catalogs()
    catalogs.require_ownership(cats)
    cache = {}
    for part_name in parts.selected(args):
        print(f"== {part_name}")
        _, _, entries, problems = build_part(cats, part_name, only or None, module, cache)
        for e in entries:
            print(summary_line(e))
        if not only and not module:
            visible = cats.visible_world_models(part_name)
            print("city worst case:", city_worst_case(catalogs.platform_kit(), [cache[m.id][1] for m in visible],
                                                      cats.visible_landmarks(part_name)))
        print("\n".join(problems) if problems else "OK")
