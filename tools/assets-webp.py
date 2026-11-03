#!/usr/bin/env python3
"""Stage the run-time art as WebP for the asset pack (platform/tools/assets-pack.sh).

  python3 platform/tools/assets-webp.py SRC DST [--quality 90] [--jobs 4] [--max-error 3.0]
  (stdout, tab separated: file, lossy|lossless, PNG bytes, WebP bytes, mean error, p99 error;
   the errors are those of the q90 version)

SRC is classic/assets/visual/generated (PNG, outside git); DST receives only what the game
reads at run time, with every PNG stored as WebP under the same name and folder
(the loaders in platform/crates/client fall back from `x.png` to `x.webp`; the tsv indexes
keep saying `.png`). Lossy WebP at --quality, alpha always lossless. A picture whose
mean absolute colour error (0..255, over its visible pixels) exceeds --max-error is
stored lossless instead. Pixel sizes never change, so atlas rectangles stay valid.

The run-time set (see lands/docs/distribution.md), by what the loaders read:
  * every sheet named by runtime-atlas-index.tsv, policies/atlas-index.tsv and
    civilizations/artwork-index.tsv, and the indexes and catalogs themselves;
  * the standalone pictures: <root>/*.png, actions/*.png, policies/**/emblem-v1.png,
    civilizations/<key>/*-v1.png.
Not shipped: references/, materials/, ornaments/, prompts, generation/provenance json,
coverage and integration reports (nothing reads them).
Requires Pillow with WebP support.
"""
import argparse, os, shutil, sys
from concurrent.futures import ProcessPoolExecutor
from PIL import Image, ImageChops, ImageStat

TSV_KEEP = [
    "runtime-atlas-index.tsv",
    "policies/atlas-index.tsv",
    "policies/catalog.tsv",
    "civilizations/artwork-index.tsv",
    "civilizations/artwork-catalog.tsv",
]


def sheets_of(src, index):
    path = os.path.join(src, index)
    if not os.path.isfile(path):
        return
    base = os.path.dirname(index)
    with open(path, encoding="utf8") as f:
        next(f, None)
        for line in f:
            cols = line.rstrip("\n").split("\t")
            # runtime/policies indexes: id path x y w h; civilizations: key role path x y w h
            for col in cols:
                if col.endswith(".png"):
                    yield os.path.normpath(os.path.join(base, col))


def runtime_pngs(src):
    want = set()
    for index in TSV_KEEP:
        if index.endswith("index.tsv"):
            want.update(sheets_of(src, index))
    for name in os.listdir(src):
        if name.endswith(".png"):
            want.add(name)
    for sub, pattern in (("actions", None), ("policies", "emblem-v1.png"), ("civilizations", "-v1.png")):
        for d, _, files in os.walk(os.path.join(src, sub)):
            for f in files:
                if not f.endswith(".png"):
                    continue
                rel = os.path.relpath(os.path.join(d, f), src)
                if sub == "actions" or (pattern == "emblem-v1.png" and f == pattern) \
                        or (sub == "civilizations" and f.endswith(pattern) and "atlases" not in rel):
                    want.add(rel)
    return sorted(p for p in want if os.path.isfile(os.path.join(src, p)))


def convert(job):
    src, dst, rel, quality, max_error = job
    image = Image.open(os.path.join(src, rel))
    image.load()
    if image.mode not in ("RGB", "RGBA"):
        image = image.convert("RGBA")
    out = os.path.join(dst, os.path.splitext(rel)[0] + ".webp")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    image.save(out, "WEBP", quality=quality, method=4, alpha_quality=100)
    error, p99 = measure(image, Image.open(out))
    mode = "lossy"
    if error > max_error:
        image.save(out, "WEBP", lossless=True, quality=100, method=4)
        mode = "lossless"
    # error and p99 are those of the lossy q-quality version, also for lossless picks
    return rel, mode, os.path.getsize(os.path.join(src, rel)), os.path.getsize(out), error, p99


def measure(a, b):
    """(mean, 99th percentile) of the absolute colour error over the visible pixels."""
    a, b = a.convert("RGBA"), b.convert("RGBA")
    diff = ImageChops.difference(a.convert("RGB"), b.convert("RGB"))
    alpha = a.getchannel("A")
    mask = None if alpha.getextrema() == (255, 255) else alpha.point(lambda v: 255 if v > 0 else 0)
    mean = sum(ImageStat.Stat(diff, mask=mask).mean) / 3
    hist = diff.convert("L").histogram(mask=mask)
    total, seen, p99 = sum(hist), 0, 0
    for level, count in enumerate(hist):
        seen += count
        if seen >= 0.99 * total:
            p99 = level
            break
    return mean, p99


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("dst")
    ap.add_argument("--quality", type=int, default=90)
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    ap.add_argument("--max-error", type=float, default=3.0)
    args = ap.parse_args()
    from PIL import features
    if not features.check("webp"):
        sys.exit("Pillow has no WebP support")
    pngs = runtime_pngs(args.src)
    for tsv in TSV_KEEP:
        if os.path.isfile(os.path.join(args.src, tsv)):
            os.makedirs(os.path.dirname(os.path.join(args.dst, tsv)), exist_ok=True)
            shutil.copy2(os.path.join(args.src, tsv), os.path.join(args.dst, tsv))
    jobs = [(args.src, args.dst, p, args.quality, args.max_error) for p in pngs]
    total_in = total_out = lossless = 0
    worst = (0.0, "")
    with ProcessPoolExecutor(args.jobs) as pool:
        for rel, mode, a, b, err, p99 in pool.map(convert, jobs, chunksize=1):
            total_in += a
            total_out += b
            lossless += mode == "lossless"
            worst = max(worst, (err, rel))
            print(f"{rel}\t{mode}\t{a}\t{b}\t{err:.2f}\t{p99}", flush=True)
    print(f"# {len(pngs)} pictures, PNG {total_in/1e6:.1f} MB -> WebP {total_out/1e6:.1f} MB "
          f"({total_out/total_in:.2f}), {lossless} lossless, worst mean error at q{args.quality} {worst[0]:.2f} ({worst[1]})",
          file=sys.stderr)


if __name__ == "__main__":
    main()
