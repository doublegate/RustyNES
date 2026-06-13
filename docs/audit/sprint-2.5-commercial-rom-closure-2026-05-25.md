# Sprint 2.5 (6 Ignored Commercial ROMs) — Closure Audit

**Date:** 2026-05-25 (post-v1.1.0)
**Sprint scope (per v2.0.0 plan):** Structural fix for the mapper-026
VRC6b pair + per-ROM debugging for the 4 remaining ignored
commercial ROMs (tiny_toon_adventures_2, fire_emblem_gaiden,
ganbare_goemon_2, mr_gimmick).
**Outcome:** **CLOSED — all 6 ROMs already un-ignored 2026-05-17
(T-60-003 + T-60-003b).** Sprint 2.5 contributes +0 ROM flips at
v1.2.0; the +4-6 estimate in the v2.0.0 plan was based on
docs/STATUS.md content that became stale between v0.9.0-rc and
v1.0.0-final.

---

## Empirical state at v1.1.0

`crates/nes-test-harness/tests/external_real_games.rs`:

```
#[test] entries: 60
#[ignore] entries: 0
```

All 60 commercial-ROM tests are strict-required. The
`--features test-roms,commercial-roms` workspace test count
stays at 60/60 (verified against the v1.1.0 final tag gauntlet:
"60-ROM commercial-ROM oracle stays at 60/60").

## What was un-ignored when

The 6 ROMs the v2.0.0 plan listed are all already-passing in v1.1.0:

| ROM | Ticket | Date | Root cause |
|---|---|---|---|
| `external_mmc3_tiny_toon_adventures_2` | T-60-003 | 2026-05-17 | ~60s intro exceeded 600-frame budget. Fix: `LONG_INTRO_START_3600` script taps START at frame 3600 to capture post-intro WACKYLAND menu screen. |
| `external_mmc4_fire_emblem_gaiden` | T-60-003 | 2026-05-17 | Same long-intro pattern as Tiny Toon 2. Same `LONG_INTRO_START_3600` fix. |
| `external_vrc4_ganbare_goemon_2` | T-60-003 | 2026-05-17 | mapper-023 sub-variant decoder fix. |
| `external_vrc6b_esper_dream_2` | T-60-003b | 2026-05-17 | **Hypothesis "VRC6b A0/A1-swap pinout decoder" was WRONG.** Actual bug was missing `$6000-$7FFF` WRAM read/write path on the VRC6 mapper. Fixed by adding the WRAM gate. |
| `external_vrc6b_madara` | T-60-003b | 2026-05-17 | Same VRC6 WRAM root cause as Esper Dream 2 — single fix flipped both ROMs as predicted. |
| `external_fme7_mr_gimmick` | T-60-003 | 2026-05-17 | Notoriously long FME-7 splash + Sunsoft logo + animated intro exceeded 600-frame budget. Same `LONG_INTRO_START_3600` fix. |

## What the v2.0.0 plan got wrong

The plan's Sprint 2.5 scope:

> **Sprint 2.5 — 6 ignored commercial ROMs investigation.**
> Structural fix for the mapper-026 VRC6b pair (`esper_dream_2`
> + `madara`) — both share one structural bug. Per-ROM debugging
> for `tiny_toon_adventures_2`, `fire_emblem_gaiden`,
> `ganbare_goemon_2`, `mr_gimmick`.

The plan was authored against the **pre-v1.0.0-final** STATUS.md
content (the bullets at lines 87-97 listing the 6 ROMs). That
list was written when those 6 ROMs were `#[ignore]`'d on the
in-flight `accuracy-stabilization` branch. By v1.0.0 final tag
they had ALL been un-ignored — but the STATUS.md bullets were
never refreshed.

The v2.0.0 plan's estimate of +4-6 ROM flips for Sprint 2.5 is
therefore zero at v1.1.0 baseline. No work is required for this
sprint.

## STATUS.md sweep needed

`docs/STATUS.md` lines 86-97 (the "6 ignored ROMs" bullet list)
needs updating to reflect the un-ignored state. Pattern: replace
the "currently ignored" framing with "previously ignored,
un-ignored on 2026-05-17 per T-60-003 / T-60-003b". This is part
of the v1.1.0 release-doc sweep that landed `[1.1.0]` in
CHANGELOG.md but didn't catch this stale bullet list.

## Sprint 2.5 status

**CLOSED at v1.1.0 baseline** — same closure pattern as Sprint
2.1 (sprite-eval residuals).

The v1.2.0 calendar budget that the plan allocated to Sprint 2.5
(1-2 weeks per the plan's estimate table) is **redeemable** —
recoverable budget that can flow to Sprint 2.2 (PPU Misc; EXTREME
cascade) or Sprint 2.3 (Implied Dummy Reads + DMC; documented
cascade target).

## Pivot recommendation

Given:
- Sprint 2.1 closed at v1.1.0 (3 of 4 already passed; 1 deferred to v2.0)
- Sprint 2.4 NOT TRACTABLE at v1.x (both targets need C1 axis or
  multi-session Mesen2 oracle work — Sprint 2.4 iter 1 audit)
- Sprint 2.5 closed at v1.1.0 (all 6 already un-ignored)

The remaining v1.2.0 options are:
- **Sprint 2.2** (PPU Misc residuals — 6 EXTREME-cascade tests;
  +1-+6 AccuracyCoin gain; requires Cascade-A re-baseline
  authorization)
- **Sprint 2.3** (Implied Dummy Reads + DMC scheduler coordinated;
  +1 AccuracyCoin; Session-19 documented cascade target with
  `Implicit DMA Abort` regression sentinel)

**Recommended next sprint: Sprint 2.3** — concrete +1 AccuracyCoin
gain on a smaller surface than Sprint 2.2's net-new state machines.
The Session-19 cascade is documented and known; the fix shape
(implied-dummy-read on cycle 2 of implied / accumulator opcodes,
coordinated with DMC scheduler awareness of bus-active cycles) is
narrower than Sprint 2.2's BG-shifter retention + sprite counter
modes + PPUMASK pipelining.

## v1.2.0 trajectory re-estimate

| Sprint | Status | Tests gained |
|---|---|---:|
| 2.1 sprite-eval | CLOSED (already passing) | +0 |
| 2.2 PPU misc | NOT YET ATTACKED (EXTREME cascade) | TBD |
| 2.3 Implied Dummy Reads | NOT YET ATTACKED (Session-19 cascade) | TBD (+1 target) |
| 2.4 APU edge cases | iter 1 ROLLED BACK; both targets NOT TRACTABLE at v1.x | +0 (deferred to v2.0) |
| 2.5 commercial ROMs | CLOSED (already un-ignored) | +0 |

Realistic v1.2.0 target if only Sprint 2.3 succeeds:
**90.65% → 91.37%** (1 test flipped).

Realistic v1.2.0 target if Sprint 2.2 + 2.3 both succeed (with
Cascade-A re-baseline authorization):
**90.65% → 95-97%** (up to 7 tests flipped).

The plan's 97% target requires the Cascade-A re-baseline gate.
A user authorization of that gate is the gating decision for the
remaining v1.2.0 calendar budget.
