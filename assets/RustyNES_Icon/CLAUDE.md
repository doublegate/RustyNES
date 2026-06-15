# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Scoped guidance for `assets/RustyNES_Icon/` — the app-icon assets. Loads in addition to the
project-root `CLAUDE.md` when working in this folder.

## What this is

`make_icon.py` is a self-contained, **parametric** icon generator. It builds the entire master
SVG from code (geometry helpers + the Press Start 2P wordmark baked to vector `<path>`s via
fontTools — no runtime font dependency), then rasterizes to a PNG set and a multi-size `.ico`.
Design concept and per-element notes live in the script's header + docstrings.

`rustynes.svg`, `rustynes.ico`, `icon-1024.png`, and `preview.png` are **generated outputs** that
were copied up from the generator's `out/` directory. This folder is currently untracked and the
icons are not yet wired into the build.

## Regenerating

```bash
# Requires the Press Start 2P font (OFL) — NOT committed here; download PressStart2P.ttf first.
python3 make_icon.py [out_dir] [font_path]   # defaults: out_dir=out  font_path=./PressStart2P.ttf
```

Dependencies (pip): `cairosvg`, `Pillow`, `fonttools`.

Output goes to `out/` by default (`out/rustynes.svg`, `out/png/icon-{16..1024}.png`,
`out/rustynes.ico`) — **not** the current dir. The committed files were copied up from there.

## Gotchas

- `rustynes.svg` is generated. Do NOT hand-edit it — change the parameters/drawer functions in
  `make_icon.py` and regenerate, or the next run silently overwrites your edits.
- `PressStart2P.ttf` is not in the repo; the generator fails without it. Fetch the OFL font and
  pass its path (or place it here as `./PressStart2P.ttf`).
- Tweak look via the geometry constants near the top (`GEAR_TEETH`, `R_TOOTH_*`, `WORD_TOP`,
  `WORD_BOT`, color consts) and the per-peripheral `draw_*` functions; raster sizes are
  `PNG_SIZES` / `ICO_SIZES`.
- All artwork is original geometric stylization — no traced/trademarked Nintendo assets.
