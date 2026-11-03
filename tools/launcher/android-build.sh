#!/bin/sh
# Builds the APK of a launcher: sh platform/tools/launcher/android-build.sh LAUNCHER_DIR [debug|release] [aarch64|x86_64] [--art]
#   release (default) is what ships; x86_64 is for the emulator (the arm64 build also runs
#   there through ARM translation, but that hides real behaviour). --art bundles the art pack
#   (run platform/tools/fetch-assets.sh first). The APK lands in $CARGO_TARGET_DIR/<profile>/apk/<apk_name>.apk (apk_name in the launcher's android/Cargo.toml).
# Needs the toolchain of lands/docs/android.md ("Toolchain"): source platform/tools/launcher/android-env.sh first.
set -eu
cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
LAUNCHER="${1:?usage: sh platform/tools/launcher/android-build.sh LAUNCHER_DIR [debug|release] [aarch64|x86_64] [--art]}"
. "$LAUNCHER/launcher.conf"
PROFILE=${2:-release}
ARCH=${3:-aarch64}
ART=${4:-}
[ "$PROFILE" = release ] && FLAG=--release || FLAG=
sh platform/tools/launcher/android-stage-assets.sh "$LAUNCHER" $ART
: "${CARGO_APK_RELEASE_KEYSTORE:=$HOME/.android/debug.keystore}"
: "${CARGO_APK_RELEASE_KEYSTORE_PASSWORD:=android}"
export CARGO_APK_RELEASE_KEYSTORE CARGO_APK_RELEASE_KEYSTORE_PASSWORD
cd "$LAUNCHER/android"
cargo ${CARGO_TOOLCHAIN:-+1.98.1} apk build --lib $FLAG --target "$ARCH-linux-android"
