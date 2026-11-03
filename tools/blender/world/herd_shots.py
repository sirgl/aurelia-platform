"""Before/after stills of pastures, farms and herd animals from the game camera (45 degrees down).

    blender -b --factory-startup -P platform/tools/blender/world/herd_shots.py -- --before DIR_A --after DIR_B --out OUT
    python3 platform/tools/blender/world/herd_shots.py --before DIR_A --after DIR_B --out OUT

DIR_A / DIR_B are the roots that hold `platform/assets/models` and `classic/assets/models` (a checkout, or a
`git archive` of an older commit); `--after` defaults to this checkout. Writes one PNG per case and view
(`<case>-before.png`, `<case>-after.png`), `herds-before-after.png` (every case, before left, after right) and
`animals-closeup-before-after.png`. The shading is raster.py's, the same as hex.wgsl, without terrain shadows.
Needs numpy (Blender's Python has it). No window is ever opened.
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
import raster  # noqa: E402
from contact_sheet import read_glb  # noqa: E402

GRASS = (0.46, 0.60, 0.30)
GRASS_B = (0.51, 0.65, 0.33)
PLAINS = (0.60, 0.64, 0.33)
SAND = (0.80, 0.71, 0.50)
HEX_DX = math.sqrt(3.0)

# (case, title, tiles): a tile is (dx, dz in hex units from the centre tile, ground colour, [(model id, yaw)]).
CASES = [
    ("pasture-sheep", "PASTURE + SHEEP", [(0, 0, GRASS, [("improvement/pasture", 0), ("resource/sheep", 0)])]),
    ("pasture-horses", "PASTURE + HORSES", [(0, 0, GRASS, [("improvement/pasture", 0), ("resource/horses", 0)])]),
    ("pasture-cattle", "PASTURE + CATTLE", [(0, 0, GRASS, [("improvement/pasture", 0), ("resource/cattle", 0)])]),
    ("farm-grassland", "FARM GRASSLAND", [(0, 0, GRASS, [("improvement/farm-grassland", 0)])]),
    ("farm-plains", "FARM PLAINS", [(0, 0, PLAINS, [("improvement/farm-plains", 0)])]),
    ("farm-desert", "FARM DESERT", [(0, 0, SAND, [("improvement/farm-desert", 0)])]),
    ("terrace-farm", "TERRACE FARM", [(0, 0, GRASS, [("improvement/terrace-farm", 0)])]),
]
CLUSTER = ("cluster", "PASTURES AND FARMS TOGETHER", [
    (0.0, 0.0, GRASS, [("improvement/pasture", 0), ("resource/sheep", 0)]),
    (HEX_DX, 0.0, GRASS_B, [("improvement/farm-grassland", 0)]),
    (-HEX_DX, 0.0, PLAINS, [("improvement/farm-plains", 0)]),
    (HEX_DX / 2, -1.5, GRASS_B, [("improvement/farm", 0)]),
    (-HEX_DX / 2, -1.5, GRASS, [("improvement/pasture", 0), ("resource/horses", 0)]),
    (HEX_DX / 2, 1.5, GRASS, [("improvement/pasture", 0), ("resource/cattle", 0)]),
    (-HEX_DX / 2, 1.5, GRASS_B, []),
])
ANIMALS = [("sheep", "resource/sheep"), ("cattle", "resource/cattle"), ("horses", "resource/horses"),
           ("deer", "resource/deer"), ("bison", "resource/bison")]


class Models:
    """GLB lookup by manifest id under one root."""

    def __init__(self, root):
        self.files = {}
        for part in ("platform", "classic"):
            base = Path(root) / part / "assets" / "models"
            manifest = base / "manifest.json"
            if not manifest.is_file():
                continue
            for entry in json.loads(manifest.read_text())["world"]["models"]:
                self.files[entry["id"]] = (base / entry["file"], entry["lods"][0]["node"], entry)
        self.cache = {}

    def lod0(self, model_id):
        if model_id not in self.cache:
            path, node, entry = self.files[model_id]
            self.cache[model_id] = (read_glb(path)[node], entry)
        return self.cache[model_id]


def hex_tile(x, z, color, y=-0.004, r=0.995):
    corners = [(x + r * math.cos(math.radians(30 + 60 * k)), y, z + r * math.sin(math.radians(30 + 60 * k)))
               for k in range(6)]
    tris = np.array([[(x, y, z), corners[(k + 1) % 6], corners[k]] for k in range(6)])
    return tris, np.tile(np.array(color), (6, 1)), np.zeros(6)


def place(arrays, x, z, yaw_deg):
    tris, colors, team = arrays
    a = math.radians(yaw_deg)
    c, s = math.cos(a), math.sin(a)
    out = tris.copy()
    out[..., 0] = tris[..., 0] * c - tris[..., 2] * s + x
    out[..., 2] = tris[..., 0] * s + tris[..., 2] * c + z
    return out, colors, team


def combine(items):
    return (np.concatenate([i[0] for i in items]), np.concatenate([i[1] for i in items]),
            np.concatenate([i[2] for i in items]))


def render_case(models, tiles, size, half, center=(0.0, 0.25, 0.0)):
    items, stats = [], []
    for dx, dz, ground, stack in tiles:
        items.append(hex_tile(dx, dz, ground))
        for model_id, yaw in stack:
            arrays, entry = models.lod0(model_id)
            items.append(place(arrays, dx, dz, yaw))
            stats.append((model_id, entry["lods"][0]["tris"]))
    view = raster.View(center, half, size)
    return raster.render(*combine(items), view, raster.LIGHTS["game"], supersample=3), stats


def titled(img, title, note=""):
    img = img.copy()
    raster.draw_text(img, 8, 8, title, size=2)
    if note:
        raster.draw_text(img, 8, img.shape[0] - 18, note, size=2)
    return img


def stack_h(images):
    return np.concatenate(images, axis=1)


def stack_v(images):
    return np.concatenate(images, axis=0)


def tris_note(stats):
    return "TRIS " + " + ".join(str(t) for _, t in stats)


def main(argv):
    args = {}
    it = iter(argv)
    for a in it:
        if a.startswith("--"):
            args[a[2:]] = next(it)
    out = Path(args["out"])
    out.mkdir(parents=True, exist_ok=True)
    before = Models(args["before"])
    after = Models(args.get("after", str(parts.REPO)))

    single = (560, 500)
    rows = []
    for case, title, tiles in CASES + [CLUSTER]:
        big = case == "cluster"
        size = (1000, 880) if big else single
        half = 2.5 if big else 0.98
        centre = (0.0, 0.1, 0.0) if big else (0.0, 0.25, 0.0)
        pair = []
        for tag, models in (("before", before), ("after", after)):
            img, stats = render_case(models, tiles, size, half, centre)
            raster.write_png(out / f"{case}-{tag}.png", img)
            pair.append(titled(img, f"{title} {tag.upper()}", tris_note(stats)))
        rows.append(stack_h(pair))
    width = max(r.shape[1] for r in rows)
    padded = [np.pad(r, ((0, 0), (0, width - r.shape[1]), (0, 0)), constant_values=236) for r in rows]
    raster.write_png(out / "herds-before-after.png", stack_v(padded))

    # Animals alone, close up: each row is one resource, before left and after right.
    lines = []
    for name, model_id in ANIMALS:
        pair = []
        for tag, models in (("before", before), ("after", after)):
            img, stats = render_case(models, [(0, 0, GRASS, [(model_id, 0)])], (560, 420), 0.62, (0.0, 0.12, 0.0))
            raster.write_png(out / f"animals-{name}-{tag}.png", img)
            pair.append(titled(img, f"{name.upper()} {tag.upper()}", tris_note(stats)))
        lines.append(stack_h(pair))
    raster.write_png(out / "animals-closeup-before-after.png", stack_v(lines))
    print("wrote", out)


if __name__ == "__main__":
    main(parts.cli_args())
