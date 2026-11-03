#!/bin/sh
# Build the browser page into dist/web (lands/docs/web.md): index.html, pkg/ (wasm + JS glue) and
# assets/ (the run-time art as WebP plus manifest.tsv).
#   sh platform/tools/launcher/web-build.sh LAUNCHER_DIR [OUT_DIR]   (default OUT_DIR: dist/web)
# The page loads the wasm, the JS glue, the baked 3D models (35 MB) and the fog engravings (6.7 MB) by
# content-hashed names listed in sizes.json (the last two are not compiled into the wasm), so a CDN may cache them for a year (_headers).
#   Env: WEB_PROFILE (default release-web: the release profile with fat LTO, one codegen unit,
#   opt-level s, strip, panic=abort; "release" = the plain one), WEB_OPT_LEVEL (s|z|3), WEB_PANIC (abort|unwind).
# The function names are not in the shipped wasm: web_bg.<hash>.wasm with names goes to symbols/ (SYMBOLS_DIR).
# Needs: rustup target wasm32-unknown-unknown for the toolchain, wasm-bindgen-cli 0.2.129
# (the version of the wasm-bindgen crate in Cargo.lock), python3 with Pillow (WebP) and
# classic/assets/visual/generated. Env: TOOLCHAIN (default 1.98.1), WEB_ASSETS (a staged WebP
# folder with visual/generated/, default: the art pack as it is when it holds no PNG, else made with platform/tools/assets-webp.py).
set -eu
ROOT="$(cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}" && pwd)"
cd "$ROOT"
LAUNCHER="${1:?usage: sh platform/tools/launcher/web-build.sh LAUNCHER_DIR [OUT_DIR]  (LAUNCHER_DIR: lands/launcher or classic/launcher)}"
. "$LAUNCHER/launcher.conf"
OUT="${2:-dist/web}"
TOOLCHAIN="${TOOLCHAIN:-1.98.1}"
export PATH="$HOME/.cargo/bin:$PATH"
# The launcher's own workspace: the folder above LAUNCHER (classic/Cargo.toml when the repositories sit side by side).
MANIFEST="$LAUNCHER/../Cargo.toml"
[ -f "$MANIFEST" ] || MANIFEST=Cargo.toml
# The web profile exists only for this build (no change to the native profiles): `--config` defines it.
PROFILE="${WEB_PROFILE:-release-web}"
if [ "$PROFILE" = release-web ]; then
  set -- --config 'profile.release-web.inherits="release"' --config 'profile.release-web.lto="fat"' \
    --config 'profile.release-web.codegen-units=1' --config "profile.release-web.opt-level=\"${WEB_OPT_LEVEL:-s}\"" \
    --config 'profile.release-web.strip="debuginfo"' --config "profile.release-web.panic=\"${WEB_PANIC:-abort}\""
else
  set --
