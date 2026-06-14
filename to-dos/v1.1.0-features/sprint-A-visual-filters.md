# v1.1.0 · Sprint A — Visual polish & filters  → beta.1

All frontend-side; the core framebuffer is untouched (determinism held). Extension
points: `crates/rustynes-frontend/src/gfx.rs` (wgpu pipeline, already has
letterbox/overscan passes), `ntsc.rs` (current filter), `config.rs` `[video]`,
`debugger/settings_panel.rs` (toggle infra).

## T-110-A1 — Full NES_NTSC composite / S-video filter

- The current `ntsc.rs` is an explicitly "simplified" 5-tap blur + scanline dim.
  Replace/augment with a proper composite model (phase/chroma/luma artifacts,
  selectable composite vs S-video vs RGB).
- **Refs:** `ref-proj/Mesen2/.../BisqwitNtscFilter.cpp`,
  `ref-proj/nestopia/source/core/NstVideoFilterNtsc.cpp`.
- **Done when:** toggle in settings; screenshot-corpus regression added; perf within budget.

## T-110-A2 — CRT / scanline WGSL shader post-pass

- Add a post-process pass (scanlines, aperture/slot mask, optional curvature,
  bloom). Slots after the existing letterbox/overscan passes in `gfx.rs`.
- **Ref:** `ref-proj/tetanes` CRT Easymode shader.
- **Done when:** selectable + tunable in settings; off by default; no perf regression.

## T-110-A3 — `.pal` palette-file loading

- Load 64- or 512-entry `.pal` files into the existing (emphasis,colour)→RGBA LUT.
- **Ref:** Mesen2 per-game palette; puNES palette editor.
- **Done when:** a `.pal` can be loaded from settings + persisted in `[video]`; falls
  back to the built-in palette.

## T-110-A4 (stretch) — Pixel-art upscalers (HQ2x / xBR / Scale2x)

- Optional if beta.1 has room. **Ref:** `ref-proj/nestopia/.../NstVideoFilterHq*.inl`,
  `NstVideoFilterxBR.cpp`, `NstVideoFilterScaleX.cpp`.

## Verification
- Screenshot-corpus regression (`tests/golden/` + `screenshots/`); AccuracyCoin/oracle
  unaffected (frontend-only); wasm clippy both flavours green.
