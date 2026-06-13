# Session 24 — Phase 4: Implied Dummy + DMC oracle blocked, alternates noted

**Date:** 2026-05-23
**Branch:** `main` (post-Session-24-Phase-3 commit `d3f8dee`)
**Scope:** Phase 4 of the v1.0.0-final brief
(`linked-puzzling-sutherland.md`): Implied Dummy + DMC coordinated fix.
**Outcome:** INVESTIGATION-ONLY (third C1-axis adjacent rollback —
Phase 4 has now cascaded in Sessions 19, 20, AND been blocked at oracle
generation in Session-24). Per Decision gate 4A of the brief: no clean
single-axis hypothesis emerges from the custom-ROM oracle path, so the
fix is deferred and the residual ambiguity is documented.

**Predecessors:**
- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`
  (Phase 3 LANDED; informs the wrapper-template pre-seed pattern).
- `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`
  (Phase 2 custom-ROM infrastructure).
- `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
  (Session-20 second cascade rollback).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md`
  (Session-19 first cascade rollback).

## Phase 4.1 — Oracle generation attempt

The Session-23 custom ROM `implied-dummy-reads.nes` (suite 19, test 1)
produces these results post-Phase-3:

| Emulator | `$046D` result | Decoded |
|---|---|---|
| RustyNES | `$0E = (3<<2)\|2` | **Fail at Test 3** (ErrorCode = 3 when failing) |
| Mesen2  | `$8A = (34<<2)\|2` | **Fail at Test 34** (passes 33 sub-tests successfully!) |

The two emulators fail at COMPLETELY DIFFERENT test depths. RustyNES
fails Test 3 (the `DMA + Open Bus prerequisite check`), Mesen2 fails
Test 34 (a much deeper sub-test).

## Phase 4.2 — Custom-ROM dependency-chain issue

`TEST_ImpliedDummyRead` (asm:11634) Test 3 begins with:

```asm
LDA <result_DMADMASync_PreTest	; If this emulator fails the pre-test for the DMA sync routine, then don't even bother trying.
CMP #1
BNE FAIL_ImpliedDummyRead1
```

`result_DMADMASync_PreTest = $12` (zero-page byte). In the FULL battery
this byte is set by earlier tests in the battery sequence (the `DMA Sync`
prerequisite test). In the custom-ROM path, those earlier tests never
run. The Session-24 wrapper-template patch attempted to pre-seed the
flag with `LDA #$01; STA <$12` before `JSR RunTest`, but the seed
doesn't survive (or the test reads a DIFFERENT path on the dependency
that requires the actual prior test runs).

## Phase 4.3 — Cross-diff blocked

Without a custom ROM that exercises the SAME test path on both
emulators, the Phase 4 cross-diff (per Phase 4.3 of the brief) cannot
produce actionable evidence. The two-emulator divergence is at a
sub-test depth that doesn't share the same code path between Mesen2
and RustyNES on the custom ROM.

## Phase 4.4 — Alternative oracle targets identified

Two of the 4 Session-23 custom sub-test ROMs DO produce clean oracle
diffs (Mesen2 passes / pass-with-codes; RustyNES fails):

| Custom ROM | RustyNES result | Mesen2 result | Delta |
|---|---|---|---|
| `controller-strobing.nes` | (LANDED Phase 3) | `$01` PASS | Closed by Session-24 Phase 3 |
| `implied-dummy-reads.nes` | `$0E` Fail Test 3 | `$8A` Fail Test 34 | **BLOCKED — dep chain** |
| `frame-counter-irq.nes` | `$1E` Fail Test 7 | `$01` PASS | Clean oracle |
| `apu-reg-activation.nes` | `$12` Fail Test 4 | `$09` PassWithCode(2) | Clean oracle |

The two clean oracles — `Frame Counter IRQ` Test 7 and `APU Register
Activation` Test 4 — are the natural next targets for a Phase-4-shape
follow-up sprint. Both are in the put/get phase plumbing family
described in Sprint 2's audit (Session-22).

### Frame Counter IRQ Test 7

`TEST_FrameCounterIRQ` (asm:10120) Test 7 is the `$4015` IRQ-flag
clear-on-next-get-cycle semantic — the exact behavior Session-22's
audit identified as a "MEDIUM-tractability" target. The test comment
(lines 10170-10199) gives the FULL architectural model:

> When reading from $4015, bit 6 will be cleared. […] However, bit 6 will
> not be cleared until the next "get" cycle. […] in the event of a regular
> non-double-read, $4015 will still only clear bit 6 on the next get cycle,
> so you probably want to clear bit 6 inside the APU cycle code of your
> emulator, and not in your "read $4015" code.
>
> I suggest making a flag for "we are clearing bit 6 on the next APU get
> cycle" to be set inside the "read $4015" code.

