"""Export the terrain props (sea stacks) to GLB and the "terrain" key of the platform's manifest.

    blender -b --factory-startup -P platform/tools/blender/export_terrain.py [-- --part NAME ...]
    # or: sh platform/tools/blender/export_all.sh

Terrain props are generic models, so only the platform has them: one GLB per variant in
platform/assets/models/terrain/sea-stack-<n>.glb with nodes LOD0 / LOD1 / LOD2, and the "terrain" section
of platform/assets/models/manifest.json. Other parts are skipped. platform/crates/render/build/terrain.rs validates
the section (budget, footprint, height, grounding, palette) and bakes LOD0 for the coast props
(gpu/coast_props.rs). Deterministic: the same script produces the same files.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parts  # noqa: E402

parts.use_library()
import manifest_io  # noqa: E402
import model_export  # noqa: E402
import terrain_assemble as assemble  # noqa: E402


def export_platform():
    models_dir = parts.models_dir(parts.PLATFORM)
    entries, problems = [], []
    for lods, entry in assemble.build():
        problems += assemble.entry_problems(entry)
        model_export.export_lods(models_dir / entry["file"], lods)
        print(entry["id"], [lod["tris"] for lod in entry["lods"]], "radius", entry["radius"])
        entries.append(entry)
    model_export.remove_stale(models_dir / "terrain", {models_dir / e["file"] for e in entries})
    manifest_io.write(parts.PLATFORM, assemble.manifest_section(entries))
    print(f"TERRAIN_MODELS {len(entries)} models")
    return problems


def main(argv):
    problems = []
    for part in parts.selected(argv):
        if part == parts.PLATFORM:
            problems += export_platform()
        else:
            print(f"{part}: no terrain props (they are generic models, the platform's)")
    if problems:
        print("\n".join(problems))
        raise SystemExit("terrain model checks failed")


if __name__ == "__main__":
    main(parts.cli_args())
