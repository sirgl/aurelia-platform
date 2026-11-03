#!/bin/sh
# Stages what the APK carries into LAUNCHER_DIR/android/assets (cargo-apk bundles that folder):
# fonts and audio always; with --art also the unpacked art pack (classic/assets/visual/generated,
# fetched by platform/tools/fetch-assets.sh, or the folder AURELIA_ART_DIR names). FILES.txt lists every file (the NDK cannot list
# sub-folders) and BUILD-ID says which staging this is, so the app unpacks again after an update.
set -eu
cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
LAUNCHER="${1:?usage: sh platform/tools/launcher/android-stage-assets.sh LAUNCHER_DIR [--art]}"
. "$LAUNCHER/launcher.conf"
shift
OUT="$LAUNCHER/android/assets"
rm -rf "$OUT"
mkdir -p "$OUT/fonts" "$OUT/audio"
cp platform/assets/fonts/*.ttf "$OUT/fonts/"
cp -r platform/assets/audio/imported platform/assets/audio/sfx "$OUT/audio/"
if [ "${1:-}" = "--art" ]; then
    ART="${AURELIA_ART_DIR:-${ART_DIR:+$ART_DIR/visual/generated}}"
    [ -n "$ART" ] || { echo "$PRODUCT has no art pack (ART_DIR is empty in $LAUNCHER/launcher.conf)" >&2; exit 2; }
    [ -d "$ART" ] || { echo "no $ART: run platform/tools/fetch-assets.sh" >&2; exit 2; }
    mkdir -p "$OUT/visual"
    cp -r "$ART" "$OUT/visual/generated"
fi
(cd "$OUT" && find . -type f ! -name FILES.txt ! -name BUILD-ID | sed 's|^\./||' | sort > FILES.txt)
HASH=$(cd "$OUT" && xargs sha256sum < FILES.txt | sha256sum | cut -c1-12)
printf '%s %s\n' "$(cat "${ART_DIR:-/nonexistent}/PACK-VERSION" 2>/dev/null || echo nopack)" "$HASH" > "$OUT/BUILD-ID"
echo "staged $(wc -l < "$OUT/FILES.txt") files, $(du -sh "$OUT" | cut -f1) in $OUT"
