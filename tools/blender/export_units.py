"""Export the unit models of the parts (all Classic unit classes + unique variants, the fallback) to GLB
and the unit keys of each part's manifest.

    blender -b --factory-startup -P platform/tools/blender/export_units.py [-- --part NAME ...]
    # or: sh platform/tools/blender/export_all.sh [--part NAME]    (the whole set, or one part)
    #     sh classic/tools/blender/export_all.sh                   (just classic)

A part writes only its own models, into its own folder, and only its own manifest:

    platform  the shared and generic unit models -> platform/assets/models/units/*.glb, manifest.json
              (budgets, formation limit, fallback)
    classic   the models only classic draws -> classic/assets/models/units/*.glb, manifest.json (models,
              classes, units, fallbacks of its "core:" keys)
    lands     no models; lands/assets/models/manifest.json gets its "lal:" unit keys

One GLB per model, holding three mesh nodes LOD0 / LOD1 / LOD2. Vertex color: RGB = palette albedo,
A = team mask. The manifests are validated and baked by platform/crates/render/build.rs. Stale GLBs are removed in
the part's own units folder only. Deterministic: the same catalogs produce the same files.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parts  # noqa: E402

parts.use_library()
import assemble  # noqa: E402
import catalogs  # noqa: E402
import manifest_io  # noqa: E402
import model_export  # noqa: E402


def export_part(cats, part):
    cat = cats.part[part]
    models_dir = parts.models_dir(part)
    entries, problems = [], []
    for model, meshes, entry in assemble.build_models(cat.unit_models()):
        model_export.export_lods(models_dir / entry["file"], meshes)
        problems += assemble.entry_problems(entry)
        entries.append(entry)
        print(assemble.summary_line(entry))
    model_export.remove_stale(models_dir / "units", {models_dir / e["file"] for e in entries})
    class_map, unit_map, fallbacks = cats.unit_maps(part)
    section = assemble.manifest_section(part, entries, class_map, unit_map, fallbacks, cat.namespace)
    manifest_io.write(part, section)
    print(f"UNIT_MODELS {part}: {len(entries)} models, {len(class_map)} classes, {len(unit_map)} units")
    return problems


def main(argv):
    cats = catalogs.Catalogs()
    catalogs.require_ownership(cats)
    problems = []
    for part in parts.selected(argv):
        problems += export_part(cats, part)
    if problems:
        print("\n".join(problems))
        raise SystemExit("unit model budgets violated")


if __name__ == "__main__":
    main(parts.cli_args())
