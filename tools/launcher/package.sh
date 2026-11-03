#!/bin/sh
# Package the release `desktop` binary and its assets for the machine this runs on.
# Usage: sh platform/tools/launcher/package.sh LAUNCHER_DIR [linux|macos|windows] [OUT_DIR]   (default: by uname; out/)
# Inputs (env): BIN      the release binary (default target/release/desktop[.exe];
#                        honours CARGO_TARGET_DIR)
#               VERSION  default: the version of $LAUNCHER/desktop/Cargo.toml
# Needs classic/assets/visual/generated to be the unpacked asset pack (sh platform/tools/fetch-assets.sh);
# an unpacked source tree (PNG originals) is refused: those are not the shipped art.
#
#   linux:   aurelia-<v>-linux-<arch>.tar.zst, aurelia_<v>_<deb arch>.deb (dpkg-deb),
#            and an AppImage when `appimagetool` is on the PATH.
#   macos:   Aurelia-<v>-macos-<arch>.dmg (an unsigned .app inside; ad-hoc signed).
#   windows: aurelia-<v>-windows-<arch>.zip (aurelia.exe + assets, unzip and run).
# The product comes from the launcher (lands/launcher, classic/launcher): its binary, its icons, its
# packaging files and, for the classic game, the art pack. This script knows no rule set.
# The executable finds `assets/` beside itself (platform/crates/client/src/shell/assets.rs; in a
# macOS bundle Contents/Resources/assets), so every layout below keeps them together.
set -eu
ROOT="$(cd "${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}" && pwd)"
cd "$ROOT"
LAUNCHER="${1:?usage: sh platform/tools/launcher/package.sh LAUNCHER_DIR [linux|macos|windows] [OUT_DIR] (LAUNCHER_DIR: lands/launcher or classic/launcher)}"
shift
# The product: NAME, PRODUCT, DESKTOP_BIN, ART_DIR (launcher.conf of the launcher).
. "$LAUNCHER/launcher.conf"
case "$(uname -s)" in
  Linux) DEFAULT=linux ;; Darwin) DEFAULT=macos ;; *) DEFAULT=windows ;;
esac
PLATFORM="${1:-$DEFAULT}"
OUT="${2:-out}"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' $LAUNCHER/desktop/Cargo.toml | head -1)}"
TARGET="${CARGO_TARGET_DIR:-target}"
EXE=""; [ "$PLATFORM" = windows ] && EXE=".exe"
BIN="${BIN:-$TARGET/release/$DESKTOP_BIN$EXE}"
case "$(uname -m)" in
  arm64|aarch64) ARCH=arm64 ;;
  *) ARCH=x86_64 ;;
esac
[ -f "$BIN" ] || { echo "no binary at $BIN (cargo build --release -p $DESKTOP_PACKAGE --bin $DESKTOP_BIN --no-default-features)" >&2; exit 1; }
# The art pack of the product, when it has one (ART_DIR holds PACK-VERSION and visual/generated).
if [ -n "${ART_DIR:-}" ]; then
  PACK="$(tr -d ' \t\r\n' < "$ART_DIR/PACK-VERSION")"
  [ "$(cat "$ART_DIR/visual/generated/.pack-version" 2>/dev/null)" = "$PACK" ] || {
    echo "$ART_DIR/visual/generated is not the asset pack $PACK: run sh platform/tools/fetch-assets.sh" >&2; exit 1; }
fi
OUT="$(mkdir -p "$OUT" && cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
# A stripped copy is what ships (the debug symbols are ~12 MB; Windows keeps them in a .pdb).
cp "$BIN" "$WORK/binary"
[ "$PLATFORM" = windows ] || strip "$WORK/binary"
BIN="$WORK/binary"

# What the game reads at run time (checked with strace, see lands/docs/distribution.md): the
# folders of assets/ below, plus the pack. Shaders, models and rules are compiled into the
# binary; the concept images of assets/visual are not read.
RUNTIME_DIRS="audio/imported audio/sfx fonts"
stage_assets() { # dest
  mkdir -p "$1"
  for d in $RUNTIME_DIRS; do mkdir -p "$1/$(dirname "$d")" && cp -R "platform/assets/$d" "$1/$d"; done
  if [ -n "${ART_DIR:-}" ]; then
    mkdir -p "$1/visual"
    cp -R "$ART_DIR/visual/generated" "$1/visual/generated"
    rm -f "$1/visual/generated/.pack-version"
  fi
  find "$1" -name '.DS_Store' -delete
}

# Licences and credits: the product's NOTICE, LICENSE-* and THIRD-PARTY.md (its root is the folder above
# the launcher) and the platform's NOTICE and THIRD-PARTY.md (font and audio credits), beside the executable.
stage_legal() { # dest
  mkdir -p "$1"
  for f in "$LAUNCHER"/../NOTICE "$LAUNCHER"/../LICENSE-* "$LAUNCHER"/../THIRD-PARTY.md; do
    [ -f "$f" ] && cp "$f" "$1/"
  done
  [ -f platform/NOTICE ] && cp platform/NOTICE "$1/NOTICE-platform"
  [ -f platform/THIRD-PARTY.md ] && cp platform/THIRD-PARTY.md "$1/THIRD-PARTY-platform.md"
  return 0
}

