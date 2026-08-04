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

### Base scanline pass — gamma + sharpness (v2.2.8 "Aperture II")

The base CRT/scanline pass (`CRT_WGSL`) reads a 16-float uniform
(`rect + crop + params + aux`). Two `aux` slots were added in v2.2.8:

- **`aux.y` — gamma round-trip.** The scanline + aperture-mask *darkening* must
  happen in **linear light** to be perceptually correct. On the native path (an
  sRGB texture + sRGB surface) the sampler decodes and the surface re-encodes, so
  the math is already linear and `aux.y = 0` (the shipped native output is
  byte-identical to pre-v2.2.8). On a plain UNORM path (**WebGL2**, which does
  neither) the host sets `aux.y = 1` and the shader sRGB-decodes on read /
  re-encodes before output — fixing a browser-only gamma error. The decode/encode
  use the **exact IEC 61966-2-1 piecewise sRGB transfer** (`srgb_to_linear` /
  `linear_to_srgb` in `CRT_WGSL`: a linear segment below `0.04045` / `0.0031308`,
  a 2.4-exponent power segment above), i.e. the same curve a hardware sRGB surface
  applies — not a `pow(2.2)` approximation — so the WebGL2 path matches the native
  sRGB path where the two curves would otherwise diverge (the shadows).
- **`aux.x` — scanline sharpness (0..1, default 0.5).** The profile blends from
  the original soft parabola (0) to a narrow Gaussian beam (1) for crisp vertical
  row boundaries instead of the linear-sampler blur. `aux.x = 0` reproduces the
  pre-v2.2.8 profile exactly; it is only visible when scanlines are enabled.

The advanced CRT stacks (CRT-Royale / Guest / Megatron) were already gamma-correct
via their own `gamma_in`/`gamma_out` knobs and are unchanged. Both the desktop
(`crt.rs`) and Android (`gfx.rs`) hosts feed the same `aux`.
