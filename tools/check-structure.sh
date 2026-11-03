#!/bin/sh
# Structure check (platform/docs/structure-conformance.md): platform/structure.md against the code.
# No build: `cargo metadata --offline` and grep only.
#
#   1. The crate list and dependency graph in platform/structure.md section 1 (between
#      the markers `crate-graph:begin` and `crate-graph:end`) equal the output of
#      platform/tools/gen-crate-graph.sh. Regenerate with that script when a crate or a
#      dependency changes.
#   2. The dependency-direction prohibitions of platform/structure.md sections 1, 46, 52
#      hold in `cargo metadata` (kernel/render/client below every module and
#      app, sandbox/lal/classic do not depend on each other, gameplay crates
#      have no GPU, window, browser or random-number crate).
#   3. Greps: no `services/` directory, no wall-clock time or `static mut` in
#      the simulation, no `Invalidate*` change in the journal, no old crate
#      names in platform/structure.md outside its first lines.
#   4. platform/tools/platform-lint.sh (the Classic coupling counters), unless --fast.
#
# Usage: sh platform/tools/check-structure.sh [--fast]
set -e
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"
status=0
fail() { echo "check-structure: $*" >&2; status=1; }

# --- 1. crate list and graph in platform/structure.md == cargo metadata -------------
generated=$(sh platform/tools/gen-crate-graph.sh)
documented=$(awk '/crate-graph:begin/ {on=1; next} /crate-graph:end/ {on=0} on && !/^```/ {print}' platform/structure.md)
if [ -z "$documented" ]; then
  fail "platform/structure.md has no crate-graph block (markers crate-graph:begin / crate-graph:end)"
elif [ "$generated" != "$documented" ]; then
  fail "platform/structure.md section 1 differs from platform/tools/gen-crate-graph.sh (< documented, > actual):"
  printf '%s\n' "$documented" > "${TMPDIR:-/tmp}/structure-doc.$$"
  printf '%s\n' "$generated" > "${TMPDIR:-/tmp}/structure-gen.$$"
  diff "${TMPDIR:-/tmp}/structure-doc.$$" "${TMPDIR:-/tmp}/structure-gen.$$" >&2 || true
  rm -f "${TMPDIR:-/tmp}/structure-doc.$$" "${TMPDIR:-/tmp}/structure-gen.$$"
fi

# --- 2. dependency direction from cargo metadata ---------------------------
cargo metadata --offline --no-deps --format-version 1 | python3 -I -c '
import json, re, sys

meta = json.load(sys.stdin)
LOGICAL = {"prof","kernel","net","sandbox-rules","sandbox-content","i18n","render","client","sandbox-ui","launcher-core","ui-input"}
def norm(n):
    return n[len("aurelia-"):] if n.startswith("aurelia-") and n[len("aurelia-"):] in LOGICAL else n
members = set(meta["workspace_members"])
pkgs = {norm(p["name"]): p for p in meta["packages"] if p["id"] in members}

def deps(name, kinds):
    return {norm(d["name"]) for d in pkgs[name]["dependencies"] if (d["kind"] or "normal") in kinds and norm(d["name"]) != name}

def match(names, pattern):
    return {n for n in names if re.fullmatch(pattern, n)}

bad = []
MODULE = r"classic-[a-z]+|lal-[a-z]+|sandbox-[a-z]+"
UPPER = r"classic-[a-z]+|lal-[a-z]+|sandbox-[a-z]+|lands-[a-z]+|net|net-server|aurelia-qa|gallery|session-profile|dev-desktop|launcher-core|client|render"

# kernel: only the profiler (and serde behind a test feature) below it.
extra = deps("kernel", {"normal", "build"}) - {"prof", "serde"}
if extra:
    bad.append("kernel depends on %s (allowed: prof, serde)" % sorted(extra))

# render never depends on client or a module; client never on a module.
for name in ("render", "client", "launcher-core"):
    up = match(deps(name, {"normal", "build"}), MODULE + "|lands-[a-z]+|dev-desktop|net|net-server|aurelia-qa|gallery")
    if name == "render":
        up |= deps(name, {"normal"}) & {"client"}
    if up:
        bad.append("%s depends on %s in [dependencies]" % (name, sorted(up)))

# The modules do not know each other, not even in tests.
families = {"classic": r"classic-[a-z]+", "lal": r"lal-[a-z]+", "sandbox": r"sandbox-[a-z]+"}
for family, pattern in families.items():
    for name in match(pkgs, pattern):
        if name.endswith("-ui"):
            continue
        for other, other_pattern in families.items():
            if other != family:
                hit = match(deps(name, {"normal", "dev", "build"}), other_pattern)
                if hit:
                    bad.append("%s (%s) depends on %s (%s)" % (name, family, sorted(hit), other))
    # the UI of a module may use the platform, not another module
    for name in match(pkgs, pattern):
        if name.endswith("-ui"):
            hit = set()
            for other, other_pattern in families.items():
                if other != family:
                    hit |= match(deps(name, {"normal", "build"}), other_pattern)
            if hit:
                bad.append("%s depends on %s" % (name, sorted(hit)))

