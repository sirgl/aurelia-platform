"""Checks the model catalogs of every part without Blender (plain python3; bpy is never imported).

    python3 -I platform/tools/blender/check_catalogs.py [--digest FILE] [--compare FILE]

    --digest FILE   write a digest of every model's three LOD meshes (points, colors, triangles, rounded to
                    1e-6) to FILE, so a refactoring of the catalogs can be proven to change no mesh
    --compare FILE  fail unless the digests equal the ones in FILE (same model ids, same digests)

What it checks, for every part present:
  1. ownership: no model id is declared twice, every model is declared in the part the ownership rule
     says (catalogs.py), every content key draws a model its part can see
  2. the catalog -> LOD meshes -> manifest entries -> manifest computation of the exporters (the same
     functions, run without writing a file) gives exactly the committed <part>/assets/models/manifest.json,
     byte for byte, apart from nothing: models, classes, units, fallbacks, world.* and landmarks included
  3. every model file a manifest names exists in that part's folder, and no GLB of the part's folders
     is left over (what remove_stale would delete)
  4. the checks the exporters run (budgets, formation, grounding, coverage of the part's documents, the
     worst case city tile), on the part together with the platform models it can see
Exit status 0 only when everything holds.
"""
import hashlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parts  # noqa: E402

parts.use_library()
import assemble  # noqa: E402
import catalogs  # noqa: E402
import manifest_io  # noqa: E402
import terrain_assemble  # noqa: E402
import world_assemble  # noqa: E402


def rounded(v):
    return round(v, 6) + 0.0


def digest(lods):
    h = hashlib.sha256()
    for mesh in lods:
        h.update(json.dumps([[[rounded(c) for c in p] for p in mesh.points], list(mesh.colors),
                             [list(t) for t in mesh.triangles], sorted(mesh.materials)]).encode())
    return h.hexdigest()


def differences(a, b, path="", limit=12):
    """First paths where two JSON values differ."""
    out = []

    def walk(x, y, where):
        if len(out) >= limit:
            return
        if type(x) is not type(y):
            out.append(f"{where}: {type(x).__name__} vs {type(y).__name__}")
        elif isinstance(x, dict):
            if list(x) != list(y) and set(x) == set(y):
                out.append(f"{where}: key order {list(x)[:6]} vs {list(y)[:6]}")
            for k in sorted(set(x) | set(y), key=str):
                if k not in x or k not in y:
                    out.append(f"{where}/{k}: only in {'computed' if k in x else 'committed'}")
                else:
                    walk(x[k], y[k], f"{where}/{k}")
        elif isinstance(x, list):
            if len(x) != len(y):
                out.append(f"{where}: {len(x)} entries vs {len(y)}")
            for i, (p, q) in enumerate(zip(x, y)):
                label = p.get("id", i) if isinstance(p, dict) else i
                walk(p, q, f"{where}[{label}]")
        elif x != y:
            out.append(f"{where}: {x!r} vs {y!r}")

    walk(a, b, path)
    return out


