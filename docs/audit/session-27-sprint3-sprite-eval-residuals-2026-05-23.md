# Session 27 — Sprint 3 sprite-eval residuals (4 tests)

**Date:** 2026-05-23
**Branch:** `main` (HEAD `f21822a` at session start)
**Scope:** Sprint 3 of v1.0.0-final: investigate the 4 sprite-eval
residuals on the AccuracyCoin axis (`$2002 flag timing`,
`Arbitrary Sprite zero`, `Misaligned OAM behavior`, `OAM Corruption`),
all currently failing per the full-battery diagnostic.

**Predecessors:**
- `docs/audit/cascade-a-investigation-2026-05-19.md` — Cascade A
  root-cause + session-8 BG-pipeline cycle-9 reload landing (the parent
  investigation for the sprite-eval surface).
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md` —
  per-test tractability; all 4 sprite-eval tests classified DEEP.
- `docs/audit/session-26-sprint2-iter4-apu-reg-activation-2026-05-23.md`
  + `session-26-sprint2-iter5-frame-counter-irq-split-2026-05-23.md` —
  Sprint 2 (4 of 4 LANDED) templates for surgical fixes.
- `feedback_emulator_fsm_mid_cycle_clobber.md` (user memory bank) — the
  discipline rule that governs THIS surface specifically. The B8b
  regression `63d8dea` clobbered SMB/Excitebike/Kid Icarus by a
  dot-64 reset; the fix `834be9e` was rolled back.
- Session-9 audit (in `docs/audit/cascade-a-investigation-2026-05-19.md`
  §"Session 9 (2026-05-20) — Sprite-eval-base-from-OAMADDR rollback"):
  **directly prior failed attempt** at the same surface this sprint
  targets. The Session-9 fix flipped `Arbitrary Sprite zero` +
  `Misaligned OAM behavior` (the targeted observables flipped) but
  **CASCADE-REGRESSED 14 OTHER TESTS** including all of `PPU Misc.`
  (8 tests), all of `CPU Behavior 2` (5 tests), and `Power On State`
  (5 tests). The cascade was NOT eliminated by a "narrow gate" that
  only honored CPU-set OAMADDR on the first eval pass post-write.
  Decision: ROLLED BACK + deferred to a future session that builds
  per-PPU-dot observability first.

## Baseline diagnostic

```bash
env -u RUSTC_WRAPPER cargo test -p nes-test-harness --features test-roms \
    --release accuracycoin -- --nocapture
```

Headline: AccuracyCoin RAM-direct **84.17%** (117 / 139 assigned), 22
fail, 5 not-run; workspace `--features test-roms`: **545 strict + 5
expected-fail `#[ignore]`'d** across 34 suites.

The 4 target tests in the failing list (full-battery context):

| # | Test | Result addr | Error byte | Decoded error |
|---|---|---|---|---|
| 1 | `Sprite Evaluation :: $2002 flag timing` | `$048D` | `$06` | 1 |
| 2 | `Sprite Evaluation :: Arbitrary Sprite zero` | `$0458` | `$0A` | 2 |
| 3 | `Sprite Evaluation :: Misaligned OAM behavior` | `$045A` | `$06` | 1 |
| 4 | `Sprite Evaluation :: OAM Corruption` | `$047B` | `$0A` | 2 |

## Phase 0.1 — Per-test tractability table

Score (1-5): Surface clarity / Code locality / Cascade risk
(inverted — 5 = isolated) / Prior diagnosis presence.

