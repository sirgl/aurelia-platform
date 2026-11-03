#!/usr/bin/env python3
"""Scan Rust sources for user-visible string literals (platform/docs/localization.md).

The scanner lexes Rust well enough to find string literals outside comments, test
modules and `#[test]` functions, then keeps the ones that look like text a player can
read (a heuristic: words, not identifiers, paths or keys). It is the inventory and the
ratchet of the localization effort:

  python3 platform/tools/i18n-scan.py --inventory     table of counts by crate and kind
  python3 platform/tools/i18n-scan.py --print         per-file counts in baseline format
  python3 platform/tools/i18n-scan.py --check         fail when a file has more raw literals than
                                             platform/tools/i18n-literals.baseline allows
  python3 platform/tools/i18n-scan.py --list FILE...  every candidate of the files, with its line

A line may be exempted with the comment `// i18n-ok: <reason>`; a literal that goes
through the catalogue (`t!`, `tf!`, `tr(`, `i18n::`) is not a raw literal.
"""
import os
import re
import sys
from collections import defaultdict

import subprocess
ROOT = os.environ.get("AURELIA_ROOT") or subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True, cwd=os.path.dirname(os.path.abspath(__file__))).strip()
BASELINE = os.path.join(ROOT, "platform", "tools", "i18n-literals.baseline")

# Crates whose text the player reads (the ratchet applies to them) and the kind of code.
UI_CRATES = ["platform/crates/client", "classic/crates/classic-ui", "lands/crates/lal-ui", "platform/crates/sandbox-ui"]
# Developer-facing text in UI crates and the apps: logs, benches, the snapshot recorder, start-up
# errors printed to the terminal. Not localized, not counted by the ratchet.
DEV_ONLY = ["lands/launcher/", "classic/launcher/", "platform/crates/launcher-core/", "integration/crates/dev-desktop/", "platform/crates/client/src/ai_round.rs", "platform/crates/client/src/perf.rs", "platform/crates/client/src/simulation_driver.rs",
            "platform/crates/client/src/shell/gpu.rs", "platform/crates/client/src/shell/art.rs", "platform/crates/client/src/shell/layout.rs",
            "platform/crates/client/src/shell/snapshot/", "platform/crates/client/src/shell/observer/capture.rs", "platform/crates/client/src/shell/hud/fog_ornaments.rs",
            "platform/crates/client/src/shell/observer/panels.rs", "platform/crates/client/src/shell/app.rs"]
# Rule crates: their strings are messages of the rules (refusals, log lines); the UI words them.
RULE_CRATES = ["platform/crates/kernel", "classic/crates/classic-rules", "lands/crates/lal-rules", "lands/crates/lal-economy", "lands/crates/lal-mapgen",
               "platform/crates/sandbox-rules", "classic/crates/classic-content", "lands/crates/lal-content", "platform/crates/sandbox-content"]
OTHER_CRATES = ["platform/crates/render", "integration/crates/aurelia-qa"]

CYR = re.compile("[\u0400-\u04ff]")
WORD3 = re.compile(r"[^\W\d_]{3,}")
PLACEHOLDER = re.compile(r"\{[a-zA-Z_0-9]*(?::[^}]*)?\}")
CATALOGUE = re.compile(r"\b(t!|tf!|tr\(|tr_n\(|i18n::|words\.|catalog)")
SKIP_LINE = re.compile(
    r"assert|panic!|unreachable!|expect\(|Id::new|id_salt|#\[|include_|env!|log::|println!|eprintln!|debug_assert|"
    r"\.unwrap_or_else\(\|\| panic|todo!|unimplemented!|tracing::|\bdbg!|std::env|\.join\(|Path::new|PathBuf|File::|"
    r"\.arg\(|Command::new|\.contains\(|\.starts_with\(|\.ends_with\(|\.split\(|\.strip_|\.find\(|== \"|!= \"|matches!|"
    r"\.get\(\"|\.insert\(\"|\.entry\(\"|\"[a-z_.-]+\" *=>|\.expect_err|serde|rename|Error::new|io::Error|context\(|\.ok_or\(")
LANG_WORD = re.compile(r"[^\W\d_]")


