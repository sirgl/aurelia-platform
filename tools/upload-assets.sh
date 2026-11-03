#!/bin/sh
# Upload an asset pack (made by platform/tools/assets-pack.sh) as the assets of a GitHub release.
# The release is named assets-<PACK-VERSION> and lives in the repository named by classic/assets/PACK-URL
# ("github:<owner>/<repo>"), or by $ASSETS_REPO. Needs the `gh` CLI, logged in with a right to write releases
# of that repository (`gh auth status`); no key is stored in a file.
# Usage: sh platform/tools/upload-assets.sh [OUT_DIR]          shows what it would do (uploads nothing)
#        sh platform/tools/upload-assets.sh [OUT_DIR] --go     creates the release and uploads
# A release of that name is immutable by convention: the script refuses to touch an existing one
# (bump classic/assets/PACK-VERSION instead). The pack is uploaded together with its .sha256 and the report.
set -eu
ROOT="$(cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}" && pwd)"
cd "$ROOT"
OUT="out"; GO=0
for arg in "$@"; do
  case "$arg" in --go) GO=1 ;; *) OUT="$arg" ;; esac
done
VERSION="$(tr -d ' \t\r\n' < classic/assets/PACK-VERSION)"
NAME="aurelia-assets-$VERSION.tar.zst"
TAG="assets-$VERSION"
for f in "$OUT/$NAME" "$OUT/$NAME.sha256"; do
  [ -f "$f" ] || { echo "missing $f (run: sh platform/tools/assets-pack.sh $OUT)" >&2; exit 1; }
done
REPO="${ASSETS_REPO:-}"
if [ -z "$REPO" ]; then
  URL="$(tr -d ' \t\r\n' < classic/assets/PACK-URL 2>/dev/null || true)"
  case "$URL" in github:*) REPO="${URL#github:}" ;; esac
fi
[ -n "$REPO" ] || { echo "no repository: set ASSETS_REPO=<owner>/<repo> or put github:<owner>/<repo> into classic/assets/PACK-URL" >&2; exit 1; }
command -v gh >/dev/null 2>&1 || { echo "the gh CLI is not installed" >&2; exit 1; }
echo "pack:     $OUT/$NAME ($(wc -c < "$OUT/$NAME") bytes) + .sha256"
echo "release:  $TAG in $REPO"
if gh release view "$TAG" -R "$REPO" >/dev/null 2>&1; then
  echo "release $TAG already exists in $REPO; versioned packs are immutable. Bump classic/assets/PACK-VERSION." >&2
  exit 1
fi
if [ "$GO" = 0 ]; then
  echo "dry run: nothing uploaded. Repeat with --go."
  exit 0
fi
EXTRA=""
[ -f "$OUT/aurelia-assets-$VERSION.report.tsv" ] && EXTRA="$OUT/aurelia-assets-$VERSION.report.tsv"
# The hash file goes with the pack in one call; fetch-assets.sh checks it before unpacking anything.
gh release create "$TAG" -R "$REPO" --title "Art pack $VERSION" \
  --notes "Run-time art pack $VERSION (WebP). Fetch with platform/tools/fetch-assets.sh; the hash is checked before unpacking." \
  "$OUT/$NAME" "$OUT/$NAME.sha256" $EXTRA
echo "uploaded. classic/assets/PACK-URL holds: github:$REPO"