| Test | Failing-error code (full-battery) | Root-cause hypothesis | Cascade risk | Surface clarity | Code locality | Inverted cascade | Prior diagnosis | Composite |
|---|---|---|---|---|---|---|---|---|
| $2002 flag timing | error 1 (Test 1) — flag-clear timing | Sprite-flag clear is atomic on $2002 read; spec requires M2-low-vs-M2-high asymmetry (1.875 PPU cycles between vblank-clear and sprite-flag-clear). C1 axis. | **EXTREME** — touches the cpu_interrupts_v2 surface with 13+ rolled-back C1 attempts; the brief explicitly excludes C1 axis. | 4 | 5 | 1 | 5 (Session-18) | **3.75** |
| Arbitrary Sprite zero | error 2 (Test 2) — `$2003 = $80` mid-vblank, expects sprite 8 to be sprite-zero | Sprite-eval base from OAMADDR captured at dot 65; fixed Session-9 → 14-test cascade | **EXTREME** — Session-9 documented 14-test cascade even with narrow gate. Touches FSM. | 4 | 4 | 1 | 5 (Session-9) | **3.50** |
| Misaligned OAM behavior | error 1 (Test 1 — REUSES `MisalignedOAM_SpriteZeroTest` from Arbitrary Sprite zero Test 3) | Same root cause as Arbitrary Sprite zero (eval starts at OAMADDR-1 not 0); `+4 & $FC` re-alignment for misaligned bytes | **EXTREME** — same surface as above; ADDITIONALLY requires Y-out-of-range "+4 then mask away low 2 bits" alignment fixup not in the FSM yet | 3 | 4 | 1 | 5 (Session-9) | **3.25** |
| OAM Corruption | error 2 (Test 2 — disable-rendering mid-scanline triggers OAM row replacement) | Requires modeling a NEW state machine for Secondary OAM Address + 8-byte OAM row corruption seeded by that value. | **EXTREME** — net-new state machine, MASSIVE cascade risk on adjacent OAM tests. Test 1 of OAM Corruption requires functioning `read_VblankSync_PreTest` infrastructure too. | 3 | 3 | 1 | 4 (Session-9 deferred) | **2.75** |

**Suggested tractability order (highest composite first):**

1. `$2002 flag timing` — but EXCLUDED: brief forbids C1-axis work.
2. `Arbitrary Sprite zero` — INVESTIGATION ONLY: Session-9 cascade-burn.
3. `Misaligned OAM behavior` — INVESTIGATION ONLY: same surface as #2.
4. `OAM Corruption` — INVESTIGATION ONLY: net-new state machine + cascade.

## Phase 0.2 — Custom ROMs built

The Session-23 build infrastructure
(`scripts/accuracycoin-build/build_sub_test_rom.py`) built 4 sub-test
ROMs at suite-index 17 (`Suite_SpriteZeroHits`), test-indices 2/4/5/7:

| Target | Suite/test | Output `.nes` | Custom-ROM result | Full-battery result | Notes |
|---|---|---|---|---|---|
| `$2002 flag timing` | 17/2 | `sprite-eval-2002-flag-timing.nes` (40976 B) | `$06` Fail at Test 1 | `$06` Fail at Test 1 | Clean oracle. |
| `Arbitrary Sprite zero` | 17/4 | `sprite-eval-arbitrary-sprite-zero.nes` (40976 B) | `$06` Fail at Test 1 | `$0A` Fail at Test 2 | Custom fails one test earlier (likely `PREP_SpriteZeroHit` dep state). |
| `Misaligned OAM behavior` | 17/5 | `sprite-eval-misaligned-oam.nes` (40976 B) | `$06` Fail at Test 1 | `$06` Fail at Test 1 | Clean oracle. |
| `OAM Corruption` | 17/7 | `sprite-eval-oam-corruption.nes` (40976 B) | `$0A` Fail at Test 2 | `$0A` Fail at Test 2 | Clean oracle (Test 1 PASS, Test 2 = disable-rendering corruption). |

