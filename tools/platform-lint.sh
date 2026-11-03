#!/bin/sh
# Platform coupling lint (platform/PLATFORM.md): counts what ties the platform crates
# (kernel, render, client) to Classic and fails when a count grows above its
# ceiling in platform/tools/platform-lint.limits. Lower a ceiling when a stage removes
# coupling.
# Usage: sh platform/tools/platform-lint.sh [--print]
set -e
# The repository root (platform/, classic/, lands/ side by side); a part that is absent counts as zero.
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"

# The crates by layer (platform/PLATFORM.md, decision D1). The platform crates may
# depend on each other, never on a rules module or its content.
KERNEL=platform/crates/kernel
RENDER=platform/crates/render
CLIENT=platform/crates/client
MODULE=classic/crates/classic-rules
DESKTOP=platform/crates/launcher-core  # the launchers' shared code (lands/launcher, classic/launcher are one-module crates)
# The launcher core names no module at all: a launcher crate registers its module.
REGISTRATION=/nonexistent  # a module is registered in the launcher crates, not here
PLATFORM_CRATES="$KERNEL $RENDER $CLIENT"
# The second rules module (stage 7): its crates prove the platform without Classic.
SANDBOX_CRATES=$(ls -d platform/crates/sandbox-* 2>/dev/null)
# The third rules module, Lands & Laws (LANDS_AND_LAWS_PLAN.md): built on the
# platform alone, beside the other two modules.
LAL_CRATES=$(ls -d lands/crates/lal-* 2>/dev/null)
# Package names of a rules module and its content.
MODULE_PACKAGES='classic-[a-z-]+'

# Non-test Rust files under a directory (`fixtures.rs` is test-only, and
# the kernel's `test_module.rs` is its test rules module).
sources() {
  find "$1" -name '*.rs' -not -path '*/tests/*' -not -name '*_tests.rs' -not -name 'tests.rs' \
    -not -name 'fixtures.rs' -not -name 'test_module.rs' -not -path '*/kernel_tests/*' 2>/dev/null
}

# Non-test code of the files named on stdin: the sources without their inline test
# modules (`#[cfg(test)]` at the start of a line, then `mod name {` up to the `}` that
# closes it at the start of a line).
strip_tests() {
  while read -r f; do
    awk '
      /^#\[cfg\(test\)\]/ { pending = 1; next }
      pending && /^#\[/ { next }
      pending && /^(pub(\([a-z]+\))? )?mod [a-z_0-9]+ \{/ { pending = 0; skip = 1; next }
      pending { pending = 0 }
      skip { if ($0 ~ /^\}/) skip = 0; next }
      { print }
    ' "$f"
  done
}

# Non-test code under a directory.
code() {
  sources "$1" | strip_tests
}

# Distinct content keys ("core:<table>/<id>") in non-test code.
keys() {
  code "$1" | grep -oE '"core:[a-z-]+/[a-z0-9_-]+"' | sort -u | wc -l | tr -d ' '
}

# Uses of the hard-coded terrain codes.
terrain() {
  code "$1" | grep -oE '\b(HILLS|MOUNTAIN|WATER|TUNDRA)_TERRAIN\b' | wc -l | tr -d ' '
}

# Classic mechanic names in the kernel's turn and AI contract (stage 3: the
# rules module owns its steps and advisors).
mechanics() {
  { sources $KERNEL/src/turn; sources $KERNEL/src/ai; } | xargs grep -hoE \
    '\b(Happiness|GoldenAge|Trade|Barbarians|Tourism|CityStates|Congress|Espionage|Victory|Religion|GreatPeople|Policies|Citadel|Conquest|NaturalWonder)' \
    2>/dev/null | wc -l | tr -d ' '
}

# Classic mechanic words (identifiers, comments stripped).
CLASSIC_WORDS='faith|relig|belief|polic|touris|happi|golden_?age|great_?(person|people|work|general|admiral)|city_?state|spy|spies|espionage|barbarian|trade_?(route|partner)|pantheon|holy|long_?count|prophet|congress|ideolog|league|theming|archaeolog|missionar|inquisit|reformation|delegat'

classic_words() {
  sed 's://.*$::' | grep -oiE "$CLASSIC_WORDS" | wc -l | tr -d ' '
}

