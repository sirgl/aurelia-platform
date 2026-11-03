#!/bin/sh
# Regenerate the models headlessly (never opens the Blender UI). Run from anywhere:
#   sh platform/tools/blender/export_all.sh                  every model part present, platform first
#   sh platform/tools/blender/export_all.sh --part classic   just that part (--part is repeatable)
#   sh platform/tools/blender/export_all.sh --props          also the legacy static props (pines)
#   sh classic/tools/blender/export_all.sh                   just classic (a wrapper of this script)
#   sh lands/tools/blender/export_all.sh                     just lands (a wrapper of this script)
# Env BLENDER_BIN names the Blender executable (default: blender).
#
# A part writes only its own models, into its own folder, and rewrites only its own manifest:
#   platform  platform/assets/models/{units,world/<kind>,terrain}/*.glb + manifest.json: the shared and
#             generic models, the palette, budgets and shapes, the generic city kit
#             (catalogs: platform/tools/blender/{units,world,terrain}/)
#   classic   classic/assets/models/{units,world/<kind>}/*.glb + manifest.json: the models only classic
#             draws and its "core:" keys (catalogs: classic/tools/blender/)
#   lands     lands/assets/models/manifest.json: its "lal:" keys onto the platform's models; no GLBs
#             (catalog: lands/tools/blender/part.py)
# Stale GLBs are removed in the part's own folders only. Then, per part, the contact sheets
# output/models-contact-<part>-*.png and output/world-contact-<part>-*.png for review. The manifests
# are validated again by platform/crates/render/build.rs.
# Without Blender, `python3 -I platform/tools/blender/check_catalogs.py` checks that the catalogs
# reproduce the committed manifests and that every model is declared in the part that owns it.
set -e
BLENDER_BIN="${BLENDER_BIN:-blender}"
DIR="$(cd "$(dirname "$0")" && pwd)"
PARTS=""
PROPS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --props) PROPS=1 ;;
    --part) PARTS="$PARTS --part $2"; shift ;;
    --part=*) PARTS="$PARTS --part ${1#--part=}" ;;
    *) echo "usage: sh export_all.sh [--part NAME]... [--props]" >&2; exit 2 ;;
  esac
  shift
done
# shellcheck disable=SC2086  # $PARTS is a list of words on purpose
"$BLENDER_BIN" --background --factory-startup --python "$DIR/export_units.py" -- $PARTS
"$BLENDER_BIN" --background --factory-startup --python "$DIR/export_world.py" -- $PARTS
"$BLENDER_BIN" --background --factory-startup --python "$DIR/export_terrain.py" -- $PARTS
"$BLENDER_BIN" --background --factory-startup --python "$DIR/units/contact_sheet.py" -- $PARTS
"$BLENDER_BIN" --background --factory-startup --python "$DIR/world/world_sheet.py" -- $PARTS
if [ "$PROPS" = 1 ]; then
  case " $PARTS " in
    *" platform "*|"  ") ;;
    *) PROPS=0 ;;
  esac
fi
if [ "$PROPS" = 1 ]; then
  for m in model_pine; do
    "$BLENDER_BIN" --background --python "$DIR/$m.py"
  done
fi