fi
cargo "+$TOOLCHAIN" build "$@" --profile "$PROFILE" --target wasm32-unknown-unknown --manifest-path "$MANIFEST" -p "$WEB_PACKAGE"
TARGET_DIR="${CARGO_TARGET_DIR:-$(dirname "$MANIFEST")/target}"
BUILT="$TARGET_DIR/wasm32-unknown-unknown/$PROFILE"
mkdir -p "$OUT/pkg" "$OUT/assets"
rm -f "$OUT"/pkg/web.js "$OUT"/pkg/web.*.js "$OUT"/pkg/web_bg*.wasm
wasm-bindgen --target web --out-name web --no-typescript --remove-name-section --remove-producers-section --out-dir "$OUT/pkg" "$BUILT/$(echo "$WEB_PACKAGE" | tr - _).wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "$OUT/pkg/web_bg.opt.wasm" "$OUT/pkg/web_bg.wasm" && mv "$OUT/pkg/web_bg.opt.wasm" "$OUT/pkg/web_bg.wasm"
fi
hash12() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -c1-12; }
# Content-hashed names: web_bg.<hash>.wasm, web.<hash>.js (the page reads them from sizes.json).
WASM_NAME="web_bg.$(hash12 "$OUT/pkg/web_bg.wasm").wasm"; mv "$OUT/pkg/web_bg.wasm" "$OUT/pkg/$WASM_NAME"
# The same wasm WITH its function names (they cost 9.8 MB, so the page gets none): kept out of dist, under the hash
# of the shipped file, to read a panic trace of `wasm-function[N]` frames (classic/docs/web-traces.md).
# Env: SYMBOLS_DIR (default: symbols/ next to dist/).
SYMBOLS="${SYMBOLS_DIR:-$(dirname "$OUT")/../symbols}"
mkdir -p "$SYMBOLS"; NAMED="$(mktemp -d)"
wasm-bindgen --target web --out-name web --no-typescript --remove-producers-section --out-dir "$NAMED" "$BUILT/$(echo "$WEB_PACKAGE" | tr - _).wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz -g --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "$NAMED/web_bg.opt.wasm" "$NAMED/web_bg.wasm" && mv "$NAMED/web_bg.opt.wasm" "$NAMED/web_bg.wasm"
fi
mv "$NAMED/web_bg.wasm" "$SYMBOLS/$WASM_NAME"; rm -rf "$NAMED"
GLUE_NAME="web.$(hash12 "$OUT/pkg/web.js").js"; mv "$OUT/pkg/web.js" "$OUT/pkg/$GLUE_NAME"
# The baked model vertices (render/build.rs output) and the fog engravings are not compiled into the wasm: the
# page fetches them from data/ by hashed names.
rm -rf "$OUT/assets/models" "$OUT/data" && mkdir -p "$OUT/data"
MODELS_OUT=""
for d in $(ls -td "$BUILT"/build/aurelia-render-*/out 2>/dev/null); do
  if [ -f "$d/unit_models.bin" ] && [ -f "$d/world_models.bin" ]; then MODELS_OUT="$d"; break; fi
done
[ -n "$MODELS_OUT" ] || { echo "web-build: render's unit_models.bin / world_models.bin not found under $BUILT/build" >&2; exit 1; }
# The models are 35 MB of floats that gzip shrinks 6x, and a CDN does not compress octet-stream: they ship as plain
# .gz files (gzip -9 -n: no timestamp, same bytes every build; the name is the hash of the shipped bytes) that the
# loader inflates itself (DecompressionStream). WEB_GZIP=0 ships them raw (sizes.json then has no "raw" entry).
# The fog PNGs are compressed already (gzip saves 3%): they stay as they are.
model_file() { # SRC NAME -> prints the file name in $OUT/data
  if [ "${WEB_GZIP:-1}" != 0 ] && command -v gzip >/dev/null 2>&1; then
    gzip -9 -n -c "$1" > "$OUT/data/.tmp.gz"
    n="$2.$(hash12 "$OUT/data/.tmp.gz").bin.gz"; mv "$OUT/data/.tmp.gz" "$OUT/data/$n"
  else
    n="$2.$(hash12 "$1").bin"; cp "$1" "$OUT/data/$n"
  fi
  echo "$n"
}
UNITS_NAME="$(model_file "$MODELS_OUT/unit_models.bin" unit_models)"
WORLD_NAME="$(model_file "$MODELS_OUT/world_models.bin" world_models)"
FOG_NAMES=""
for f in serpent kraken compass; do
  src="platform/assets/visual/fog-$f-v1.png"
  [ -f "$src" ] || { echo "web-build: $src not found" >&2; exit 1; }
  n="fog-$f.$(hash12 "$src").png"; cp "$src" "$OUT/data/$n"; FOG_NAMES="$FOG_NAMES $n"
done
# index.html, mode.js, mobile.js, trace.js, art.js, loader.js, manifest.webmanifest, icons/
cp -r "$LAUNCHER/web/static/." "$OUT/"
rm -rf "$OUT/assets/visual"
if [ -n "${ART_DIR:-}" ]; then
  # The published art pack is WebP already: use it as it is. PNG sources (a local art build) are converted.
  if [ -z "${WEB_ASSETS:-}" ] && [ -z "$(find "$ART_DIR/visual/generated" -name '*.png' -print -quit 2>/dev/null)" ]; then
    WEB_ASSETS="$ART_DIR"
  fi
  if [ -z "${WEB_ASSETS:-}" ]; then
    WEB_ASSETS="$OUT/.stage"
    python3 -I platform/tools/assets-webp.py "$ART_DIR/visual/generated" "$WEB_ASSETS/visual/generated" > "$OUT/assets-report.tsv"
  fi
  mkdir -p "$OUT/assets/visual"
  cp -r "$WEB_ASSETS/visual/generated" "$OUT/assets/visual/generated"
  if [ "$WEB_ASSETS" = "$OUT/.stage" ]; then rm -rf "$OUT/.stage"; fi
