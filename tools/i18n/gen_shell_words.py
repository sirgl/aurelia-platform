#!/usr/bin/env python3
"""One-off migration: turn the Debug dumps of ShellWords::ENGLISH / RUSSIAN into
assets/lang/{en,ru}/shell.ftl and print the Rust builder. Kept for the record (the
shell words are maintained in the .ftl files from now on)."""
import re, sys, os

def parse_dump(path):
    """Debug pretty-print -> {path: str | [str] | ('line', label, help) ...}"""
    lines = open(path, encoding='utf-8').read().split('\n')
    out = {}
    stack = []
    i = 0
    pending_list = None
    def parse_value(i, prefix):
        # returns new index
        line = lines[i].strip()
        m = re.match(r'(\w+): (.*)$', line)
        if not m:
            return i + 1
        name, rest = m.groups()
        key = prefix + [name]
        if rest.startswith('"'):
            out[tuple(key)] = eval_str(rest.rstrip(','))
            return i + 1
        if rest.endswith('{'):  # struct
            i += 1
            while lines[i].strip() not in ('},', '}'):
                i = parse_value(i, key)
            return i + 1
        if rest == '[':  # list
            i += 1
            items = []
            idx = 0
            while lines[i].strip() != '],':
                l = lines[i].strip()
                if l.startswith('"'):
                    items.append(eval_str(l.rstrip(',')))
                    i += 1
                elif l == '(':  # tuple
                    a = eval_str(lines[i + 1].strip().rstrip(','))
                    b = eval_str(lines[i + 2].strip().rstrip(','))
                    items.append((a, b))
                    i += 4
                elif l.startswith('Line {'):
                    lab = eval_str(lines[i + 1].strip()[len('label: '):].rstrip(','))
                    hlp = eval_str(lines[i + 2].strip()[len('help: '):].rstrip(','))
                    items.append(('Line', lab, hlp))
                    i += 4
                else:
                    raise SystemExit('unexpected list line: ' + l)
            out[tuple(key)] = items
            return i + 1
        out[tuple(key)] = rest.rstrip(',')
        return i + 1
    i = 1
    while i < len(lines) and lines[i].strip() != '}':
        i = parse_value(i, [])
    return out

def eval_str(s):
    # Rust Debug string -> python string
    assert s.startswith('"') and s.endswith('"'), s
    body = s[1:-1]
    body = body.replace('\\"', '"').replace("\\'", "'").replace('\\\\', '\\')
    body = re.sub(r'\\u\{([0-9a-f]+)\}', lambda m: chr(int(m.group(1), 16)), body)
    return body



# ---------------------------------------------------------------------------------------
GROUPS = {'main_menu': 'main-menu', 'saves': 'saves', 'options': 'options', 'hud': 'hud', 'observer': 'observer'}
# Templates: path -> argument names in the order of the `{}` in the English text.
TEMPLATES = {
    'saves.saved': ['turn'], 'saves.loaded': ['turn'], 'saves.other_module': ['saved', 'running'],
    'hud.health': ['current', 'max'], 'hud.strength': ['attack', 'defense'], 'hud.ranged': ['strength', 'range'],
    'hud.moves': ['moves'], 'hud.sight': ['sight'], 'hud.population': ['name', 'population'],
    'hud.combat_strength': ['strength'], 'hud.food': ['stored', 'needed'],
    'hud.producing': ['name', 'progress', 'cost'], 'hud.hit_points': ['current', 'max'],
    'observer.round_of': ['round', 'limit'], 'observer.round': ['round'], 'observer.event_line': ['round', 'text'],
    'observer.ev_founded': ['city'], 'observer.ev_captured': ['city', 'power'], 'observer.ev_captured_plain': ['city'],
    'observer.ev_lost': ['city', 'power'], 'observer.ev_lost_plain': ['city'], 'observer.ev_war': ['power'],
    'observer.ev_peace': ['power'], 'observer.ev_learned': ['tech'], 'observer.winner': ['name'],
    'observer.winners': ['names'], 'observer.finale_line': ['round', 'reason'], 'observer.city_place': ['place', 'count'],
    'observer.unit_name': ['kind', 'power'], 'observer.unit_health': ['current', 'max'], 'observer.hp_value': ['current', 'max'],
    'observer.unit_sight': ['sight'],
}
PLURALS = {'options.every_turns', 'hud.turns', 'hud.grows_in'}
SPEEDS_SETTINGS = ['normal', 'fast', 'skip']
QUALITY = ['low', 'medium', 'high']
OBSERVER_KEYS = ['pause', 'speed', 'step', 'grid', 'tab', 'arrows', 'numbers', 'cities', 'reveal', 'follow', 'hover', 'click-city', 'click-unit', 'escape']

