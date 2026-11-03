#!/usr/bin/env python3
"""Check that every relative Markdown link in the repository's documents resolves.

Usage: python3 -I platform/tools/check-doc-links.py [--all] [paths...]

Without paths it checks every tracked Markdown file except the generated
reports under output/, ru/benches/results/ and the private archive ru/ (`--all` adds them). Links to
http(s), mailto, file: and absolute paths, pure #anchors and evidence files
under output/ (screenshots and logs kept outside git) are skipped. A link with an #anchor to a Markdown file also needs the heading.
Exit status 1 when a link does not resolve (docs/README.md, "Quality").
"""
import os
import re
import subprocess
import sys

ROOT = os.environ.get("AURELIA_ROOT") or subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
LINK = re.compile(r"(?<!\!)\[[^\]\n]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
FENCE = re.compile(r"^\s*(```|~~~)")


def slug(heading: str) -> str:
    heading = re.sub(r"[`*_]", "", heading.strip().lower())
    heading = re.sub(r"[^\w\s-]", "", heading, flags=re.UNICODE)
    return re.sub(r"\s", "-", heading.strip())


def headings(path: str) -> set:
    result = set()
    in_fence = False
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if FENCE.match(line):
                in_fence = not in_fence
            elif not in_fence and line.startswith("#"):
                result.add(slug(line.lstrip("#")))
    return result


def documents(include_all: bool):
    if os.path.isdir(os.path.join(ROOT, ".git")):
        tracked = subprocess.check_output(["git", "ls-files", "*.md"], cwd=ROOT, text=True).split("\n")
    else:  # AURELIA_ROOT is a plain folder that holds the parts side by side (CI of one part)
        tracked = [os.path.relpath(os.path.join(d, n), ROOT) for d, _, names in os.walk(ROOT)
                   if "/.git" not in d and "/target" not in d for n in names if n.endswith(".md")]
    for path in filter(None, tracked):
        generated = path.startswith(("output/", "ru/", "ru/benches/results/"))
        if generated and not include_all:
            continue
        yield path


def check(path: str):
    problems = []
    in_fence = False
    full = os.path.join(ROOT, path)
    with open(full, encoding="utf-8") as handle:
        for number, line in enumerate(handle, 1):
            if FENCE.match(line):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            for target in LINK.findall(line):
                if re.match(r"^(https?:|mailto:|file:|/|#)", target):
                    continue
                file_part, _, anchor = target.partition("#")
                resolved = os.path.normpath(os.path.join(os.path.dirname(full), file_part))
                if os.path.relpath(resolved, ROOT).startswith("output" + os.sep):
                    continue
                if not os.path.exists(resolved):
                    problems.append((number, target, "missing"))
                elif anchor and resolved.endswith(".md") and slug(anchor) not in headings(resolved):
                    problems.append((number, target, "no such heading"))
    return problems


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    paths = args or list(documents("--all" in sys.argv))
    bad = 0
    for path in paths:
        for number, target, why in check(path):
            print(f"{path}:{number}: {target} ({why})")
            bad += 1
    print(f"{len(paths)} documents, {bad} unresolved links")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
