# ADR 0013 — Composable post-process shader stack + CRT preset bank

**Status:** Accepted.
**Date:** 2026-06-15
**Author:** RustyNES maintainers
**Relates to:** the v1.2.0 "Curator" release plan (Workstream C2); the existing
`rustynes-frontend` post-process filters (`crt.rs`, `ntsc.rs`,
`ntsc_bisqwit.rs`); ADR 0009-adjacent presentation work (overscan crop, 8:7 PAR).

## Context

Through v1.1.0 the frontend's post-process video path was a **single-select**
chain: at render time `Gfx::render_with_overlay` ran exactly one of the CRT, the
simplified NTSC blur, or the true-composite (Bisqwit) NTSC filter (an
`if let Some(crt) … else if let Some(bisqwit) … else if let Some(ntsc) … else
direct-blit` ladder). Each filter samples the NES texture (or, for Bisqwit, the
`R16Uint` palette-index texture) and blits straight to the swapchain with the
shared letterbox + overscan-crop uniform.

The v1.2.0 plan (Workstream C2, inspired by GeraNES' `ShaderPass` system) calls
for a **composable** stack — combine, say, a composite-NTSC pass *and* a CRT
scanline pass — plus a user-tunable parameter model and a saveable CRT preset
bank.

The hard constraint is the determinism / "no presentation regressions" contract:
**the default build and any pre-C2 `config.toml` must present a byte-for-byte
identical image.** A risky rewrite of the existing blit path was explicitly off
the table.

Out of scope (rejected): importing RetroArch `.slangp` shader presets, or any
GLSL->WGSL translation layer. Those are large, fragile surface areas with poor
payoff for a built-in-shader emulator; the stack ships a small set of curated
built-in WGSL passes instead.

## Decision

Add a **parallel, additive** shader-stack system rather than replacing the
single-select ladder.

1. **Config** (`shader_pass::ShaderStackConfig`, persisted at
   `[graphics] shader_stack` with `#[serde(default)]`): an ordered
   `Vec<ShaderPassDesc { id, enabled, params }>`. The serde default is an **empty
   stack**, so every existing config and the shipped default deserialize to "no
   stack".

2. **Render path:** `Gfx` gains an `Option<ShaderStack>`. It is `Some` only when
   `ShaderStackConfig::has_enabled_passes()` is true (≥1 enabled, recognized
   pass). The render ladder gains one leading arm: `if let Some(stack) = …`. When
   the stack is `None` — the default — control falls through to the **unchanged**
   pre-C2 ladder. This is the byte-identical guarantee: the empty-stack code path
   is literally the old code, untouched.

3. **Execution:** an active stack owns the post-process path. It renders enabled
   passes by ping-ponging two NES-resolution intermediate render targets (in the
   NES texture's sRGB-matched format, so the round-trip stays identity); only the
   final pass applies the letterbox + overscan crop and writes the swapchain.
   Intermediate passes use an identity transform (full `[0,1]` UV, no crop).

4. **Bisqwit composite-rt is special-cased:** it samples the `R16Uint`
   palette-index texture, not RGBA, so it can only be the **first** pass; the
   stack drops a `composite-rt` pass found at any other position. Its live
   `NtscKnobs` (Workstream C1) are forwarded into the stack pass.

5. **Parameters:** built-in passes declare knobs with RetroArch-style
   `#pragma parameter <name> "<label>" <default> <min> <max> <step>` header
   comments (valid WGSL comments). `shader_pass::parse_pragma_parameters` parses
   them — mirroring GeraNES' `parseShaderParameters` — to drive generic egui
   sliders. Per-pass overrides persist in the config.

6. **Preset bank** (`shader_pass::ShaderPresetBank`, `[graphics.shader_presets]`,
   `#[serde(default)]` empty): named saved stacks, with Save / Load / Delete UX
   and a built-in CRT bank (Sharp / Classic / Heavy-Aperture) that reuses the
   existing `crt.rs` shader at varying `scanline` / `mask` values.

The existing `CrtFilter` / `NtscFilter` / `NtscBisqwitFilter` types and their
single-select wiring are left intact; the stack reuses their *shader source*
(now `pub(crate)`) through a generic pass-compiler, so their behaviour cannot
drift.

## Consequences

- **Byte-identical default holds by construction:** an empty stack never builds
  a `ShaderStack`, so the default render path is the unchanged pre-C2 code. The
  core, AccuracyCoin, and oracle results are untouched (presentation-only).
- **Two filter systems coexist.** When the stack is active the legacy filters are
  cleared so they never both render. The single-select NTSC/CRT settings UI
  remains for users who don't want the stack; the stack is opt-in behind a
  collapsing header. The minor cost is two parallel paths to maintain.
- **wasm-safe:** the intermediate RTs are NES-resolution (256×240) and the passes
  reuse already-WebGL2-validated shaders, so the WebGPU/WebGL build and size
  budget are unaffected. No storage buffers, no dynamic value-array indexing.
- **No new accuracy surface:** the stack lives entirely in `rustynes-frontend`;
  the chip crates are untouched.