def kebab(s):
    return s.replace('_', '-')

def key_of(path):
    parts = list(path)
    if parts[0] in GROUPS:
        return 'shell.' + GROUPS[parts[0]] + '.' + '.'.join(kebab(p) for p in parts[1:])
    return 'shell.' + '.'.join(kebab(p) for p in parts)

def named(text, names):
    it = iter(names)
    return re.sub(r'\{\}', lambda m: '{ $' + next(it) + ' }', text)

def entries(d, lang):
    """-> list of (key, ftl text), in dump order."""
    out = []
    seen_plural = set()
    for path, v in d.items():
        dotted = '.'.join(path)
        if path[-1] == 'plural' and len(path) == 1:
            continue
        # plural groups: path ends with one/few/many under a PLURALS entry
        if len(path) >= 2 and '.'.join(path[:-1]) in PLURALS:
            base = '.'.join(path[:-1])
            if base in seen_plural:
                continue
            seen_plural.add(base)
            forms = {f: d[tuple(base.split('.')) + (f,)] for f in ('one', 'few', 'many')}
            key = key_of(tuple(base.split('.')))
            if lang == 'en':
                # "Every {} turn" / "Every {} turns": one and many differ; if they do not the English keeps its habit.
                if forms['one'] == forms['many']:
                    out.append((key, named(forms['many'], ['n'])))
                else:
                    out.append((key, select(forms, ['n'], {'one': 'one', 'other': 'many'})))
            else:
                out.append((key, select(forms, ['n'], {'one': 'one', 'few': 'few', 'many': 'many', 'other': 'few'})))
            continue
        if isinstance(v, list):
            base = path
            for i, item in enumerate(v):
                if base == ('options', 'quality_levels'):
                    k = 'shell.options.quality-' + QUALITY[i]
                    out.append((k + '.label', item[1]))
                    out.append((k + '.help', item[2]))
                elif base == ('options', 'speeds'):
                    out.append(('shell.options.speed-' + SPEEDS_SETTINGS[i], item))
                elif base == ('observer', 'speeds'):
                    out.append(('shell.observer.speed-%d' % (i + 1), item))
                elif base == ('observer', 'keys'):
                    k = 'shell.observer.key-' + OBSERVER_KEYS[i]
                    out.append((k + '.keys', item[0]))
                    out.append((k + '.what', item[1]))
                else:
                    raise SystemExit('list ' + str(base))
            continue
        if dotted in TEMPLATES:
            out.append((key_of(path), named(v, TEMPLATES[dotted])))
            continue
        if path[-1] in ('label', 'help') and len(path) >= 3:
            out.append((key_of(path), v))
            continue
        out.append((key_of(path), v))
    return out

def select(forms, names, mapping):
    var = names[0]
    lines = ['{ $%s ->' % var]
    done = {}
    for cat, form in mapping.items():
        text = named(forms[form], names)
        done[cat] = text
    order = ['one', 'few', 'many']
    for cat in order:
        if cat in done:
            lines.append('    [%s] %s' % (cat, done[cat]))
    lines.append('   *[other] %s' % done['other'])
    lines.append('}')
    return '\n'.join(lines)

def ftl(entries_, header):
    out = [header]
    last = None
    for k, v in entries_:
        section = '.'.join(k.split('.')[:2]) if k.count('.') >= 2 else 'shell'
        if section != last:
            out.append('')
            out.append('## ' + section)
            last = section
        if '\n' in v:
            body = v.split('\n')
            out.append('%s = %s' % (k, body[0]))
            out.extend(body[1:])
        else:
            out.append('%s = %s' % (k, v))
    return '\n'.join(out) + '\n'

def main():
    base, root = sys.argv[1], sys.argv[2]
    for lang in ('en', 'ru'):
        d = parse_dump(os.path.join(base, lang + '.txt'))
        es = entries(d, lang)
        header = '# The shell: menu, settings, HUD frame, observer. Source of truth: en.'
        if lang != 'en':
            # the translated file keeps its own header line (the repository holds no Russian outside assets/lang/ru)
            old = os.path.join(root, 'assets/lang', lang, 'shell.ftl')
            if os.path.exists(old):
                header = open(old, encoding='utf-8').readline().rstrip('\n')
        text = ftl(es, header)
        os.makedirs(os.path.join(root, 'assets/lang', lang), exist_ok=True)
        open(os.path.join(root, 'assets/lang', lang, 'shell.ftl'), 'w', encoding='utf-8').write(text)
        print(lang, len(es), 'entries')

if len(sys.argv) > 2:
    main()
