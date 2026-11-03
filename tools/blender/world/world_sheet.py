"""Contact sheets of the world models of each part: the model on its tile at game scale, LOD0 under
four lights, LOD1 and the far marker; plus whole city tiles per era and size.

    python3 platform/tools/blender/world/world_sheet.py [--part NAME ...]       # from the exported GLBs
    python3 platform/tools/blender/world/world_sheet.py --catalog               # straight from the catalogs
    python3 platform/tools/blender/world/world_sheet.py --catalog --part classic farm  # ids containing "farm"
    python3 platform/tools/blender/world/world_sheet.py --part classic --module=wonders_early --tag=early
        # one authoring module of the part only (others may be mid-edit); writes ...-<group>-early.png

Default: every part present. Writes output/world-contact-<part>-<group>.png (output/ at the repository
root). A part's sheets show the part's own models; its city tiles are composed of the platform's centers
and houses and the landmarks of the part. Needs numpy (Blender's Python has it).
"""
import json
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[0]))
import parts  # noqa: E402

parts.use_library()
import catalogs  # noqa: E402
import geom  # noqa: E402
import raster  # noqa: E402
import world_assemble as assemble  # noqa: E402

OUT = parts.REPO / "output"
CELL = 150
GAME_CELL = 210
LABEL_W = 250
GAME_PX_PER_UNIT = 53.0  # default game zoom at 1080p (contact_sheet.py)

GROUPS = [
    ("improvements", lambda e: e["kind"] == "improvement"),
    ("resources", lambda e: e["kind"] == "resource"),
    ("natural-wonders", lambda e: e["kind"] == "natural-wonder"),
    ("wonders-1", lambda e: e["kind"] == "wonder" and e["id"] < "wonder/m"),
    ("wonders-2", lambda e: e["kind"] == "wonder" and e["id"] >= "wonder/m"),
    ("city-pieces", lambda e: e["kind"] == "city"),
]
GROUND = {"water": (0.25, 0.51, 0.63), "land": (0.46, 0.60, 0.30)}


# Sources --------------------------------------------------------------------------------
def mesh_arrays(mesh):
    pts = np.array([assemble.to_game(p) for p in mesh.points])
    tri = np.array(mesh.triangles).reshape(-1, 3)
    colors = np.array([geom.PALETTE[mesh.colors[t[0]]] for t in tri])
    team = np.array([1.0 if mesh.colors[t[0]] == "team" else 0.0 for t in tri])
    return pts[tri], colors, team


def from_catalog(cats, part, only, module=None, cache=None):
    """(own entries, lods of the own and the platform's models, platform entries, problems)."""
    cache = {} if cache is None else cache
    _, meshes, entries, problems = assemble.build_part(cats, part, only or None, module, cache)
    platform = []
    if part != parts.PLATFORM and not only and not module:
        assemble.build_entries(cats.part[parts.PLATFORM].world, cache)
        platform = [cache[m.id][1] for m in cats.part[parts.PLATFORM].world]
    lods = {mid: [mesh_arrays(m) for m in lod] for mid, lod in meshes.items()}
    for entry in platform:
        lods[entry["id"]] = [mesh_arrays(m) for m in cache[entry["id"]][0]]
    return entries, lods, platform, problems


def from_glb(part, only):
    from contact_sheet import read_glb
    lods = {}

    def load(p):
        manifest = json.loads(parts.manifest_path(p).read_text())["world"]
        chosen = [e for e in manifest["models"] if not only or any(o in e["id"] for o in only)]
        for entry in chosen:
            nodes = read_glb(parts.models_dir(p) / entry["file"])
            lods[entry["id"]] = [nodes[lod["node"]] for lod in entry["lods"]]
        return chosen

    entries = load(part)
    platform = load(parts.PLATFORM) if part != parts.PLATFORM and not only else []
    return entries, lods, platform, []


