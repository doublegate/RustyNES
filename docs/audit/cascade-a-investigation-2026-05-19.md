# Cascade A — `VerifySpriteZeroHits` Step 2 Investigation (2026-05-19)

**Status:** Root cause identified + minimal fix prototyped + ROLLED BACK
due to 59-of-60 commercial-ROM-snapshot regression. The fix is correct
per the nesdev wiki + Mesen2 reference, but it shifts BG pixel
positioning by one column for every game, requiring a coordinated
visual-baseline re-capture (gated on explicit user authorisation per
session-6's precedent for the OAMADDR re-baseline).

## TL;DR

- **Root cause** of `VerifySpriteZeroHits` step 2 failure: the BG
  shift-register pipeline reloads tile data ONE PPU CLOCK TOO EARLY
  vs Mesen2 / nesdev wiki. Per nesdev "PPU rendering": "The shifters
  are reloaded during ticks 9, 17, 25, …, 257." Per Mesen2
  `NesPpu.cpp:670` `LoadTileInfo` `case 1` of `(_cycle & 0x07)`: the
  reload fires at cycles 1, 9, 17, …, 257, 321, 329 (i.e., phase 0 of
  each 8-cycle fetch group). Our `crates/nes-ppu/src/ppu.rs:971`
  reloads at phase 7 (cycles 8, 16, …, 256, 328, 336). Additionally,
  Mesen2 (`ProcessScanlineImpl` lines 881-884) calls `DrawPixel`
  BEFORE `ShiftTileRegisters`; our `tick` shifts BEFORE
  `emit_pixel`. The combined effect: BG-pattern bit 7 of the freshly
  pre-fetched tile lands at shift-register bit 15 (the emit read
  point) at PPU cycle 8 = pixel column 7 instead of cycle 9 = pixel
  column 8.
- **Visible impact** on AccuracyCoin: for multi-pixel BG tiles (the
  vast majority — every game's text + background) the off-by-one is
  invisible because every pixel of every column is opaque. For
  single-pixel-pattern BG tiles (e.g., AccuracyCoin's tile `$C0`,
  which has exactly one opaque pixel at (col 0, row 0)) the off-by-
  one places the dot at the wrong screen column, breaking three
  sprite-zero-hit tests that depend on single-pixel BG/sprite
  geometry.
- **Tests gated** on the fix (3 tests + the `Sprite 0 Hit behavior`
  sub-test 13 = Test D):
  - `Sprite Evaluation :: Suddenly Resize Sprite [error 1]` —
    `JSR VerifySpriteZeroHits` prerequisite at the start, fails when
    step 2's expected sprite-zero hit doesn't fire.
  - `PPU Behavior :: $2007 read w/ rendering [error 1]` — same
    prereq.
  - `PPU Misc. :: Stale Sprite Shift Regs [error 1]` — same prereq.
  - `Sprite Evaluation :: Sprite 0 Hit behavior` advances from error
    13 → error 14+ once Test D ("sprite-in-BG-hole") is unblocked.
- **Pass-rate delta if applied:** +3 to +6 tests, AccuracyCoin
  `79.86% → 81.30%` to `82.01%`. Within the v0.9.x 0.80 target;
  contributes toward v1.0.0 0.90 gate.
- **Why ROLLED BACK:** 59 of 60 `external_real_games` commercial-ROM
  framebuffer FNV-1a hash snapshots regress (the BG pixel shift is
  observable in every game's framebuffer). `m22_vrc2a_chr_banking`,
  `mmc1_a12_non_mmc3_a12_is_inert`, and `instr_test_basics_frame_60`
  visual snapshots also regress for the same reason. None of these
  are FUNCTIONAL regressions (the games still play correctly and the
  text is still legible — the change is a 1-column shift of where
  BG tiles land), but they violate the HARD CONSTRAINT #1
  (`NEVER regress any of: 60 commercial-ROM oracle`). Re-baselining
  is the correct response, but it requires explicit user
  authorisation (matching session-6's OAMADDR re-baseline pattern at
  audit doc `accuracycoin-readme-analysis-2026-05-17.md`
  §"Addendum (2026-05-19, session 6)").

## The puzzle (geometry)

`VerifySpriteZeroHits` step 2 setup (from `AccuracyCoin.asm:18089-18113`,
called from `TEST_SuddenlyResizeSprite` at line 1438):

- `OAM[0..4] = [Y=$05, tile=$C0, attr=$03, X=$08]` (sprite 0).
- BG tile `$C0` written to PPU address `$2C21` (NT 3, col 1, row 1).
- `v = t = $2C00` (loaded via `SetPPUADDRFromWord .byte $2C, $00`).
- `PPUMASK = $18` (BG + sprite, leftmost-8 mask).
- `PPUCTRL = 0` (sprite + BG pattern tables both at `$0000`).
- Pattern: `EnableRendering` (during VBL) → `WaitForVBLSpriteZeroHit`
  reads `PPUSTATUS.6` at start of next VBL.

CHR data for tile `$C0` in PT0 (extracted from
`tests/roms/accuracycoin/AccuracyCoin.nes`):

```
row 0: 3.......
row 1: ........
...
row 7: ........
```

A single opaque pixel (palette index 3) at (col 0, row 0). All other
pixels transparent.

**The non-obvious bit:** `v = $2C00 = 0010 1100 0000 0000` decodes to:

- Bits 14-12 (fine Y) = `010` = **2** (not 0 as the address byte
  pattern superficially suggests).
- Bits 11-10 (NN nametable) = `11` = 3 (NT 3).
- Bits 9-5 (coarse Y) = 0.
- Bits 4-0 (coarse X) = 0.

So scanline 0 renders NT 3 row 0 with fine Y offset 2. The fine-Y
counter wraps from 7 to 0 (incrementing coarse Y to 1) at the end of
scanline 5, so scanline 6 fetches NT 3 row 1 line 0. The `$C0` tile at
NT 3 position `$21` (col 1, row 1) therefore displays at screen `(8,
6)`, with its single opaque pixel at exactly `(8, 6)`.

Sprite at Y=5 X=8 tile `$C0` attr `$03`: per nesdev sprite-Y semantics
(sprite Y=N first visible at scanline N+1), the sprite renders at
scanlines 6..13, with its single opaque pixel at (col 0, row 0)
landing at screen `(8, 6)`.

Sprite (8, 6) AND BG (8, 6) — OVERLAP → SPRITE-ZERO HIT expected.

## Why our PPU misses the hit (the BG pipeline off-by-one)

Reference: Mesen2 `Core/NES/NesPpu.cpp` `LoadTileInfo()` (line 667)
and `ProcessScanlineImpl()` (line 868). Reference: nesdev wiki "PPU
rendering".

**Mesen2 order per visible cycle:**

```
1. LoadTileInfo (case 1 at cycles 1, 9, 17, …, 257):
   _lowBitShift |= _tile.LowByte;
   _highBitShift |= _tile.HighByte;
   (case 5 latches new LowByte; case 7 latches new HighByte;
    case 3 latches attribute palette.)
2. inc_hori_v at cycles 8, 16, …, 256.
3. DrawPixel — reads bit (15 - fine_x) of `_lowBitShift` /
   `_highBitShift`.
4. ShiftTileRegisters — `_lowBitShift <<= 1; _highBitShift <<= 1`.
```

**Our `tick` order per visible cycle** (current `main`):

```
1. shift_bg  — shifts BG + AT shift registers left by 1.
2. fetch (NT/AT/BG-lo/BG-hi at phases 1, 3, 5, 7).
3. reload_bg_shift_regs + inc_hori_v at phase 7 (= cycle 8).
4. emit_pixel — reads bit (15 - fine_x).
```

Combined, our `shift_bg` is BEFORE `emit_pixel` (Mesen2 is after),
AND our reload is at phase 7 = cycle 8 (Mesen2 is at phase 0 of the
NEXT 8-cycle group = cycle 9). These two errors cancel for most
cases but not all.

**Trace for VerifySpriteZeroHits step 2 (scanline 6):**

Pre-fetch at scanline 5 cycles 321-336 loads tile-A (NT 3 col 0 row 1
= `$24` = lo `$00`) and tile-B (NT 3 col 1 row 1 = `$C0` = lo `$80`)
into the shift register. After scanline 5 cycle 336:

- Mesen2: `_lowBitShift = $0000` (tile-B is in `_tile.LowByte` but
  NOT loaded into the shift register yet).
- Ours: `_lowBitShift = $0080` (tile-B was already reloaded into low
  8 bits at cycle 336 phase 7).

Then scanline 6 begins:

- Mesen2 cycle 1: `LoadTileInfo` case 1 → `_lowBitShift |= $80` →
  `_lowBitShift = $0080`. DrawPixel reads bit 15 = 0. Shift →
  `_lowBitShift = $0100`.
- Mesen2 cycle 8: DrawPixel reads bit 15 = 0 (from $4000 after 7
  shifts). Shift → `$8000`.
- Mesen2 cycle 9: `LoadTileInfo` case 1 → `_lowBitShift |= tile_C_lo`
  → `_lowBitShift = $80NN`. DrawPixel reads bit 15 = 1.
  **OPAQUE BG AT pixel x=8.** ✓
- Ours cycle 1: `shift_bg` → `_lowBitShift = $0100`. Phase 0 (no
  fetch). emit_pixel reads bit 15 = 0.
- Ours cycle 8: `shift_bg` → `_lowBitShift = $8000`. Phase 7 reload
  → `_lowBitShift = $80NN`. emit_pixel reads bit 15 = 1.
  **OPAQUE BG AT pixel x=7.** ✗
- Ours cycle 9: `shift_bg` → `_lowBitShift = $01YY`. emit_pixel reads
  bit 15 = 0.

So our BG opaque pixel lands at x=7, sprite opaque pixel lands at
x=8 (sprite pipeline is correct — `spr_x[0]` counts down and emits
the first sprite bit at the cycle where `spr_x == 0`, which is cycle
9 = pixel 8). **No overlap. No sprite-zero hit.**

## The (correct) fix

Apply both changes simultaneously:

1. Move `shift_bg()` from BEFORE the BG fetch block to AFTER
   `emit_pixel()` (Mesen2's `DrawPixel` → `ShiftTileRegisters`
   order).
2. Move `reload_bg_shift_regs()` from phase 7 (cycle 8 of the
   current group) to phase 0 (cycle 9 = cycle 1 of the NEXT group;
   for the LAST group of pre-fetches at cycles 329-336, the reload
   is deferred to scanline N cycle 1 of the next scanline).

Pseudo-code:

```rust
if in_bg_fetch {
    let phase = (self.dot - 1) & 7;
    if phase == 0 { self.reload_bg_shift_regs(); }
    match phase {
        1 => self.fetch_nt(bus),
        3 => self.fetch_at(bus),
        5 => self.fetch_bg_lo(bus),
        7 => self.fetch_bg_hi(bus),
        _ => {}
    }
    if phase == 7 { self.inc_hori_v(); }
}
// ... other tick work ...
if visible && (1..=256).contains(&self.dot) {
    self.emit_pixel();
}
if render_line && rendering && in_bg_fetch {
    self.shift_bg();
}
```

This passes the `cascade_a_verify_sprite_zero_hits_step2` unit test
in isolation (the BG opaque dot lands at (8, 6) → overlap with
sprite-zero opaque dot at (8, 6) → hit fires).

## Why it was rolled back

The fix shifts every BG pixel by 1 column. While this is the CORRECT
behaviour per nesdev/Mesen2 (and matches what real hardware does),
the entire 60-ROM `external_real_games` commercial-ROM snapshot
oracle plus the 3 visual_regression snapshots regress:

```
59 of 60 commercial ROMs FAIL (FB-FNV1a hash + audio-FNV1a hash
snapshot mismatch). Only `external_namco163_famista_91` did NOT fail
in the sample run (likely because its first 60 frames happen to be
identical post-shift — coincidence).

m22_vrc2a_chr_banking_0_127 FAIL (visual snapshot).
mmc1_a12_non_mmc3_a12_is_inert FAIL (visual snapshot).
instr_test_basics_frame_60 FAIL (visual snapshot).
```

This is NOT a functional regression. The games render legibly; the
sprite/BG overlap is now CORRECT instead of off-by-one. But it
violates the HARD CONSTRAINT #1 (`NEVER regress any of the 60
commercial-ROM oracle`) without explicit user authorisation to
re-baseline.

The prior session-6 OAMADDR re-baseline (10 commercial ROMs) had
explicit user sign-off. This 60-ROM re-baseline is 6× larger and
affects every game's framebuffer, so the user-authorisation step is
correspondingly more significant.

## Recommendation for next session

1. **Get explicit user authorisation** to re-baseline all 60
   `external_real_games` snapshots + 3 visual_regression snapshots
   + ~21 audio_tests / m22 / mmc1_a12 visual snapshots as part of
   this fix.
2. **Apply the fix** (see "The (correct) fix" section above) on a
   feature branch (`cascade-a-bg-pipeline-fix` recommended).
3. **Regenerate ALL framebuffer snapshots** via
   `INSTA_UPDATE=always cargo test --workspace --features
   test-roms,commercial-roms` (or `cargo insta accept`). Visual
   inspection of the regenerated screenshots against
   `screenshots/` baselines is recommended for the sacred trio
   (SMB / Excitebike / Kid Icarus PAL) to verify the shift is
   purely cosmetic.
4. **Confirm AccuracyCoin pass-rate delta**: target
   `79.86% → 82.01%` (+3-6 tests). Re-measure via
   `cargo test -p nes-test-harness --features test-roms --release
   accuracycoin_pass_rate_meets_floor`.
5. **Document the re-baseline reason** in `CHANGELOG.md` `[Unreleased]`
   and update `docs/STATUS.md`'s AccuracyCoin trajectory line.

## Why the existing 8 tests still passed under the buggy pipeline

- `cascade_a_sprite_zero_hit_y0_x8_tile_fc_overlap`: tile `$FC` is
  fully opaque (every pixel of every row), so the 1-column BG shift
  is invisible — overlap with the opaque sprite still occurs.
- `cascade_a_sprite_zero_hit_y0_x8_via_register_writes`: same
  tile `$FC` scenario.
- `cascade_a_verify_sprite_zero_hits_step2`: written as a
  characterisation assertion encoding "today's WRONG behaviour"
  (`assert!(!hit)`), so it doesn't fail under the bug; it would
  fail under the fix (with `!hit` swapping to `hit`).
- blargg `sprite_hit_tests/01-basics.nes`: uses multi-pixel
  patterns; 1-column shift doesn't break the test (still has overlap).

The bug is exposed ONLY by AccuracyCoin's single-pixel `$C0` tile +
single-pixel sprite pattern, which is exactly the case
`VerifySpriteZeroHits` step 2 (and `Sprite 0 Hit behavior` sub-test
13 / Test D) was designed to exercise. That's the whole point of
AccuracyCoin's contribution to NES emulator accuracy: it disambiguates
behaviours the simpler blargg / kevtris tests cannot.

## References

- Mesen2 source: `https://github.com/SourMesen/Mesen2/blob/master/Core/NES/NesPpu.cpp`
  - `LoadTileInfo()` at line 667.
  - `ProcessScanlineImpl()` at line 868.
  - `LoadSprite()` at line 702.
  - `GetPixelColor()` at line 817 (sprite-0 hit logic).
- nesdev wiki:
  - `https://www.nesdev.org/wiki/PPU_rendering` ("The shifters are
    reloaded during ticks 9, 17, 25, …, 257").
  - `https://www.nesdev.org/wiki/PPU_OAM` ("Sprite data is delayed
    by one scanline").
  - `https://www.nesdev.org/wiki/PPU_sprite_evaluation`.
- AccuracyCoin source: `AccuracyCoin.asm:18089-18113`
  (`VerifySpriteZeroHits`) + `AccuracyCoin.asm:6961-6978`
  (`Test D` / `TEST_Sprite0Hit_Behavior` sub-test 13).
- Prior cascade analysis:
  `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`.
- Prior re-baseline precedent: `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`
  §"Addendum (2026-05-19, session 6)".

## Files NOT modified (rollback list)

The investigation prototyped the fix in `crates/nes-ppu/src/ppu.rs`
(in the `tick` function, BG fetch block around line 951-994). All
changes were reverted via `git checkout -- crates/nes-ppu/src/ppu.rs`
because the 59 commercial-ROM snapshot regression violates HARD
CONSTRAINT #1.

Workspace test count at this checkpoint: **537 strict pass + 5
ignored** (unchanged from `main` HEAD `0636795`).

## RESOLUTION (2026-05-20, session 8)

User explicitly authorised the re-baseline 2026-05-19, and the fix
landed on `main` 2026-05-20 across two commits:

1. **`086ce4d` `fix(ppu): BG shift-register cycle-9 reload + post-emit
   shift`** — the load-bearing code change in
   `crates/nes-ppu/src/ppu.rs::Ppu::tick`. Three conceptual edits:
   - Move `reload_bg_shift_regs()` call from phase 7 (cycle 8 of the
     8-cycle fetch group) to phase 0 (cycle 9 = first cycle of the
     NEXT group). Reloads now fire at dots 1, 9, 17, …, 249 of the
     visible region + dots 321, 329 of the pre-fetch region.
   - Move `shift_bg()` call from BEFORE the BG fetch block to AFTER
     `emit_pixel()`. Shifts now happen only on visible scanlines
     (not pre-render) at dots 1..=256, after the pixel emission.
   - Add explicit `bg_shift_lo <<= 8; bg_shift_hi <<= 8` at phase
     7 of the pre-fetch region (dots 328 and 336), substituting
     for the missing per-cycle shifts during 321-336 (Mesen2-faithful
     per `ProcessScanlineImpl()` lines 941-944).
   - Flip the `cascade_a_verify_sprite_zero_hits_step2`
     characterisation probe from `assert!(!hit)` to `assert!(hit)`.

2. **`f79e44c` `test(snapshots): re-baseline visual snapshots for
   BG-pipeline fix`** — the corpus re-baseline:
   - 60 commercial-ROM framebuffer FNV-1a hash snapshots
     (`crates/nes-test-harness/tests/snapshots/external_real_games_*`).
     Audio FNV-1a hashes + cumulative cycle counts byte-identical
     across all 60 (verified).
   - 3 visual snapshots (`m22_vrc2a_chr_banking_0_127`,
     `mmc1_a12_non_mmc3_a12_is_inert`,
     `instr_test_basics_frame_120`).
   - 68 PNGs under `screenshots/external/` regenerated with the
     current `sanitize()` naming scheme (flat layout); the legacy
     per-mapper-subdir layout was simultaneously cleaned up.

### Measured outcomes

- AccuracyCoin pass rate: **79.86% → 82.73%** (+4 tests, +2.87pp).
- Tests flipped FAIL → PASS:
  - `Sprite Evaluation :: Suddenly Resize Sprite [error 1]`
  - `Sprite Evaluation :: Sprite 0 Hit behavior`
  - `Sprite Evaluation :: Sprite overflow behavior`
  - `PPU Behavior :: $2007 read w/ rendering [error 1]`
- v0.9.x AccuracyCoin target 0.80 **CLEARED**.
- Workspace strict tests unchanged at 537/0 (the fix flipped an
  existing characterisation probe's polarity rather than adding new
  tests). With `--features test-roms,commercial-roms`: 597/0.
- Sacred trio visual verification (SMB / Excitebike / Kid Icarus
  PNG inspection): all render legibly with the corrected BG
  alignment — title screen, menus, and gameplay backgrounds shift
  1 column right (the architectural intent per Mesen2 + nesdev
  wiki) but text remains legible and sprites overlay correctly.

### Residual axes (post-fix)

Cascade A is now PARTIALLY closed at the architectural level. The
post-fix residual on the sprite-eval / PPU-misc axes is a smaller
set of tests gated on more subtle state (stale-shift-register
modeling, post-B8 sprite-FSM interactions, $2002 sub-cycle flag
timing). The next v1.0+ work item on the AccuracyCoin axis is
either: (a) the stale-BG/sprite-shift-register residuals + the
$2002 flag-timing residual (cluster of 4-6 tests), or (b) the
canonical CPU `T_last - 1` IRQ-sample-point rework on the Track
C1 axis (closes 3 `cpu_interrupts_v2` + `mmc3_test_2/4` #3 + has
12 prior rollbacks). The Track C1 path has the higher reward but
also the higher risk per its rollback history; the sprite-eval
residual cluster is the lower-risk follow-up.

---

## Session 9 (2026-05-20) — Sprite-eval-base-from-OAMADDR rollback

### Targeted residuals

Per the session-9 ultrathink dispatch, the priority was the 11
PPU-side residuals on the Cascade-A-adjacent surface (Sprite
Evaluation flag/OAM tests + PPU Misc shift/serial/scanline tests).

### Phase 1 + 2 — Mapping to source

Read each `TEST_<name>` block in `/tmp/AccuracyCoin.asm` and traced
to Mesen2 PPU source (`Core/NES/NesPpu.cpp`). Key mappings:

- **`Arbitrary Sprite zero` Test 2 + `Misaligned OAM behavior`
  Test 1** (same root cause): when CPU writes `$2003` to a
  non-zero value just before sprite-eval begins on scanline 0,
  the first read at PPU cycle 65 must come from `OAM[OAMADDR..]`,
  not `OAM[0..]`. Cycle 66's in-range decision then flips the
  sprite-zero-in-line latch if and only if that first read was
  in range — regardless of physical OAM index. Reference:
  - Mesen2 `NesPpu::ProcessSpriteEvaluationStart`
    (`NesPpu.cpp:959-977`) captures `_spriteAddrH = (_spriteRamAddr
    >> 2) & 0x3F` and `_spriteAddrL = _spriteRamAddr & 0x03` at
    cycle 65.
  - Mesen2 line 1018: `_oamCopybuffer = ReadSpriteRam(_spriteRamAddr);`
    reads from the live OAMADDR.
  - Mesen2 lines 1040-1044: `if(_cycle == 66) { _sprite0Added =
    true; }` — sprite-zero latch fires at cycle 66 when the
    first read is in range, "even if this isn't actually the
    first sprite in OAM (i.e because OAMADDR was not 0 when
    evaluation started)."

- **`Sprites On Scanline 0` Test 2**: pre-render line is treated
  as "scanline 5" for the in-range checks at dots 256-319
  (sprite-tile-load phase, not eval). Per the test comment +
  nesdev forum thread `https://forums.nesdev.org/viewtopic.php?t=26291`,
  the line value used for in-range checks during pre-render is
  `(scanline & 255) = (261 & 255) = 5`. Our FSM uses `-1`
  (always-fail) for pre-render eval and doesn't model an
  in-range re-check at dots 257-319 at all.

- **`OAM Corruption` Test 2**: requires modeling the hardware
  quirk where disabling rendering mid-visible-scanline triggers
  an 8-byte OAM row replacement on the next render-enable
  transition, seeded by the secondary-OAM-address-at-disable
  value. Significant new state machine — defer.

- **`$2007 Stress Test` Test 2**: requires modeling the PPU
  DATA state machine cycle-by-cycle (5-D-latch chain with the
  Read/Write SR latch) PLUS the BG/sprite-fetch read cadence,
  yielding stable AT/PL/PH bytes in the read buffer at specific
  PPU clock alignments. Even Mesen2 doesn't model this fully.
  Defer.

- **`$2004 Stress Test` Test 2**: requires modeling the full
  OAM-buffer per-PPU-cycle value across an entire scanline,
  including sprite-fetch dots, the alternating primary/secondary
  reads during dots 257-320, and the secondary-OAM-address
  walk. Equally deep. Defer.

- **`$2002 flag timing` Test 1**: requires sub-cycle M2 model
  (vblank flag latched at M2-high, sprite flags read at M2-low,
  ~1.875 PPU cycles apart). Our $2002 read is atomic. The
  fix is mechanically simple but the impact on every other
  test is unpredictable. Defer.

- **`t Register Quirks` Test 1**: writes `$2C, $00` to `$2006`
  then `$17, $17` to `$2005`. Expects sprite-zero hit on the
  nametable-3 tile at scroll position. Our `$2005`/`$2006`/
  `$2000` code matches Mesen2 byte-for-byte (only difference:
  Mesen2's `_updateVramAddrDelay = 3` for `$2006`'s second
  write, which we apply immediately — but this doesn't affect
  the test since writes happen during vblank). Why this fails
  is unclear without deeper observability. Defer pending an
  observability sprint.

- **`Stale BG/Sprite Shift Registers`**: requires modeling the
  shifters as having stale data when rendering toggles during
  HBlank. Adjacent to the 086ce4d (session-8) BG-pipeline fix;
  touching this surface again carries snapshot-regression risk.
  Defer.

- **`BG Serial In` Test 2**: most complex BG-pipeline test in
  the suite — requires modeling the BG shift-register serial-
  input bit pattern when rendering is disabled across the
  cycle-7 (reload) boundary. Defer.

### Phase 3 + 4 — Fix attempted + ROLLED BACK

**Target**: `Arbitrary Sprite zero` Test 2 + `Misaligned OAM
behavior` Test 1 (Tier A, same root cause, high-confidence
mapping per Mesen2).

**Implementation** (now reverted): added a
`sprite_eval_start_oam_addr` field captured at PPU dot 65 from
`oam_addr`, and modified the eval read address from
`(n*4 + m) & 0xFF` to `(start + n*4 + m) & 0xFF`. The
sprite-zero-found latch already fires only when `n == 0` (the
cycle-66 decision), which combined with the new base correctly
identifies the first-read-found-in-range sprite as sprite zero
regardless of physical OAM index.

A "narrow gate" was prototyped that only honors the CPU-set
OAMADDR base on the immediately-following eval pass after a
non-zero `$2003` write, reverting to base=0 for all subsequent
passes. This was intended to limit the blast radius to only
the case the test ROMs care about.

**Targeted observables**: ALL fixed.
- `Arbitrary Sprite zero` flipped FAIL [error 2] → PASS.
- `Misaligned OAM behavior` flipped FAIL [error 1] → PASS.

**Downstream regression cascade**: 14 tests across `Sprite
Evaluation` (`INC $4014`), `PPU Misc.` (all 8), `CPU Behavior 2`
(all 5), and `Power On State` (all 5) moved from PASS/FAIL to
`not-run`. The RAM-result-byte diagnostic showed:
- `INC $4014` result byte = `$00` (uninitialized) → the test
  ROM never wrote a result for INC $4014, meaning EITHER the
  test routine internally hung OR the runner itself stalled
  before invoking the routine.
- AccuracyCoin (RAM) pass rate calculation reported **88.00%
  over 125 assigned tests** (vs pre-fix 82.73% over 139); the
  rate went UP because the denominator dropped, but absolute
  pass count went DOWN from 115 to 110.
- The framebuffer decoder showed `not_run=160` (all cells
  unparseable), suggesting the test ROM rendered an
  unrecognised final screen — likely the title-screen-menu
  loop after a CPU JAM or NMI/IRQ hang.

**Hypothesis** for the cascade: the c230489 (session-7) fix
already walks `oam_addr` during sprite-eval to expose the read
position via `$2004` (required for AccuracyCoin
`Address $2004 behavior` Tests 8 + 9). The new fix combines
that walking with a base captured from CPU-written OAMADDR.
When the CPU sets OAMADDR != 0, the eval reads OAM addresses
[base, base+4, ..., base+252] mod 256 and walks `oam_addr`
through that same sequence. End-of-eval `oam_addr` ≈
`base + 252 mod 256`. If a test then:
1. Disables rendering BEFORE the dots-257-320 OAMADDR reset
   fires (so the reset doesn't run for that scanline);
2. Issues an OAM DMA via `INC $4014` later;
the OAM DMA starts writing at the leftover `base + 252`
address rather than `OAM[0]`, corrupting sprite zero data.

The narrow gate did NOT eliminate the cascade — strongly
suggesting the actual cascade mechanism is something else
(a single misaligned eval pass corrupts secondary OAM /
sprite shifters / sprite-overflow flag, which propagates to
subsequent test code via `$2002` reads or sprite-rendering
state, even after the gate clears the dirty flag).

**Decision**: ROLL BACK the fix and the new field. The
fix-as-implemented is net-negative (gains 2 tests, costs 14
tests via cascade). The targeted tests are well-understood
and the fix shape matches Mesen2's algorithm, but the
RustyNES PPU pipeline has subtle interactions that defeat
the straightforward port. Future sessions should:
1. Add cycle-by-cycle observability via the existing
   `irq_trace` machinery (extended to capture `oam_addr` and
   `secondary_oam` per PPU dot) to diagnose the actual
   cascade mechanism in INC $4014.
2. Possibly model `_spriteRamAddr` as a fully Mesen2-faithful
   register (separate from `oam_addr`) with full sprite-eval
   semantics, rather than the current hybrid where `oam_addr`
   is both the CPU-visible OAMADDR AND the eval read position.
3. Re-attempt the fix only after the observability work
   identifies the load-bearing intermediate-state corruption.

### Workspace test deltas

- Pre-fix baseline: 537 strict pass + 5 ignored across 34
  suites with `--features test-roms`; AccuracyCoin 82.73% over
  139 assigned (108 pass + 7 pass-with-code of 139); commercial-
  ROM oracle 60 green.
- Post-rollback: identical to baseline (the rollback is clean).
  Characterisation tests added during the attempt are also
  reverted with the field they pinpoint; no diagnostic-only
  surface lands.

### Successor next-session focus

**Cluster (a) — Cycle-precise sprite-eval observability** (low
risk, high info-yield):
1. Extend `irq_trace` to optionally capture per-PPU-dot
   snapshots of `oam_addr`, `secondary_oam`, `spr_count`,
   `spr_zero_in_line`, `spr_shift_lo/hi[0..8]`, `spr_x[0..8]`,
   and the `mask`/`status` registers.
2. Run AccuracyCoin INC $4014 Test 2 with the trace
   enabled and compare against a known-good run (pre-c230489
   baseline if available) to pinpoint the divergence frame.
3. Use the divergence frame's PPU state as a unit-test
   reproducer.

**Cluster (b) — Lower-hanging-fruit single-test attacks**:
- `t Register Quirks` Test 1: instrument the rendering
  pipeline to verify v / t / x values at dot 257 of each
  rendered scanline, and confirm against the Test 1
  expectation. The mapping is mechanical but the failure
  mode is opaque without state dumps.
- `Sprites On Scanline 0` Test 2: add a new in-range check
  at dots 257-319 using `scanline & 255`. Localized change
  to `sprite_eval_per_dot` — but requires modeling a new
  state-machine phase. Probably worth its own session.

## Session 10 (2026-05-20) — PPU observability tooling landed

Infrastructure-only session. **No accuracy fix attempted.**
Implements the Session-9 §"Successor next-session focus"
Cluster (a) recommendation: per-PPU-dot state-trace fixture
modeled on the per-CPU-cycle `irq_trace` pattern that
empirically unblocked Track C1 Phase B4.

### What landed

* **`ppu-state-trace` cargo feature** (off by default,
  forwarded `nes-test-harness → nes-core → nes-ppu`).
  Verified zero-cost when off: default
  `cargo check -p nes-ppu` and the 537-strict workspace
  test count are byte-identical to pre-Session-10.
* **`PpuStateRecord` (111-byte packed binary schema v1)**
  capturing per-dot frame / scanline / dot, ctrl / mask /
  status / oam_addr, loopy v/t/fine_x/w_toggle, the
  8-field sprite-eval FSM, per-scanline sprite line-up
  (spr_count, spr_zero_in_line, 4 × `[u8; 8]` shifters),
  BG pipeline, 32-byte secondary OAM, FNV-1a-64 of
  primary OAM, NMI line. All multi-byte fields LE.
* **`PpuTraceConfig`** with `all` / `visible_only` /
  `sprite_eval_window` presets + a custom-range
  constructor.
* **`PpuStateTrace`** linear buffer with capacity cap and
  overflow counter; binary + CSV emitters; binary parser
  with magic / version / alignment validation.
* **`ppu_trace_diff` CLI** at
  `crates/nes-test-harness/src/bin/ppu_trace_diff.rs`
  for field-level divergence reporting.
  `--first-divergence` (default) / `--all-divergences` /
  `--skip-fields` / `--max-reports`.
* **Mesen2 Lua reference-trace script** at
  `scripts/mesen2_ppu_trace.lua`. Emits the same binary
  schema; per-scanline granularity (Mesen2's Lua API has
  no per-PPU-cycle event type as of 2026-05-20). The
  recommended `--skip-fields` invocation for
  per-scanline-vs-per-dot comparison is documented in
  `docs/ppu-trace-tooling.md`.
* **Integration test fixture** at
  `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs`
  driving AccuracyCoin (300-frame splash + 6-frame Start
  press + N-frame visible-only capture), with
  env-var-overridable start / end frames + output path.
* **ADR-0005** + **`docs/ppu-trace-tooling.md`** + entries
  in `CHANGELOG.md [Unreleased]` and `docs/STATUS.md`
  feature-flag matrix.

### Verification

* `cargo test -p nes-ppu --features ppu-state-trace`:
  12 new unit tests pass under
  `state_trace::tests` (record roundtrip, FNV-1a known
  vector, config filters, capacity / overflow, binary
  magic / schema / alignment validation, CSV header).
* `cargo test -p nes-test-harness --release --features
  test-roms,ppu-state-trace --test ppu_state_trace_fixture`
  with a 2-frame window: 163,680 records (= 2 × 240 ×
  341 visible-only as expected), 18,168,496-byte binary,
  zero overflow, binary roundtrip-validates. ~1.04s on
  the test machine.
* `./target/debug/ppu_trace_diff` against an identical
  reference copy: exit 0, "All 163680 records match."
* `./target/debug/ppu_trace_diff` against a synthetically
  mutated copy (3 byte flips at known offsets): exit 1,
  field names correctly identified (`ctrl`, `v`,
  `spr_shift_lo`).

### What didn't land

This is infrastructure-only. The Cascade A sprite-eval cascade
fix itself is the next session's work. The committed tooling
unblocks that investigation by enabling per-field diff against
a Mesen2 reference trace. Stop conditions per ADR-0005
§"Stop conditions" all met.

### Next-session focus (Session 11)

1. Capture a Mesen2 reference trace for AccuracyCoin
   INC $4014 Test 2 (test offset `0x0480` per the
   `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` catalog).
2. Capture the RustyNES trace for the same ROM + frame
   window with the Session-10 fixture.
3. Run `ppu_trace_diff` with the documented
   `--skip-fields` list (sprite-eval FSM + per-scanline
   sprite line-up + BG latches + secondary OAM are
   Mesen2-unknown).
4. Use the first divergence as the per-dot reproducer
   for a unit test, then attempt the fix with the
   determinism contract preserved.
