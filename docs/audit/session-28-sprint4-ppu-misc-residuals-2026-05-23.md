# Session 28 — Sprint 4 PPU misc residuals (6 tests)

**Date:** 2026-05-23
**Branch:** `main` (HEAD `f3136ae` at session start)
**Scope:** Sprint 4 of v1.0.0-final: investigate the 6 `PPU Misc.`
residuals on the AccuracyCoin axis (`Stale BG Shift Registers`,
`Stale Sprite Shift Regs`, `BG Serial In`, `Sprites On Scanline 0`,
`$2004 Stress Test`, `$2007 Stress Test`), all currently failing per
the full-battery diagnostic.

**Predecessors:**
- `docs/audit/cascade-a-investigation-2026-05-19.md` — Session 8
  BG-pipeline cycle-9 reload + post-emit shift landing (the parent
  investigation for the BG-pipeline surface that 3 of the 6 Sprint 4
  candidates sit on).
- `docs/audit/session-27-sprint3-sprite-eval-residuals-2026-05-23.md`
  — Sprint 3 INVESTIGATION-ONLY outcome on the sprite-eval FSM
  surface. The Sprint 3 doc documents the eval-base-from-OAMADDR
  axis Session-9 14-test cascade — directly relevant to Sprint 4's
  `$2004 Stress` candidate because the OAMADDR walk exposed via
  `$2004` reads is the same surface.
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md` —
  per-test tractability; all 6 PPU misc tests classified **D (Deep)**
  with **HIGH cascade risk**.
- `feedback_emulator_fsm_mid_cycle_clobber.md` (user memory bank) —
  the discipline rule that governs THIS surface specifically. The
  B8b regression `63d8dea` clobbered SMB/Excitebike/Kid Icarus by a
  dot-64 reset; the fix `834be9e` was rolled back. The Sprint 4
  candidates that touch sprite-eval FSM (`Stale Sprite Shift Regs`,
  `Sprites On Scanline 0`, `$2004 Stress`) share this surface.

## Baseline diagnostic

```bash
env -u RUSTC_WRAPPER cargo test -p nes-test-harness --features test-roms \
    --release accuracycoin -- --nocapture
```

Headline: AccuracyCoin RAM-direct **84.17%** (117 / 139 assigned), 22
fail, 5 not-run; workspace `--features test-roms`: **545 strict + 5
expected-fail `#[ignore]`'d** across 34 suites. Commercial-ROM oracle:
60/60.

The 6 target tests in the failing list (full-battery context):

| # | Test | Suite/test (0-based) | Result addr | Error byte | Decoded error |
|---|---|---|---|---|---|
| 1 | `PPU Misc. :: Stale BG Shift Registers` | 18/2 | `$0483` | `$0E` | 3 |
| 2 | `PPU Misc. :: Stale Sprite Shift Regs` | 18/3 | `$048F` | `$0E` | 3 |
| 3 | `PPU Misc. :: BG Serial In` | 18/4 | `$0487` | `$0A` | 2 |
| 4 | `PPU Misc. :: Sprites On Scanline 0` | 18/5 | `$0484` | `$0A` | 2 |
| 5 | `PPU Misc. :: $2004 Stress Test` | 18/6 | `$048C` | `$0A` | 2 |
| 6 | `PPU Misc. :: $2007 Stress Test` | 18/7 | `$048E` | `$0A` | 2 |

## Phase 0.1 — Per-test tractability table

Score (1-5): Surface clarity / Code locality / Cascade risk (inverted —
5 = isolated) / Prior diagnosis presence.