fi
rm -rf "$OUT/assets/fonts" && cp -r platform/assets/fonts "$OUT/assets/fonts"
# manifest.tsv: path<TAB>bytes of every file under assets/
(cd "$OUT/assets" && find . -type f ! -name manifest.tsv | sed 's|^\./||' | LC_ALL=C sort | while read -r f; do printf '%s\t%s\n' "$f" "$(wc -c < "$f")"; done) > "$OUT/assets/manifest.tsv"
# sizes.json: what the loader needs to show one honest progress bar, and the hashed names. Sizes are of the
# decoded files (what the page counts), whatever the CDN's Content-Encoding. "art" versions the art URLs
# (?v=): the art keeps its names, so a new release must not meet an old cached picture.
ART_VERSION="$(cd "$OUT/assets" && find . -type f ! -name manifest.tsv | LC_ALL=C sort | xargs cat | { sha256sum 2>/dev/null || shasum -a 256; } | cut -c1-12)"
python3 -I - "$OUT" "$WASM_NAME" "$GLUE_NAME" "$UNITS_NAME" "$WORLD_NAME" "$ART_VERSION" "$MODELS_OUT" $FOG_NAMES <<'PY'
import json, os, sys
out, wasm, glue, units, world, art, models_out, serpent, kraken, compass = sys.argv[1:11]
size = lambda rel: os.path.getsize(os.path.join(out, rel))
entry = lambda rel: {"file": rel, "bytes": size(rel)}
# "bytes" is what travels (the progress bar counts it); a .gz file also says the size it inflates to ("raw") and "encoding".
def model(rel, source):
    e = entry(rel)
    if rel.endswith(".gz"):
        e["encoding"] = "gzip"; e["raw"] = os.path.getsize(os.path.join(models_out, source))
    return e
# "boot": the art files the first frame reads (atlas indexes, fonts): the page fetches them in the same progress bar.
boot = 0
for line in open(os.path.join(out, "assets", "manifest.tsv")):
    path, n = line.rstrip("\n").split("\t")
    if path.endswith(".tsv") or path.startswith("fonts/"):
        boot += int(n)
doc = {
    "wasm": entry("pkg/" + wasm),
    "glue": entry("pkg/" + glue),
    "models": {"units": model("data/" + units, "unit_models.bin"), "world": model("data/" + world, "world_models.bin")},
    "fog": {"serpent": entry("data/" + serpent), "kraken": entry("data/" + kraken), "compass": entry("data/" + compass)},
    "boot": {"bytes": boot},
    "art": art,
}
with open(os.path.join(out, "sizes.json"), "w") as f:
    json.dump(doc, f, indent=1); f.write("\n")
PY
# Cloudflare Pages headers (https://developers.cloudflare.com/pages/configuration/headers/): a path pattern on its
# own line, its headers indented below. The .gz models are plain files (no Content-Encoding is set, the page inflates
# them); a CDN that decoded them on the way would still work: the loader looks at the gzip magic bytes. Rules that
# match one URL are all applied and equal header names are joined,
# so the patterns below do not overlap. Hashed files and the art (versioned with ?v=) are immutable; the page, the
# loader scripts, sizes.json and the web manifest are revalidated on every load (a 304 costs no body).
{
  printf '/pkg/*\n  Cache-Control: public, max-age=31536000, immutable\n'
  printf '/data/*\n  Cache-Control: public, max-age=31536000, immutable\n'
  printf '/assets/*\n  Cache-Control: public, max-age=31536000, immutable\n'
  printf '/\n  Cache-Control: no-cache\n'
  for f in $(cd "$OUT" && ls -p | grep -v '/$' | grep -v '^_headers$'); do printf '/%s\n  Cache-Control: no-cache\n' "$f"; done
} > "$OUT/_headers"
echo "symbols (not uploaded): $SYMBOLS/$WASM_NAME, $(wc -c < "$SYMBOLS/$WASM_NAME") bytes"
echo "built $OUT: $(du -sh "$OUT/pkg" | cut -f1) pkg, $(du -sh "$OUT/assets" | cut -f1) assets; wasm $(wc -c < "$OUT/pkg/$WASM_NAME") bytes"
