# v1.1.0 · Sprint A — Visual polish & filters  → beta.1

All frontend-side; the core framebuffer is untouched (determinism held). Extension
points: `crates/rustynes-frontend/src/gfx.rs` (wgpu pipeline, already has
letterbox/overscan passes), `ntsc.rs` (current filter), `config.rs` `[video]`,
`debugger/settings_panel.rs` (toggle infra).

## T-110-A1 — Full NES_NTSC composite / S-video filter  (stage 1/2 DONE)

- The current `ntsc.rs` is an explicitly "simplified" 5-tap blur + scanline dim.
  Replace/augment with a proper composite model (phase/chroma/luma artifacts,
  selectable composite vs S-video vs RGB).
- **Refs:** `ref-proj/Mesen2/.../BisqwitNtscFilter.cpp`,
  `ref-proj/nestopia/source/core/NstVideoFilterNtsc.cpp`.
- **Done when:** toggle in settings; screenshot-corpus regression added; perf within budget.
- **Approach (maintainer-chosen):** index-based "true" NES_NTSC — the core emits a
  per-pixel palette-index buffer + per-frame phase; the shader reconstructs a real
  composite signal (Bisqwit GenerateNtscSignal → NtscDecodeLine) with genuine
  artifacts. Done in two stages so each is reviewable.
- **STAGE 1 — core foundation ✅ DONE (2026-06-14):** `rustynes-ppu` now emits a
  parallel `index_framebuffer` (256×240 `u16`, `(emph<<6)|colour`, 0..=511) in the
  same emit path as the RGBA, plus a per-frame `ntsc_phase` (0..=2) snapshotted from a
  free-running master-cycle `dot_counter`. Accessors `Ppu::index_framebuffer()` /
  `ntsc_phase()` routed through `Bus` + `Nes`. Output-only (unit test:
  `rgba_lut[index] == framebuffer[pixel]` for every emitted pixel; phase stays 0..=2
  and crawls across frames) → determinism / AccuracyCoin / `no_std` all unaffected.
- **STAGE 2 — composite shader (TODO):** upload the index buffer as `R16Uint`; port
  Bisqwit `GenerateNtscSignal` (8 samples/px, 12-phase `_bitmaskLut`, emphasis wave) +
  `NtscDecodeLine` (windowed Y/I/Q sum, sine table, contrast/saturation matrix) to
  WGSL at 8× horizontal res; per-row phase = `videoPhase*4 + y*341*8`, decode
  `phase0 = (startCycle+7)%12`. Settings (composite/S-video), scaling, screenshot
  regression. Replaces/augments the current `ntsc.rs` blur.

## T-110-A2 — CRT / scanline WGSL shader post-pass  ✅ DONE (2026-06-14)

- Add a post-process pass (scanlines, aperture/slot mask, optional curvature,
  bloom). Slots after the existing letterbox/overscan passes in `gfx.rs`.
- **Ref:** `ref-proj/tetanes` CRT Easymode shader.
- **Done when:** selectable + tunable in settings; off by default; no perf regression.
- **DONE:** new `crates/rustynes-frontend/src/crt.rs` (`CrtFilter`, mirroring
  `ntsc.rs`): source-row-space parabolic scanlines + a subtle RGB aperture-grille
  mask + brightness compensation, driven by a `params` uniform. Wired into
  `Gfx` (field + `enable_crt`/`disable_crt`/`set_crt_scanline` + a render branch
  that takes priority over NTSC), `[graphics] crt_filter`/`crt_scanline` config,
  the Settings → Display toggle + intensity slider + graphics-reset path, the
  `app.rs` live-apply + `on_gfx_ready` startup init. Off by default (byte-identical
  presentation), frontend-only (no accuracy/determinism impact). WGSL parse+validate
  test in CI; native + wasm-winit + wasm-canvas clippy clean. Remaining for a later
  pass: optional curvature/bloom + a configurable mask intensity (fixed-subtle now).

## T-110-A3 — `.pal` palette-file loading  ✅ DONE (2026-06-14)

- Load 64- or 512-entry `.pal` files into the existing (emphasis,colour)→RGBA LUT.
- **Ref:** Mesen2 per-game palette; puNES palette editor.
- **Done when:** a `.pal` can be loaded from settings + persisted in `[video]`; falls
  back to the built-in palette.
- **DONE:** `rustynes-ppu::build_rgba_lut_from_base` + `Ppu::set_custom_palette`
  (custom 64-entry base, 2C02 composite emphasis) → `Nes::set_custom_palette` (bus
  route). Frontend: `config::parse_pal` (192-byte form; first 64 of longer files),
  `[graphics] palette_file`, a Settings → Display **Load .pal… / Built-in** picker
  (native rfd, deferred after the egui pass), and re-apply on each ROM load /
  startup. Off by default = byte-identical (unit test:
  `build_rgba_lut_from_base(&NES_PALETTE) == built-in composite LUT`); native +
  both wasm clippy clean, no_std clean, AccuracyCoin/oracle unaffected. Later: a
  512-entry full-emphasis `.pal` mode (currently uses the first 64 colours).

## T-110-A4 (stretch) — Pixel-art upscalers (HQ2x / xBR / Scale2x)

- Optional if beta.1 has room. **Ref:** `ref-proj/nestopia/.../NstVideoFilterHq*.inl`,
  `NstVideoFilterxBR.cpp`, `NstVideoFilterScaleX.cpp`.

## Verification
- Screenshot-corpus regression (`tests/golden/` + `screenshots/`); AccuracyCoin/oracle
  unaffected (frontend-only); wasm clippy both flavours green.