| Test | Failing-error code | Root-cause hypothesis | Cascade risk | Surface clarity | Code locality | Inverted cascade | Prior diagnosis | Composite |
|---|---|---|---|---|---|---|---|---|
| Stale BG Shift Regs (T3) | error 3 — render-disable mid-HBlank preserves BG shifters | When rendering is disabled at the precise dot during HBlank where the unused NT-read at dot 241-248 fed `$2C00` byte (a solid white tile in the test setup), the BG shifters latch `11111111 00000000` and retain that on re-enable. Requires: (a) per-PPU-dot atomic rendering-disable behavior at HBlank boundary; (b) BG-shifter retention semantics across the disabled window; (c) re-enable timing precision. | **EXTREME** — touches the same BG-pipeline surface that Cascade A (commit `086ce4d`) just rebaselined the 60-ROM commercial oracle for. The Session-8 fix is the gold-standard "documented coordinated re-baseline" precedent; a follow-on tweak here without re-baseline authorization will almost certainly diverge framebuffer FNV-1a hashes on multiple commercial ROMs. | 3 | 3 | 1 | 4 (Session-8) | **2.75** |
| Stale Sprite Shift Regs (T3) | error 3 — render-disable preserves sprite counter "halted"/"counting" mode | Net-new state machine: per-sprite shifter counter has two modes ("halted" = drawing, "counting" = decrement until 0). `$2001` disable mid-scanline must preserve mode. Dot 339 (last dot of pre-render scanline) is the canonical "reload counter to $FF and set mode=counting" event; if rendering is disabled at dot 339, counter retains prior state. | **EXTREME** — touches sprite-eval FSM (same surface B8b regression `63d8dea` clobbered SMB / Excitebike / Kid Icarus). Requires net-new `spr_mode: [SpriteCounterMode; 8]` field threaded through every dot of every visible scanline — every write site needs the FSM mid-cycle-clobber audit per `feedback_emulator_fsm_mid_cycle_clobber.md`. | 2 | 2 | 1 | 3 (B8b lesson) | **2.00** |
| BG Serial In (T2) | error 2 — empty NT but BG shifters get '1' bits via serial-in | If `$2001` disable falls on `dot%8==6` and re-enable falls on `dot%8==0`, the shifter reload at phase 0 is skipped but the serial-in latch on the high bit plane still feeds '1' bits into the shift register. The 2-5 PPU cycle PPUMASK→take-effect delay (alignment-dependent) is the load-bearing axis: the test deliberately writes `$3E01` (a `$2001` mirror) to deal with the "wrong-value-written-for-1-ppu-cycle" hardware glitch, then re-writes `$2001` 4 CPU cycles later. | **EXTREME** — requires modeling per-PPU-clock PPUMASK delay (currently immediate at `cpu_write_register`), AND BG-shifter serial-in semantics (currently the shifter only loads from latches, no serial-in for the '1' high-bit-plane). The PPUMASK delay change alone would cascade into every blargg `ppu_vbl_nmi/*` test (10/10 strict load-bearing). | 3 | 2 | 1 | 2 (cross-references nesdev BG-pipeline) | **2.00** |
| Sprites On Scanline 0 (T2) | error 2 — pre-render line treated as scanline 5 for in-range checks at dots 256-319 | The pre-render line (scanline 261) is internally treated as `261 & 255 = 5` for sprite-eval in-range checks during the sprite-tile-fetch window (dots 256-319). This means secondary OAM populated on the PREVIOUS frame's scanline 239 (or whatever was in secondary OAM before F-Blank) can be re-fetched into the sprite shifters for scanline 0. Also requires: dot-340-odd-skip causing the BG background to "jitter" left by 1 pixel + the first pixel of sprite shifter drawing at x=0 instead of intended x. Composite vs RGB PPU distinction. | **EXTREME** — net-new pre-render-as-scanline-5 sprite-eval logic that touches the same FSM B8b regressed; the dot-340-odd-skip-shifter-effect is a parallel pipe to the existing `mask_skip_pipe1` and would need its own audit. Composite/RGB distinction adds a configuration dimension to the existing region-only state. | 2 | 2 | 1 | 2 (nesdev forum thread cited in upstream) | **1.75** |
| $2004 Stress (T2) | error 2 — per-PPU-dot OAMADDR walk visible across $2004 reads, multiple frames | The test prepares OAM with descending `$FF..$00`, reads `$2004` on every PPU dot of a chosen scanline across multiple frames, expects an exact 341-byte pattern (`Test_2004_Stress_AnswerKey1` at asm:2040+). The expected pattern reflects the FSM's `(n*4+m) & 0xFF` walk through OAMADDR during sprite-eval dots 65-256. | **EXTREME — overlaps Sprint 3 deferred axis** — this is the same eval-base-from-OAMADDR axis that Sprint 3's `Arbitrary Sprite zero` + `Misaligned OAM behavior` candidates were deferred on. Session-9 documented a 14-test cascade when attempting to fix this surface. The narrow gate did NOT eliminate the cascade. Per the Sprint 4 brief: "If at any point a fix REQUIRES touching … eval-base-from-OAMADDR axis, STOP and report. Defer to v1.x." | 4 | 3 | 1 | 5 (Session-9 cascade) | **2.50 — but Sprint-3-axis-excluded** |
| $2007 Stress (T2) | error 2 — PPU DATA state machine analog ALE-vs-Read interaction | Test 2 reads `$2007` on every dot of a visible scanline while rendering, across multiple frames, expects an exact pattern reflecting the **PPU DATA state machine** (3-cycle latency: M2-low → ALE-set → idle → read) interacting with the **BG/sprite fetch read cadence** (2-cycle: ALE → read). The intersection creates "stable" and "unstable" reads depending on whether `$2007` read ends on a low or high PPU clock edge. The upstream comment explicitly states "a real shame half the bytes are affected by analogue behavior." | **EXTREME** — net-new PPU DATA state machine model (currently `cpu_read_register::case 7` does a single inline read with no internal latency model). Modeling the analog ALE-vs-Read feedback loop requires sub-PPU-clock granularity that the lockstep scheduler does not provide. Even Mesen2 may not pass this cleanly. | 2 | 2 | 1 | 2 (upstream notes own analog limits) | **1.75** |

