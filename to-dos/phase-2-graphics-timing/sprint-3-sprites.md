# Sprint 2-3 — Sprite evaluation + rendering + sprite-zero hit

**Phase:** Phase 2 — Graphics + Timing
**Sprint goal:** Per-cycle sprite evaluation (with the documented `n+m` overflow bug), per-dot sprite tile fetch, sprite rendering with priority, and sprite-zero hit detection.
**Estimated duration:** 2 weeks

## Tickets

### T-23-001 — Cycles 1-64 secondary OAM clear

**Description:** During cycles 1-64, secondary OAM is cleared to `$FF`. The fetch is still made from primary OAM but reads are forced to `$FF`.

**Acceptance criteria:**
- [x] Secondary OAM cleared to `$FF` at the start of `evaluate_sprites_for_next_line` (collapsed from cycles 1-64 to cycle 257 for Phase 2).
- [x] Verified via `sprite_overflow_tests/1.Basics` and `sprite_hit_tests/01.basics` passing.

**Dependencies:** Sprint 2-2 complete.
**Reference:** `docs/ppu-2c02.md` §Sprite evaluation.
**Estimated complexity:** S.

---

### T-23-002 — Cycles 65-256 sprite evaluation FSM (with overflow bug)

**Description:** Implement the alternating odd/even cycle read/write sprite evaluation FSM. After 8 sprites are found, the buggy overflow check increments **both `n` and `m`** without carry, producing the documented hardware behavior.

**Acceptance criteria:**
- [ ] Algorithm matches `docs/ppu-2c02.md` §Sprite evaluation literally. (Deferred — Phase 2 uses the simpler "scan all 64 OAM entries" model; the n+m diagonal-scan bug is not yet reproduced. Tests still pass with the simpler model.)
- [x] After evaluation, secondary OAM contains correct sprites for the next scanline (verified by sprite_hit_tests corpus).
- [x] Sprite overflow flag set when more than 8 in-range sprites are found.
- [x] `sprite_overflow_tests/*` (5 sub-ROMs) all pass — including `5.Emulator`.

**Dependencies:** T-23-001.
**Reference:** `docs/ppu-2c02.md` §Sprite evaluation.
**Estimated complexity:** L.

---

### T-23-003 — Cycles 257-320 sprite tile fetch

**Description:** Per-dot sprite tile fetch: 8 sprites × 4 fetches (garbage NT, garbage NT, PT-low, PT-high). OAMADDR forced to 0 during this range. Sprite X-positions and attributes load during the second garbage fetch.

**Acceptance criteria:**
- [x] All 8 sprite slots populated for the next scanline (verified by sprite_hit_tests).
- [ ] OAMADDR is 0 after cycle 320. (Phase 2 uses a single-shot fetch at cycle 257 instead of dot 257-320 sequencing; OAMADDR side-effects are not yet modeled.)
- [x] Sprite latches: pattern bytes via CHR fetch (`addr_lo`/`addr_hi`), attr/X via copy from secondary OAM. Horizontal flip via `b.reverse_bits()`.

**Dependencies:** T-23-002.
**Reference:** `docs/ppu-2c02.md` §Per-dot fetch sequencing.
**Estimated complexity:** M.

---

### T-23-004 — Sprite pixel emission with priority

**Description:** During visible dots, decrement each sprite's X-counter; when zero, shift out the sprite's pattern bits. Combine with BG pixel: BG-vs-sprite priority per sprite attribute byte; left-column-show flags honored.

**Acceptance criteria:**
- [x] Sprite pixels render in correct positions (X-counter decrement + shift).
- [x] BG-priority sprites render behind non-transparent BG (`spr_priority_front` flag).
- [x] PPUMASK bits 2 (left-column sprites) and 4 (sprite enable) honored.
- [x] 8×16 sprite mode (PPUCTRL bit 5) renders correctly with the bit-0-of-tile pattern table selection — verified by `sprite_hit_tests/08.double_height` passing.
- [x] Visual diff: 11/11 sprite_hit_tests + 5/5 sprite_overflow_tests pass.

**Dependencies:** T-23-003.
**Reference:** `docs/ppu-2c02.md` §Behavior.
**Estimated complexity:** L.

---

### T-23-005 — Sprite-zero hit

**Description:** During sprite rendering, when a non-transparent sprite-0 pixel overlaps a non-transparent BG pixel, set PPUSTATUS bit 6. Apply all the constraints (cannot fire at X=255, cannot fire if left-column hidden and X<8, cannot fire on pre-render scanline).

**Acceptance criteria:**
- [x] `sprite_hit_tests_2005.10.05/*` (11 sub-ROMs) all pass — including `09.timing_basics`, `10.timing_order`, `11.edge_timing`.
- [ ] `ppu_sprite_hit/*` passes. (Not yet vendored.)
- [x] Sprite-0 hit cleared at scanline 261/311 dot 1 (existing `tick()` clear).

**Dependencies:** T-23-004.
**Reference:** `docs/ppu-2c02.md` §Sprite-0 hit.
**Estimated complexity:** M.

---

### T-23-006 — `oam_read` and `oam_stress` pass

**Description:** Validate OAM read/write behavior at `$2003`/`$2004` per the test ROMs.

**Acceptance criteria:**
- [x] `oam_read.nes` passes through the lockstep `Nes` runner.
- [ ] `oam_stress.nes` (status 0x01 — known: requires precise OAMADDR-during-rendering corruption that we deferred).

**Dependencies:** T-23-005.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

## Sprint review checklist

- [ ] All tickets checked off.
- [ ] PPU is feature-complete except for the OAMADDR-during-rendering corruption (deferred; rare in practice).
- [ ] CHANGELOG entry: "PPU sprite rendering complete; sprite-zero hit accurate."
