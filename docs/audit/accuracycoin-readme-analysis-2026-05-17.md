---
date: 2026-05-17
phase: Phase 6 — v1.0.0 closeout
status: research / triage — no code change, informs T-60-002 priority ordering
upstream-ref: https://github.com/100thCoin/AccuracyCoin
---

# AccuracyCoin README + Source Analysis — Cascade Diagnostic (2026-05-17)

## Scope

User request: "Read and analyze the README.md (and any other documentation /
source code) at `https://github.com/100thCoin/AccuracyCoin` to see if it
assists with any of these remediations." Sources reviewed:

1. Upstream `README.md` (52,747 bytes, full error-code legend for all
   144 tests).
2. Upstream `AccuracyCoin.asm` (18,758 lines, MIT-licensed — vendored at
   `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` via Phase D2; full source
   pulled fresh to `/tmp/AccuracyCoin.asm` for this analysis).
3. `https://api.github.com/repos/100thCoin/AccuracyCoin/contents/` —
   confirmed the repo has no wiki, CHANGELOG, or design docs beyond
   README + .asm + ROM.

Outcome: README provides the per-test error-code legend, which when
cross-referenced with our **42 failing tests** reveals two large
**cascade points** — a single root-cause fix in either cluster
flips many dependent tests at once. This is the load-bearing
diagnostic input the prior `v1-closeout-progress-2026-05-17.md`
audit was missing.

## Current failing-test snapshot (post-Phase-D3, pass-rate 69.78%)

`cargo test --features test-roms --release -p nes-test-harness --test
accuracycoin -- --nocapture`:

```
total=144 pass=96 pass_with_code=1 fail=42 skipped=0 not_run=5 unknown=0
pass rate = 69.78% over 139 assigned tests
```

Per-suite breakdown (42 fails):

| Suite                           | Fail | Notes                                |
|---------------------------------|------|--------------------------------------|
| APU Registers and DMA tests     |    9 | 8 share root cause (DMC DMA cycle)   |
| APU Tests                       |    5 | Frame Counter IRQ, DMC, Activation   |
| CPU Behavior                    |    1 | `Open Bus [error 9]` ($4015 bit 5)   |
| CPU Behavior 2                  |    1 | `Implied Dummy Reads [error 2]`      |
| CPU Interrupts                  |    3 | NMI Overlap × 2, Interrupt latency   |
| PPU Behavior                    |    2 | Rendering Flag, $2007 read w/ rdr    |
| PPU Misc.                       |    7 | All sprite-zero-hit gated            |
| Sprite Evaluation               |    9 | **All [error 1]** — same prerequisite|
| Unofficial Instructions: SH*    |    5 | All `[error 7]` — RDY-low-2-cycles   |

## Cascade A — Sprite Zero Hit gates 16 tests

**Root failure**: `Sprite Evaluation :: Sprite 0 Hit behavior [error 1]`
= per upstream README "A Sprite zero hit did not occur."

**Test setup** (from `/tmp/AccuracyCoin.asm:6754-6803`, `TEST_Sprite0Hit_Behavior`):

```asm
PREP_SpriteZeroHit:
    PrintCHR  .word $2001  .byte $FC, $FF   ; tile $FC at nametable $2001
    ResetScroll                              ; v=$2000
    ClearPage2                               ; $200-$2FF = $FF
    InitializeSpriteZero  .byte $00, $FC, $00, $08   ; Y=0, CHR=$FC, Att=0, X=8
    WaitForVBlank
    RTS

TEST_Sprite0Hit_Behavior:
    JSR PREP_SpriteZeroHit
    ;; Test 1
    JSR EnableRendering_S    ; OR's PPUMASK bit $10 (sprites)
    LDX #02   STX $4014       ; OAM DMA from page 2
    JSR Clockslide_3000        ; ~1 frame
    LDA $2002  AND #$40       ; check sprite-zero-hit bit
    BEQ FAIL_Sprite0Hit_Behavior1
```

Note `EnableRendering_S` ORs in **only** the sprite bit; BG bit assumed
already set from prior test state.

