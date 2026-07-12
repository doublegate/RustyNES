# CRT / Composite Video

**References:** the authoritative detail is in [`frontend.md`](frontend.md) (§ Display pipeline / shader ladder); the palette generator is in [`ppu-2c02.md`](ppu-2c02.md). This page is a curated handbook entry point.

RustyNES reproduces the NES's analog look with GPU post-passes over the PPU
framebuffer. Every filter here is **display-only**: it never touches the core, the
index framebuffer, audio, or any golden vector — the `visual_regression` corpus
stays byte-identical with any filter active (introduced in v2.1.2 "Prism").

## In-core generated NTSC palette

Rather than ship a hand-authored RGB table, RustyNES *generates* its base palette
from a model of the 2C02's composite-video output:
`rustynes_ppu::generate_base_palette` (a Bisqwit / ares YIQ integration), with the
standard 2C02 composite emphasis applied. This is a core function (deterministic,
no GPU), so the same colors appear headless and on screen.

## The shader ladder (v2.1.2 "Prism")

Presentation filters run as GPU post-passes. Two selection surfaces coexist:

- **Legacy single-select** (Settings → Video): an **NTSC filter** dropdown
  (`[graphics] ntsc_filter` = `off` / `composite` / `rgb` / `composite-rt`) plus a
  binary **CRT** toggle (`crt_filter` + `crt_scanline`). The `composite-rt`
  (Bisqwit) option is the only place the Bisqwit picture knobs (contrast /
  saturation / brightness / hue) have a UI.
- **Composable stack** (Settings → Shaders): add / reorder / toggle / remove any
  of the six `BuiltinPass` variants, each with `#pragma parameter` sliders, plus a
  preset bank and constrained `.slangp` / `.cgp` import.

**Precedence:** when the stack has any enabled pass it owns the post-process path
and the legacy single-select is bypassed; otherwise the legacy filter applies.
The fixed render order is: stack → CRT → Bisqwit → NTSC → direct blit
(`Gfx::render_with_overlay`).

### The three composite rungs

1. **`Ntsc`** — a cheap simplified blur (5-tap + scanline dim + coarse fringe);
   not a real signal encode/decode.
2. **`Lmp88959`** — a real single-pass composite encode→decode (the EMMIR/LMP
   model), an RGBA post-pass that composes anywhere in the stack.
3. **`CompositeRt`** — the faithful **Bisqwit** per-dot composite
   (`bisqwit.wgsl`, `rustynes-gfx-shaders`); it samples the `R16Uint` palette-
   **index** framebuffer, so it must be the first pass in the stack.

The shared WGSL lives in `crates/rustynes-gfx-shaders`. See
[`frontend.md`](frontend.md) for the full pipeline, the CRT / scanline passes,
and the preset / import machinery.
