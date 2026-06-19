# 17. HD-Pack Builder (in-emulator pack authoring)

Date: 2026-06-18

## Status

Accepted (v1.7.0 "Forge" Workstream G5).

## Context

RustyNES has *played* Mesen-style HD-packs since v1.2.0 (ADR 0013/0014; the
`crate::hdpack` compositor behind the default-off `hd-pack` feature). What it
lacked was a way to *author* one. Mesen2 ships an "HD Pack Builder": you play the
game and it records every distinct background/sprite tile the PPU draws, keyed by
the same CRC-32-of-the-16-CHR-bytes hash the pack-loader substitutes on, then
emits a `hires.txt` manifest plus a packed `tiles.png` tile sheet. An artist
repaints the sheet at hi-res and the result loads straight back through the
existing pack pipeline. Without a builder, a creator has to hand-author the
`hires.txt` and dump tiles by hand — the workstream-G "play any NES / create for
any NES" reach this release is about.

The constraint is the same one ADR 0014 navigated: the render path copies the
framebuffer under a **brief** `Arc<Mutex<EmuCore>>` lock and then
composites/presents with the lock **not** held (the present branch runs with
`nes = None`). So an authoring recorder must not read live `Nes` state while it
runs, and — critically — it must add **zero** determinism surface: the shipped /
native / `no_std` / wasm builds must stay byte-identical and AccuracyCoin must
hold 100% (139/139).

## Decision

Add an **output-only, native-only** recorder (`crate::hdpack_builder`, gated with
`hd-pack`) that reuses the snapshots the present path *already captures* for the
compositor and writes a Mesen-compatible starter pack.

- `HdPackBuilder::observe(framebuffer, tile_source, chr_peek)` is fed, after the
  emu lock drops, exactly the three things the compositor consumes: the **stock**
  256x240 RGBA framebuffer (`present_staging`), the per-pixel
  `rustynes_ppu::HdTileSource` telemetry (`present_hd_tiles`), and the 8 KiB CHR
  snapshot (`present_chr_snapshot`). For each visible 8x8 cell whose dominant
  pixel references a real tile, it computes the **same Mesen CRC-32** key the
  loader keys on (`crate::hdpack::crc32` over the 16 un-flipped CHR bytes) and, on
  first sight of a key, lifts that cell's native 8x8 RGBA pixels out of the
  framebuffer and stores them. Repeat tiles dedup by CRC (flip- and
  palette-agnostic, mirroring the loader's key).
- The present path's under-lock snapshot block (previously gated on
  `hd_compositor.is_some()`) now also fires when the builder is recording, so the
  recorder gets the same tile-source + CHR snapshot. The watched-memory snapshot
  (ADR 0014) is still compositor-only.
- `HdPackBuilder::write_pack(dir)` emits `tiles.png` (a packed RGBA8 sheet, 16
  tiles per row, encoded with the same `png` crate the loader decodes with) and
  `hires.txt` in the **real Mesen `<ver>106` format** (ADR 0018): `<ver>106` /
  `<scale>` / `<img>tiles.png` + one
  `<tile>bitmapIndex,tileData,palette,x,y,brightness,defaultTile` rule per
  distinct tile, in stable insertion order, where `tileData` is the captured
  16 CHR bytes as 32 hex chars (Mesen's match key) and `bitmapIndex` is `0`.
  This makes the builder's output consumable by real Mesen tooling, not just the
  RustyNES loader.
- A two-item HD-Pack submenu (`Build HD Pack (Record)` / `Stop & Save HD Pack`)
  plus `MenuAction::HdPackBuilderStart` / `HdPackBuilderStop` drive it. Recording
  needs a loaded ROM and an unrestricted (non-replay/non-netplay) session.

## Consequences

- **Determinism-safe, byte-identical.** The recorder reads only the
  already-deterministic snapshots taken under the lock; it mutates no emulation
  state, holds no lock during its CPU-heavy hashing, and is never serialized into
  a save-state. The whole module + the widened snapshot gate are
  `#[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]`, so with the
  feature off — i.e. the shipped / wasm / `no_std` builds — there is no change at
  all. AccuracyCoin holds 100% (139/139); the oracle is untouched.
- **Round-trips through the loader.** A unit test writes a captured pack and
  re-loads it via `HdPack::load`, proving the emitted `hires.txt` + `tiles.png`
  are loader-compatible (the authoring and playback halves can't drift).
- **Native pixels, not a stylized capture.** Tiles are captured as the renderer
  actually drew them (the stock framebuffer), so the artist starts from the true
  in-game appearance. A given tile is captured under whatever palette it first
  appeared in (the CRC key is palette-agnostic) — the same simplification Mesen
  makes; per-palette variants are a future extension.
- **Scope.** This is the v1.2.0-era "first cut" of authoring: distinct-tile
  capture + sheet + manifest. Not yet covered (future work, no architectural
  change needed): condition/background authoring, sprite-vs-BG separation in the
  sheet, per-palette tile variants, and `.zip` pack output. HD-pack remains an
  output-only, default-off, native-oriented feature.
