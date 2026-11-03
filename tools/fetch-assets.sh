#!/bin/sh
# Download the run-time art pack named by classic/assets/PACK-VERSION into classic/assets/visual/generated.
# No account and no gh needed: the pack is fetched by plain https from a public base URL:
#   1. $AURELIA_ASSETS_URL, else
#   2. the one line of classic/assets/PACK-URL: either a folder url that holds aurelia-assets-<version>.tar.zst
#      and its .sha256, or `github:<owner>/<repo>`: the assets of the release assets-<version> of that
#      repository, downloaded with the `gh` CLI (needed while the repository is private: `gh auth status`).
#      `UNSET` until a pack is uploaded.
# The hash is always checked before anything is unpacked. Without a pack the game still builds
# and runs (placeholder pictures): see lands/docs/BUILDING.md.
# Usage: sh platform/tools/fetch-assets.sh [--force]
set -eu
ROOT="$(cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}" && pwd)"
cd "$ROOT"
VERSION="$(tr -d ' \t\r\n' < classic/assets/PACK-VERSION)"
NAME="aurelia-assets-$VERSION.tar.zst"
MARK="classic/assets/visual/generated/.pack-version"
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1
if [ "$FORCE" = 0 ] && [ -f "$MARK" ] && [ "$(cat "$MARK")" = "$VERSION" ]; then
  echo "assets: pack $VERSION already unpacked"
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM
BASE="${AURELIA_ASSETS_URL:-$(tr -d ' \t\r\n' < classic/assets/PACK-URL 2>/dev/null || true)}"
[ "$BASE" = UNSET ] && BASE=""
case "$BASE" in
  github:*)
    command -v gh >/dev/null 2>&1 || { echo "assets: the gh CLI is needed for $BASE (or set AURELIA_ASSETS_URL)" >&2; exit 1; }
    echo "assets: downloading $NAME from the release assets-$VERSION of ${BASE#github:}"
    gh release download "assets-$VERSION" -R "${BASE#github:}" -p "$NAME" -p "$NAME.sha256" -D "$TMP" \
      || { echo "assets: cannot download from ${BASE#github:}" >&2; exit 1; }
    BASE="" ; FETCHED=1 ;;
esac
if [ "${FETCHED:-0}" = 1 ]; then
  :
elif [ -n "$BASE" ]; then
  BASE="${BASE%/}"
  echo "assets: downloading $NAME from $BASE"
  curl -fsSL --retry 3 -o "$TMP/$NAME" "$BASE/$NAME" || { echo "assets: cannot download $BASE/$NAME" >&2; exit 1; }
  curl -fsSL --retry 3 -o "$TMP/$NAME.sha256" "$BASE/$NAME.sha256" || { echo "assets: cannot download $BASE/$NAME.sha256" >&2; exit 1; }
else
  cat >&2 <<MSG
assets: no download location is set yet.
  The art pack $NAME is not hosted publicly yet (classic/assets/PACK-URL says UNSET).
  Set AURELIA_ASSETS_URL=<folder url> (or put that url in classic/assets/PACK-URL), or build without art:
  the game runs with placeholder pictures (lands/docs/BUILDING.md, "Without the art pack").
MSG
  exit 2
fi

# The .sha256 file is "<hash>  <name>"; compare the hash with our own computation.
WANT="$(cut -d' ' -f1 < "$TMP/$NAME.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  HAVE="$(sha256sum "$TMP/$NAME" | cut -d' ' -f1)"
else
  HAVE="$(shasum -a 256 "$TMP/$NAME" | cut -d' ' -f1)"
fi
[ "$WANT" = "$HAVE" ] || { echo "assets: hash mismatch for $NAME (want $WANT, got $HAVE)" >&2; exit 1; }

# Members must stay under visual/generated/: refuse anything else.
if zstd -dc "$TMP/$NAME" | tar -t | grep -qvE '^visual/generated(/|$)|^\./visual/generated(/|$)'; then
  echo "assets: the pack has members outside visual/generated; not unpacking" >&2; exit 1
fi
mkdir -p classic/assets/visual
# Unpack over the existing folder so files tracked in git (catalog tsv) are overwritten in place.
zstd -dc "$TMP/$NAME" | tar -x -C classic/assets
printf '%s\n' "$VERSION" > "$MARK"
echo "assets: pack $VERSION unpacked into classic/assets/visual/generated"
