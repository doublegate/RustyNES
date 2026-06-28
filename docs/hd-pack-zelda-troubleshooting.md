# HD-Pack troubleshooting — *The Legend of Zelda* (working doc)

A living scratchpad for diagnosing and fixing the remaining HD-pack rendering
issues seen with a *Legend of Zelda* (USA) Mesen HD-pack. It records the
symptoms, the architecture, what has already been ruled out (with code
evidence), the open hypotheses, and the exact diagnostic procedure so a future
session can resume without re-deriving everything.

> **Status:** OPEN. Sprites render correctly; backgrounds are wrong. Root cause
> not yet confirmed — it needs an in-app **HD-Pack Pixel Inspector** reading
> and/or a Mesen2 cross-check (see [Diagnostic procedure](#diagnostic-procedure));
> any fix must preserve byte-identity (see [Constraints](#constraints-on-any-fix)).
> Last updated 2026-06-24.

## How to reproduce

- Build the maximal native binary (HD-pack + debug hooks on):
  `cargo full-run "/path/to/Legend of Zelda.nes"`
  (`full` aggregates `hd-pack` + `debug-hooks` + the rest; see the project
  CLAUDE.md build section.)
- Load the Mesen Zelda HD-pack (the folder/zip with `hires.txt`) the usual way.
- Let the title auto-load and watch the scrolling story text, then start a game /
  enter the old-man cave.

Zelda (USA) is **MMC1 (mapper 1)** with **8 KiB CHR-RAM** (no CHR-ROM), so
*every* tile — background and sprite — is matched by the CHR-RAM content path
(`CalculateHash(palette ++ 16 CHR bytes)`), not the CHR-ROM index path. That is
important: any bug specific to the **content/palette key for backgrounds** would
break BG tiles while leaving sprites fine, which is exactly the observed split.

## Symptoms (observed)

1. **Title scrolling text — partial glyphs.** After the auto-load, the scrolling
   story text is only partially rendered. Example: the long vertical stem of an
   "F" is cut off. The text scrolls **vertically** (fine-Y scroll), so a screen
   cell straddles two vertical nametable tiles.
2. **In-game / cave — black backgrounds.** Starting a game, or inside the
   old-man cave, the backgrounds render **black with nothing drawn**, even though
   the **sprites are placed correctly** (sword, the old man, etc.).

So: **sprite replacement works; background replacement does not.** Backgrounds
are either not matching their HD replacements or are being blanked.

## Architecture recap (where each piece lives)

The HD-pack render path is **output-only** and `hd-pack`-gated; it never touches
the deterministic core (AccuracyCoin / byte-identity must hold — see
[Constraints](#constraints-on-any-fix)). Data flow per frame:

1. **PPU telemetry (`crates/rustynes-ppu/src/ppu.rs`).** While rendering, the PPU
   fills a parallel `HdTileSource` record per pixel (256×240): the CHR tile base
   address, bg/sprite flag, flips, the packed Mesen `PaletteColors`, the absolute
   CHR-ROM tile index (or a CHR-RAM sentinel), per-pixel `offset_x`/`offset_y`,
   `color_mask`, and the covering sprite list. Output-only.
2. **Per-frame snapshots (frontend).** At produce time, under the emu lock, the
   frontend snapshots (a) the framebuffer, (b) the 8 KiB CHR (`$0000..=$1FFF`),
   (c) the watched-memory addresses for conditions. These feed the lock-free
   compositor (`docs/adr/0014`).
3. **Compositor (`crates/rustynes-hdpack/src/hdpack.rs`).** Upscales the base
   framebuffer, draws `<background>` regions, then for each pixel computes the
   tile key, looks up a `<tile>` replacement (exact key → wildcard key), checks
   its `<condition>`s, and blits the matched texel at the pixel's intra-tile
   offset. `<options>disableOriginalTiles` hides the stock tile under a backdrop.

Key matching functions (`hdpack.rs`):

- `chr_ram_key(palette_colors, &content[16]) = calculate_hash(palette ++ content)`
  — the CHR-RAM (content) key. **This is the path Zelda uses.**
- `chr_rom_key(tile_index, palette_colors) = tile_index ^ palette_colors`
  — the CHR-ROM (index) key. Not used by Zelda.
- Lookup is two-stage: exact key (palette-discriminated), then the wildcard key
  (`palette = 0xFFFFFFFF`, the `defaultTile=Y` form).

## What is already confirmed working

These landed earlier in the HD-pack parity work and are believed correct:

- CHR-RAM content matching (`CalculateHash`) — commit `20fc28c`.
- Per-pixel `offset_x`/`offset_y` positioning + Mesen `usePrev` fine-X tile
  selection — commit `63ad021`.
- Run-ahead snapshot timing (CHR snapshot taken from the visible frame) —
  commit `38c60d4`.
- CHR-ROM index matching — commit `f9692dc`.
- `<options>disableOriginalTiles` — commit `afa143e` (suspected to *expose* the
  bug by blanking unmatched BG; see open hypotheses).

The whole P1–P3 + `<addition>` + tileData-hash Mesen2 parity set is implemented
(see `docs/adr/0014`), and **sprites match correctly in this very pack**, which
proves the sprite key path + the snapshot + the blit all work end-to-end.

## Hypotheses RULED OUT (with code evidence)

Each was checked in code this session and does **not** explain the symptoms:

| # | Hypothesis | Evidence it is NOT the cause |
|---|---|---|
| 1 | BG `chr_addr` includes fine-Y, so the content hash reads 16 bytes from the wrong offset for scrolled BG | `ppu.rs` BG fetch: `self.hd_bg_addr_latch = addr & 0x1FF0;` — the low nibble (fine-Y) **is masked**, so the captured address is the tile base. |
| 2 | BG palette packed wrong (so BG never matches but sprites do) | `Ppu::hd_bg_palette_colors` packs the four palette bytes at `base = $3F00 + group*4` (with `pr[0]` the universal backdrop in the high byte) into Mesen's `PaletteColors`; sprites use the `0xFF000000`-tagged form with no `pr[0]`. Both Mesen-faithful. |
| 3 | grayscale/emphasis `color_mask` blacks out the BG replacement | `apply_color_mask` returns `rgb` unchanged when `mask==0`, and otherwise only averages (grayscale) or scales channels by 0.9–1.1 (emphasis). It **never** zeroes a pixel. |
| 4 | CHR snapshot covers only one 4 KiB pattern table, so BG (other table) reads garbage | The snapshot is `$0000..=$1FFF` (`0x2000` = 8 KiB, both tables). `app.rs` resizes/zero-fills to `0x2000`; `emu.rs::capture_hd_chr` fills it. |
| 5 | The per-pixel blit drops the bottom rows (off-by-one) | The composite blit samples `src_ty = rule.y + offset_y*scale` with `if sy >= img_h … continue`; for an 8×8·scale tile region this stays in-bounds for `offset_y ∈ 0..=7`. No systematic bottom cutoff. |
| 6 | The vertical tile boundary isn't tracked (cell straddles two nametable tiles) | `HdTileSource` is **per-pixel**, and the PPU re-fetches per scanline, so each pixel's `chr_addr` is the nametable tile for *that* scanline and `offset_y` is that scanline's fine-Y. The split is handled pixel-for-pixel. |

## Open hypotheses (ranked)

Ordered by suspicion, given "sprites fine, BG black, title text partial":

1. **BG content/palette key mismatch (most likely).** The pack's BG `<tile>`
   rules are keyed under a palette (or exact content) that differs from what the
   game produces at that moment, so the **exact** key misses and (if the rule
   isn't a `defaultTile=Y` wildcard) the **wildcard** key misses too → no match.
   The partial title glyphs fit this: some font tiles' keys align, others don't.
2. **`disableOriginalTiles` exposing the gap.** If the pack sets
   `<options>disableOriginalTiles`, every unmatched BG tile is replaced by the
   backdrop (black in Zelda dungeons). Before that option landed (`afa143e`) the
   unmatched BG showed the *original* NES graphics; now it shows black. This is
   Mesen-faithful behaviour, so it is an **amplifier**, not necessarily the bug —
   the underlying cause is still hypothesis 1.
3. **The pack genuinely lacks dungeon BG art.** Some packs only replace sprites +
   title. If so, black-under-`disableOriginalTiles` is *correct* and matches
   Mesen. The Mesen cross-check below settles this.
4. **Snapshot/run-ahead timing for content that changes per frame.** Lower
   suspicion for Zelda (the alphabet in CHR-RAM is static during the title
   scroll), but worth confirming if run-ahead is enabled.
5. **A real blit bug specific to a sampling state.** Only if the inspector says a
   tile is *APPLIED* yet visibly isn't drawn.

## Diagnostic procedure

Use the in-app **HD-Pack Pixel Inspector**
(`crates/rustynes-frontend/src/debugger/hd_pixel_panel.rs`; needs `hd-pack` +
`debug-hooks`, both in the `full` build). Hover a pixel and it prints:

```text
Pixel (x, y)
base  / final swatches
tile CHR $XXXX   bg | sprite   pal N
flip H _  flip V _            (sprites only)
CHR hash XXXXXXXX
→ exactly one verdict:
    "Replacement APPLIED (image #N)."
    "No replacement rule keys this tile hash."
    "Rule for image #N gated off (a condition failed)."
  + a Conditions list with [hold] / [fail] per condition
```

Collect readings for **three** spots and record them in
[Captured readings](#captured-readings):

1. A **black in-game / cave BG tile** (a wall that should be HD).
2. The **rendered part** of a title-screen "F".
3. The **cut-off part** of that same "F".

Also do the decisive cross-check: **load the identical pack in Mesen2.** Does the
dungeon BG render in Mesen?

## Decision tree (reading → cause → fix area)

- Wall says **"No replacement rule keys this tile hash"** *and* Mesen renders it →
  **BG key mismatch (hypothesis 1).** Compare the inspector's `pal N` + `CHR hash`
  to the pack's `<tile>` lines for that art. Likely a palette-discrimination
  issue: confirm whether the pack keys BG tiles as `defaultTile=Y` (wildcard) or
  per-palette, and whether `hd_bg_palette_colors` reproduces the exact palette the
  pack assumed. Fix in the key computation (`ppu.rs` palette pack and/or
  `hdpack.rs` key/lookup).
- Wall says **"No replacement rule keys this tile hash"** *and* Mesen is **also**
  black there → the **pack lacks dungeon BG** (not a RustyNES bug). Close as
  "pack limitation"; optionally document that `disableOriginalTiles` makes it
  black by design.
- Wall says **"Rule … gated off"** → a `<condition>` is failing. Read the
  `[fail]` condition name and check that predicate's eval against the snapshot in
  `hdpack.rs::eval_condition`.
- The F's cut-off part says **"APPLIED" but isn't visibly drawn** → a real **blit
  bug**; capture `pal`, `CHR hash`, `offset_x/offset_y`, the image # and the
  output (x,y), and inspect the composite blit (`hdpack.rs`, the per-pixel tile
  loop) for that offset.
- The F's cut-off part says **"No replacement rule keys this tile hash"** while
  the rendered part says **APPLIED** → two different font tiles; the boundary tile
  isn't in the pack (hypothesis 1 again, scoped to specific glyph tiles).

## Code reference map

| Concern | Location |
|---|---|
| BG tile-address capture (fine-Y masked) | `crates/rustynes-ppu/src/ppu.rs` — `hd_bg_addr_latch = addr & 0x1FF0` |
| BG / sprite palette-key packing | `crates/rustynes-ppu/src/ppu.rs` — `hd_bg_palette_colors`, `hd_sprite_palette_colors` |
| Per-pixel emit (offset_x/offset_y, cur/next, sprite list) | `crates/rustynes-ppu/src/ppu.rs` — the `hd_tile_source` emit block |
| Key functions + two-stage lookup | `crates/rustynes-hdpack/src/hdpack.rs` — `chr_ram_key`, `chr_rom_key`, `calculate_hash` |
| Compositor + per-pixel blit + `disableOriginalTiles` | `crates/rustynes-hdpack/src/hdpack.rs` — `composite` |
| grayscale/emphasis mask | `crates/rustynes-hdpack/src/hdpack.rs` — `apply_color_mask` |
| Condition eval | `crates/rustynes-hdpack/src/hdpack.rs` — `eval_condition` / `all_hold` |
| CHR snapshot (8 KiB) | `crates/rustynes-frontend/src/emu.rs` — `capture_hd_chr`; `crates/rustynes-frontend/src/app.rs` — `present_chr_snapshot` |
| Pixel Inspector panel | `crates/rustynes-frontend/src/debugger/hd_pixel_panel.rs` |
| HD-pack spec / parity status | `docs/adr/0014-hd-pack-conditions-and-backgrounds.md`, ADR 0018 (real Mesen tile format), `docs/ppu-2c02.md` (HD-pack tile-source export) |
| Mesen2 reference | `ref-proj/Mesen2/Core/NES/HdPacks/` |

## Captured readings

Fill these in during the next session (replace the placeholders):

```text
[1] Black cave/dungeon BG wall tile:
    tile CHR $____   bg   pal __
    CHR hash ________
    verdict: ____________________________________
    conditions: _________________________________
    Mesen renders this tile? (Y/N): __

[2] Title "F" — rendered part:
    tile CHR $____   bg   pal __
    CHR hash ________
    verdict: ____________________________________

[3] Title "F" — cut-off part:
    tile CHR $____   bg   pal __
    CHR hash ________
    verdict: ____________________________________
    (if APPLIED but not drawn) offset_x/offset_y: __ / __  image #: __  out (x,y): __ / __

Pack metadata:
    <options> line (does it contain disableOriginalTiles?): _______________
    A sample BG <tile> line for dungeon art: ______________________________
    Is the pack folder or zip?: ____   Run-ahead enabled?: ____
```

## Constraints on any fix

- HD-pack is **output-only** and `hd-pack`-gated. Any fix must keep the core
  byte-identical (default / `no_std` / wasm builds unchanged) and **AccuracyCoin
  139/139**. The PPU telemetry must not perturb the deterministic framebuffer or
  audio — prove on/off byte-identity for PPU-side changes.
- Touch the chip docs in the same change as chip code (`docs/ppu-2c02.md` for PPU
  telemetry, `docs/adr/0014` for compositor/condition changes).
- No commercial ROMs in the repo; the Zelda ROM + HD-pack stay local. Any
  regression fixture must be synthetic (the existing `hdpack` unit tests are the
  template) or a CC0/public-domain pack.

## Investigation log

- **2026-06-24:** Symptoms reported (partial title glyphs, black in-game BG,
  sprites OK). Ruled out hypotheses 1–6 above by code review. Confirmed the Pixel
  Inspector reports the verdict + `pal` + `CHR hash` needed to pinpoint the
  matching failure. Awaiting inspector readings + a Mesen2 cross-check. Authored
  this document.