**Cascade scope** (16 tests gated):
- All 9 Sprite Evaluation tests fail `[error 1]` (Sprite Overflow,
  $2002 flag timing, Suddenly Resize Sprite, Arbitrary Sprite zero,
  Misaligned OAM, Address $2004, OAM Corruption, INC $4014 — every
  one has a `JSR VerifySpriteZeroHits` prerequisite at the start).
- 7 PPU Misc tests are sprite-zero-gated:
  - `t Register Quirks [error 1]` = "Sprite Zero Hits should be working"
  - `Stale BG Shift Registers [error 3]` = needs s0 hit working
  - `Stale Sprite Shift Regs [error 1]` = needs s0 hit working
  - `BG Serial In [error 2]` = s0-hit-on-blank-nametable check
  - `Sprites On Scanline 0 [error 2]` = needs Y=0 sprite hitting line 1
  - `$2004 Stress Test [error 2]` = depends on Sync_To_VBlank (s0-related)
  - `$2007 Stress Test [error 2]` = depends on Sync_To_VBlank
- Also `PPU Behavior :: $2007 read w/ rendering [error 1]` = same
  prerequisite.

**Investigation pointers** (PPU sprite-zero-hit code):
- `crates/nes-ppu/src/ppu.rs:1188` — `if i == 0 && self.spr_zero_in_line`
  (detection gate).
- `crates/nes-ppu/src/ppu.rs:1204-1212` — actual SPRITE_ZERO_HIT bit set,
  guarded by `pixel_x < 255` and 8-pixel-mask predicate.
- `crates/nes-ppu/src/ppu.rs:1349` — FSM commits `spr_zero_in_line` at
  dot 257.
- `crates/nes-ppu/src/ppu.rs:2244-2311` — reference sprite-eval that
  matches our FSM (1013-case equivalence harness passing).