# Rendering -----------------------------------------------------------------------------------
def hex_ground(color, y=-0.004, r=1.0):
    corners = [(r * math.cos(math.radians(30 + 60 * k)), y, r * math.sin(math.radians(30 + 60 * k)))
               for k in range(6)]
    tris = np.array([[(0.0, y, 0.0), corners[k], corners[(k + 1) % 6]] for k in range(6)])
    # Wind upward (normal +y) for the rasterizer's flat shading.
    tris = tris[:, [0, 2, 1]]
    return tris, np.tile(np.array(color), (6, 1)), np.zeros(6)


def combine(parts):
    return (np.concatenate([p[0] for p in parts]), np.concatenate([p[1] for p in parts]),
            np.concatenate([p[2] for p in parts]))


def place(arrays, x, z, yaw_deg=0.0):
    tris, colors, team = arrays
    a = math.radians(yaw_deg)
    c, s = math.cos(a), math.sin(a)
    out = tris.copy()
    out[..., 0] = tris[..., 0] * c - tris[..., 2] * s + x
    out[..., 2] = tris[..., 0] * s + tris[..., 2] * c + z
    return out, colors, team


def closeup(arrays, light):
    tris = arrays[0]
    lo, hi = tris.reshape(-1, 3).min(0), tris.reshape(-1, 3).max(0)
    center = (lo + hi) / 2
    half = max(hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]) * 0.62 + 1e-3
    view = raster.View(center, half, (CELL, CELL))
    return raster.render(*arrays, view, raster.LIGHTS[light])


def in_game(entry, arrays, size=GAME_CELL, px=GAME_PX_PER_UNIT):
    ground = GROUND["water" if entry.get("ground") == "water" or entry.get("site") == "water" else "land"]
    scene = combine([hex_ground(ground), arrays])
    half = size / px / 2
    view = raster.View((0.0, 0.35, 0.0), half, (size, size))
    return raster.render(*scene, view, raster.LIGHTS["game"], supersample=3)


def label_cell(lines, height):
    label = np.full((height, LABEL_W, 3), 236, np.uint8)
    for i, text in enumerate(lines):
        raster.draw_text(label, 8, 10 + 20 * i, text[:30], size=2)
    return label


def pad(img, height):
    if img.shape[0] == height:
        return img
    out = np.full((height, img.shape[1], 3), 236, np.uint8)
    top = (height - img.shape[0]) // 2
    out[top:top + img.shape[0]] = img
    return out


def row(entry, lods):
    height = max(CELL, GAME_CELL)
    tris = "/".join(str(lod["tris"]) for lod in entry["lods"])
    lines = [entry["id"].split("/", 1)[1], f"TRIS {tris}", f"{entry['budget']} {entry['ground']}",
             f"H {entry['bounds']['max'][1]:.2f} R {entry['radius']:.2f}"]
    cells = [label_cell(lines, height), pad(in_game(entry, lods[0]), height)]
    for light in raster.LIGHTS:
        cells.append(pad(closeup(lods[0], light), height))
    cells.append(pad(closeup(lods[1], "game"), height))
    cells.append(pad(closeup(lods[2], "game"), height))
    line = np.concatenate(cells, axis=1)
    line[-1:, :] = 120
    return line


def header(width, titles, xs):
    bar = np.full((28, width, 3), 210, np.uint8)
    for x, title in zip(xs, titles):
        raster.draw_text(bar, x, 8, title, size=2)
    return bar


def model_sheet(part, group, entries, lods, tag=""):
    rows = [row(e, lods[e["id"]]) for e in entries]
    titles = ["MODEL", "ON ITS TILE", "LOD0 GAME SUN", "RIGHT", "BACK", "LOW", "LOD1", "MARKER"]
    xs = [8, LABEL_W + 8] + [LABEL_W + GAME_CELL + 8 + CELL * i for i in range(6)]
    sheet = np.concatenate([header(rows[0].shape[1], titles, xs)] + rows, axis=0)
    path = OUT / f"world-contact-{part}-{group}{tag}.png"
    raster.write_png(path, sheet)
    return path


