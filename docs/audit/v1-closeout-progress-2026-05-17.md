---
date: 2026-05-17
phase: Phase 6 — v1.0.0 closeout (in progress)
status: partial — investigations + small wins landed; major gates remain
---

# v1.0.0 Closeout — Session Progress Audit (2026-05-17)

## Scope

This session worked Phase 6 of `to-dos/ROADMAP.md` (v1.0.0 closeout) per the
session `/goal` directive "continue with Phase 6 ... until tag/release 1.0.0
can be applied." Work spans T-60-001 (C1 IRQ-timing), T-60-002 (AccuracyCoin
push), and T-60-003a/b/c (6 ignored commercial ROMs).

## Wins landed

### T-60-003a — long-intro commercial ROMs (CLOSED)

**2 of 6** ignored commercial ROMs flipped from `#[ignore]`'d to strict-pass:
- `Mr. Gimmick` (FME-7 / mapper 69)
- `Tiny Toon Adventures 2 - Trouble in Wackyland` (MMC3 / mapper 4)

Root cause: not real bugs — the default `IdleOnly { frames: 600 }` input
script landed on uniform-palette animation frames during these games'
~60-second intro sequences. New `LONG_INTRO_START_3600` input script
(idle 3600 → 1-frame START → idle 60 → free-run 240, captures at
f3661 / f3901) bypasses the intro and captures the post-intro menu
screens with rich pixel signal. Diagnostic confirmed 10 distinct
framebuffer states across f60..f3600 for both ROMs (game IS running
and animating; intro is just long).

Commit: `7fa2c90` `test(roms): flip Mr. Gimmick + Tiny Toon 2 from
#[ignore] to passing (T-60-003a)`.

### T-60-002 — SH* TAS opcode partial fix

**$9B TAS / SHS / XAS** restored to canonical formula. Pre-fix the
impl used `addr_abs_y(bus)` (POST-Y effective address) for the
`(H+1)` AND; the canonical nesdev formula uses the BASE address's
high byte (pre-Y), with the same page-crossing corrupted-high-byte
rule as the other SH* opcodes ($93/$9F/$9C/$9E). Pre-fix AccuracyCoin
reported $9B with error 1 (independent failure shape from the other
4); post-fix all 5 SH* opcodes fail uniformly with error 7 — the
shared failure mode now points at a single upstream-spec-vs-impl
gap rather than 2 separate bugs.

Commit: `0439800` `fix(cpu): TAS ($9B) use BASE high byte +
page-cross corruption (T-60-002)`.

## Negative results (rolled back)

### T-60-003b/c attempt — WRAM init value $00 → $FF

Hypothesis: the 4 stuck commercial ROMs (Fire Emblem Gaiden,
Ganbare Goemon 2, Esper Dream 2, Mouryou Senki Madara) all stall
at the bit-identical `89ee4c476c97a325` "uniform gray $00 palette"
hash, and all read WRAM at boot. Real-silicon SRAM powers up near
$FF, not $00; standard emulator convention. Maybe these Konami ROMs
have a battery-save validation path that goes wrong when WRAM is $00.

Tested by changing `prg_ram: vec![0u8; ...]` to
`prg_ram: vec![0xFFu8; ...]` across all 5 mappers with WRAM (MMC1,
MMC3, MMC5, MMC4, VRC family).

**Result: ZERO change** to the 4 stuck ROMs' hashes — the WRAM init
value doesn't affect their early-init path at all. AND **Kirby's
Adventure regressed** to a different hash (genuine save-RAM
behavioral divergence). Net: -1 strict test for 0 stuck-ROM fixes.
Reverted via `git checkout HEAD --`.

The 4 stuck ROMs need a deeper diagnostic — probably cycle-by-cycle
tracing through one of their stuck loops to identify the specific
wait condition that's not being met.

### T-60-002 attempt — SH* full unstable family fix

The SH* opcodes are unstable: real silicon has **4 different
behaviors** depending on manufacturer (per upstream AccuracyCoin.asm
lines 5161-5200):
- **Behavior 1**: `M = A & X & (H+1)` (the textbook formula; our impl)
- **Behavior 2**: high byte corruption = `X & (H+1)` only
- **Behavior 3**: high byte corruption = `(A & MAGIC) & (H+1)`
- **Behavior 4**: high byte corruption = `(A | X) & (H+1)`

AccuracyCoin's SH* tests adaptively detect which behavior the impl
exhibits, then run sub-tests against that behavior's expected
contract. Our Behavior 1 impl produces a value that doesn't match
the adaptive test's downstream expectations — likely a subtle
off-by-one in the page-crossing corrupted-high-byte logic OR the
"AND with H not H+1 when an interrupt fires mid-indexing" silicon
detail that visual6502 documents but most emulators don't model.

Closing the SH* family requires careful comparison against the
upstream test ROM source (now downloaded at
`https://github.com/100thCoin/AccuracyCoin/blob/main/AccuracyCoin.asm`
lines 5161+ for SH* test definitions). Estimated 4-8 hours of
focused work to flip all 5 tests.

