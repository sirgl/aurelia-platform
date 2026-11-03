#!/bin/sh
# Phone and tablet offscreen screenshots of the shell (no device, no browser): the layouts a
# phone's page gets in mobile mode (lands/docs/web-mobile.md).
#   sh platform/tools/launcher/mobile-shots.sh LAUNCHER_DIR OUT_DIR       env: AURELIA_DESKTOP_BIN, AURELIA_GPU_LOCK
# Each size is the CSS size of a device at its pixel ratio: 390x844 at 3 (an iPhone 14), 844x390 (the
# same sideways), 360x740 at 3 (a small Android phone), 768x1024 at 2 (a tablet), plus 1024x768 sideways.
set -eu
cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
LAUNCHER=${1:?usage: sh platform/tools/launcher/mobile-shots.sh LAUNCHER_DIR OUT_DIR}
. "$LAUNCHER/launcher.conf"
OUT=${2:?out dir}
BIN=${AURELIA_DESKTOP_BIN:-${CARGO_TARGET_DIR:-target}/release/$DESKTOP_BIN}
rules=$DESKTOP_PACKAGE
LOCK=${AURELIA_GPU_LOCK:-}
SIZES=${SIZES:-"390x844x3 844x390x3 360x740x3 768x1024x2 1024x768x2"}
mkdir -p "$OUT"
{
  case $rules in
    classic-desktop) SCREENS=${SCREENS_CLASSIC:-"menu setup map city research civics settings"} ;;
    *) SCREENS=${SCREENS_LAL:-"menu new_game map city research laws event settings"} ;;
  esac
  for spec in $SIZES; do
    w=${spec%%x*}; rest=${spec#*x}; h=${rest%%x*}; scale=${rest##*x}
    px="$((w * scale))x$((h * scale))"
    for screen in $SCREENS; do
      f="$OUT/$rules-${w}x${h}-$screen.png"
      env AURELIA_GAME_SCREEN=$screen AURELIA_GAME_SEED=42 AURELIA_VISUAL_SIZE=$px \
        AURELIA_VISUAL_NATIVE_SCALE=$scale ${LOCK:+flock "$LOCK"} "$BIN" --screenshot "$f" > /dev/null 2>&1 \
        && echo "$f" || echo "FAILED $rules $w x $h $screen" >&2
    done
  done
}
