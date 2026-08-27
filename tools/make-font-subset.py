# -*- coding: utf-8 -*-
"""Regenerate the embedded Korean font subset.

The HUD used to load a system Korean font (Malgun Gothic, 12.8 MB) at runtime.
Measured cost: 36.8 MB of process memory, plus a hard dependency on a Windows
font path. Embedding a small subset of Noto Sans KR (SIL OFL 1.1) removes both.

Run this after adding Korean text to the UI, otherwise new syllables render as
tofu boxes. Requires `fonttools` (pip install fonttools brotli) and a copy of
Noto Sans KR; the path below is the one Windows ships.

    python tools/make-font-subset.py

Output: crates/orbit-hud/assets/NotoSansKR-subset.ttf
"""
import glob
import io
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC_DIRS = [
    os.path.join(ROOT, 'crates', 'orbit-hud', 'src'),
    os.path.join(ROOT, 'crates', 'orbit-core', 'src'),
]
NOTO = os.environ.get('NOTO_SANS_KR', r'C:\Windows\Fonts\NotoSansKR-VF.ttf')
OUT_DIR = os.path.join(ROOT, 'crates', 'orbit-hud', 'assets')
OUT = os.path.join(OUT_DIR, 'NotoSansKR-subset.ttf')


def used_chars():
    """Every non-ASCII character in our sources.

    Comments are included on purpose: it costs a few KB and guarantees the set
    is a superset of what any string literal can render.
    """
    chars = set()
    for d in SRC_DIRS:
        for f in glob.glob(os.path.join(d, '**', '*.rs'), recursive=True):
            for ch in io.open(f, encoding='utf-8').read():
                if ord(ch) > 0x00A0:
                    chars.add(ch)
    return chars


def main():
    from fontTools import subset
    from fontTools.ttLib import TTFont
    from fontTools.varLib import instancer

    chars = used_chars()
    text = ''.join(chr(c) for c in range(0x20, 0x7F)) + ''.join(sorted(chars))

    font = TTFont(NOTO)
    # Pin the variable font to Regular so the axis data drops out entirely.
    try:
        font = instancer.instantiateVariableFont(font, {'wght': 400}, updateFontNames=False)
    except Exception as exc:  # non-variable copy of the font
        print('instancing skipped: %s' % exc)

    opts = subset.Options()
    opts.layout_features = ['*']
    opts.name_IDs = ['*']  # keep name/license records for OFL compliance
    opts.notdef_outline = True
    sub = subset.Subsetter(options=opts)
    sub.populate(text=text)
    sub.subset(font)

    os.makedirs(OUT_DIR, exist_ok=True)
    font.save(OUT)
    print('glyphs: %d source chars -> %d KB' % (len(chars), os.path.getsize(OUT) // 1024))


if __name__ == '__main__':
    main()