## Stuck-ROM cycle-trace diagnostic (negative finding)

Built throwaway diagnostic harness at
`crates/nes-test-harness/tests/stuck_roms_diagnostic.rs` (deleted
after use; commit history preserves it). Probed each of the 4 stuck
ROMs over 600 frames:

| ROM | unique PCs | PPUMASK | PPUSTATUS | Stuck loop range |
|---|---|---|---|---|
| Fire Emblem Gaiden | **13** | `$1E` (rendering ENABLED!) | `$00` | `$C052-$C158` |
| Ganbare Goemon 2 | **17** | `$00` (rendering off) | `$00` | `$C203-$C249` + `$800A` + `$C6B3` |
| Esper Dream 2 | **12** | `$00` | `$00` | `$86B5-$86C6` + `$A7F5` + `$E41C` |
| Mouryou Senki Madara | **9** | `$00` | `$00` | `$80E1-$80F0` + `$E855-$E877` |

Comparators (known-working):
| Akumajou Densetsu (VRC6, works) | **86** | `$1E` | `$00` | (no tight loop) |
| Fire Emblem proper (works) | **67** | `$1E` | `$00` | (no tight loop) |

Loop-body byte dumps reveal **all 4 stuck ROMs have a `LDA $2002;
BPL` wait-for-vblank pattern** in their stuck PC set — the wait
returns (vblank fires), but program then loops back into another
tight loop, presumably failing some other validation. Fire Emblem
Gaiden specifically has rendering ENABLED — yet framebuffer is
uniform gray = no nametable/palette data ever written. The game is
stuck in an early init loop AFTER enabling rendering but BEFORE
writing anything to PPU memory.

Next-step targets for closing T-60-003b/c (need 2-5 day investigation
per ROM):

1. **VRC6b shared decoder** — Esper Dream 2 + Madara identical hash.
   Inspect `crates/nes-mappers/src/sprint3.rs` for mapper-26 (VRC6b)
   pinout permutation vs mapper-24 (VRC6a, which works). The
   PRG-bank-register pinout differs; possibly our impl has VRC6b
   swapped with VRC6a or has a wrong pin layout.

2. **Ganbare Goemon 2 mapper-23 sub-variant** — Wai Wai World +
   Akumajou Special pass on mapper 23; Ganbare Goemon 2 doesn't.
   Possibly Konami's VRC4a/b/c/d/e/f pinout sub-variants need
   per-sub-variant register-bit permutation.

3. **Fire Emblem Gaiden** — same mapper as Fire Emblem proper
   (mapper 10 / MMC4) but different behavior. Inspect MMC4
   CHR-latch logic; FE Gaiden may use a specific bank-switching
   sequence at boot that hits a quirk.

## Critical-path remaining work for v1.0.0

| Gate | Status | Estimated effort |
|---|---|---|
| **T-60-001 — C1 IRQ-timing rework** | open. 7 prior rollbacks; M2-low IRQ sample (8th) is the first positive signal. Needs canonical CPU `T_last - 1` IRQ-sample-point rework on `cpu_interrupts_v2` axis | 3-7 days focused work |
| **T-60-002 — AccuracyCoin 69.78% → 90%** | open. 42 failing tests catalogued; per-failing-test diagnostics print every CI run via `accuracycoin.rs`. SH* family (5 tests) partially diagnosed; needs upstream-source-driven per-opcode fix loop | 5-10 days incremental |
| **T-60-003b/c — 4 stuck commercial ROMs** | open. Diagnostic data captured (cycle-trace + loop-byte dump). Per-ROM root-cause investigation per the 3-target list above | 3-5 days per ROM |
| **T-60-004 — Multi-OS release-artifact smoke test** | open. User-driven manual gate | hours (user only) |
| **T-60-005 — `v1.0.0` tag + release notes** | blocked by 1-4 above | 1 day |

**Realistic v1.0.0 timeline: ~3-5 weeks** of focused single-author
work assuming T-60-003 stuck ROMs are deferrable to v1.x (move them
to "known-deferred-to-v1.x" with a public rationale rather than
fixing them all pre-tag).

## Session commits

