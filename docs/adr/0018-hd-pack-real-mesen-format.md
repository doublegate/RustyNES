# 18. HD-pack loader parses the real Mesen `hires.txt` format

Date: 2026-06-18

## Status

Accepted (v1.7.0 "Forge", task #56 bug fix).

## Context

The v1.2.0–v1.6.0 HD-pack loader (`crates/rustynes-frontend/src/hdpack.rs`,
behind the default-off `hd-pack` feature; ADR 0013/0014) parsed an **invented**
`<tile>` grammar — `hash,image,x,y[,condition]` (and a reversed
`image,x,y,hash` form) with a trailing comma-joined condition reference. No real
Mesen HD-pack uses that layout, so **every real pack failed to load**: each
`<tile>` line failed to parse `x`/`y` (it mis-read the real Y coordinate as a
condition name), `parse_tile_fields` returned `None` for every tile, the rule
set came out empty, `finish()` returned `None`, and `load()` returned `None` —
surfacing the red "hires.txt" status-bar error (user-reported task #56).

The real Mesen (`<ver>` ≈ 100..=200) format — confirmed against
`Mesen2/Core/NES/HdPacks/HdPackLoader.cpp` + `HdData.h` and a real `<ver>106`
*Zelda Remastered* pack — is materially different:

- A `<tile>` line is
  `bitmapIndex,tileData,palette,x,y,brightness,defaultTile[,chrBankPage,tileIndex]`.
  `tileData` is **32 hex chars = the tile's 16 CHR bytes**, and it (with the
  palette, for CHR-RAM tiles) IS the match key — not a position.
- `<img>` declarations are referenced by **declaration index** (`bitmapIndex`),
  not by filename in the tile line.
- Per-line conditions are a **`[Cond1&Cond2]` prefix** before the tag, AND-joined,
  and every `<condition>` implicitly declares an inverted `!name` twin a
  `[!name]` prefix may reference. Condition memory addresses/operands/masks are
  parsed as **hex** (`HexUtilities::FromHex`), so `16` means `0x16`.
- `<background>` is `name,brightness[,hScroll,vScroll][,priority][,left,top][,blendMode]`.
- `<ver>`, `<options>`, `<supportedRom>`, `<overscan>`, `#`-comments, and CRLF
  line endings all appear in real packs.

## Decision

Port the real Mesen layout into the existing supported-subset parser, keeping
the lock-safe snapshot + condition architecture of ADR 0014 unchanged:

- `parse_tile_fields` reads the real field order and converts the 32-hex
  `tileData` into the **CRC-32 of the 16 CHR bytes** — the exact key
  `HdCompositor::composite` already computes from the live CHR snapshot
  (`hash_tile`). So a real pack's tiles now key the substitution map directly.
- The line dispatcher strips a leading `[...]` condition prefix and feeds the
  AND-joined names to the existing name→index resolver; `<tile>`/`<background>`
  no longer carry a trailing condition field.
- Every `<condition>` registers a base + an inverted `!name` twin (`Condition`
  gains an `inverted` flag honoured in `eval_condition`).
- Condition memory addresses/operands/masks parse as hex.
- `<img>` is interned in declaration order; the tile `bitmapIndex` indexes it.
- The G5 **HD-Pack Builder** (`hdpack_builder.rs`, ADR 0017) now **emits** the
  real `<ver>106` `<tile>bitmapIndex,tileData,palette,x,y,brightness,defaultTile`
  form (the captured 16 CHR bytes as the `tileData` match key), so author→load
  round-trips AND its output is consumable by real Mesen tooling.

## Consequences

- **Real packs load.** The reference *Zelda Remastered* `<ver>106` pack parses
  from 0 → **15,849** tile rules (372 images, 3 backgrounds); the red error is
  gone. A committed synthetic `<ver>106` fixture (`SAMPLE_VER106`) regression-
  guards the parser; copyrighted PNG/OGG assets are never committed (a local-only
  `RUSTYNES_HDPACK_LOCAL`-gated `#[ignore]` test verifies against a real pack).
- **Still a documented subset.** The CRC-of-CHR match key is palette-agnostic
  (RustyNES's PPU telemetry has no palette-discriminated tile identity), so the
  real `palette` field is parsed-and-ignored. Tiles gated *only* on a still-
  unsupported neighbour/position condition (`tileNearby`, `tileAtPosition`,
  `spriteNearby`, `spriteAtPosition`, `positionCheck*`) are dropped, not mis-
  applied. `<addition>`/`<fallback>`/`<patch>`/`<overlay>` and blend/parallax
  remain inert (ADR 0014).
- **Output-only, byte-identical.** All of this is `#[cfg(feature = "hd-pack")]`
  frontend code; the core is untouched, AccuracyCoin holds 100% (139/139), and
  with `hd-pack` off the shipped / native / `no_std` / wasm builds stay byte-
  identical.
