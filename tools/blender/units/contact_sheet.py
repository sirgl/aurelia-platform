"""Contact sheets of the unit models of each part: formation at game scale, LOD0 under four lights,
LOD1 and the far marker. Reads the exported GLBs + manifest (what build.rs bakes).

    python3 platform/tools/blender/units/contact_sheet.py [--part NAME ...]
        # from <part>/assets/models (default: every part present)
    python3 platform/tools/blender/units/contact_sheet.py --catalog [--part NAME ...]
        # straight from the catalogs, no export
    python3 platform/tools/blender/units/contact_sheet.py --part classic warrior  # only ids containing "warrior"

Writes output/models-contact-<part>-<group>.png (output/ at the repository root). A part's sheets show
the part's own models. Needs numpy (Blender's Python has it).
"""
import json
import struct
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import parts  # noqa: E402

parts.use_library()
import assemble  # noqa: E402
import catalogs  # noqa: E402
import geom  # noqa: E402
import raster  # noqa: E402

OUT = parts.REPO / "output"
CELL = 150
FORMATION_CELL = 190
LABEL_W = 250
GAME_PX_PER_UNIT = 53.0  # default game zoom at 1080p (camera.rs distance 42 / zoom 2.25, 45 degrees)

GROUPS = [
    ("foot", lambda e: e["budget"] == "figure" and e["category"] == "military"),
    ("mounted", lambda e: e["budget"] == "mounted"),
    ("machines", lambda e: e["budget"] in ("siege", "vehicle")),
    ("naval", lambda e: e["contact"] == "water"),
    ("air", lambda e: e["category"] == "air"),
    ("civil", lambda e: e["budget"] in ("civilian", "great")),
]


# Sources ------------------------------------------------------------------------------
def from_catalog(cats, part):
    entries, lods = [], {}
    for model, meshes, entry in assemble.build_models(cats.part[part].unit_models()):
        entries.append(entry)
        lods[model.id] = [mesh_arrays(mesh, model.scale) for mesh in meshes]
    return entries, lods


def mesh_arrays(mesh, k):
    pts = np.array([assemble.to_game(p, k) for p in mesh.points])
    tri = np.array(mesh.triangles)
    colors = np.array([geom.PALETTE[mesh.colors[t[0]]] for t in mesh.triangles])
    team = np.array([1.0 if mesh.colors[t[0]] == "team" else 0.0 for t in mesh.triangles])
    return pts[tri], colors, team


def from_glb(part):
    manifest = json.loads(parts.manifest_path(part).read_text())
    lods = {}
    for entry in manifest["models"]:
        nodes = read_glb(parts.models_dir(part) / entry["file"])
        k = entry["scale"]
        pivot = np.array(entry["pivot"])
        lods[entry["id"]] = []
        for lod in entry["lods"]:
            tris, colors, team = nodes[lod["node"]]
            lods[entry["id"]].append(((tris - pivot) * k, colors, team))
    return manifest["models"], lods


def read_glb(path):
    data = path.read_bytes()
    length = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20:20 + length])
    blob = data[20 + length + 8:]

    def accessor(index):
        acc = doc["accessors"][index]
        view = doc["bufferViews"][acc["bufferView"]]
        count = acc["count"]
        width = {"SCALAR": 1, "VEC3": 3, "VEC4": 4}[acc["type"]]
        dtype = {5126: np.float32, 5123: np.uint16, 5121: np.uint8, 5125: np.uint32}[acc["componentType"]]
        start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
        arr = np.frombuffer(blob, dtype=dtype, count=count * width, offset=start).reshape(count, width)
        if acc.get("normalized"):
            arr = arr / float(np.iinfo(dtype).max)
        return arr.astype(np.float64) if dtype in (np.float32,) or acc.get("normalized") else arr

    out = {}
    for node in doc["nodes"]:
        if "mesh" not in node:
            continue
        prim = doc["meshes"][node["mesh"]]["primitives"][0]
        pos = accessor(prim["attributes"]["POSITION"])
        col = accessor(prim["attributes"]["COLOR_0"])
        idx = accessor(prim["indices"]).reshape(-1, 3)
        tris = pos[idx]
        first = col[idx[:, 0]]
        out[node["name"]] = (tris, first[:, :3], first[:, 3])
    return out


