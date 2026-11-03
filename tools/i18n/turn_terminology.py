#!/usr/bin/env python3
"""The project owner's terminology rule (2026-10-20): the player reads "Turn" everywhere; "Round" is gone from the user
texts (English "round" -> "turn", German Runde -> Zug, Spanish ronda -> turno, Japanese ラウンド -> ターン, Russian
round -> turn; French tour and Chinese 回合 were already right). Message KEYS follow: a key part `round` becomes `turn`
(except `lal.view.refusal.rule.*`, whose keys are derived from the rule text the engine emits). Identifiers and
argument names in code are not touched.

  (the Russian rules of this one-shot migration were applied once and are not kept: the repository holds
  Russian text only under assets/lang/ru, see docs/localization.md)

  python3 tools/i18n/turn_terminology.py [--check]      rewrite assets/lang/*/*.ftl and the keys in crates/**/*.rs
"""
import glob, os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
CHECK = '--check' in sys.argv

TEXT = {
    'en': [(r'(?<![$\w])Rounds\b', 'Turns'), (r'(?<![$\w])rounds\b', 'turns'), (r'(?<![$\w])Round\b', 'Turn'),
           (r'(?<![$\w])round\b', 'turn'), (r'\bR\{ \$round \}', 'T{ $round }')],
    'de': [(r'Runden', 'Züge'), (r'Runde', 'Zug'), (r'runden', 'züge'), (r'runde', 'zug')],
    'es': [(r'[Rr]ondas', lambda m: 'Turnos' if m.group(0)[0] == 'R' else 'turnos'),
           (r'[Rr]onda', lambda m: 'Turno' if m.group(0)[0] == 'R' else 'turno')],
    'ja': [(r'ラウンド', 'ターン')],
    'fr': [], 'zh-Hans': [],
}
SKIP_KEY_PREFIX = ('lal.view.refusal.rule.',)


def rekey(key):
    if key.startswith(SKIP_KEY_PREFIX):
        return key
    return re.sub(r'(?<![a-z])round(s?)(?![a-z])', lambda m: 'turn' + m.group(1), key)


def main():
    changed = 0
    keymap = {}
    for path in sorted(glob.glob(os.path.join(ROOT, 'assets/lang/*/*.ftl'))):
        lang = path.split('/')[-2]
        if 'content-classic' in path or lang not in TEXT:
            continue
        out, cur_is_msg = [], False
        for line in open(path, encoding='utf-8').read().split('\n'):
            m = re.match(r'^([a-z0-9@.\-]+)( =.*)$', line)
            if m:
                nk = rekey(m.group(1))
                if nk != m.group(1):
                    keymap[m.group(1)] = nk
                head, body = nk, m.group(2)
                for pat, rep in TEXT[lang]:
                    body = re.sub(pat, rep, body)
                line = head + body
            elif not line.lstrip().startswith('#'):
                for pat, rep in TEXT[lang]:
                    line = re.sub(pat, rep, line)
            out.append(line)
        new = '\n'.join(out)
        if new != open(path, encoding='utf-8').read():
            changed += 1
            if not CHECK:
                open(path, 'w', encoding='utf-8').write(new)
    # keys in code (string literals exactly equal to a renamed key)
    for path in glob.glob(os.path.join(ROOT, 'crates/**/*.rs'), recursive=True):
        s = open(path, encoding='utf-8').read()
        n = s
        for old, new in keymap.items():
            n = n.replace('"%s"' % old, '"%s"' % new)
        if n != s:
            changed += 1
            if not CHECK:
                open(path, 'w', encoding='utf-8').write(n)
    print('files changed: %d, keys renamed: %d' % (changed, len(keymap)))
    return 1 if (CHECK and changed) else 0


sys.exit(main())