RustyNES already has `pending_irq_clear` in `crates/nes-apu/src/frame_counter.rs`
that models this. The test failure at Test 7 means either (a) the
`apu_aligned` argument we pass to `read_status` is the wrong polarity
for the specific SLO $4015,X double-read sub-cycle alignment, or (b)
the `pending_irq_clear` semantic is delayed by one cycle off.

### APU Register Activation Test 4

`TEST_APURegActivation` (asm:8000) Test 4 is the "OAM DMA from page
$40 should NOT read APU registers" semantic. ErrorCode starts at 2
after Test 1 pre-reqs; INC to 3 after Test 2 (controller open bus);
INC to 4 after Test 3 (the SEI + frame-counter-IRQ-flag-clear-on-read
sequence — analogous to Frame Counter IRQ #7 mechanism). Test 4
itself is the OAM DMA + APU register activation test. Our `$12` result
means Test 4 failed, which is the OAM DMA + APU register interaction
surface. Mesen2's `$09` = PassWithCode(2) indicates Mesen2 passes Test
4 with a quirk-code indicating one specific edge case is handled
differently.

## Phase 4.5 — Decision

Per the brief's Decision gate 4A, no clean single-axis hypothesis
emerges for the SPECIFIC Implied Dummy + DMC coordinated target. The
two prior sessions (19, 20) demonstrated this surface is high-cascade-
risk against the load-bearing `dmc_dma_during_read4` (5 strict) +
`apu_test` (8 strict) surfaces. The third attempt is INVESTIGATION-
ONLY per the brief.

**Status update for Sprint 1**: `to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`
remains OPEN. The custom-ROM dependency-chain issue uncovered in this
session adds a new diagnostic ceiling: even with the Session-23 custom-
ROM infrastructure unblocking the Mesen2 wall-time issue, the
`implied-dummy-reads.nes` ROM is degenerate as an oracle because its
internal dependency chain on `result_DMADMASync_PreTest` (zp $12)
cannot be replicated by simple pre-seeding in the wrapper.

The future path for Sprint 1 closure: either (a) construct a custom
ROM that runs the relevant `Suite_DMATests` entries BEFORE the
`TEST_ImpliedDummyRead` entry (multi-test custom ROM), or (b) tackle
the easier `Frame Counter IRQ Test 7` / `APU Register Activation Test
4` targets first since their custom ROMs have clean oracles.

## Phase 4 outcome

**INVESTIGATION-ONLY (third rollback adjacent to the C1 axis surface).**

No chip-stack code changed. Workspace tests remain at **541 strict +
5 ignored** post-Phase-3. AccuracyCoin pass rate remains at the
**83.45%** Phase 3 landed value (109 pass + 7 pass-with-code of 139
assigned tests).

The Session-23 wrapper-template + Phase-3 controller-drain +
Phase-4-attempted pre-seed of `result_DMADMASync_PreTest` are landed
as permanent diagnostic infrastructure. Future sessions that build
custom ROMs benefit from these layers.

## File changes summary

- `docs/audit/session-24-phase4-implied-dummy-dmc-2026-05-23.md`: this doc.
- `scripts/accuracycoin-build/build_sub_test_rom.py`: wrapper template
  extended with `LDA #$01; STA <$12` pre-seed for the
  `result_DMADMASync_PreTest` dependency. Doesn't fully unblock
  `implied-dummy-reads.nes`, but documents the dependency chain so
  future custom-ROM authors don't re-discover the same issue.
- `tests/roms/AccuracyCoin/sub-tests/implied-dummy-reads.nes`: rebuilt
  with the dependency pre-seed.  (Currently still fails at Test 3 on
  RustyNES; Mesen2 advances further but to a different test depth.)
- `to-dos/phase-6-v1.0.0-final/sprint-gate-conditions.md`: Phase 4
  entry added marking INVESTIGATION-ONLY.

## References

- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 11634-11700
  (TEST_ImpliedDummyRead — Test 3 prerequisite-chain).
- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 10120-10199
  (TEST_FrameCounterIRQ — Test 7 put/get semantic, the alternate
  oracle target).
- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 8000-8130
  (TEST_APURegActivation — Test 4 OAM DMA + APU register activation,
  the other alternate target).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` + 
  `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
  (the two prior cascade-revert sessions).
- `crates/nes-apu/src/frame_counter.rs:109-152` (the existing
  `pending_irq_clear` plumbing that Frame Counter IRQ #7 exercises).