## Phase 0.2 — Axis filter

For each candidate, cross-check vs the Sprint 4 brief's excluded axes:

| Test | C1 axis? | SH* axis? | Open Bus error 9? | eval-base-from-OAMADDR (Sprint 3)? | In Sprint 4 scope? |
|---|---|---|---|---|---|
| Stale BG Shift Regs | No | No | No | No | YES (but EXTREME cascade) |
| Stale Sprite Shift Regs | No | No | No | Adjacent (FSM mid-cycle clobber) | YES (but EXTREME cascade) |
| BG Serial In | No | No | No | No | YES (but EXTREME cascade) |
| Sprites On Scanline 0 | No | No | No | Adjacent (FSM mid-cycle clobber) | YES (but EXTREME cascade) |
| **$2004 Stress** | No | No | No | **YES — directly on Sprint 3 deferred axis** | **EXCLUDED** |
| $2007 Stress | No | No | No | No | YES (but EXTREME cascade) |

**One candidate filtered out by axis-overlap with Sprint 3's
deferred eval-base-from-OAMADDR cascade**: `$2004 Stress`. The test
reads `$2004` on every PPU dot of a scanline to expose the FSM's
internal OAMADDR walk — the very surface Session-9 cascaded 14
tests on. Per the brief: "back-reference Sprint 3's findings if a
Sprint 4 candidate hits this." Confirmed.

## Phase 0.3 — Custom ROMs built

The Session-23 build infrastructure
(`scripts/accuracycoin-build/build_sub_test_rom.py`) built 6 sub-test
ROMs at suite-index 18 (`Suite_PPUMisc`), test-indices 2/3/4/5/6/7:

| Target | Suite/test | Output `.nes` | Custom-ROM result | Full-battery result | Notes |
|---|---|---|---|---|---|
| `Stale BG Shift Registers` | 18/2 | `ppu-misc-stale-bg-shift-regs.nes` (40976 B) | `$0E` Fail at Test 3 (first set frame 71) | `$0E` Fail at Test 3 | Clean oracle. |
| `Stale Sprite Shift Regs` | 18/3 | `ppu-misc-stale-sprite-shift-regs.nes` (40976 B) | `$0E` Fail at Test 3 (first set frame 92) | `$0E` Fail at Test 3 | Clean oracle. |
| `BG Serial In` | 18/4 | `ppu-misc-bg-serial-in.nes` (40976 B) | `$0A` Fail at Test 2 (first set frame 59) | `$0A` Fail at Test 2 | Clean oracle. |
| `Sprites On Scanline 0` | 18/5 | `ppu-misc-sprites-on-scanline-0.nes` (40976 B) | `$0A` Fail at Test 2 (first set frame 39) | `$0A` Fail at Test 2 | Clean oracle. |
| `$2004 Stress Test` | 18/6 | `ppu-misc-2004-stress.nes` (40976 B) | `$0A` Fail at Test 2 (first set frame 69) | `$0A` Fail at Test 2 | Clean oracle, but Sprint-3-axis-excluded. |
| `$2007 Stress Test` | 18/7 | `ppu-misc-2007-stress.nes` (40976 B) | `$0A` Fail at Test 2 (first set frame 400) | `$0A` Fail at Test 2 | Clean oracle. |