# Rendering -------------------------------------------------------------------------------
def closeup(arrays, light):
    tris = arrays[0]
    lo, hi = tris.reshape(-1, 3).min(0), tris.reshape(-1, 3).max(0)
    center = (lo + hi) / 2
    half = max(hi[1] - lo[1], hi[0] - lo[0], hi[2] - lo[2]) * 0.62 + 1e-3
    view = raster.View(center, half, (CELL, CELL))
    return raster.render(*arrays, view, raster.LIGHTS[light])


def formation(entry, arrays):
    tris, colors, team = arrays
    parts_t, parts_c, parts_m = [], [], []
    for x, z, s in entry["formation"]["slots"]:
        parts_t.append(tris * s + np.array([x, 0.0, z]))
        parts_c.append(colors)
        parts_m.append(team)
    half = FORMATION_CELL / GAME_PX_PER_UNIT / 2
    view = raster.View((0.0, 0.25, 0.0), half, (FORMATION_CELL, FORMATION_CELL))
    return raster.render(np.concatenate(parts_t), np.concatenate(parts_c), np.concatenate(parts_m), view,
                         raster.LIGHTS["game"], supersample=3)


def row(entry, lods):
    height = max(CELL, FORMATION_CELL)
    label = np.full((height, LABEL_W, 3), 236, np.uint8)
    raster.draw_text(label, 8, 10, entry["id"].split("/", 1)[1][:30], size=2)
    tris = "/".join(str(lod["tris"]) for lod in entry["lods"])
    raster.draw_text(label, 8, 32, f"TRIS {tris}", size=2)
    figures = entry["formation"]["figures"]
    raster.draw_text(label, 8, 52, f"X{figures} = {entry['lods'][0]['tris'] * figures}", size=2)
    raster.draw_text(label, 8, 72, entry["budget"] + " " + entry["contact"], size=2)
    if entry.get("variant_of"):
        raster.draw_text(label, 8, 92, "OF " + entry["variant_of"].split("/", 1)[1][:24], size=2)
    cells = [label, pad(formation(entry, lods[0]), height)]
    for light in raster.LIGHTS:
        cells.append(pad(closeup(lods[0], light), height))
    cells.append(pad(closeup(lods[1], "game"), height))
    cells.append(pad(closeup(lods[2], "game"), height))
    line = np.concatenate(cells, axis=1)
    line[-1:, :] = 120
    return line


def pad(img, height):
    if img.shape[0] == height:
        return img
    out = np.full((height, img.shape[1], 3), 236, np.uint8)
    top = (height - img.shape[0]) // 2
    out[top:top + img.shape[0]] = img
    return out


def header(width):
    bar = np.full((28, width, 3), 210, np.uint8)
    titles = ["MODEL", "IN GAME", "LOD0 GAME SUN", "RIGHT", "BACK", "LOW", "LOD1", "MARKER"]
    xs = [8, LABEL_W + 8] + [LABEL_W + FORMATION_CELL + 8 + CELL * i for i in range(6)]
    for x, title in zip(xs, titles):
        raster.draw_text(bar, x, 8, title, size=2)
    return bar


def main(argv):
    use_catalog = "--catalog" in argv
    only = parts.positional(argv)
    cats = catalogs.Catalogs() if use_catalog else None
    OUT.mkdir(exist_ok=True)
    written = []
    for part in parts.selected(argv):
        entries, lods = from_catalog(cats, part) if use_catalog else from_glb(part)
        for group, accept in GROUPS:
            chosen = [e for e in entries if accept(e) and (not only or any(o in e["id"] for o in only))]
            if not chosen:
                continue
            rows = [row(e, lods[e["id"]]) for e in chosen]
            sheet = np.concatenate([header(rows[0].shape[1])] + rows, axis=0)
            path = OUT / f"models-contact-{part}-{group}.png"
            raster.write_png(path, sheet)
            written.append(path)
            print("wrote", path, len(chosen), "models")
    return written


if __name__ == "__main__":
    main(parts.cli_args())