1. `7fa2c90` `test(roms): flip Mr. Gimmick + Tiny Toon 2 from #[ignore] to passing (T-60-003a)` — 2 commercial ROMs ignored count `6 → 4`.
2. `0439800` `fix(cpu): TAS ($9B) use BASE high byte + page-cross corruption (T-60-002)` — SH* family consistency fix; AccuracyCoin unchanged at 69.78% but pre-fix $9B independent failure shape consolidated with the other 4.
3. `7a8859a` `docs(audit): Phase 6 session progress + T-60-003a closure (2026-05-17)` — this doc + ROADMAP delta.
4. `754583f` `docs(changelog): Phase 6 v1.0.0 closeout session 1 summary` — `[Unreleased]` Phase 6 session 1 subsection.
5. `090671b` `fix(cpu): drop opcode-fetch IRQ sample on taken branches (C1 9th attempt)` — taken conditional branches now drop their cycle-1 IRQ sample so IRQ detection is deferred to the next instruction per nesdev wiki §"CPU interrupts" §"Branch instructions". Correctness improvement; test count unchanged (510 strict + 5 ignored). C1 residual `cpu_interrupts_v2/{2,3,5}` still ignored — the branch axis was not the load-bearing one for those 3 tests.
6. `<this commit>` `feat(apu): defer $4015 frame-IRQ clear to next get-cycle on put-cycle read` — per AccuracyCoin upstream `APU Frame Counter IRQ` tests 6+7 and `Implied Dummy Reads` test 2 documentation: reading `$4015` on a PUT cycle defers the frame-IRQ-flag clear to the NEXT GET cycle. `FrameCounter` gets a new `pending_irq_clear` field; `read_status` + `tick` take an `apu_aligned` parameter threaded through from `apu.rs`. **+1 strict pass** (new `read_4015_on_put_cycle_defers_irq_clear_to_next_get` unit test). AccuracyCoin rate unchanged at 69.78% — the deferred clear is correctly modeled at the frame-counter level but the AccuracyCoin tests' specific SLO RMW pattern bracketing needs additional cycle-precise apu_phase propagation through the RMW sequence.

Test count after this session: **532 strict + 5 ignored** (was 531 + 5 at session start).

## C1 9th attempt analysis

The 9th C1 attempt targeted the "branch_delays_irq" axis: per nesdev,
taken conditional branches DELAY IRQ detection by 1 instruction.
Our impl was sampling IRQ at cycle 1 (opcode fetch) then skipping
subsequent cycles, but the cycle-1 sample was still arming the IRQ
for the branch instruction itself. The fix drops the cycle-1 sample
in the taken-branch path of `branch()`. **Test count unchanged**
post-fix — the C1 residuals `cpu_interrupts_v2/{2,3,5}` continue
to fail with the same shape, indicating those tests are sensitive
to a DIFFERENT cycle of the IRQ-sample path (likely the
`promote_post_step_interrupts` semantics for tests 2/3, or the
page-cross specific timing for test 5).

The 9th attempt is **non-regressing + correctness-improving** —
matches nesdev spec better than pre-fix even though no test catches
the difference YET. Keeps the fix landed as a foundation; future
C1 work on tests 2/3/5 can build on top.

## AccuracyCoin per-test investigation summary

Subsequent per-test investigations (downloaded upstream source from
`https://github.com/100thCoin/AccuracyCoin/blob/main/AccuracyCoin.asm`,
read full test definitions for SH* / NMI-overlap / Frame Counter
IRQ / $2007 read / Rendering Flag Behavior / Implied Dummy Reads /
t Register Quirks / $2002 Flag Timing / INC $4014) revealed each
remaining test requires deep multi-hour investigation:

- **NMI Overlap BRK / NMI Overlap IRQ + Interrupt flag latency**:
  all gated on the same canonical CPU `T_last - 1` IRQ-sample-point
  rework as cpu_interrupts_v2/{2,3,5} — would close with C1 success.
- **Frame Counter IRQ test 6 + Implied Dummy Reads test 2**: both
  depend on the same APU `$4015` read-clearing-on-next-get-cycle
  behavior. Single fix could close both but requires get/put-cycle
  alignment plumbing in the APU `frame_counter.rs::read_status`
  + apu cycle-parity propagation; estimated 4-8 hours focused work
  with significant snapshot-rebaseline risk.
- **$2002 flag timing test 1**: depends on the M2 15/24 duty-cycle
  silicon detail (vblank flag read at M2-high, sprite flags read at
  M2-low ~1.875 PPU cycles later). Same C1 axis.
- **Rendering Flag Behavior test 2**: independent shift-register
  population when only sprites enabled — requires decoupling BG
  shift-register population from BG visibility in PPU rendering
  pipeline. Multi-hour PPU rework with sprite-shift-test risk.
- **Sprite Evaluation suite (11 tests, all error 1)**: B8 FSM
  doesn't model OAM corruption, $2004-during-render quirks, sprite-
  resize-during-render. Each needs per-test investigation.
- **INC $4014 test 2**: RMW on $4014 should trigger DMA twice
  (read+write+write). Multi-day work with DMA-test risk.

The pattern across all 42 failing tests is: most are gated on
either the C1 IRQ-timing axis (close 8-10 tests when C1 lands)
or per-test silicon edge cases requiring 4-8 hours each.

## Test count after this session

- `cargo test --workspace --features test-roms`: 531 strict + 5 ignored (unchanged).
- `cargo test -p nes-test-harness --features test-roms,commercial-roms --test external_real_games`: **56 strict + 4 ignored** (was 54 + 6).
- AccuracyCoin RAM-direct pass rate: 69.78% (unchanged; SH* fix consolidated but didn't flip).
