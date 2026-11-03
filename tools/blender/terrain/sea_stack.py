"""Sea stacks: free-standing sculpted rock pillars off a cliff headland (MESHING.md 2.1:
objects where the height field cannot stand). No bpy: runs under python3 and in Blender.

A stack is two or three stacked columnar blocks, each a loft of jittered rings that leans
a little and steps in at a ledge, on a flared wet foot, under a cap of turf. The faces are
broad planar facets (few sides, strong jitter), dark and wet at the foot, slate on the
walls, lighter on the ledges, green on top. Model units: footprint radius at most 0.485
(the radius the coast props scale from), height 1.0, pivot on the ground, z up.
"""
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "units"))
sys.path.insert(0, str(HERE.parent / "world"))

import kit  # noqa: E402
from geom import Mesh  # noqa: E402

RADIUS = 0.485
HEIGHT = 1.0
VARIANTS = 3


def _ring(seed, level, z, radius, sides, lean, jitter):
    """A jittered convex ring at height z, its centre shifted by the lean."""
    pts = []
    turn = kit.rand(seed, 900 + level) * 0.6
    for i in range(sides):
        a = math.tau * (i + turn + (kit.rand(seed, 100 * level + i) - 0.5) * 0.35) / sides
        r = radius * (1.0 + (kit.rand(seed, 100 * level + i + 50) - 0.5) * jitter)
        pts.append((lean[0] * z + math.cos(a) * r, lean[1] * z + math.sin(a) * r, z))
    return pts


def build(mesh, seed):
    """One stack variant into `mesh` (its `detail` picks the LOD)."""
    sides = mesh.pick(9, 7, 5)
    tiers = mesh.pick(3, 2, 1)
    lean = ((kit.rand(seed, 1) - 0.5) * 0.18, (kit.rand(seed, 2) - 0.5) * 0.18)
    # Tier tops and radii: the column narrows upward, each ledge steps in.
    tops = [0.38 + kit.rand(seed, 3) * 0.08, 0.70 + kit.rand(seed, 4) * 0.08, 0.92]
    radii = [RADIUS * 0.80, RADIUS * 0.62, RADIUS * 0.50]
    if tiers == 2:
        tops, radii = [0.55, 0.92], [RADIUS * 0.78, RADIUS * 0.56]
    elif tiers == 1:
        tops, radii = [0.92], [RADIUS * 0.72]
    # A flared wet foot in the swash.
    foot = _ring(seed, 0, 0.0, RADIUS * 0.88, sides, lean, 0.18)
    knee = _ring(seed, 1, 0.10, RADIUS * 0.84, sides, lean, 0.22)
    mesh.loft([foot, knee], "rock_dark")
    bottom = 0.10
    for t, (top, radius) in enumerate(zip(tops, radii)):
        level = 10 * (t + 1)
        low = _ring(seed, level, bottom, radius * 1.04, sides, lean, 0.30)
        mid = _ring(seed, level + 1, (bottom + top) * 0.5, radius * (0.97 + kit.rand(seed, level) * 0.06), sides, lean, 0.34)
        high = _ring(seed, level + 2, top, radius * 0.92, sides, lean, 0.30)
        mesh.loft([low, mid, high], "rock" if t % 2 == 0 else "stone")
        # The ledge: a thin lighter shelf where the next block steps in.
        if t + 1 < len(tops):
            shelf_low = _ring(seed, level + 3, top - 0.015, radius * 0.95, sides, lean, 0.25)
            shelf_high = _ring(seed, level + 4, top + 0.02, radius * 0.86, sides, lean, 0.25)
            mesh.loft([shelf_low, shelf_high], "limestone")
        bottom = top
    # Turf cap with a few tufts.
    cap_low = _ring(seed, 60, bottom - 0.01, radii[-1] * 0.95, sides, lean, 0.25)
    cap_high = _ring(seed, 61, HEIGHT - 0.03, radii[-1] * 0.70, sides, lean, 0.25)
    apex = [(lean[0] * HEIGHT + (kit.rand(seed, 62) - 0.5) * 0.05, lean[1] * HEIGHT + (kit.rand(seed, 63) - 0.5) * 0.05, HEIGHT)]
    mesh.loft([cap_low, cap_high, apex], "grass")


def meshes(variant):
    """LOD0, LOD1, LOD2 of one variant."""
    out = []
    for detail in range(3):
        mesh = Mesh(detail)
        build(mesh, 7919 * (variant + 1))
        out.append(mesh)
    return out