# Cities ---------------------------------------------------------------------------------------
CITY_CASES = [
    ("POP 2", 2, []),
    ("POP 6 WALLS", 6, ["core:building/walls", "core:building/monument"]),
    ("POP 12", 12, ["core:building/walls", "core:building/castle", "core:building/temple",
                    "core:building/harbor"]),
    ("POP 20 CAPITAL", 20, ["core:building/palace", "core:building/walls", "core:building/castle",
                            "core:building/factory", "core:building/stadium", "core:building/university"]),
]


def city_cases(landmarks):
    """The cases above when the landmarks are classic's; otherwise the same sizes with the first
    landmarks (by priority) of the part."""
    if all(k in landmarks for _, _, buildings in CITY_CASES for k in buildings):
        return CITY_CASES
    by_priority = [k for k, _ in sorted(landmarks.items(), key=lambda kv: (kv[1]["priority"], kv[0]))]
    return [(title, population, by_priority[:count])
            for (title, population, _), count in zip(CITY_CASES, (0, 2, 4, 6))]


def city_sheet(part, entries, lods, landmarks, tag=""):
    import city_kit
    present = {e["id"] for e in entries}
    rows = []
    size = 260
    for era, style in enumerate(city_kit.KIT["styles"]):
        cells = [label_cell([style.upper()], size)]
        for title, population, buildings in city_cases(landmarks):
            pieces = city_kit.compose(era, population, buildings, water_dirs=(2,), seed=era * 31 + population,
                                      landmarks=landmarks)
            parts = [hex_ground(GROUND["land"])]
            for model_id, x, z, yaw in pieces:
                if model_id in present:
                    parts.append(place(lods[model_id][0], x, z, yaw))
            scene = combine(parts)
            view = raster.View((0.0, 0.3, 0.0), size / 120.0 / 2, (size, size))
            img = raster.render(*scene, view, raster.LIGHTS["game"], supersample=2)
            raster.draw_text(img, 6, 6, title, size=2)
            tris = sum(e["lods"][0]["tris"] for e in entries for p in pieces if p[0] == e["id"])
            raster.draw_text(img, 6, size - 16, f"{tris} TRIS", size=2)
            cells.append(img)
        line = np.concatenate(cells, axis=1)
        line[-1:, :] = 120
        rows.append(line)
    path = OUT / f"world-contact-{part}-cities{tag}.png"
    raster.write_png(path, np.concatenate(rows, axis=0))
    return path


def main(argv):
    use_catalog = "--catalog" in argv or any(a.startswith("--module=") for a in argv)
    module = next((a.split("=", 1)[1] for a in argv if a.startswith("--module=")), None)
    tag = next(("-" + a.split("=", 1)[1] for a in argv if a.startswith("--tag=")), "")
    only = parts.positional(argv)
    cats = catalogs.Catalogs()
    OUT.mkdir(exist_ok=True)
    written, cache = [], {}
    for part in parts.selected(argv):
        if use_catalog:
            entries, lods, platform, problems = from_catalog(cats, part, only, module, cache)
        else:
            entries, lods, platform, problems = from_glb(part, only)
        for group, accept in GROUPS:
            chosen = [e for e in entries if accept(e)]
            if chosen:
                written.append(model_sheet(part, group, chosen, lods, tag))
                print("wrote", written[-1], len(chosen), "models")
        landmarks = cats.visible_landmarks(part)
        if not only and any(e["kind"] == "city" for e in entries + platform):
            written.append(city_sheet(part, entries + platform, lods, landmarks, tag))
            print("wrote", written[-1])
        if problems:
            print("\n".join(problems))
    return written


if __name__ == "__main__":
    main(parts.cli_args())
