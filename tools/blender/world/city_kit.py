"""Modular city kit: a city tile is composed at run time from pieces that follow the
owner's era and the city's population and buildings.

Composition (mirrored by platform/crates/render/src/gpu/world_objects/city.rs; keep in sync):
1. The era's center piece stands at the tile anchor.
2. Ring landmarks (walls) are drawn around the tile, they take no slot.
3. Slotted landmarks of the city's buildings, by ascending (priority, key), at most
   `max_landmarks`. A landmark with "supersedes" hides the landmark of the building it
   names (a seaport hides the harbor). Slot kinds: "inner" -> first free inner slot, "outer" -> first free outer slot,
   "water" -> the free outer slot closest to a water neighbour (no water: not drawn).
4. Houses fill the remaining slots in slot order, as many as `houses_by_population`
   gives for the population (last row whose threshold <= population).
   Slot order is rotated by `seed % 6` inside each ring, so neighbouring cities differ.
   House style: the era's style, except every `older_style_share`-th house (counting
   `slot + seed`) keeps the previous era's style. Variant: (slot * 7 + seed) % variants.

Coordinates in KIT are game coordinates: x right, z toward the camera, yaw in degrees,
positive yaw turns +x toward +z. Pieces are authored like every world model (kit.py).
"""
import math

STYLES = ["ancient", "classical", "medieval", "renaissance", "industrial", "modern", "atomic", "information"]
INNER_R = 0.34
OUTER_R = 0.60


def _slots():
    """Inner ring of 6 and outer ring of 12 house sites: (x, z, yaw, radius, ring).
    Pieces mostly face the camera (+z), turned by up to 30 degrees for variety."""
    turns = [-30.0, -15.0, 0.0, 15.0, 30.0]
    inner = [[round(INNER_R * math.cos(math.radians(60 * k)), 4), round(INNER_R * math.sin(math.radians(60 * k)), 4),
              turns[(3 * k) % 5], 0.15, "inner"] for k in range(6)]
    outer = [[round(OUTER_R * math.cos(math.radians(15 + 30 * k)), 4),
              round(OUTER_R * math.sin(math.radians(15 + 30 * k)), 4), turns[(3 * k + 1) % 5], 0.14, "outer"]
             for k in range(12)]
    return inner + outer


def _landmark(model, slot, priority, supersedes=None):
    entry = {"model": model, "slot": slot, "priority": priority}
    if supersedes:
        entry["supersedes"] = supersedes
    return entry


# Building key -> landmark entry, filled by the parts that draw landmarks: each module part keeps its own
# table (classic/tools/blender/world/city_kit.py, lands/tools/blender/part.py). The generic kit has none.
LANDMARKS = {}

KIT = {
    "styles": STYLES,
    "centers": {s: f"city/center-{s}" for s in STYLES},
    "houses": {s: [f"city/house-{s}-{i}" for i in range(3)] for s in STYLES},
    "houses_by_population": [[1, 3], [2, 4], [3, 5], [4, 6], [5, 7], [6, 8], [8, 10], [10, 12], [12, 14],
                             [15, 16], [20, 18]],
    "older_style_share": 3,
    "max_landmarks": 4,
    "slots": _slots(),
    "landmarks": LANDMARKS,
}


def house_count(population):
    count = 0
    for threshold, houses in KIT["houses_by_population"]:
        if population >= threshold:
            count = houses
    return count


def _rotated(kind, seed):
    """Slot indices of one ring in fill order (rotated by seed)."""
    ring = [i for i, s in enumerate(KIT["slots"]) if s[4] == kind]
    shift = seed % 6 * (len(ring) // 6)
    return ring[shift:] + ring[:shift]


def compose(era, population, buildings, water_dirs=(), seed=0, landmarks=None):
    """[(model id, x, z, yaw)] in game coordinates for one city tile. `landmarks`: building key -> entry
    of the parts that draw landmarks (default: KIT["landmarks"], which the generic kit leaves empty)."""
    style = STYLES[min(max(era, 0), len(STYLES) - 1)]
    pieces = [(KIT["centers"][style], 0.0, 0.0, 0.0)]
    owned = set(buildings)
    landmarks = KIT["landmarks"] if landmarks is None else landmarks
    chosen = [(e["priority"], key, e) for key, e in landmarks.items() if key in owned]
    hidden = {e.get("supersedes") for _, _, e in chosen}
    chosen = sorted(c for c in chosen if c[1] not in hidden)
    order = _rotated("inner", seed) + _rotated("outer", seed)
    free = list(order)
    slotted = 0
    for _, key, entry in chosen:
        if entry["slot"] == "ring":
            pieces.append((entry["model"], 0.0, 0.0, 0.0))
            continue
        if slotted >= KIT["max_landmarks"]:
            continue
        slot = _pick(entry["slot"], free, water_dirs)
        if slot is None:
            continue
        free.remove(slot)
        slotted += 1
        x, z, yaw, _, _ = KIT["slots"][slot]
        pieces.append((entry["model"], x, z, yaw))
    for n, slot in enumerate(free[:house_count(population)]):
        house_era = era
        if (slot + seed) % KIT["older_style_share"] == 0:
            house_era = max(era - 1, 0)
        variants = KIT["houses"][STYLES[min(house_era, len(STYLES) - 1)]]
        x, z, yaw, _, _ = KIT["slots"][slot]
        pieces.append((variants[(slot * 7 + seed) % len(variants)], x, z, yaw))
    return pieces


def _pick(kind, free, water_dirs):
    slots = KIT["slots"]
    if kind in ("inner", "outer"):
        for slot in free:
            if slots[slot][4] == kind:
                return slot
        return None
    if not water_dirs:
        return None
    best, best_d = None, None
    for slot in free:
        if slots[slot][4] != "outer":
            continue
        angle = math.degrees(math.atan2(slots[slot][1], slots[slot][0])) % 360.0
        d = min(abs((angle - 60.0 * w + 180.0) % 360.0 - 180.0) for w in water_dirs)
        if best_d is None or d < best_d - 1e-6:
            best, best_d = slot, d
    return best


def models():
    import city_pieces
    return city_pieces.models()