All 6 ROMs reach their target test by frame ≤ 400 (most ≤ 92).
Permanent regression-prevention infrastructure for any future PPU
misc fix attempt.

## Phase 0.4 — Decision: investigation-only sprint

Given:

1. **Five of six candidates** carry **EXTREME cascade risk** into
   commercial-ROM oracle FNV-1a hashes. The BG-pipeline and sprite
   FSM surfaces are exactly the ones that:
   - Cascade A (Session-8, commit `086ce4d`) just re-baselined under
     explicit user authorization. A follow-on tweak without
     authorization will diverge those hashes again.
   - B8b (`63d8dea`) clobbered when an "equivalent" FSM landed; the
     `feedback_emulator_fsm_mid_cycle_clobber.md` discipline rule
     exists specifically to govern this surface.

2. **One candidate is Sprint-3-axis-excluded**: `$2004 Stress` is on
   the eval-base-from-OAMADDR axis that Session-9 documented as
   cascading 14 tests even with a narrow gate. The brief explicitly
   defers this to v1.x.

3. **Two candidates require net-new state machines** that go beyond
   surgical fixes:
   - `Stale Sprite Shift Regs` Test 3: net-new per-sprite counter
     mode (`halted`/`counting`) threaded through every visible dot.
   - `$2007 Stress` Test 2: net-new PPU DATA state machine
     (3-cycle latency) + ALE-vs-Read analog feedback model that
     requires sub-PPU-clock granularity the lockstep scheduler does
     not provide.

4. **One candidate is alignment-dependent analog** (`BG Serial In`
   Test 2): the test exploits the 2-5 PPU cycle alignment-dependent
   PPUMASK delay. The test even writes to `$3E01` (a `$2001` mirror)
   to dodge the "wrong-value-written-for-1-ppu-cycle" hardware
   glitch — explicitly an analog hardware-level test. Modeling this
   accurately requires per-CPU-cycle PPUMASK pipelining that would
   cascade through every blargg `ppu_vbl_nmi/*` test (10/10 strict
   load-bearing).

5. **One candidate requires pre-render-as-scanline-5 logic + Composite
   vs RGB distinction**: `Sprites On Scanline 0` Test 2. This is a
   net-new configuration dimension that doesn't fit the current
   region-only state.

The honest path through this sprint, matching Sprint 3's outcome, is:

1. Build the 6 custom ROMs as **permanent regression-prevention
   infrastructure** (DONE — files committed under
   `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes`).
2. **Document the per-test tractability + cascade analysis** in this
   audit doc (DONE — Phase 0.1 table above).
3. **Land NO chip-stack change** in this sprint — pass rate unchanged
   at **84.17%**.
4. **Move forward** per `sprint-gate-conditions.md` §"Per-sprint gate"
   rule 4 (pass rate 83-87% band — proceed to next sprint priority).

## Phase 1 — Per-test architectural rationale

### `Stale BG Shift Registers` Test 3

The test:
1. Sets up sprite zero at `Y=$06, tile=$C8, attr=$03, X=$00`.
2. Sets BG tile `$C8` (a solid white tile) at `$2C00` (NT 3, row 0,
   col 0).
3. Syncs to scanline 0 dot 1.
4. Stalls 767 cpu cycles + 3 nops → arrives at scanline 6 mid-HBlank
   (somewhere after dot 285).
5. Writes `LDA #0 / STA $2001` to disable rendering. The sprite zero
   for scanline 7 has already been evaluated, so the test expects it
   to still be in the sprite shifters.
6. Stalls until HBlank is over (50+20 CPU cycles ≈ 211 PPU cycles).
7. Writes `LDA #$18 / STA $2001` to enable rendering.
8. Expects: sprite zero hit occurs because (a) sprite zero shifter
   was preserved across the disabled window; (b) the BG shift
   registers retained the `11111111 00000000` pattern from the
   unused NT read at dot 241-248 of the previous scanline.

Why our PPU fails Test 3: the BG shift registers in `crates/nes-ppu/
src/ppu.rs:174-176` are unconditionally shifted at line 1181 every
visible dot when `rendering_enabled` (current `mask` snapshot). The
shift_bg path does not currently model the "shifters frozen when
rendering disabled" behavior — but it ALSO doesn't model the
"unused NT byte from dots 241-248 fed into the shifters as if a
tile-load happened" behavior. The two flaws partially compensate;
fixing either alone would diverge.

