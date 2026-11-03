#!/bin/sh
# Make the run-time art pack: the art that is not in git (classic/assets/visual/generated),
# reduced to what the game reads at run time and stored as WebP (quality 90, lossless
# where q90 shows: platform/tools/assets-webp.py), as aurelia-assets-<version>.tar.zst plus .sha256
# and a per-file report. The version comes from classic/assets/PACK-VERSION (bump it whenever
# the art changes, then re-pack and re-upload). The PNG originals stay where they are.
# Members are rooted at the asset folder ("visual/generated/..."), so the pack is
# unpacked with `tar -x -C assets` (platform/tools/fetch-assets.sh does that).
# Usage: sh platform/tools/assets-pack.sh [OUT_DIR]        (default OUT_DIR: out/)
# Needs GNU tar (Linux), zstd, python3 with Pillow (WebP). Env: WEBP_QUALITY (default 90),
# WEBP_JOBS. Same input and same Pillow/libwebp = same bytes.
set -eu
ROOT="$(cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}" && pwd)"
cd "$ROOT"
OUT="${1:-out}"
VERSION="$(tr -d ' \t\r\n' < classic/assets/PACK-VERSION)"
case "$VERSION" in
  ""|*[!A-Za-z0-9._-]*) echo "classic/assets/PACK-VERSION must be one word of letters, digits, . _ -" >&2; exit 1 ;;
esac
[ -f classic/assets/visual/generated/runtime-atlas-index.tsv ] || {
  echo "classic/assets/visual/generated is missing or incomplete (no runtime-atlas-index.tsv)" >&2; exit 1; }
tar --version 2>/dev/null | grep -q "GNU tar" || { echo "GNU tar is required (run this on Linux)" >&2; exit 1; }

NAME="aurelia-assets-$VERSION.tar.zst"
mkdir -p "$OUT"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT INT TERM
python3 -I platform/tools/assets-webp.py classic/assets/visual/generated "$STAGE/visual/generated" \
  --quality "${WEBP_QUALITY:-90}" ${WEBP_JOBS:+--jobs "$WEBP_JOBS"} > "$OUT/aurelia-assets-$VERSION.report.tsv"
# Level 10: WebP does not compress further; higher levels only cost time.
LC_ALL=C tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
  -C "$STAGE" -cf - visual/generated | zstd -10 -T0 -q -f -o "$OUT/$NAME"
(cd "$OUT" && sha256sum "$NAME" > "$NAME.sha256")
ls -l "$OUT/$NAME" "$OUT/$NAME.sha256"
cat "$OUT/$NAME.sha256"
echo
echo "Next (uploads are the project owner's decision; nothing was uploaded):"
echo "  sh platform/tools/upload-assets.sh $OUT          (dry run), then add --go: a GitHub release assets-$VERSION of the repository in classic/assets/PACK-URL"