# Nothing but the apps depends on an app; a module crate does not depend on the harnesses.
for name in pkgs:
    up = deps(name, {"normal", "dev", "build"}) & {"lands-desktop", "lands-web", "lands-android", "classic-desktop", "classic-web", "classic-android", "dev-desktop"}
    if up:
        bad.append("%s depends on the app %s" % (name, sorted(up)))
for name in match(pkgs, MODULE) | {"kernel", "render", "client"}:
    up = deps(name, {"normal", "build"}) & {"aurelia-qa", "gallery", "session-profile", "net-server"}
    if up:
        bad.append("%s depends on the harness %s" % (name, sorted(up)))

# One launcher per game: each depends on the platform and its own module only.
for name in sorted(pkgs):
    if name.startswith("lands-"):
        hit = {d for d in deps(name, {"normal", "dev", "build"}) if re.fullmatch(r"classic-[a-z]+|sandbox-[a-z]+|gallery|aurelia-qa|dev-desktop", d)}
        if hit:
            bad.append("%s (the Lands & Laws launcher) depends on %s" % (name, sorted(hit)))
    if re.fullmatch(r"classic-(desktop|web|android)", name):
        hit = {d for d in deps(name, {"normal", "dev", "build"}) if re.fullmatch(r"lal-[a-z]+|lands-[a-z]+|sandbox-[a-z]+|gallery|aurelia-qa|dev-desktop", d)}
        if hit:
            bad.append("%s (the classic launcher) depends on %s" % (name, sorted(hit)))

# The simulation crates have no GPU, window, browser, UI or random-number crate.
SIM = {"kernel", "prof", "classic-rules", "classic-content", "lal-rules", "lal-economy", "lal-mapgen",
       "lal-content", "sandbox-rules", "sandbox-content", "net"}
FORBIDDEN = {"wgpu", "winit", "egui", "eframe", "web-sys", "wasm-bindgen", "js-sys", "pollster",
             "rand", "rand_core", "rand_chacha", "getrandom", "chrono", "instant", "tokio"}
for name in sorted(SIM & set(pkgs)):
    hit = deps(name, {"normal", "build"}) & FORBIDDEN
    if hit:
        bad.append("simulation crate %s depends on %s" % (name, sorted(hit)))

if bad:
    for line in bad:
        print("check-structure: " + line, file=sys.stderr)
    sys.exit(1)
' || status=1

# --- 3. greps ---------------------------------------------------------------
if find platform/crates classic/crates classic/launcher lands/crates lands/launcher integration/crates -type d -name services -not -path '*/target/*' | grep -q .; then
  fail "a services/ directory exists (platform/structure.md section 48)"
fi
sim_sources() {
  find platform/crates/kernel/src classic/crates/classic-rules/src lands/crates/lal-rules/src lands/crates/lal-economy/src \
    platform/crates/sandbox-rules/src -name '*.rs' -not -path '*/tests/*' -not -name '*_tests.rs' \
    -not -name 'tests.rs' -not -name 'fixtures.rs' -not -name 'test_module.rs' -not -path '*/kernel_tests/*'
}
hits=$(sim_sources | xargs grep -nE '\bInstant::|\bSystemTime\b|std::time::(Instant|SystemTime)' 2>/dev/null || true)
[ -z "$hits" ] || fail "wall-clock time in simulation code:
$hits"
hits=$(sim_sources | xargs grep -nE 'static mut |lazy_static!' 2>/dev/null || true)
[ -z "$hits" ] || fail "global mutable state in simulation code:
$hits"
hits=$(grep -nE 'Invalidate[A-Z]' platform/crates/kernel/src/transaction/*.rs | grep -v '^[^:]*:[0-9]*://' || true)
[ -z "$hits" ] || fail "an Invalidate* change in the journal (platform/structure.md section 14):
$hits"
hits=$(sed -n '7,$p' platform/structure.md | grep -nE 'nation-core|nation-render|nation-client' || true)
[ -z "$hits" ] || fail "old crate names (nation-core, nation-render, nation-client) in platform/structure.md after the header note:
$hits"

# --- 4. the Classic coupling counters ------------------------------------------
if [ "$1" != "--fast" ]; then
  sh platform/tools/platform-lint.sh || status=1
fi

[ "$status" = 0 ] && echo "check-structure: ok"
exit $status
