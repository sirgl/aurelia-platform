"""Export the world models of the parts (improvements, resources, natural wonders, world wonders, city
kit pieces) to GLB and the "world" key of each part's manifest.

    blender -b --factory-startup -P platform/tools/blender/export_world.py [-- --part NAME ...] [--only ID ...]
    # or: sh platform/tools/blender/export_all.sh [--part NAME]    (the whole set, or one part)
    #     sh classic/tools/blender/export_all.sh                   (just classic)

A part writes only its own models, into its own folder, and only its own manifest:

    platform  the shared and generic world models -> platform/assets/models/world/<kind>/<slug>.glb, the
              "world" section of platform/assets/models/manifest.json (budgets, shapes, the generic city kit)
    classic   the models only classic draws -> classic/assets/models/world/<kind>/<slug>.glb, its "world"
              section (models, keys, icon_only, no_map_presence, city kit landmarks)
    lands     no models; lands/assets/models/manifest.json gets its "lal:" keys and landmarks

One GLB per model with nodes LOD0 / LOD1 / LOD2. A part is checked together with the platform models it
can see (budgets, shapes, coverage of its documents, the worst case city tile); the platform alone on its
own. `--only ID...` (substrings of model ids) rewrites just those GLBs and their manifest entries (and the
budgets/shapes), leaving every other file untouched: use it when the installed Blender writes other bytes than the
one that made the committed files. platform/crates/render/build/world.rs validates the merged result and bakes it. Stale GLBs are removed in the
part's own world folder only. Deterministic: the same catalogs produce the same files.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parts  # noqa: E402

parts.use_library()
import catalogs  # noqa: E402
import manifest_io  # noqa: E402
import model_export  # noqa: E402
import world_assemble as assemble  # noqa: E402


def export_part(cats, part, cache, only=()):
    models_dir = parts.models_dir(part)
    _, meshes, entries, problems = assemble.build_part(cats, part, cache=cache)
    if only:
        return export_some(cats, part, entries, meshes, only, problems)
    for entry in entries:
        model_export.export_lods(models_dir / entry["file"], meshes[entry["id"]])
        print(assemble.summary_line(entry))
    expected = {models_dir / e["file"] for e in entries}
    world_dir = models_dir / "world"
    if world_dir.is_dir():
        for kind_dir in sorted(p for p in world_dir.iterdir() if p.is_dir()):
            model_export.remove_stale(kind_dir, expected)
    section = assemble.manifest_section(part, entries, cats.part[part], catalogs.platform_kit())
    manifest_io.write(part, section)
    print(f"WORLD_MODELS {part}: {len(entries)} models, {len(cats.part[part].keys)} keys")
    return problems


def export_some(cats, part, entries, meshes, only, problems):
    """Writes the models whose id contains one of `only`, replaces their entries in the manifest in place."""
    import json
    models_dir = parts.models_dir(part)
    chosen = [e for e in entries if any(o in e["id"] for o in only)]
    for entry in chosen:
        model_export.export_lods(models_dir / entry["file"], meshes[entry["id"]])
        print(assemble.summary_line(entry))
    existing = json.loads(parts.manifest_path(part).read_text())["world"]
    fresh = {e["id"]: e for e in chosen}
    existing["models"] = [fresh.pop(e["id"], e) for e in existing["models"]]
    existing["models"] += sorted(fresh.values(), key=lambda e: e["id"])
    section = assemble.manifest_section(part, entries, cats.part[part], catalogs.platform_kit())["world"]
    for key in ("budgets", "shapes"):
        if key in section:
            existing[key] = section[key]
    manifest_io.write(part, {"world": existing})
    print(f"WORLD_MODELS {part}: {len(chosen)} of {len(entries)} models written")
    return problems


def main(argv):
    cats = catalogs.Catalogs()
    catalogs.require_ownership(cats)
    cache, problems = {}, []
    only = []
    if "--only" in argv:
        i = argv.index("--only")
        only, argv = [a for a in argv[i + 1:] if not a.startswith("--")], argv[:i] + [a for a in argv[i + 1:] if a.startswith("--")]
    for part in parts.selected(argv):
        problems += export_part(cats, part, cache, only)
    if problems:
        print("\n".join(problems))
        raise SystemExit("world model checks failed")


if __name__ == "__main__":
    main(parts.cli_args())
