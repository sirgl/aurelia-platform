#!/usr/bin/env python3
"""Subsets the CJK fallback fonts to the glyphs the bundles use (docs/localization.md, section 8).

  python3 tools/i18n/subset_fonts.py [--source /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc]

Writes assets/fonts/NotoSansSC-Subset.otf (Simplified Chinese glyph forms; used for zh-Hans
and for any Han character the Japanese font lacks) and assets/fonts/NotoSansJP-Subset.otf
(Japanese forms: kana and kanji of the ja bundle). Noto Sans CJK is under the SIL Open Font
License (assets/fonts/OFL.txt). Needs fontTools (`pip install fonttools`, user space).
Characters the shipped subset lacks (an external bundle with rare characters) fall back to a
system CJK font at run time when one exists (client::shell::theme::system_cjk_font)."""
import glob, os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
try:
    from fontTools import ttLib, subset
except ImportError:
    sys.exit('fontTools is needed: pip install --user fonttools')

SOURCE = '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
if '--source' in sys.argv:
    SOURCE = sys.argv[sys.argv.index('--source') + 1]


def chars_of(lang_dirs):
    out = set()
    for d in lang_dirs:
        for path in glob.glob(os.path.join(ROOT, 'assets/lang', d, '*.ftl')):
            text = open(path, encoding='utf-8').read()
            out.update(c for c in text if ord(c) > 0x7F)
    return out


def face_index(ttc, family_tail):
    for i, font in enumerate(ttLib.TTCollection(ttc).fonts):
        name = font['name'].getDebugName(1) or ''
        if name == family_tail:
            return i
    sys.exit(f'no face ending in {family_tail!r} in {ttc}')


def build(out_name, family_tail, dirs, extra_ranges):
    chars = chars_of(dirs)
    for lo, hi in extra_ranges:
        chars.update(chr(c) for c in range(lo, hi + 1))
    index = face_index(SOURCE, family_tail)
    options = subset.Options()
    options.layout_features = ['kern', 'vert', 'vrt2', 'palt']
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]  # keep the license notes
    options.notdef_outline = True
    options.glyph_names = False
    options.hinting = False
    options.desubroutinize = True
    options.font_number = index
    font = subset.load_font(SOURCE, options)
    subsetter = subset.Subsetter(options)
    subsetter.populate(text=''.join(sorted(chars)))
    subsetter.subset(font)
    path = os.path.join(ROOT, 'assets/fonts', out_name)
    subset.save_font(font, path, options)
    print(f'{out_name}: {len(chars)} characters, {os.path.getsize(path)} bytes')


# Fullwidth punctuation, CJK symbols; kana for Japanese.
COMMON = [(0x2000, 0x206F), (0x3000, 0x303F), (0xFF01, 0xFF5E), (0xFFE0, 0xFFE6)]
build('NotoSansSC-Subset.otf', 'Noto Sans CJK SC', ['zh-Hans', 'zh-Hant'], COMMON)
build('NotoSansJP-Subset.otf', 'Noto Sans CJK JP', ['ja'], COMMON + [(0x3040, 0x30FF), (0x31F0, 0x31FF), (0xFF66, 0xFF9F)])