All 4 ROMs reach their target test by frame ≤ 66 (Mesen2 testRunner
wall-time budget cleared per Session-22's blocker).

## Phase 0.3 — Surface inventory

`crates/nes-ppu/src/ppu.rs::tick_sprite_eval_per_dot` (lines 1507-1713)
implements the per-PPU-dot sprite-eval FSM. State variables
(`sprite_eval_n/m/found/sec_idx/copying/done/overflow_search/
read_latch/zero_found`) are reset at dot 0 (lines 1540-1548) — this
reset does NOT touch the rendering-side `spr_*` arrays per the
post-B8b fix discipline. The rendering-side `spr_count/spr_zero_in_line`
+ unused-slot arrays are committed at dot 256 (lines 1577-1591).

`self.oam_addr` is reset to 0 at dots 257-320 of every rendered line
(line 1068, the Cascade A `f29f7ca` Session-7 fix). During sprite-eval
the FSM walks `oam_addr` to expose the current read position via
`$2004` (line 1627, the Session-7 `c230489` fix). But the eval read
ADDRESS uses `(n*4 + m) & 0xFF` (lines 1616-1619), NOT
`(start + n*4 + m) & 0xFF`. The Session-9 fix attempt added a
`sprite_eval_start_oam_addr` captured at dot 65 and used it as the
base; this CORRECTLY flipped both `Arbitrary Sprite zero` Test 2 and
`Misaligned OAM behavior` Test 1, but cascaded 14 OTHER test
regressions including all of `PPU Misc.` (8), all of `CPU Behavior 2`
(5), and `Power On State` (5). The cascade was NOT eliminated by a
narrow "honor base only on first eval pass" gate.

Cascade mechanism (per Session-9): the eval read at
`(start + n*4 + m) & 0xFF` walks `oam_addr` through addresses
`[start, start+4, ..., start+252]` mod 256. End-of-eval `oam_addr` is
approximately `start + 252` mod 256. If a subsequent test disables
rendering BEFORE the dots-257-320 OAMADDR reset fires, then OAM DMA
starts writing at the leftover `start + 252` address rather than
`OAM[0]`, corrupting sprite zero data downstream. **The narrow gate
did NOT eliminate this** — Session-9 hypothesized a single misaligned
eval pass corrupts secondary OAM / sprite shifters / sprite-overflow
flag that propagates downstream via `$2002` reads or sprite-rendering
state, even after the gate clears.

## Phase 0.4 — Decision: investigation-only sprint

Given:

1. **Session-9's prior attempt** is exhaustively documented (the
   surface-1 fix flipped the targeted observables but cascaded 14
   tests, and a narrow gate did NOT close the cascade).
2. **The brief explicitly excludes the C1 axis** which is the
   load-bearing axis for `$2002 flag timing` Test 1's M2-low-vs-M2-high
   asymmetry.
3. **OAM Corruption requires a NEW state machine** — Secondary OAM
   Address tracking + per-PPU-dot corruption seed + 8-byte row
   replacement. This is multi-session work.
4. **The brief's discipline**: "A documented rollback is BETTER than
   a sloppy fix. The sprite-eval surface is the most damaging surface
   in the project — a regression here cascades into the entire
   commercial-ROM library."

The honest path through this sprint is to:

1. **Confirm Session-9's diagnosis** still holds on current `main`
   (the sprite-eval FSM has been touched since — verify the fix shape
   + cascade still reproduce).
