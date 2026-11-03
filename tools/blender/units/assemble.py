"""Unit catalogs -> LOD meshes, budget checks and the manifest entries and section of one part. No bpy.

Used by export_units.py (inside Blender), contact_sheet.py (plain python3 or Blender) and
check_catalogs.py. The catalogs of the parts are declared in <part>/tools/blender/units/catalog.py
(platform: shared and generic models, classic: the models only classic draws) and reached through
catalogs.py; the models of a part are written to <part>/assets/models/units/ and its manifest.json.
"""
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import parts  # noqa: E402

parts.use_library()
import catalog  # noqa: E402
import rigs  # noqa: E402
from geom import Mesh, scale  # noqa: E402

LOD_NAMES = ("LOD0", "LOD1", "LOD2")


def build_lod(model, detail):
    """Mesh for one LOD in model units (z up). The far marker of a formation is boosted."""
    mesh = Mesh(detail)
    if detail == 2 and model.figures > 1:
        with mesh.at(scale(catalog.MARKER_BOOST)):
            model.build(mesh)
    else:
        model.build(mesh)
    return mesh


def to_game(point, k):
    """Blender z-up model units -> glTF/game y-up hex units (what the bake sees)."""
    return (point[0] * k, point[2] * k, -point[1] * k)


def bounds(meshes, k):
    pts = [to_game(p, k) for mesh in meshes for p in mesh.points]
    lo = [min(p[i] for p in pts) for i in range(3)]
    hi = [max(p[i] for p in pts) for i in range(3)]
    return lo, hi


def manifest_entry(model, meshes):
    k = model.scale
    lo, hi = bounds(meshes[:1], k)
    materials = sorted(set().union(*(mesh.materials for mesh in meshes)))
    slots = [list(s) for s in catalog.SLOTS[model.figures]]
    return {
        "id": model.id,
        "file": "units/" + model.id.split("/", 1)[1] + ".glb",
        "category": model.category,
        "budget": model.budget,
        "variant_of": model.variant_of or None,
        "contact": model.contact,
        "scale": k,
        "pivot": [0.0, 0.0, 0.0],
        "bounds": {"min": [round(v, 4) for v in lo], "max": [round(v, 4) for v in hi]},
        "materials": materials,
        "team_mask": "team" in materials,
        "lods": [{"node": name, "tris": len(mesh.triangles)} for name, mesh in zip(LOD_NAMES, meshes)],
        "lod_px": list(catalog.LOD_PX),
        "formation": {"figures": model.figures, "slots": slots},
    }


def manifest_section(part, entries, class_map, unit_map, fallbacks, namespace=None):
    """The keys of part's manifest that the unit exporter writes (see manifest_io for their order)."""
    if part == parts.PLATFORM:
        return {
            "owner": part,
            "generator": "platform/tools/blender/export_units.py",
            "note": ("Generated. The shared and generic models of the platform; the models of a module are in "
                     "<module>/assets/models/manifest.json (docs/models-ownership.md). Edit "
                     "platform/tools/blender/units/catalog.py and run sh platform/tools/blender/export_all.sh."),
            "axes": "glTF y-up, hex units after `scale`; models face +z (the camera), pivot = ground contact",
            "fallback": "unit/unknown",
            "budgets": {name: list(v) for name, v in sorted(catalog.BUDGETS.items())},
            "formation_lod0_max": catalog.FORMATION_LOD0_MAX,
            "models": entries,
        }
    return {
        "owner": part,
        "key_namespace": namespace,
        "generator": f"platform/tools/blender/export_units.py --part {part}",
        "note": (f"Generated. The models only the {part} module draws, and its content keys. Edit the "
                 f"catalogs in {part}/tools/blender/ and run sh {part}/tools/blender/export_all.sh."),
        "models": entries,
        "classes": dict(sorted(class_map.items())),
        "units": dict(sorted(unit_map.items())),
        "fallbacks": dict(sorted(fallbacks.items())),
    }


def check_budgets(entry):
    """Same rules as build.rs, so a bad model fails before Blender exports it."""
    limits = catalog.BUDGETS[entry["budget"]]
    tris = [lod["tris"] for lod in entry["lods"]]
    problems = []
    for name, t, limit in zip(LOD_NAMES, tris, limits):
        if t > limit:
            problems.append(f"{entry['id']} {name}: {t} tris > {limit}")
    if not tris[0] >= tris[1] >= tris[2]:
        problems.append(f"{entry['id']}: LOD triangle counts must not grow {tris}")
    if tris[0] * entry["formation"]["figures"] > catalog.FORMATION_LOD0_MAX:
        problems.append(f"{entry['id']}: formation {tris[0]} x {entry['formation']['figures']} "
                        f"> {catalog.FORMATION_LOD0_MAX}")
    return problems


def contact_ok(entry):
    """Same grounding rule as build.rs: feet on 0, hull across the waterline, hover above."""
    low, high = entry["bounds"]["min"][1], entry["bounds"]["max"][1]
    if entry["contact"] == "ground":
        return -0.01 <= low <= 0.02
    if entry["contact"] == "water":
        return -0.15 < low < 0.0 < high
    return low >= 0.02


def footprint(entry):
    lo, hi = entry["bounds"]["min"], entry["bounds"]["max"]
    return max(abs(lo[0]), abs(hi[0]), abs(lo[2]), abs(hi[2]))


def summary_line(entry):
    tris = "/".join(str(lod["tris"]) for lod in entry["lods"])
    figures = entry["formation"]["figures"]
    return (f"{entry['id']:<32} {entry['budget']:<9} x{figures} tris {tris:<13} "
            f"formation {entry['lods'][0]['tris'] * figures:>5}  h {entry['bounds']['max'][1]:.2f} "
            f"r {footprint(entry):.2f}")


def radius_ok(entry):
    """Every figure of the formation stays inside the hex (inner radius 0.866)."""
    r = footprint(entry)
    for x, z, s in entry["formation"]["slots"]:
        if math.hypot(x, z) + r * s > 0.86:
            return False
    return True


def entry_problems(entry):
    """Every per-model rule: budgets, formation inside the hex, grounding."""
    problems = check_budgets(entry)
    if not radius_ok(entry):
        problems.append(f"{entry['id']}: formation leaves the hex")
    if not contact_ok(entry):
        problems.append(f"{entry['id']}: not on its {entry['contact']} contact {entry['bounds']}")
    return problems


def build_models(models):
    """[(model, three LOD meshes, manifest entry)] straight from the catalog. The entry carries the
    animation rig, weapon and baked yaw of rigs.py, as in the committed manifests."""
    out = []
    for model in models:
        meshes = [build_lod(model, detail) for detail in range(3)]
        out.append((model, meshes, rigs.apply(manifest_entry(model, meshes), model)))
    return out
