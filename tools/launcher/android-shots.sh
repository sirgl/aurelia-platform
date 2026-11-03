#!/bin/sh
# Phone-size offscreen screenshots of the shell (no device needed): sh platform/tools/launcher/android-shots.sh LAUNCHER_DIR OUT_DIR
# 1080x2400 and 2400x1080 pixels at density 2.75 (a 393x873 dp phone), touch targets on, so the
# layout is what the Android build draws. Uses the release desktop binary under the GPU lock when
# $GPU_LOCK exists (a shared dev machine); else runs directly.
set -eu
cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
LAUNCHER=${1:?usage: sh platform/tools/launcher/android-shots.sh LAUNCHER_DIR OUT_DIR}
. "$LAUNCHER/launcher.conf"
OUT=${2:?out dir}
BIN=${AURELIA_DESKTOP_BIN:-${CARGO_TARGET_DIR:-target}/release/$DESKTOP_BIN}
rules=$DESKTOP_PACKAGE
LOCK=${AURELIA_GPU_LOCK:-}
mkdir -p "$OUT"
{
  case $rules in
    classic-desktop) SCREENS="map menu city research civics religion culture overview diplomacy settings" ;;
    *) SCREENS="map menu city research laws overview settings" ;;
  esac
  for size in 1080x2400 2400x1080; do
    for screen in $SCREENS; do
      f="$OUT/$rules-$size-$screen.png"
      env AURELIA_GAME_SCREEN=$screen AURELIA_GAME_SEED=42 AURELIA_VISUAL_SIZE=$size \
        AURELIA_VISUAL_NATIVE_SCALE=2.75 ${LOCK:+flock "$LOCK"} "$BIN" --screenshot "$f" > /dev/null 2>&1 \
        && echo "$f" || echo "FAILED $rules $size $screen" >&2
    done
  done
}