case "$PLATFORM" in
linux)
  DIR="$WORK/$NAME-$VERSION-linux-$ARCH"
  mkdir -p "$DIR"
  install -m755 "$BIN" "$DIR/$NAME"
  stage_assets "$DIR/assets"
  cp $LAUNCHER/packaging/aurelia.desktop $LAUNCHER/packaging/aurelia.png "$DIR/"
  stage_legal "$DIR"
  tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -C "$WORK" -cf - "$NAME-$VERSION-linux-$ARCH" \
    | zstd -10 -T0 -q -f -o "$OUT/$NAME-$VERSION-linux-$ARCH.tar.zst"

  # .deb: /usr/lib/$NAME/{$NAME,assets}, /usr/bin/$NAME symlink (the kernel resolves
  # /proc/self/exe, so the executable still finds assets/ beside its real path).
  DEB="$WORK/deb"
  mkdir -p "$DEB/DEBIAN" "$DEB/usr/lib/$NAME" "$DEB/usr/bin" "$DEB/usr/share/applications" \
    "$DEB/usr/share/icons/hicolor/512x512/apps"
  install -m755 "$BIN" "$DEB/usr/lib/$NAME/$NAME"
  stage_assets "$DEB/usr/lib/$NAME/assets"
  ln -s "../lib/$NAME/$NAME" "$DEB/usr/bin/$NAME"
  stage_legal "$DEB/usr/share/doc/$NAME"
  cp $LAUNCHER/packaging/aurelia.desktop "$DEB/usr/share/applications/$NAME.desktop"
  cp $LAUNCHER/packaging/aurelia.png "$DEB/usr/share/icons/hicolor/512x512/apps/$NAME.png"
  DEBARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
  cat > "$DEB/DEBIAN/control" <<CONTROL
Package: $NAME
Version: $VERSION
Architecture: $DEBARCH
Maintainer: Aurelia developers <noreply@example.com>
Section: games
Priority: optional
Depends: libc6, libasound2 | libasound2t64, libvulkan1
Description: $PRODUCT
 $PRODUCT is a turn-based strategy game. It needs a Vulkan capable graphics driver.
CONTROL
  # Packaging runs as the invoking user: -Zzstd needs dpkg >= 1.21 to install (Debian 12, Ubuntu 22.04).
  find "$DEB" -type d -exec chmod 755 {} +
  find "$DEB/usr/lib/$NAME/assets" -type f -exec chmod 644 {} +
  dpkg-deb --root-owner-group -Zzstd -z6 --build "$DEB" "$OUT/${NAME}_${VERSION}_$DEBARCH.deb" >/dev/null

  # AppImage: assets sit beside the executable inside the image.
  APP="$WORK/AppDir"
  mkdir -p "$APP/usr/bin"
  install -m755 "$BIN" "$APP/usr/bin/$NAME"
  stage_assets "$APP/usr/bin/assets"
  stage_legal "$APP/usr/bin"
  cp $LAUNCHER/packaging/aurelia.desktop "$APP/$NAME.desktop"
  cp $LAUNCHER/packaging/aurelia.png "$APP/$NAME.png"
  printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/%s" "$@"\n' "$NAME" > "$APP/AppRun"
  chmod +x "$APP/AppRun"
  if command -v appimagetool >/dev/null 2>&1; then
    ARCH="$(uname -m)" appimagetool --appimage-extract-and-run "$APP" "$OUT/$NAME-$VERSION-$(uname -m).AppImage" >/dev/null
  else
    echo "package: appimagetool not found, AppImage skipped (the AppDir layout is made but not kept)"
  fi
  ;;
macos)
  APP="$WORK/$PRODUCT_FILE.app"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  install -m755 "$BIN" "$APP/Contents/MacOS/$NAME"
  stage_assets "$APP/Contents/Resources/assets"
  stage_legal "$APP/Contents/Resources"
  sed "s/@VERSION@/$VERSION/g" $LAUNCHER/packaging/Info.plist > "$APP/Contents/Info.plist"
  # Icon: an iconset from the 512 px PNG.
  ICONSET="$WORK/$NAME.iconset"
  mkdir -p "$ICONSET"
  for s in 16 32 64 128 256 512; do sips -z $s $s $LAUNCHER/packaging/aurelia.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null; done
  cp "$ICONSET/icon_32x32.png" "$ICONSET/icon_16x16@2x.png"
  cp "$ICONSET/icon_64x64.png" "$ICONSET/icon_32x32@2x.png"
  cp "$ICONSET/icon_256x256.png" "$ICONSET/icon_128x128@2x.png"
  cp "$ICONSET/icon_512x512.png" "$ICONSET/icon_256x256@2x.png"
  rm "$ICONSET/icon_64x64.png"
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/$NAME.icns"
  # Unsigned for Gatekeeper (no developer certificate); ad-hoc signing lets arm64 run it.
  codesign --force --deep --sign - "$APP"
  mkdir "$WORK/dmg" && cp -R "$APP" "$WORK/dmg/" && ln -s /Applications "$WORK/dmg/Applications"
  hdiutil create -volname "$PRODUCT" -srcfolder "$WORK/dmg" -ov -format UDZO "$OUT/$PRODUCT_FILE-$VERSION-macos-$ARCH.dmg" >/dev/null
  ;;
windows)
  DIR="$WORK/$NAME-$VERSION-windows-$ARCH"
  mkdir -p "$DIR"
  install -m755 "$BIN" "$DIR/$NAME.exe"
  stage_assets "$DIR/assets"
  stage_legal "$DIR"
  (cd "$WORK" && 7z a -tzip -mx=1 "$OUT/$NAME-$VERSION-windows-$ARCH.zip" "$NAME-$VERSION-windows-$ARCH" >/dev/null)
  ;;
*) echo "unknown platform $PLATFORM" >&2; exit 1 ;;
esac
ls -l "$OUT"