def lex(src):
    """Yield (kind, start, end, text, line) for string literals outside comments, with
    `depth_skip` marks for test code. Returns the list of (line, text, in_test)."""
    out = []
    i, n, line = 0, len(src), 1
    depth = 0
    test_until = []  # brace depths at which a test block ends
    pending_test = False  # a #[cfg(test)] or #[test] seen, waiting for its `{`
    last_attr_test_line = -10
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        # comments
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            i = j
            continue
        if src.startswith("/*", i):
            d = 1
            j = i + 2
            while j < n and d:
                if src.startswith("/*", j):
                    d += 1
                    j += 2
                elif src.startswith("*/", j):
                    d -= 1
                    j += 2
                else:
                    if src[j] == "\n":
                        line += 1
                    j += 1
            i = j
            continue
        # attributes that open test code
        if c == "#" and src.startswith("#[", i):
            j = src.find("]", i)
            attr = src[i : j + 1] if j > 0 else ""
            if re.match(r"#\[(cfg\(test\)|test|tokio::test|cfg\(all\(test|cfg\(any\(test)", attr):
                pending_test = True
            # an attribute may carry strings; skip it whole (nesting of [] is rare)
            depth_br = 0
            k = i
            while k < n:
                if src[k] == "[":
                    depth_br += 1
                elif src[k] == "]":
                    depth_br -= 1
                    if depth_br == 0:
                        break
                elif src[k] == "\n":
                    line += 1
                k += 1
            i = k + 1
            continue
        if c == "{":
            depth += 1
            if pending_test:
                test_until.append(depth)
                pending_test = False
            i += 1
            continue
        if c == "}":
            if test_until and test_until[-1] == depth:
                test_until.pop()
            depth -= 1
            i += 1
            continue
        if c == ";" and pending_test:
            pending_test = False  # `#[cfg(test)] use ...;`
        # char literal or lifetime
        if c == "'":
            if i + 1 < n and src[i + 1] == "\\":
                j = src.find("'", i + 2)
                i = (j if j > 0 else i) + 1
                continue
            if i + 2 < n and src[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        # raw / byte / normal strings
        m = re.match(r'(b?r(#*)")', src[i : i + 40]) if c in "rb" else None
        if m:
            hashes = m.group(2)
            j = src.find('"' + hashes, i + len(m.group(1)))
            if j < 0:
                break
            text = src[i + len(m.group(1)) : j]
            out.append((line, text, bool(test_until), False))
            line += src.count("\n", i, j)
            i = j + 1 + len(hashes)
            continue
        if c == '"' or (c == "b" and src.startswith('b"', i)):
            is_byte = c == "b"
            j = i + (2 if is_byte else 1)
            start = j
            while j < n and src[j] != '"':
                if src[j] == "\\":
                    j += 1
                j += 1
            text = src[start:j]
            start_line = line
            line += src.count("\n", i, j)
            if not is_byte:
                out.append((start_line, text, bool(test_until), True))
            i = j + 1
            continue
        i += 1
    return out


def unescape(text):
    # keep it simple: line continuations and common escapes
    text = re.sub(r"\\\n\s*", "", text)
    return text.replace("\\n", " ").replace('\\"', '"').replace("\\'", "'").replace("\\\\", "\\")


def visible(text):
    """Does the literal look like text a player reads?"""
    t = unescape(text).strip()
    if not WORD3.search(PLACEHOLDER.sub(" ", t)):
        return False
    if CYR.search(t):
        return True
    has_space = bool(re.search(r"\s", t))
    if not has_space:
        # a single token: visible only as a capitalised word ("Audio", "Turn"), not a key
        return bool(re.fullmatch(r"[A-Z][a-z]{2,}[a-z-]*[.:!?…]?", t))
    # several tokens: skip format-only, paths, code-like
    if re.match(r"^[a-z_]+ = ", t):
        return False  # a `key = value` line of a config file
    if re.search(r"::|=>|->|\.rs\b|\.ron\b|\.png\b|\.cfg\b|\.json\b|https?://|\\u\{|^[#/.\w-]+\.[a-z]+$", t):
        return False
    letters = len(LANG_WORD.findall(t))
    return letters >= 4


def crate_of(path):
    p = path.replace(os.sep, "/")
    if any(p.startswith(d) for d in DEV_ONLY):
        return "dev", p.split("/")[0]
    for group, crates in (("ui", UI_CRATES), ("rules", RULE_CRATES), ("other", OTHER_CRATES)):
        for c in crates:
            if p.startswith(c + "/"):
                return group, c
    return None, None


def is_test_file(p):
    p = p.replace(os.sep, "/")
    base = os.path.basename(p)
    return ("/tests/" in p or "/benches/" in p or "/examples/" in p or base.endswith("_tests.rs") or base in ("tests.rs", "fixtures.rs", "test_module.rs")
            or "/kernel_tests/" in p or "/bin/" in p and "importer" in p or "/dev/" in p and "classic-ui" in p)


def files():
    for top in ("platform/crates", "classic/crates", "lands/crates", "lands/launcher", "classic/launcher", "integration/crates"):
        for dp, dn, fn in os.walk(os.path.join(ROOT, top)):
            dn[:] = [d for d in dn if d not in ("target", ".git")]
            for f in fn:
                if f.endswith(".rs"):
                    full = os.path.join(dp, f)
                    yield os.path.relpath(full, ROOT)


def scan_file(rel):
    """[(line, text, kind)] of raw user-visible candidates in non-test code."""
    if is_test_file(rel):
        return []
    with open(os.path.join(ROOT, rel), encoding="utf-8", errors="replace") as fh:
        src = fh.read()
    lines = src.split("\n")
    res = []
    for line, text, in_test, _ in lex(src):
        if in_test or not visible(text):
            continue
        ctx = lines[line - 1] if 0 < line <= len(lines) else ""
        if "i18n-ok" in ctx or SKIP_LINE.search(ctx) or CATALOGUE.search(ctx):
            continue
        t = unescape(text)
        kind = "ru" if CYR.search(t) else "en"
        if PLACEHOLDER.search(t):
            kind += "+fmt"
        res.append((line, t, kind))
    return res


def scan_all():
    data = {}
    for rel in sorted(files()):
        group, crate = crate_of(rel)
        if not group:
            continue
        found = scan_file(rel)
        if found:
            data[rel] = found
    return data


def load_baseline():
    base = {}
    if os.path.exists(BASELINE):
        for ln in open(BASELINE, encoding="utf-8"):
            ln = ln.strip()
            if ln and not ln.startswith("#"):
                path, n = ln.rsplit(None, 1)
                base[path] = int(n)
    return base


def main(argv):
    mode = argv[1] if len(argv) > 1 else "--inventory"
    if mode == "--list":
        for rel in argv[2:]:
            for line, t, kind in scan_file(rel):
                print(f"{rel}:{line}\t{kind}\t{t[:140]}")
        return 0
    data = scan_all()
    if mode == "--print":
        for rel, found in data.items():
            if crate_of(rel)[0] == "ui":
                print(f"{rel} {len(found)}")
        return 0
    if mode == "--check":
        base = load_baseline()
        bad = []
        for rel, found in data.items():
            if crate_of(rel)[0] != "ui":
                continue
            if len(found) > base.get(rel, 0):
                bad.append((rel, len(found), base.get(rel, 0), found))
        if bad:
            print("i18n-lint: new raw user-visible string literals (use t!/tf! and add the key to assets/lang/en):")
            for rel, now, allowed, found in bad:
                print(f"  {rel}: {now} (allowed {allowed})")
                for line, t, kind in found[allowed:][:5] if allowed < len(found) else found[:5]:
                    print(f"      {rel}:{line}: {t[:100]}")
            return 1
        total = sum(len(f) for r, f in data.items() if crate_of(r)[0] == "ui")
        allowed = sum(base.values())
        print(f"i18n-lint: ok ({total} raw literals in UI crates, baseline {allowed})")
        if total < allowed:
            print("  the baseline can be lowered: sh platform/tools/i18n-lint.sh --update")
        return 0
    # inventory
    rows = defaultdict(lambda: defaultdict(int))
    for rel, found in data.items():
        group, crate = crate_of(rel)
        for line, t, kind in found:
            rows[crate]["total"] += 1
            rows[crate][kind] += 1
            rows[crate]["files"] += 0
        rows[crate]["files"] += 1
    print(f"{'crate':<22}{'literals':>9}{'en':>7}{'en+fmt':>8}{'ru':>7}{'ru+fmt':>8}{'files':>7}")
    tot = defaultdict(int)
    for group in ("ui", "rules", "other"):
        crates = {"ui": UI_CRATES, "rules": RULE_CRATES, "other": OTHER_CRATES}[group]
        print(f"-- {group}")
        for c in crates:
            r = rows.get(c)
            if not r:
                continue
            print(f"{c:<22}{r['total']:>9}{r['en']:>7}{r['en+fmt']:>8}{r['ru']:>7}{r['ru+fmt']:>8}{r['files']:>7}")
            if group == "ui":
                for k, v in r.items():
                    tot[k] += v
    print(f"{'UI total':<22}{tot['total']:>9}{tot['en']:>7}{tot['en+fmt']:>8}{tot['ru']:>7}{tot['ru+fmt']:>8}{tot['files']:>7}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