def main(argv):
    problems = []
    cats = catalogs.Catalogs()
    digests = {}
    summary = []

    # 1. ownership
    own = catalogs.ownership_problems(cats)
    problems += [f"ownership: {p}" for p in own]
    summary.append(f"ownership: {'OK' if not own else str(len(own)) + ' problems'} "
                   f"({', '.join(f'{n}: {len(c.unit_models())} unit + {len(c.world)} world models' for n, c in cats.part.items())})")

    cache = {}
    sections = {}
    for part in cats.names:
        cat = cats.part[part]
        # unit models
        unit_entries = []
        for model, meshes, entry in assemble.build_models(cat.unit_models()):
            digests[model.id] = digest(meshes)
            problems += [f"{part}: {p}" for p in assemble.entry_problems(entry)]
            unit_entries.append(entry)
        class_map, unit_map, fallbacks = cats.unit_maps(part)
        sections[part] = [assemble.manifest_section(part, unit_entries, class_map, unit_map, fallbacks, cat.namespace)]
        # world models
        _, meshes, entries, world_problems = world_assemble.build_part(cats, part, cache=cache)
        for model_id, lods in meshes.items():
            digests[model_id] = digest(lods)
        problems += [f"{part}: {p}" for p in world_problems]
        sections[part].append(world_assemble.manifest_section(part, entries, cat, catalogs.platform_kit()))
        # terrain props (platform only)
        if part == parts.PLATFORM:
            terrain = terrain_assemble.build()
            for lods, entry in terrain:
                digests[entry["id"]] = digest(lods)
                problems += [f"{part}: {p}" for p in terrain_assemble.entry_problems(entry)]
            sections[part].append(terrain_assemble.manifest_section([entry for _, entry in terrain]))

    # 2. the computed manifests equal the committed ones
    for part in cats.names:
        path = parts.manifest_path(part)
        committed_text = path.read_text() if path.exists() else ""
        computed = manifest_io.merged(part, {}, sections[part])
        if manifest_io.text(computed) == committed_text:
            summary.append(f"{part}: manifest equals the computed one ({len(committed_text)} bytes)")
            continue
        committed = json.loads(committed_text) if committed_text else {}
        diff = differences(computed, committed)
        problems += [f"{part}: manifest differs from the catalog: {d}" for d in diff] or \
            [f"{part}: manifest differs from the catalog only in its text layout"]

    # 3. files of the manifests
    expected_ids = {}
    for part in cats.names:
        path = parts.manifest_path(part)
        if not path.exists():
            problems.append(f"{part}: no manifest {path}")
            continue
        manifest = json.loads(path.read_text())
        models_dir = parts.models_dir(part)
        listed = [e["file"] for e in manifest.get("models", [])] + \
                 [e["file"] for e in manifest.get("world", {}).get("models", [])] + \
                 [e["file"] for e in manifest.get("terrain", {}).get("models", [])]
        for e in manifest.get("models", []) + manifest.get("world", {}).get("models", []) + \
                manifest.get("terrain", {}).get("models", []):
            if e["id"] in expected_ids:
                problems.append(f"model {e['id']} is in the manifests of {expected_ids[e['id']]} and {part}")
            expected_ids[e["id"]] = part
        for f in listed:
            if not (models_dir / f).is_file():
                problems.append(f"{part}: {f} is in the manifest but not in {models_dir}")
        # The folders the exporters own: units/, world/<kind>/ and terrain/ (the legacy props beside them
        # are not generated here).
        strays = [g for sub in ("units", "world", "terrain") for g in sorted((models_dir / sub).glob("**/*.glb"))]
        for stray in strays:
            if str(stray.relative_to(models_dir)) not in listed and not stray.name.startswith("._"):
                problems.append(f"{part}: {stray.relative_to(models_dir)} is in no manifest of the part")
    summary.append(f"files: {len(expected_ids)} models, each in the manifest of one part only")

    # digests
    ids = sorted(digests)
    summary.append(f"digests: {len(ids)} models")
    for i, arg in enumerate(argv):
        if arg == "--digest" and i + 1 < len(argv):
            Path(argv[i + 1]).write_text(json.dumps({k: digests[k] for k in ids}, indent=1, sort_keys=True) + "\n")
            summary.append(f"digests written to {argv[i + 1]}")
        if arg == "--compare" and i + 1 < len(argv):
            before = json.loads(Path(argv[i + 1]).read_text())
            missing = sorted(set(before) - set(digests))
            added = sorted(set(digests) - set(before))
            changed = sorted(k for k in set(before) & set(digests) if before[k] != digests[k])
            problems += [f"digest: {k} is gone" for k in missing] + [f"digest: {k} is new" for k in added]
            problems += [f"digest: {k} changed" for k in changed]
            summary.append(f"digests vs {argv[i + 1]}: {len(before)} before, {len(digests)} now, "
                           f"{len(missing)} gone, {len(added)} new, {len(changed)} changed")

    print("\n".join(summary))
    if problems:
        print("\n".join(problems))
        print(f"FAILED: {len(problems)} problems")
        return 1
    print("OK")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