# Classic words in the kernel's effect vocabulary and in its list of rule
# tables (stage 4: the module's words and tables live in the module).
vocabulary() {
  { cat $KERNEL/src/rules/effects.rs
    awk '/^pub struct CompiledRuleset/,/^}/' $KERNEL/src/rules/compiled.rs
  } | classic_words
}

# Classic words in the rows of the kernel's tables (compiled.rs outside the
# table list): fields of buildings, units, technologies... that a module
# row extension would carry.
kernel_rows() {
  awk '/^pub struct CompiledRuleset/,/^}/ { next } { print }' $KERNEL/src/rules/compiled.rs | classic_words
}

# Classic words in a platform crate's non-test code, the table rows above
# counted apart (kernel_rows).
words() {
  code "$1" | classic_words
}

kernel_words() {
  echo $(( $(words $KERNEL/src) - $(kernel_rows) ))
}

# Dependencies of the platform crates on a rules module or its content:
# its entries in [dependencies] (and target-specific dependencies).
# Dev-dependencies are tests' and may instantiate the platform with Classic.
module_deps() {
  for crate in $PLATFORM_CRATES; do
    awk -v module="^($MODULE_PACKAGES) *=" '
      /^\[/ { deps = ($0 ~ /^\[(target\..*\.)?dependencies\]$/) ; next }
      deps && $0 ~ module { print }
    ' "$crate/Cargo.toml"
  done | wc -l | tr -d ' '
}

# Direct uses of the Classic module's crates (classic_rules, classic_content, classic_ui) in the
# app shell (client) and the desktop app outside the file that registers the module.
shell_module_uses() {
  { sources $CLIENT/src; sources $DESKTOP/src | grep -vx "$REGISTRATION"; } | strip_tests \
    | sed 's://.*$::' | grep -oE '\bclassic_(rules|content|ui)\b' | wc -l | tr -d ' '
}

# The sandbox crates' ties to Classic: entries of a Classic crate in any of their
# dependency tables (dev and build included), and uses of a Classic crate in their code.
sandbox_deps() {
  for crate in $SANDBOX_CRATES; do
    awk -v module="^($MODULE_PACKAGES) *=" '
      /^\[/ { deps = ($0 ~ /^\[(target\..*\.)?(dev-|build-)?dependencies\]$/) ; next }
      deps && $0 ~ module { print }
    ' "$crate/Cargo.toml"
    find "$crate" -name '*.rs' | xargs grep -hoE '\bclassic_(rules|content|ui)\b' 2>/dev/null || true
  done | wc -l | tr -d ' '
}

# Classic content keys ("core:<table>/...") anywhere in the sandbox crates: code,
# tests and data.
sandbox_keys() {
  for crate in $SANDBOX_CRATES; do
    find "$crate" -type f \( -name '*.rs' -o -name '*.ron' \) | xargs grep -hoE 'core:[a-z-]+/' 2>/dev/null || true
  done | wc -l | tr -d ' '
}

# The Lands & Laws crates' ties to the other modules: entries of a Classic or
# sandbox crate in any of their dependency tables (dev and build included), and
# uses of those crates in their code.
lal_deps() {
  for crate in $LAL_CRATES; do
    awk -v module="^(classic-[a-z-]+|sandbox-[a-z-]+) *=" '
      /^\[/ { deps = ($0 ~ /^\[(target\..*\.)?(dev-|build-)?dependencies\]$/) ; next }
      deps && $0 ~ module { print }
    ' "$crate/Cargo.toml"
    find "$crate" -name '*.rs' | xargs grep -hoE '\b(classic|sandbox)_(rules|content|ui)\b' 2>/dev/null || true
  done | wc -l | tr -d ' '
}

# Lands & Laws words in the platform crates' non-test code: its content keys
# ("lal:..."), its names, and every catalog id its pack names (TERR_GRASS,
# UNIT_SCOUT, slice_80, ...).
lal_keys() {
  ids=$(ls lands/crates/lal-content/data/pack/*.ron 2>/dev/null | grep -v '/tiers_2_3_' | xargs grep -hoE '\bid: "[A-Za-z0-9_]+"' 2>/dev/null | sed 's/^id: "//; s/"$//' | sort -u | paste -sd '|' -)
  words='lal:|\blal[_-][a-z]|\bLal[A-Z]?[a-z]*\b|Lands ?(&|and) ?Laws'
  [ -n "$ids" ] && words="$words|\\b($ids)\\b"
  for crate in $PLATFORM_CRATES; do
    code "$crate/src"
  done | sed 's://.*$::' | grep -oE "$words" | wc -l | tr -d ' '
}

# Module content keys ("core:...", "lal:...") in the platform's data files. The code lint above
# reads Rust only; a model manifest, an animation table or any other data file under
# platform/assets may not name a module's content either: the models of a module and the keys
# that bind them live in `<module>/assets/models/` (docs/models-ownership.md).
platform_manifest_keys() {
  cat platform/assets/models/manifest.json platform/assets/models/*.table 2>/dev/null \
    | grep -oE '"?\b(core|lal):[A-Za-z0-9_-]' | wc -l | tr -d ' '
}

# The same for every other data file of the platform's assets (shaders, fonts and the art
# are binary or code-free; the language files carry the shell's keys only). The art pack that
# `fetch-assets.sh` unpacks into `platform/assets/visual` is a download, not source: it is skipped.
platform_data_keys() {
  find platform/assets -path platform/assets/visual -prune -o -type f \( -name '*.json' -o -name '*.table' -o -name '*.ron' -o -name '*.toml' -o -name '*.tsv' -o -name '*.ftl' -o -name '*.txt' \) \
    -not -path 'platform/assets/models/manifest.json' -not -name '*.table' -print 2>/dev/null \
    | xargs grep -hoE '\b(core|lal):[A-Za-z0-9_-]' 2>/dev/null | wc -l | tr -d ' '
}

# Models in the platform's manifest that a module owns: a model id that only one module's keys
# draw (the build checks this too once two modules are present; the lint needs no build).
platform_foreign_models() {
  python3 -I - <<'PY'
import json, os
def load(part):
    path = f"{part}/assets/models/manifest.json"
    return json.load(open(path)) if os.path.exists(path) else None
platform = load("platform")
modules = [m for m in (load("classic"), load("lands")) if m]
if platform is None or len(modules) < 2:
    print(0)
    raise SystemExit
def draws(m):
    out = set()
    for t in ("classes", "units", "fallbacks"):
        out.update(m.get(t, {}).values())
    w = m["world"]
    out.update(w.get("keys", {}).values())
    out.update(l["model"] for l in w.get("city_kit", {}).get("landmarks", {}).values())
    return out
used = [draws(m) for m in modules]
kit = platform["world"]["city_kit"]
generic = set(kit["centers"].values()) | {h for hs in kit["houses"].values() for h in hs} | {platform["fallback"]}
ids = [e["id"] for e in platform["models"]] + [e["id"] for e in platform["world"]["models"]]
print(sum(1 for i in ids if i not in generic and sum(i in u for u in used) < 2))
PY
}

count() {
  case "$1" in
    platform_manifest_keys) platform_manifest_keys ;;
    platform_data_keys) platform_data_keys ;;
    platform_foreign_models) platform_foreign_models ;;
    module_deps) module_deps ;;
    kernel_keys) keys $KERNEL/src ;;
    classic_rules_keys) keys $MODULE/src ;;
    render_keys) keys $RENDER/src ;;
    client_keys) keys $CLIENT/src ;;
    desktop_keys) keys $DESKTOP/src ;;
    kernel_mechanics) mechanics ;;
    kernel_vocabulary) vocabulary ;;
    kernel_rows) kernel_rows ;;
    kernel_words) kernel_words ;;
    render_words) words $RENDER/src ;;
    client_words) words $CLIENT/src ;;
    shell_module_uses) shell_module_uses ;;
    sandbox_deps) sandbox_deps ;;
    sandbox_keys) sandbox_keys ;;
    lal_deps) lal_deps ;;
    lal_keys) lal_keys ;;
    terrain_codes) echo $(( $(terrain $KERNEL/src) + $(terrain $MODULE/src) + $(terrain $RENDER/src) + $(terrain $DESKTOP/src) )) ;;
    *) echo "platform-lint: unknown count $1" >&2; exit 2 ;;
  esac
}

status=0
while read -r name limit; do
  case "$name" in ''|'#'*) continue ;; esac
  value=$(count "$name")
  if [ "$1" = "--print" ]; then
    echo "$name $value (limit $limit)"
  fi
  if [ "$value" -gt "$limit" ]; then
    echo "platform-lint: $name grew to $value (limit $limit)" >&2
    status=1
  fi
done < platform/tools/platform-lint.limits
exit $status
