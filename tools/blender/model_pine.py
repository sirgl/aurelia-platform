"""Pines pine_0/1/2: trunk + 3 cone tiers, hue variants. ~1.2 tall."""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lib

TRUNK = (0.30, 0.22, 0.15, 0.0)
GREENS = [
    ((0.13, 0.32, 0.15, 0.0), (0.16, 0.38, 0.17, 0.0)),
    ((0.16, 0.38, 0.17, 0.0), (0.20, 0.44, 0.18, 0.0)),
    ((0.11, 0.28, 0.16, 0.0), (0.15, 0.35, 0.20, 0.0)),
]


def build(variant):
    dark, lite = GREENS[variant % len(GREENS)]
    lib.clear_scene()
    bm, mesh, obj = lib.new_workbench(f"Pine{variant}")
    P = []
    s = (1.0, 0.9, 1.1)[variant % 3]
    P.append((lib.cone(bm, lib.mat(tz=0.15 * s), segments=5, r1=0.07 * s, r2=0.05 * s, depth=0.35 * s), TRUNK))
    tiers = [(0.42, 0.34, 0.30), (0.66, 0.27, 0.28), (0.88, 0.19, 0.26)]
    segs = (6, 7, 6)[variant % 3]
    for i, (z, r, h) in enumerate(tiers):
        col = dark if i % 2 == 0 else lite
        P.append((lib.cone(bm, lib.mat(tz=(z + h / 2) * s, rz=i * 30.0),
                           segments=segs, r1=r * s, r2=0.0, depth=h * s), col))
    return bm, mesh, P


if __name__ == "__main__":
    base = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "assets", "models")
    for v in range(3):
        bm, mesh, P = build(v)
        lib.paint_and_flush(bm, mesh, P)
        lib.export_selected(os.path.join(base, f"pine_{v}.glb"))
