"""The manifest of one part (<part>/assets/models/manifest.json): its layout and how the exporters write
it. No bpy, so check_catalogs.py can build the same bytes without Blender.

Every exporter owns some keys of the manifest and rewrites only those, so units, world objects and
terrain props can be regenerated independently. The keys are written in a fixed order and with
json.dumps(indent=1) + a newline, so a regeneration that changes nothing changes no byte.

    platform   schema owner generator note axes fallback palette budgets formation_lod0_max models
               world{generator note axes budgets shapes city_tile_lod0_max models city_kit}
               terrain{generator note axes budgets shapes models}
    module     schema owner key_namespace generator note models classes units fallbacks
               world{models keys icon_only no_map_presence city_kit{landmarks}}
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parts  # noqa: E402

parts.use_library()
import geom  # noqa: E402

SCHEMA = "aurelia-models/2"

ORDER = {
    parts.PLATFORM: [
        "schema", "owner", "generator", "note", "axes", "fallback", "palette", "budgets", "formation_lod0_max",
        "models", "world", "terrain",
    ],
    "module": [
        "schema", "owner", "key_namespace", "generator", "note", "models", "classes", "units", "fallbacks", "world",
    ],
}
WORLD_ORDER = {
    parts.PLATFORM: ["generator", "note", "axes", "budgets", "shapes", "city_tile_lod0_max", "models", "city_kit"],
    "module": ["models", "keys", "icon_only", "no_map_presence", "city_kit"],
}
KIT_ORDER = ["styles", "centers", "houses", "houses_by_population", "older_style_share", "max_landmarks", "slots",
             "landmarks"]
TERRAIN_ORDER = ["generator", "note", "axes", "budgets", "shapes", "models"]


def ordered(data, order):
    """`data` with the keys in `order` first, any other keys after them in their own order."""
    out = {k: data[k] for k in order if k in data}
    out.update({k: v for k, v in data.items() if k not in out})
    return out


def layout(part, data):
    kind = parts.PLATFORM if part == parts.PLATFORM else "module"
    out = ordered(data, ORDER[kind])
    if isinstance(out.get("world"), dict):
        world = ordered(out["world"], WORLD_ORDER[kind])
        if isinstance(world.get("city_kit"), dict):
            world["city_kit"] = ordered(world["city_kit"], KIT_ORDER)
        out["world"] = world
    if isinstance(out.get("terrain"), dict):
        out["terrain"] = ordered(out["terrain"], TERRAIN_ORDER)
    return out


def merged(part, existing, sections):
    """The manifest after replacing the top-level keys in `sections` (a dict, or several to apply in order)."""
    data = dict(existing)
    for section in sections:
        data.update(section)
    data["schema"] = SCHEMA
    if part == parts.PLATFORM:
        data["palette"] = {name: list(rgb) for name, rgb in sorted(geom.PALETTE.items())}
    return layout(part, data)


def text(manifest):
    return json.dumps(manifest, indent=1) + "\n"


def write(part, *sections):
    """Replaces the keys of `sections` in the part's manifest and keeps the others."""
    path = parts.manifest_path(part)
    existing = json.loads(path.read_text()) if path.exists() else {}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text(merged(part, existing, sections)))
    return path
