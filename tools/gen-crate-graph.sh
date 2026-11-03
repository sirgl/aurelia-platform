#!/bin/sh
# Prints the workspace crate list and the dependency graph between workspace
# crates, generated from `cargo metadata` (no build, works offline).
# platform/structure.md section 1 embeds this output between the markers
# `crate-graph:begin` and `crate-graph:end`; platform/tools/check-structure.sh fails
# when the two differ.
#
# Format, one line per crate, dependencies before dependents:
#   <name>  <path>  <- <normal workspace dependencies>  [dev: <dev-only ones>]
# `<-` reads "depends on". External crates are not listed here (the check
# script reads them for the "no GPU/platform crate below the UI" rule).
#
# Usage: sh platform/tools/gen-crate-graph.sh
set -e
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"
cargo metadata --offline --no-deps --format-version 1 | python3 -I -c '
import json, sys

meta = json.load(sys.stdin)
root = meta["workspace_root"]
LOGICAL = {"prof","kernel","net","sandbox-rules","sandbox-content","i18n","render","client","sandbox-ui","launcher-core","ui-input"}
def norm(n):
    return n[len("aurelia-"):] if n.startswith("aurelia-") and n[len("aurelia-"):] in LOGICAL else n
pkgs = {norm(p["name"]): p for p in meta["packages"] if p["id"] in set(meta["workspace_members"])}

def deps(p, kind):
    out = set()
    for d in p["dependencies"]:
        if norm(d["name"]) in pkgs and norm(d["name"]) != norm(p["name"]) and d.get("path") and (d["kind"] or "normal") == kind:
            out.add(norm(d["name"]))
    return out

normal = {n: deps(p, "normal") for n, p in pkgs.items()}
dev = {n: deps(p, "dev") - normal[n] for n, p in pkgs.items()}

# Layer = longest chain of normal dependencies below the crate.
level = {}
def depth(n):
    if n not in level:
        level[n] = 0 if not normal[n] else 1 + max(depth(d) for d in normal[n])
    return level[n]
for n in pkgs:
    depth(n)

width = max(len(n) for n in pkgs)
pwidth = max(len(p["manifest_path"][len(root) + 1:].rsplit("/", 1)[0]) for p in pkgs.values())
for n in sorted(pkgs, key=lambda n: (level[n], n)):
    path = pkgs[n]["manifest_path"][len(root) + 1:].rsplit("/", 1)[0]
    line = "%-*s  %-*s  <- %s" % (width, n, pwidth, path, ", ".join(sorted(normal[n])) or "-")
    if dev[n]:
        line += "  [dev: %s]" % ", ".join(sorted(dev[n]))
    print(line)
'