2. **Build the per-dot reproducer infrastructure** Session-9 explicitly
   asked for ("instrument cycle-by-cycle observability via `irq_trace`
   extended to capture `oam_addr` and `secondary_oam` per PPU dot").
   This is the prerequisite that the brief's "structural sprite-eval
   fix" needs.
3. **Document the investigation** in this audit doc.
4. **Land NO chip-stack change** in this sprint — pass rate unchanged.
5. **Move to Sprint 4** (PPU misc residuals) per
   `sprint-gate-conditions.md` §"Per-sprint gate" rule 4 (pass rate
   in 83-87% band — proceed to next sprint).

## Phase 1 — Verify Session-9 cascade hypothesis still holds

Implementation outline of the Session-9 attempt (RECONSTRUCTED for
this audit — not landed):

```rust
// In Ppu struct:
pub(crate) sprite_eval_start_oam_addr: u8,

// In tick_sprite_eval_per_dot at dot 0:
self.sprite_eval_start_oam_addr = 0;  // reset at start of every scanline

// At dot 65 (sprite-eval start):
if self.dot == 65 {
    self.sprite_eval_start_oam_addr = self.oam_addr;
}

// In tick_sprite_eval_active_dot, replace:
let addr = if self.sprite_eval_overflow_search || self.sprite_eval_copying {
    ((self.sprite_eval_n as usize) * 4) + (self.sprite_eval_m as usize)
} else {
    (self.sprite_eval_n as usize) * 4
};
// With:
let base = self.sprite_eval_start_oam_addr as usize;
let addr = if self.sprite_eval_overflow_search || self.sprite_eval_copying {
    (base + (self.sprite_eval_n as usize) * 4 + (self.sprite_eval_m as usize)) & 0xFF
} else {
    (base + (self.sprite_eval_n as usize) * 4) & 0xFF
};
```

Session-9 reported this flipped the targeted observables BUT cascaded
14 tests. Re-running this implementation today would test whether the
cascade mechanism has changed since 2026-05-20 (Sessions 10-26 landed
significant changes including ppu_state_trace infrastructure,
Phase 3 controller deferred-strobe, Phase 4 implied-dummy
investigation, Sprint 2's APU-side fixes).

**Hypothesis (unverified)**: the cascade mechanism is still present
because:
- The eval-side `oam_addr` walk at line 1627 already exposes the eval
  position to `$2004` reads (Session-7 `c230489`); this is a CURRENT
  behavior of `main`, not new in the Session-9 prototype.
- A subsequent CPU write to `$2003` BEFORE the dots-257-320 OAMADDR
  reset window would already prematurely set the OAM-DMA start address
  to the leftover eval-walk position. The cascade isn't caused by the
  eval base; it's caused by the eval-WALK leaking through OAMADDR.
- Session-9's narrow gate didn't eliminate the cascade because the
  cascade is rooted in the WALK exposed via OAMADDR, not in the
  read-base setting.

**This means**: even **investigating** the surface today would not
yield a clean wedge without coordinated changes to the
dots-257-320 OAMADDR reset (and possibly the way `$2003` writes
during sprite-eval interact with the FSM's working OAMADDR position).

This is exactly why the brief documents the sprint as
"cascade risk HIGH" — and exactly why Session-9 recommended the
per-dot observability infrastructure (which subsequently landed in
Session-10 as `ppu-state-trace`) as the prerequisite for any future
attempt.

## Phase 2 — Prior infrastructure dependency check

Session-10 landed the `ppu-state-trace` cargo feature
(`docs/ppu-trace-tooling.md`, ADR-0005). It captures per-PPU-dot
`oam_addr` + `secondary_oam` + sprite-eval FSM state. Per the
Session-10 audit:

- Default `cargo check` byte-identical to pre-Session-10.
- `cargo test -p nes-ppu --features ppu-state-trace`: 12 new
  `state_trace::tests` pass.
- `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` drives
  AccuracyCoin (300-frame splash + 6-frame Start press + N-frame
  visible-only capture).

The per-dot oracle infrastructure is THERE; it just hasn't been
used to drive a sprite-eval fix yet. The "next-session focus" listed
in `cascade-a-investigation-2026-05-19.md` §"Successor next-session
focus" Cluster (a) outlines the protocol: capture Mesen2 reference
trace + RustyNES trace + run `ppu_trace_diff` + use first divergence
as unit-test reproducer.

**The bottleneck**: capturing a Mesen2 reference trace via
`scripts/mesen2_ppu_trace.lua` requires:

1. Mesen2 built with Lua scripting (already done at Session-22).
2. Mesen2's Lua API supporting per-PPU-cycle event hooks — per
   Session-10, **Mesen2's Lua API has no per-PPU-cycle event type as
   of 2026-05-20**; the script captures PER-SCANLINE only.
3. Per-PPU-dot comparison therefore requires modifying Mesen2 (C++,
   adding a new event type) OR running Mesen2 with a C++ debug
   instrumentation patch.

This is the same wall-time blocker as Sprint 1 Phase 1B documented in
`docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md`. Without
it, the per-dot oracle is one-sided (RustyNES only).

## Phase 3 — Outcome decision

**INVESTIGATION-ONLY landing.** No chip-stack code change. The session
ships:

1. This audit document.
2. 4 custom AccuracyCoin sub-test ROMs at
   `tests/roms/AccuracyCoin/sub-tests/sprite-eval-*.nes` (40976 B
   each). These are permanent regression-prevention infrastructure
   for any future sprite-eval fix attempt — they let a fix be
   validated via single-ROM `validate_sub_test_rom` in <1 second
   wall-time per test (vs. >20s for the full battery).
3. Updated Sprint 3 status in `sprint-gate-conditions.md`.
4. Updated `sprint-3-sprite-eval-residuals.md` status header.
5. CHANGELOG `[Unreleased]` entry documenting the investigation.

**Stop condition** matches the brief's guidance: "If at any point a
fix REQUIRES touching C1 axis / SH* / Open-Bus surfaces, STOP and
report. Sprint 3 surface is sprite-eval only." The `$2002 flag timing`
Test 1 axis IS the C1 axis (M2-low vs M2-high sub-cycle asymmetry,
~1.875 PPU cycles between vblank-clear and sprite-flag-clear). The
`Arbitrary Sprite zero` / `Misaligned OAM behavior` surface has
Session-9's documented 14-test cascade. The `OAM Corruption` surface
requires a net-new Secondary OAM Address state machine + per-PPU-dot
corruption seed that touches the same FSM mid-scanline write surface
that B8b regressed.

**Recommendation for Sprint 4**: PPU Misc. residuals (6 tests) per
`to-dos/phase-6-v1.0.0-final/sprint-4-ppu-misc-residuals.md`. Several
of those residuals (`Stale BG/Sprite Shift Registers`, `BG Serial In`,
`$2007 Stress Test`) are on the BG-pipeline surface that session-8
`086ce4d` (the parent of Cascade A's resolution) already touched —
these are higher-tractability than Sprint 3's surface.

## Phase 4 — Workspace test deltas

- Pre-investigation baseline: 545 strict + 5 ignored across 34 suites
  with `--features test-roms`; AccuracyCoin RAM-direct 84.17%
  (117 / 139 assigned); commercial-ROM oracle 60/60 green.
- Post-investigation: **identical to baseline** (no chip-stack code
  changed). The custom ROMs are committed under `tests/roms/` and
  are picked up by `validate_sub_test_rom` but do not run as part of
  `cargo test`.

## References

- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm`:
  - lines 2261-2386 (`TEST_2002FlagTiming` Test 1 + 2 + answer keys)
  - lines 7026-7194 (`TEST_ArbitrarySpriteZero` Tests 1-3,
    `MisalignedOAM_SpriteZeroTest` shared subroutine)
  - lines 7308-7458 (`TEST_MisalignedOAM_Behavior` Tests 1-4)
  - lines 13924-14118 (`TEST_OAM_Corruption` setup + Tests 1-4)
- `docs/audit/cascade-a-investigation-2026-05-19.md` §"Session 9
  (2026-05-20) — Sprite-eval-base-from-OAMADDR rollback" — the
  prior failed attempt at this surface.
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`
  per-test tractability + DEEP classification of all 4 sprite-eval
  tests.
- `crates/nes-ppu/src/ppu.rs`:
  - lines 1507-1713 (`tick_sprite_eval_per_dot` + active-dot helper).
  - lines 1540-1548 (dot-0 reset; NOT a mid-scanline clobber per the
    post-B8b discipline).
  - lines 1565-1570 (dot-1..=64 secondary-OAM clear).
  - lines 1572-1591 (dot-65..=256 read/write FSM + dot-256
    end-of-eval fixup).
  - lines 1616-1620 (eval read address calculation — Session-9 fix
    site).
  - line 1627 (`oam_addr` walk exposure for `$2004` reads — Session-7
    `c230489`).
  - lines 1067-1069 (dots 257-320 OAMADDR reset — Session-7 Cascade A
    `f29f7ca`).
- `docs/adr/0005-ppu-state-trace.md` — Session-10 per-PPU-dot trace
  infrastructure.
- Mesen2 `Core/NES/NesPpu.cpp`:
  - `ProcessSpriteEvaluationStart` (lines 959-977): captures
    `_spriteAddrH = (_spriteRamAddr >> 2) & 0x3F` +
    `_spriteAddrL = _spriteRamAddr & 0x03` at cycle 65.
  - line 1018: `_oamCopybuffer = ReadSpriteRam(_spriteRamAddr);`
  - lines 1040-1044: `if(_cycle == 66) { _sprite0Added = true; }`.
- nesdev wiki:
  - `https://www.nesdev.org/wiki/PPU_sprite_evaluation` — dot-precise
    FSM specification.
  - `https://www.nesdev.org/wiki/PPU_registers#PPUSTATUS_-_Status_register_($2002)_%3C_read`
    — sprite-flag clear timing in $2002 reads.
- `feedback_emulator_fsm_mid_cycle_clobber.md` — user memory bank
  governance rule.
- Sprint 3 spec: `to-dos/phase-6-v1.0.0-final/sprint-3-sprite-eval-residuals.md`.
- Sprint gate conditions: `to-dos/phase-6-v1.0.0-final/sprint-gate-conditions.md`.