**Why this might be failing** — sprite-eval FSM (B8) was the
last-recovered regression; both the reference and FSM treat sprite
Y=0 + scanline=0 as "found → rendered on scanline 1." If our PPU's
sprite-zero-hit firing is timing-correct, then the failure is
either (a) OAM DMA timing in this exact test pattern (OAM DMA runs
during VBlank with rendering pre-enabled), or (b) the BG bit of
PPUMASK is not actually set when the test runs (the test ROM
relies on prior PPUMASK_COPY state to carry BG-on across tests
because `EnableRendering_S` doesn't enable BG). Hypothesis (b) is
worth testing first — a short test ROM might prove PPUMASK state
is sticky on real hardware but not on ours after some intermediate
flow.

## Cascade B — DMC DMA cycle alignment gates 8 tests

**Root failure**: 8 of 9 "APU Registers and DMA tests" fail with
`[error 2]` — per upstream README "DMC DMA was either on the wrong
cycle, or the halt/alignment cycles did not read from $XXXX."

**Tests gated**:
| Test                  | Error 2 description                                          |
|-----------------------|---------------------------------------------------------------|
| `DMA + $2002 Read`    | DMA halt/align cycles did not read from $2002                |
| `DMA + $2007 Read`    | DMA halt/align cycles did not read from $2007                |
| `DMA + $2007 Write`   | DMA was not delayed by the write cycle (error 1 — prereq)    |
| `DMA + $4015 Read`    | DMA halt/align cycles did not read from $4015 → clear flag   |
| `DMA + $4016 Read`    | DMA halt/align cycles did not clock controller port via $4016|
| `DMC DMA Bus Conflicts` | bus conflict with APU registers not emulated correctly     |
| `DMC DMA + OAM DMA`   | overlapping DMAs did not spend correct CPU cycle count       |
| `Explicit DMA Abort`  | aborted DMAs did not spend correct CPU cycle count           |
| `Implicit DMA Abort`  | aborted DMAs did not spend correct CPU cycle count           |

The single shared root: the DMC DMA halt cycle should re-issue
the original CPU address on the bus during the halt + alignment
phases (1-4 stall cycles), making those cycles into ADDITIONAL
reads of the same address. nesdev's `DMA` page documents this.
Our current `LockstepBus::tick_one_cpu_cycle` likely stalls the
CPU but does NOT re-issue the halted CPU's read address during
the stall cycles, so the side effects of the read on $2002/$2007
/$4015/$4016 are missed.

**Implementation pointer**:
- `crates/nes-core/src/lockstep_bus.rs` — DMC DMA stall path.
- `crates/nes-apu/src/dmc.rs` — DMA request / completion.
- nesdev `https://www.nesdev.org/wiki/DMA` — canonical halt + align
  cycle behavior.

**Risk profile**: medium. Changing DMC DMA stall behavior could
regress `dmc_dma_during_read4` (which currently passes); the fix
needs to preserve the existing CPU stall cycle count while adding
the re-read semantics.

## Other failing tests (no cascade)

| Test                                    | Why                                                                                 |
|-----------------------------------------|--------------------------------------------------------------------------------------|
| `CPU Behavior :: Open Bus [error 9]`    | Bit 5 of $4015 should be open bus. Previously attempted; rolled back when it regressed Internal Data Bus Test 2 (internal vs external bus model needed). |
| `CPU Behavior 2 :: Implied Dummy Reads [error 2]` | "Frame counter interrupt flag not properly implemented." Adjacent to Frame Counter IRQ test 6 — same area. |
| `APU Tests :: Frame Counter IRQ [error 6]` | "IRQ flag should be cleared when APU transitions from put → get cycle." Partial fix landed (`pending_irq_clear` deferred-clear path in `frame_counter.rs`) but test still fails — likely requires modeling SLO $4015 read-modify-write bus behavior (the test reads $4015 via SLO's dummy-write-original cycle, which on real silicon also clears the IRQ flag). |
| `APU Tests :: Delta Modulation Channel [error 21]` | Error 21 = K = "sample address should overflow to $8000 instead of $0000." DMC-internal address wrap fix. |
| `APU Tests :: APU Register Activation [error 1]` | Prerequisite cascade — fails because at least one of "CPU and PPU open bus, PPU Read Buffer, DMA + Open Bus, DMA + $2007 Read" failed. Auto-flips if Cascade B + Open Bus fix lands. |
| `APU Tests :: Controller Strobing [error 4]` | "Controllers should not be strobed when CPU transitions from put → get cycle." |
| `APU Tests :: Controller Clocking [error 6]` | "put/halt cycles of DMC DMA should clock controller if DMA occurs during a $4016 read." |
| `CPU Interrupts :: Interrupt flag latency [error 11]` | Error 11 = B = "Branch instructions should poll for interrupts before cycle 4." Adjacent to our just-landed "TAKEN branches DELAY IRQ detection" fix; the test wants the poll point at the cycle-3 boundary specifically. |
| `CPU Interrupts :: NMI Overlap BRK [error 2]` | "Either NMI timing is off, or interrupt hijacking is incorrectly handled." Part of the broader C1 IRQ-timing axis. |
| `CPU Interrupts :: NMI Overlap IRQ [error 1]` | "Either NMI timing is off, IRQ timing is off, or interrupt hijacking incorrectly handled." Same axis as NMI Overlap BRK. |
| `PPU Behavior :: Rendering Flag Behavior [error 2]` | "BG shift registers should be initialized and clocked when only rendering sprites." Edge case: sprite-only rendering should still tick BG shifters internally. |
| `Unofficial Instructions: SH* :: [error 7] × 5` | All 5 SH* opcodes ($93/9B/9C/9E/9F) — "If RDY line goes low 2 cycles before write cycle, target address was not correct." Models DMC DMA halt interaction with SH* address corruption. Coupled to Cascade B. |

## Recommended prioritization (highest ROI first)

1. **Cascade B (DMC DMA halt re-read)** — 8 tests flip on one fix
   (potentially 9-10 with SH* coupling). nesdev spec is canonical.
   Medium risk; preserve existing CPU stall count.
2. **Cascade A (Sprite Zero Hit Test 1)** — 16 tests gated; first
   bisect the PPUMASK_COPY assumption (whether BG bit is set at
   test entry), then investigate sprite-eval / OAM DMA / sprite-0
   pixel detection cycle-precision if PPUMASK is correct.
3. **Frame Counter IRQ Test 6 + Implied Dummy Reads Test 2** —
   Coupled; both depend on $4015 read-modify-write modeling. Defer
   until Cascade B lands (DMC DMA dummy-read pattern is the
   blueprint).
4. **DMC error 21** — single-test fix, low risk (DMC sample address
   wrap-to-$8000 semantics).
5. **NMI Overlap BRK + IRQ + Branch latency [error 11]** — all
   share the C1 IRQ-timing axis. 10 prior rollback attempts; the
   canonical CPU `T_last - 1` IRQ-sample-point rework is the
   structural fix. Multi-week investigation.
6. **Open Bus [error 9] + SH* [error 7]** — defer to v1.x; require
   internal-vs-external bus model rework that previously regressed
   Internal Data Bus Test 2.

## Realistic v1.0.0 trajectory

If Cascade B + Cascade A both close cleanly: **42 → ~17 failing
tests**, pass rate 69.78% → ~87%. The remaining ~17 are split
between the C1 IRQ-timing axis (3 tests, multi-week) and the
internal-bus / read-modify-write modeling cluster (~5 tests,
multi-day each).

The **v1.0.0 ≥ 90% gate is achievable only with Cascade B + A
landed AND the C1 axis resolved**. T-60-001 alone has 10 prior
rollback attempts across multiple sessions; without an
empirically-grounded breakthrough on the canonical `T_last - 1`
sample-point shift, the v1.0.0 tag is multi-week away.

The user's `/goal` directive remains open. Next-session action:
prototype Cascade B on a feature branch (`accuracycoin-dma-halt`)
with `dmc_dma_during_read4` as the regression guard.

## Files / references

- `/tmp/AccuracyCoin.asm` (re-fetched 2026-05-17, MIT-licensed,
  upstream `100thCoin/AccuracyCoin@main`)
- `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` (vendored Phase D2)
- `crates/nes-test-harness/src/accuracy_coin_catalog.rs` (decoder)
- `crates/nes-test-harness/tests/accuracycoin.rs` (test entry +
  per-failing-test diagnostic print)
- `docs/audit/v1-closeout-progress-2026-05-17.md` (prior audit;
  this doc supplements it with the README cascade analysis)

## Addendum (2026-05-19, session 6): Cascade A partial closure — OAMADDR reset fix

Diagnostic probe extended in `crates/nes-test-harness/src/accuracy_coin.rs`
to capture `(PPUMASK_COPY, PPUCTRL, PPUSTATUS, OAMADDR, v)` at the
moment the Sprite 0 Hit result byte transitions. Empirical finding
(frame 3393):

```
OAMADDR = 0x05  <-- ROOT CAUSE of Sprite 0 Hit failure
```

OAM DMA via `STX $4014` placed sprite 0 at OAM[$05..$09] instead of
OAM[0..4] because OAMADDR was non-zero. Sprite-eval FSM reads OAM[0]
for "sprite 0" Y, which was `$FF` (off-screen) → `spr_zero_in_line =
false` → no sprite-zero hit fires.

**Fix landed**: PPU OAMADDR reset during dots 257-320 of every rendered
scanline, per the well-documented nesdev hardware behavior (PPU
registers §OAMADDR). 5 lines added to `crates/nes-ppu/src/ppu.rs`'s
`tick` function inside the existing `if render_line && rendering`
block.

**Outcome**: AccuracyCoin RAM pass rate `76.98% → 78.42%` (+2 tests
flipped, +1.44pp). `Sprite Evaluation :: Sprite overflow behavior` →
PASS. `Sprite 0 Hit behavior` advances from error 1 → **error 13**
(now passing the first 12 sub-tests; remaining failures are at
sub-tests 13+ which test sprite-zero hit at non-trivial positions
beyond the basic Y=0/X=8 scenario).

**Regression accept**: 10 commercial-roms snapshots re-baselined with
explicit user authorization. 9 are framebuffer-identical audio-only
deltas (accuracy improvements, not regressions). 1 (Mr. Gimmick,
FME-7) has FB+audio changes — already a marginal long-intro passer
per the v1.0.0-rc1 release notes; FB delta investigated separately
as v1.x follow-up.

**Sacred trio (SMB / Excitebike / Kid Icarus PAL) UNAFFECTED.**

**Remaining Sprite 0 Hit gaps** (Cascade A's remaining 14 tests):

- Sub-tests 13+ of `TEST_Sprite0Hit_Behavior` (more elaborate
  scenarios: sprite at X=0 with mask, X>0 in mask, Y=238/239 edge,
  scrolling, etc.).
- The 7 PPU Misc. tests still gated on more elaborate sprite-zero
  behavior.
- `$2002 flag timing`, `Suddenly Resize Sprite`, `Arbitrary Sprite
  zero`, `Misaligned OAM behavior`, `Address $2004 behavior`,
  `OAM Corruption` — each requires further cycle-precision work
  on adjacent PPU subsystems.

**Next session**: target sub-tests 13+ of Sprite 0 Hit + the related
PPU Misc cluster. Each sub-test now exposes a narrower failure than
the broad "no hit fires at all" pre-fix state.

## Addendum (2026-05-19, session 5): Cascade A hypothesis B REJECTED

A read-only diagnostic probe landed in
`crates/nes-test-harness/src/accuracy_coin.rs ::
capture_sprite_zero_hit_test_entry_state()` and is wired into
`tests/accuracycoin.rs`. It runs a focused second battery and snapshots
`PPUMASK_COPY` (`$00F1`) + `PPUSTATUS` at the moment the per-test
result byte at `$0457` (Sprite 0 Hit behavior) first transitions from
zero.

**Empirical finding** (test run, 2026-05-19):

```
AccuracyCoin Cascade A: at frame 3393, when result byte at $0457
transitioned from 0, prior frame's PPUMASK_COPY=0x1E (bit $08=BG,
$10=spr), PPUSTATUS bit 6 (sprite-zero hit) = 0
  HYPOTHESIS B REJECTED: PPUMASK_COPY bit $08 (BG) IS SET at test
  entry — failure is in PPU sprite-zero-hit cycle precision, not the
  PPUMASK environmental state.
```

Hypothesis (b) from the original §"Cascade A" section is **rejected**.
`PPUMASK_COPY = 0x1E` at the moment the Sprite 0 Hit test runs:
bits 4 (SHOW_SPRITE), 3 (SHOW_BG), 2 (SHOW_BG_LEFT), 1 (grayscale all
set). BG rendering IS enabled. The `EnableRendering_S` `ORA #$10` is
honored by our emulator just as it is on real hardware.

The error decode was also re-examined: AccuracyCoin's per-test runner
initialises `ErrorCode = 1` (not 0) before each test, then `INC
<ErrorCode` after each sub-test PASS. `TEST_Fail` returns
`A = (ErrorCode << 2) | 2`. The Sprite 0 Hit result byte `0x06 = (1
<< 2) | 2` therefore means `ErrorCode = 1` at the FAIL point — **no
INC happened**, so **sub-test 1 itself failed** (not sub-test 2 as a
prior reading suggested). The audit's original "no sprite-zero hit
fired when expected" reading is correct; the cascade is gated on a
real PPU sprite-zero-hit-detection bug.

**Verified facts** (read-only):

- `PPUMASK_COPY = 0x1E` at test entry — BG + SPR + BG_LEFT all set.
- `PPUSTATUS` bit 6 = `0` when result byte transitions — hit never
  fired.
- Tile `$FC` in pattern table 0 is fully opaque (all 8 rows
  `lo=0xFF / hi=0x00 = "11111111"`). Confirmed by extracting the
  CHR ROM at offset `0x10 + 32 KiB + 0xFC0`.
- `blargg/sprite_hit_tests/01-basics.nes` PASSES on our PPU.
  AccuracyCoin's `TEST_Sprite0Hit_Behavior` FAILS. Both should trigger
  sprite-zero hit; the diagnostic delta is between them.

**Implied bug location** — one of:

1. Sprite-eval FSM: Y=0 edge case (sprite-evaluation for scanline 1
   with sprite Y=0). The B8c FSM landed under a 1013-case equivalence
   harness; the dot-64-reset regression that broke SMB / Excitebike /
   Kid Icarus is fixed (commit `834be9e`). But the Y=0 case might
   have a separate timing issue.
2. `fetch_sprite_tile`: pattern data load. `row` clamp at line 1520
   (`(next_line.wrapping_sub(y)).clamp(0, sprite_height - 1)`) — for
   sprite Y=0 at scanline 0 → row = 0. Loads tile `$FC` row 0 lo/hi.
   Should give `spr_shift_lo = $FF`, `spr_shift_hi = $00`.
3. Pixel emission: spr_x decrement timing. Sprite X = 8 → 8 dots
   before sprite renders. At pixel_x = 8 (dot 9), `spr_x[0] = 0`,
   sprite pattern bits read at bit 7 of the shift register. If shift
   was loaded with `0xFF`, bit 7 = 1, `val = 1`, `spr_idx != 0`,
   `spr_zero_pixel = true`. Hit predicate met.
4. The sprite-zero hit predicate at line 1205-1212 — `pixel_x < 255`
   and `!(pixel_x < 8 && ...)`. At pixel_x = 8, both pass.

**Next-session investigation** (multi-day):

- Add per-scanline `(spr_zero_in_line, spr_count, spr_shift_lo[0],
  spr_x[0])` instrumentation to PPU under a feature flag and dump for
  scanlines 0..5 during the AccuracyCoin Sprite 0 Hit test window.
- Compare against a known-correct reference (Mesen2's per-scanline
  log on the same ROM).
- The fix MUST not regress the 78+ committed baselines (audio_tests +
  60 commercial ROMs + 21 permissive baselines) — Path A's revert of
  the Codex worktree PPU change established this floor.

The CHANGELOG entry for the v1.0.0 trajectory is updated alongside
this addendum.

## Addendum (2026-05-19): Cascade B closed

`fix(apu,bus): DMC DMA scheduler — close AccuracyCoin Cascade B`
(commit `9b0c81c`, Codex-authored on a feature worktree; the
`Path-A split-commit` audit handoff lives at
`ref-docs/2026-05-19-dmc-dma-cascade-b-handoff.md`). The DMC DMA
scheduler rework closes the full 8-test "APU Registers and DMA tests"
cluster the original audit identified (all 8 → pass; previously
2 pass / 8 fail). AccuracyCoin RAM-direct pass rate advances
**69.78% → 76.98%** (+11 tests flipped — 8 in the cascade + 3 net
across CPU/PPU surfaces as side-benefits; 102 pass + 5 pass_with_code
of 139 assigned tests).

**Trajectory update (replaces the "Realistic v1.0.0 trajectory"
estimate above)**:

- Cascade B (DMC DMA): **CLOSED** 2026-05-19. Pass rate 76.98%.
- Cascade A (Sprite Zero Hit, 16 tests gated): OPEN. The original
  Codex worktree carried a candidate PPU change (sprite-eval emit
  ordering + pre-render contribution removal) but it broke 78+
  committed baselines (19 audio_tests + 59 commercial ROMs) so was
  reverted under Path A. Cascade A requires a fix that does not
  regress those baselines; the audit-flagged FME-7 audio hash drift
  on the DMC fix is the only other follow-up.
- C1 IRQ-timing axis (5 tests: 4 × `cpu_interrupts_v2/{2..5}` +
  `mmc3_test_2/4` sub-test #3): OPEN. 11 prior rollback attempts.
- Internal-bus model (~5 tests, `Open Bus [error 9]` + SH* `[error 7]`):
  OPEN. Defer to v1.x per the v1.0.0-rc1 release notes.

If Cascade A closes cleanly without regressing the baselines:
**32 → ~16 failing tests, pass rate 76.98% → ~88%**. The v1.0.0
≥ 90% gate remains contingent on Cascade A + C1 IRQ-timing axis.
