#!/usr/bin/env python3
"""Writes the inventory of user-visible text into docs/localization.md (section 1) from
measurements: the literal scanner (tools/i18n-scan.py), the bundles (i18n-check coverage
--files) and the rule data. Run from the repository root:

  python3 tools/i18n/inventory.py [--write]       print (or write between the INVENTORY markers)

START holds the scanner's counts on the commit before the localization work (bdc85183),
measured with the same scanner, so the coverage figure is a before/after."""
import os, re, subprocess, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
START = {  # crate: (literals, english, russian)  [measured on bdc85183 with tools/i18n-scan.py]
    'crates/client': (461, 245, 216), 'crates/classic-ui': (2300, 2053, 247), 'crates/lal-ui': (2243, 210, 2033),
    'crates/sandbox-ui': (30, 30, 0),
}
START_RULES = {'crates/kernel': 293, 'crates/classic-rules': 404, 'crates/lal-rules': 217, 'crates/lal-mapgen': 60,
               'crates/classic-content': 367, 'crates/lal-content': 15, 'crates/sandbox-rules': 6, 'crates/sandbox-content': 1}


def run(*cmd):
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True).stdout


def scanner():
    rows = {}
    for line in run('python3', 'tools/i18n-scan.py', '--inventory').split('\n'):
        m = re.match(r'^(crates/\S+|apps/\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)$', line)
        if m:
            rows[m.group(1)] = tuple(int(x) for x in m.groups()[1:])
    return rows


def count(path, pattern):
    try:
        return len(re.findall(pattern, open(os.path.join(ROOT, path), encoding='utf-8').read(), flags=re.M))
    except OSError:
        return 0


def main():
    now = scanner()
    out = []
    out.append('Measured by `python3 tools/i18n-scan.py --inventory` (a heuristic lexer: string literals outside comments and test code that look like text a player reads; format strings are literals with `{}`). **Before** = commit bdc85183, **now** = this branch.\n')
    out.append('| UI crate | literals before | literals now | removed | of which Russian before |')
    out.append('|---|---:|---:|---:|---:|')
    tb = tn = 0
    for crate, (lit, en, ru) in START.items():
        n = now.get(crate, (0,))[0]
        tb += lit
        tn += n
        out.append(f'| {crate} | {lit} | {n} | {lit - n} | {ru} |')
    out.append(f'| **UI total** | **{tb}** | **{tn}** | **{tb - tn}** | |')
    out.append('')
    cov = 100.0 * (tb - tn) / tb if tb else 0
    out.append(f'**Coverage tracked: {cov:.1f} % of the UI literals of the start are now catalogue messages** (UI crates only; developer-facing text — logs, benches, the snapshot recorder, start-up errors in `apps/` — is excluded and listed in `DEV_ONLY` of the scanner).\n')
    out.append('Rule crates (messages of the rules, words by tables of the UI; not counted as UI): ' + ', '.join(f'{c.split("/")[1]} {n}' for c, n in START_RULES.items()) + ' literals (measured on the start commit). They stay English and are translated by pattern tables in the UI crates (`lal-ui/src/view/refusal.rs`, `classic-ui/src/game/reasons_rules.rs`).\n')
    out.append('Other sources:\n')
    classic = count('assets/lang/en/content-classic.ftl', r'\.name = ')
    classicd = count('assets/lang/en/content-classic.ftl', r'\.desc = ')
    lal = count('assets/lang/en/content-lal.ftl', r'^lal\.')
    out.append(f'* Rule data, Classic (`assets/rules/classic/*.ron`): {classic} names and {classicd} descriptions (units, buildings, technologies, policies, promotions, beliefs, nations, city-states, resources, terrain...), now messages in `assets/lang/en/content-classic.ftl`.')
    out.append(f'* Rule data, Lands & Laws (`crates/lal-content/data/pack/*.ron`): {lal} messages (names, stories, hints, rule texts, event cards) in `assets/lang/*/content-lal.ftl`.')
    out.append('* Reason enums: every typed refusal and `*Block` enum of the rules has one text per variant in `lal-ui/src/view/refusal.rs` and `classic-ui/src/game/reasons.rs`; rule texts that arrive as English strings are matched by pattern tables (hundreds of patterns).')
    out.append('* Plural and gender: hand-written Russian plural code in 11 places of `lal-ui` before (`match (n % 10, n % 100)`), plus `ShellWords`\' own `Plural` type; Classic text used one form for all counts ("1 turns"). Now: plural selects in the bundles.')
    out.append('* Numbers: values were printed as plain digits and `Amount` hard-coded the Russian decimal comma; now `{ NUMBER($n) }` and `i18n::number`.')
    out.append('* Hotkey and legend text: the key tables of the three modules (`key_help`) and the observer\'s key list; keycaps stay as printed on the keyboard, the descriptions are messages.')
    out.append('* Text baked into art: none found (section 11).')
    out.append('* The fonts: Inter has no CJK; see section 8.')
    out.append('')
    out.append('Messages in the catalogue per file and language (`i18n-check coverage --files`, share of the English messages that the language has):\n')
    out.append('```')
    binary = os.path.join(os.environ.get('CARGO_TARGET_DIR', 'target'), 'release', 'i18n-check')
    if os.path.exists(binary):
        out.append(run(binary, 'coverage', '--files'))
    else:
        out.append(run('cargo', '+1.98.1', 'run', '-q', '--release', '-p', 'aurelia-i18n', '--bin', 'i18n-check', '--', 'coverage', '--files'))
    out.append('```')
    text = '\n'.join(out)
    if '--write' in sys.argv:
        p = os.path.join(ROOT, 'docs/localization.md')
        doc = open(p, encoding='utf-8').read()
        a = doc.index('<!-- INVENTORY:BEGIN')
        a = doc.index('\n', a) + 1
        b = doc.index('<!-- INVENTORY:END -->')
        open(p, 'w', encoding='utf-8').write(doc[:a] + text + '\n' + doc[b:])
        print('written')
    else:
        print(text)


main()
