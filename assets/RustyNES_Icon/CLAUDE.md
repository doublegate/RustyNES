# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Scoped guidance for `assets/RustyNES_Icon/` — the app-icon assets. Loads in addition to the
project-root `CLAUDE.md` when working in this folder.

## What this is

`make_icon.py` is a self-contained, **parametric** icon generator. It builds the entire master
SVG from code (geometry helpers + the Press Start 2P wordmark baked to vector `<path>`s via
fontTools); some smaller labels remain as `<text>` set in the bundled Press Start 2P font
(committed here as `PressStart2P.ttf`), so that font is needed at regeneration / raster time.
Then it rasterizes to a PNG set and a multi-size `.ico`. Design concept and per-element notes
live in the script's header + docstrings.

`rustynes.svg`, `rustynes.ico`, `icon-1024.png`, and `preview.png` are **generated outputs** that
were copied up from the generator's `out/` directory. These assets are committed and the icon is
wired into the build (the winit window icon, the in-app About dialog, and the README header).

## Regenerating

```bash
# The Press Start 2P font (SIL OFL) is committed here as PressStart2P.ttf; pass a path to override.
python3 make_icon.py [out_dir] [font_path]   # defaults: out_dir=out  font_path=./PressStart2P.ttf
```

Dependencies (pip): `cairosvg`, `Pillow`, `fonttools`.

Output goes to `out/` by default (`out/rustynes.svg`, `out/png/icon-{16..1024}.png`,
`out/rustynes.ico`) — **not** the current dir. The committed files were copied up from there.

## Gotchas

- `rustynes.svg` is generated. Do NOT hand-edit it — change the parameters/drawer functions in
  `make_icon.py` and regenerate, or the next run silently overwrites your edits.
- `PressStart2P.ttf` is committed here (SIL OFL — see `OFL.txt`); the generator uses it by
  default. Pass a different path as the second arg to override.
- Tweak look via the geometry constants near the top (`GEAR_TEETH`, `R_TOOTH_*`, `WORD_TOP`,
  `WORD_BOT`, color consts) and the per-peripheral `draw_*` functions; raster sizes are
  `PNG_SIZES` / `ICO_SIZES`.
- The artwork is original geometric stylization (nothing traced from Nintendo's own artwork),
  but the design does render a stylized "Nintendo" wordmark (a `<text>` element in the SVG) as
  period styling — that is a third-party trademark, so treat it as a deliberate inclusion, not
  an "no trademarks present" guarantee.
