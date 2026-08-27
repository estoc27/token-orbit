# Bundled font

`NotoSansKR-subset.ttf` is a subset of **Noto Sans KR**, containing only the
characters this application renders (see `tools/make-font-subset.py`).

- Upstream: https://github.com/notofonts/noto-cjk
- Copyright: © 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'.
- License: SIL Open Font License, Version 1.1 — full text in [OFL.txt](OFL.txt)

The subset keeps the upstream name and license records, so the license travels
with the file itself as well.

Why it is bundled: the HUD previously loaded a system Korean font at runtime,
which cost 36.8 MB of process memory (measured) and tied the app to a Windows
font path. The 147 KB subset removes both, and makes text identical on every OS.