The fix shape (architectural, multi-session):
- Add `bg_shift_frozen_at_disable: bool` field that latches when
  `$2001` BG-enable transitions 1→0.
- When BG-enable is 1→0 during HBlank, store the current shifter
  state snapshot, then continue updating from the "unused NT read"
  path (currently absent).
- On re-enable, restore the snapshot.

Cascade risk: any change to the BG shifter persistence is observable
in every game that disables rendering mid-frame (status bar split,
some HUD effects). 60-ROM oracle FNV-1a re-baseline likely required.

### `Stale Sprite Shift Regs` Test 3

The test:
1. Sets up sprite zero at `Y=$04, tile=$C5, attr=$03, X=$30`.
2. Stalls 562 cpu cycles → arrives at scanline 4 dot 322.
3. Writes `LDA #0 / STA $2001` to disable rendering at dot 0-ish of
   scanline 5 (after the delay).
4. Stalls 1130 cpu cycles → arrives at scanline 14 dot 324.
5. Writes `LDA #$1E / STA $2001` to enable rendering at scanline 14
   dot 340.
6. Expects: sprite zero hit. The sprite shift registers were frozen
   during the disabled window (scanlines 5-14), so when rendering
   re-enables exactly at dot 340 (scanline 14), the sprite at
   X=$30 still has its shifter loaded from the original scanline 4
   evaluation, and the dot-340 odd-frame-skip-aware shifter-counter
   reload puts it into "counting" mode for scanline 15. When the
   counter decrements to 0 at scanline 15 dot $30 = 48, the sprite
   draws and triggers the sprite zero hit.

Why our PPU fails Test 3: there is no per-sprite "halted vs counting"
mode in `crates/nes-ppu/src/ppu.rs`. The sprite_x array (line 203)
is unconditionally decremented every visible dot when sprite_x > 0
and the sprite is drawing when sprite_x == 0. The "halted" mode is
not modeled.

The fix shape (architectural, multi-session):
- Add `spr_mode: [SpriteCounterMode; 8]` field with `Halted`/`Counting`
  variants.
- At dot 339 of every visible+pre-render scanline, if rendering
  enabled, set all 8 modes to `Counting` AND reload `spr_x` from
  the OAM/secondary-OAM X values.
- When `mode == Halted`, do NOT decrement `spr_x` (the sprite
  draws immediately on enable).

Cascade risk: touches the same FSM B8b regression `63d8dea` clobbered.
Every write site to `spr_*` arrays needs the mid-cycle-clobber audit
per `feedback_emulator_fsm_mid_cycle_clobber.md` rule. Cascade
through sprite-zero-hit tests + sprite-overflow tests + every
sprite-rendering game.

### `BG Serial In` Test 2

The test:
1. Disables rendering, clears NT 2 with empty tiles (`$24`), sets
   sprite zero at (Y=0, tile=`$C0`=single-dot, attr=`$03`, X=`$92`).
2. Sets palette index 1 of palette 3 to (Black, White, Red).
3. Syncs to pre-render dot 324 - 18 PPU cycles (= scanline -1 dot
   306-ish).
4. Stalls a precise number of cycles to land at scanline 5 dot ~6.
5. Loop: writes `LDA #$0 / STA $3E01` (disables rendering via
   `$2001` mirror). Comment: "Writing to a mirror of `$2001`. This
   prevents a hardware issue where the wrong value is written to
   the ppu register for a single ppu cycle." Then `LDA #$1E / STA
   $2001` to re-enable.
6. The test crafts the dot-alignment so the disable falls on
   `dot%8==6` and the re-enable falls on `dot%8==0` (with the 2-5
   PPU cycle PPUMASK delay).
7. With this alignment, the shift register reload at phase 0 is
   SKIPPED, but the high bit plane's serial-in latch keeps feeding
   '1' bits.
8. Expects: a sprite zero hit occurs because the BG shifters are
   filled with '1' bits even though the NT is empty.

Why our PPU fails Test 2: `crates/nes-ppu/src/ppu.rs` does not
model BG shifter serial-in. The `shift_bg` path (line 1308) is
just `<<= 1` with no bit-0 fill. The PPUMASK 2-5 cycle delay is
also not modeled — `cpu_write_register::case 1` (line 769-775)
sets `self.mask = PpuMask::from_bits_truncate(value)` immediately.

The fix shape (multi-axis, multi-session):
- Add `mask_pending: PpuMask` + `mask_pending_dots_remaining: u8`
  fields. On `$2001` write, set pending and a 2-5 dot countdown
  (the exact value depends on which PPU clock edge the M2 fell on).
- Add BG shifter serial-in: when rendering is disabled mid-fetch
  group, instead of latching the (unloaded) tile pattern, feed
  the prior latch value into shift bit 0.

Cascade risk: PPUMASK delay change cascades into `ppu_vbl_nmi/*`
(10/10 strict load-bearing) — those tests already rely on a
2-PPU-clock delay model in the `mask_for_skip_check` / `mask_skip_pipe1`
fields (line 90-91), but only for the dot-skip check. Extending
the delay to the rendering-side mask consumer would diverge.

### `Sprites On Scanline 0` Test 2

The test:
1. Sets sprite zero at (Y=0, tile=`$C6`, attr=`$00`, X=`$80`).
2. Sets BG tile `$C0` (single-dot) at NT $2010 and tile `$24`
   (empty) at NT $2000.
3. Uses NMI to sync CPU and PPU.
4. In the NMI handler, sets PPUCTRL=0, OAMADDR=$00, PPUMASK=$1E,
   runs OAM DMA (`STA $4014` with page 2), stalls 1918 CPU cycles,
   calls `TEST_Scanline0Sprites_TimeItRight`.
5. `TimeItRight` disables rendering, OAM-DMAs 56 times to keep OAM
   from decaying, stalls 354 cycles, enables rendering "briefly
   after dot 66 on the pre-render line."
6. Stalls 158 more cycles, reads `$2002.6` for sprite zero hit.
7. The test expects: on a Composite PPU, sprite zero hit occurs at
   X=$80 (because pre-render line is treated as scanline 5 for
   in-range checks during dots 256-319, and sprite Y=0 lands
   in-range when scanline=5).
8. Repeats with tile shifted to NT $2000 to test X=0 case.
9. Expects: composite PPU detection (one of the two sets has both
   hits, the other has neither).

Why our PPU fails Test 2: the `is_render_scanline` predicate in
`crates/nes-ppu/src/ppu.rs` does not treat pre-render (scanline 261)
as scanline 5 for in-range checks. The sprite_eval FSM at line
1507-1713 evaluates against the CURRENT scanline only.

The fix shape (net-new logic, multi-session):
- During pre-render dots 256-319 (sprite-tile-fetch window for the
  NEXT scanline), interpret sprite Y as if scanline=5 instead of
  scanline=-1 / 261.
- Add Composite vs RGB PPU mode flag.
- Implement dot-340-odd-skip-shifter-shift: the 1-pixel BG jitter
  on odd frames due to the skipped dot, AND the sprite-shifter
  first-pixel-at-x=0 effect.

Cascade risk: alters sprite-zero-hit timing on the FIRST scanline of
every frame; observable in every game with a status bar at the top
of the screen (SMB, Excitebike, Zelda, etc.) and any game that
relies on scanline-0 sprite-zero-hit for its timing engine. Almost
certainly diverges the 60-ROM oracle.

### `$2004 Stress Test` Test 2

The test:
1. Disables rendering, clears NT 2 with empty tiles.
2. Prepares OAM with descending `$FF..$00` (256 bytes).
3. Reads `$2004` on every PPU dot of a chosen visible scanline,
   across multiple frames.
4. Expects: an exact 341-byte pattern (`Test_2004_Stress_AnswerKey1`
   at asm:2040+).

Why our PPU fails Test 2: the eval read address calculation in
`crates/nes-ppu/src/ppu.rs::tick_sprite_eval_active_dot` uses
`(n*4 + m) & 0xFF`. The expected pattern reflects a different
calculation that includes the OAMADDR value at eval start (dot 65),
which is the Session-9 "eval base from OAMADDR" axis.

**This is Sprint 3's deferred axis.** Per the Sprint 4 brief: defer
with back-reference. See Session 27 Sprint 3 audit doc for the
Session-9 14-test cascade documentation.

### `$2007 Stress Test` Test 2

The test:
1. Disables rendering, writes 00, 01, 02, ... 03 to `$2C00..$2FFF`.
2. Reads `$2007` on every PPU dot of a chosen visible scanline,
   across multiple frames.
3. Expects: an exact 341-byte pattern reflecting the PPU DATA state
   machine 3-cycle latency interacting with the BG fetch read
   cadence 2-cycle pattern.

The upstream comments are extensive (asm:2531-2675 includes a full
circuit diagram of the PPU DATA state machine with NOR / D-Latch /
SR latch components). The relevant excerpt: "the read from the PPU
DATA State Machine aligns with the PPU Background/Sprite fetch read
cadence, the result going into the read buffer is either stable or
unstable. Naturally, we'll only be checking the results of the
stable reads."

Why our PPU fails Test 2: `cpu_read_register::case 7` (line 690-740)
performs the `$2007` read as a single inline operation. There is no
3-cycle latency model, no ALE-vs-Read interaction with the BG/sprite
fetch cadence.

The fix shape (architectural, beyond v1.0.0):
- Add PPU DATA state machine with 5-stage D-latch chain.
- Model the analog ALE-vs-Read feedback loop (separate signal
  pipeline from the digital BG/sprite fetch path).
- This requires sub-PPU-clock granularity (half-cycle clock edges)
  the lockstep scheduler does not currently expose.

Cascade risk: even Mesen2 may not pass this cleanly. The upstream
test acknowledges "honestly a real shame half the bytes are
affected by analogue behavior."

## Phase 2 — Outcome decision

**INVESTIGATION-ONLY landing.** No chip-stack code change. The
session ships:

1. This audit document.
2. 6 custom AccuracyCoin sub-test ROMs at
   `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes` (40976 B
   each). Permanent regression-prevention infrastructure.
3. Updated Sprint 4 status in `sprint-gate-conditions.md`.
4. Updated `sprint-4-ppu-misc-residuals.md` status header.
5. CHANGELOG `[Unreleased]` entry documenting the investigation.

**Stop conditions** all matched per the brief:

- `Stale BG Shift Regs`: requires net-new BG shifter retention +
  unused-NT-read state, EXTREME cascade risk into 60-ROM oracle
  framebuffer FNV-1a hashes (the same surface Cascade A
  re-baselined).
- `Stale Sprite Shift Regs`: requires net-new per-sprite counter
  mode state machine; touches FSM mid-cycle-clobber surface
  (B8b lesson).
- `BG Serial In`: requires net-new BG shifter serial-in + PPU MASK
  per-CPU-cycle pipelining; PPUMASK delay change cascades into
  blargg `ppu_vbl_nmi/*` 10/10 strict load-bearing surface.
- `Sprites On Scanline 0`: requires net-new pre-render-as-scanline-5
  in-range check logic + Composite vs RGB PPU config + dot-340-
  odd-skip-shifter effect; touches FSM B8b surface.
- `$2004 Stress`: **Sprint 3 deferred eval-base-from-OAMADDR axis**
  (Session-9 documented 14-test cascade).
- `$2007 Stress`: requires PPU DATA state machine model with
  sub-PPU-clock granularity; even Mesen2 may not pass cleanly.

**Recommendation for Sprint 5**: Per `sprint-gate-conditions.md`
§"Per-sprint gate" rule 4 (pass rate 84.17% in 83-87% band), proceed
to the next sprint priority. Per the Sprint 4 brief's strategic
note: "Sprint 4 is the LAST tractable sprint before the v1.x-deferred
surfaces (C1, SH*, eval-base-from-OAMADDR). If after Sprint 4 the
AccuracyCoin is still < 90%, the gap is empirically gated on Sprint
5 (C1 IRQ-timing rework, 13 prior rollbacks), Sprint 6 (SH* unstable
stores + internal-bus model), Sprint 3 residuals (eval-base-from-
OAMADDR axis)."

Both Sprint 3 (sprite-eval) and Sprint 4 (PPU misc) closed
INVESTIGATION-ONLY with the same root cause: the remaining
v1.0.0-final gap of `90% - 84.17% = 5.83 percentage points = ~8.1
tests` is concentrated on architectural surfaces with documented
multi-session cascade history (C1: 13 prior rollbacks; eval-base-
from-OAMADDR: Session-9 14-test cascade) or analog hardware-precision
modeling that exceeds the current architecture (PPU DATA state
machine, BG shifter serial-in, sprite counter mode, pre-render-as-
scanline-5).

**Honest framing**: The Option-B mandate (continue grinding toward
90%) remains the user's call. The empirical evidence after Sprint 3
+ Sprint 4 closure is that the residual gap requires multi-session
architectural work, not surgical fixes. The next strategic
conversation should weigh:

- **Option B-continued**: pick up Sprint 5 (C1 IRQ-timing rework,
  attempt 17+). The infrastructure is fully landed (Phases A + B1
  + B2/B3 + B4 + per-CPU-cycle IRQ trace + 6 golden baselines).
  Attempt 17 would target the canonical CPU `T_last - 1` IRQ-
  sample-point rework — the architecturally-grounded axis per ADR-
  0002.
- **Option B-pivot**: pick up the Sprint 3 eval-base-from-OAMADDR
  axis with the per-PPU-dot oracle infrastructure (Session-10
  `ppu-state-trace` + Sprint 3's 4 custom ROMs) — this would
  attempt the structural sprite-eval fix that Session-9 cascade-
  reverted, this time with per-dot RustyNES traces (Mesen2's Lua
  API has the per-PPU-cycle blocker; pure RustyNES self-trace can
  reproduce the cascade deterministically).
- **Option D (new)**: re-frame v1.0.0 final at the achieved rate
  (84.17%) and defer the 90% gate to v1.x. This was Option B from
  the rc2 strategic conversation; the user's stated commitment was
  Option B (grind toward 90%). Honor that — but Sprint 3 + Sprint 4
  empirical evidence may motivate a re-evaluation.

## Phase 3 — Workspace test deltas

- Pre-investigation baseline: 545 strict + 5 ignored across 34 suites
  with `--features test-roms`; AccuracyCoin RAM-direct 84.17%
  (117 / 139 assigned); commercial-ROM oracle 60/60 green.
- Post-investigation: **identical to baseline** (no chip-stack code
  changed). The custom ROMs are committed under `tests/roms/` and
  are picked up by `validate_sub_test_rom` but do not run as part of
  `cargo test`.

## References

- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm`:
  - lines 1926-2009 (`TEST_2004_Stress` Tests 1-2 + 256-byte answer
    key + 85-byte answer key).
  - lines 2040-2196 (`TEST_2004_Stress_Evaluate` + answer-key
    decoder).
  - lines 2434-3012 (`TEST_2007_Stress` Tests 1-2 + extensive PPU
    DATA state-machine circuit-diagram annotation).
  - lines 3013-3208 (`TEST_StaleSpriteShiftRegs` Tests 1-7).
  - lines 15255-15351 (`TEST_StaleBGShiftRegisters` Tests 1-4 +
    `Test_StaleShiftRegisters_Run` subroutine).
  - lines 15353-15549 (`TEST_Scanline0Sprites_TimeItRight` +
    `TEST_Scanline0Sprites` Tests 1-3).
  - lines 15780-15860 (`TEST_BGSerialIn` Tests 1-2 +
    `TEST_BGSerialIn_Loop`).
- `docs/audit/cascade-a-investigation-2026-05-19.md` — Session-8
  BG-pipeline cycle-9 reload + post-emit shift landing.
- `docs/audit/session-27-sprint3-sprite-eval-residuals-2026-05-23.md`
  — Sprint 3 INVESTIGATION-ONLY outcome; Session-9 14-test cascade
  on eval-base-from-OAMADDR axis (relevant to `$2004 Stress`).
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`
  — per-test tractability table.
- `crates/nes-ppu/src/ppu.rs`:
  - lines 90-91 (`mask_for_skip_check` / `mask_skip_pipe1` —
    existing 2-PPU-clock PPUMASK pipeline, but only for the
    dot-skip check; BG Serial In requires extending to rendering
    consumers).
  - lines 174-176 (BG shift registers).
  - lines 198-207 (per-sprite shift / x-counter / attr / count).
  - lines 614-743 (`cpu_read_register` — `$2002`, `$2004`, `$2007`
    handlers).
  - lines 1075-1135 (BG fetch + shift-register reload at phase 0 —
    the Session-8 BG-pipeline fix).
  - lines 1167-1183 (emit_pixel + shift_bg post-emit ordering).
  - lines 1507-1713 (`tick_sprite_eval_per_dot`).
- `feedback_emulator_fsm_mid_cycle_clobber.md` — user memory bank
  governance rule for FSM mid-scanline write audits.
- Sprint 4 spec: `to-dos/phase-6-v1.0.0-final/sprint-4-ppu-misc-
  residuals.md`.
- Sprint gate conditions: `to-dos/phase-6-v1.0.0-final/sprint-gate-
  conditions.md`.
