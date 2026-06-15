# Phase 6 — v1.0.0 Closeout (SUPERSEDED)

> **SUPERSEDED (2026-05-25).** v1.0.0 was released 2026-05-23 at AccuracyCoin
> 90.65%; the 90% gate was cleared via the Phase 1a/b/d + 3a/b closures (see
> `docs/STATUS.md`), not the sprint plan below. T-60-001 (C1 IRQ-timing) and
> the sub-cycle residuals are deferred to the **v2.0 master-clock refactor**
> (`docs/audit/gap-analysis-remediation-plan-2026-05-25.md`). T-60-004
> (multi-OS release smoke) is the only still-open item. T-60-003 closed;
> T-60-002/005 done. The content below is retained for historical provenance.

**Goal:** close all open v1.0.0 gates and ship the v1.0.0 tag.

**Exit criterion:** `cargo test --workspace --features test-roms` shows
**513 strict pass** (510 + the 3 remaining `cpu_interrupts_v2/{2,3,5}`
flipped) + `mmc3_test_2/4-scanline_timing` sub-test #3 flipped +
AccuracyCoin ≥ 90% + multi-OS release-artifact smoke test green + the
6 `#[ignore]`'d commercial ROMs investigated (fix flipped or moved to
"known-defer-to-v1.x" with public-facing rationale).

**Status as of session 2 (2026-05-17):**

| Ticket | Status | Notes |
|--------|--------|-------|
| T-60-001 — C1 IRQ-timing rework | OPEN | 10 prior axis rollbacks; needs canonical `T_last - 1` breakthrough |
| T-60-002 — AccuracyCoin ≥ 90% | OPEN at 69.78% | 42 failing tests; cascade-structured in audit doc |
| T-60-003 — 6 ignored commercial ROMs | **CLOSED** ✓ | Session 1 — all 6 strict-passing |
| T-60-004 — Multi-OS smoke test | OPEN | User-driven only; not closeable in-CLI |
| T-60-005 — `v1.0.0` tag + release notes | BLOCKED | Gated on T-60-001 + T-60-002 + T-60-004 |

---

## Next-session execution plan

### Step 1 — Cascade B (DMC DMA cycle precision) — 8 tests gated

**Hypothesis:** the AccuracyCoin "APU Registers and DMA tests" sub-tests
check that the DMC DMA halt + alignment cycles re-issue the original
CPU read address on the bus with cycle-precise timing. Our current
`Bus::service_dmc_dma` (`crates/rustynes-core/src/bus.rs:1013`) uses a fixed
4-cycle stall with replay at halt cycles 1 + 2 — too coarse.

**Implementation pointer:**

- `crates/rustynes-core/src/bus.rs:1013-1040` `service_dmc_dma`
- nesdev reference: `https://www.nesdev.org/wiki/DMA` (canonical halt
  - alignment behavior)

**Proposed change:**

1. Replace fixed 4-cycle stall with **variable 1-4 halt + 0-2 alignment
   - 1 read + 1 dummy** per nesdev's `DMA` page.
2. Trigger replay on EVERY halt + alignment cycle (currently only at 1
   - 2), so each cycle re-issues the last CPU read.
3. The trigger of DMC DMA detection moves into APU-clock-aligned
   sampling (the M2-phase boundary plumbed in session 1's Phase B1).

**Validation gates (must all stay green before commit):**

- `cargo test --workspace --features test-roms` (510+ strict).
- `cargo test --workspace dmc_dma_during_read4 -- --nocapture` (the
  canonical regression guard for DMC DMA timing).
- `cargo test commercial-roms` (60 + 0 strict).
- AccuracyCoin RAM pass-rate must not regress below 0.60 (current 0.78
  ratio in framebuffer terms).

**Expected outcome:** if successful, flips 6-8 AccuracyCoin tests
(`DMA + $2002 Read`, `DMA + $2007 Read`, `DMA + $2007 Write`,
`DMA + $4015 Read`, `DMA + $4016 Read`, `DMC DMA Bus Conflicts`, plus
potentially `Explicit / Implicit DMA Abort`). Pass rate moves
69.78% → ~75-77%.

**Risk profile:** medium. DMC DMA timing is load-bearing for
DMC-using games (Castlevania 3, Mega Man 3-6, etc.). The
`dmc_dma_during_read4` test ROM is the regression sentinel. The
`apu_test/*` ROMs may also be affected; check them in the validation
gate.

### Step 2 — Cascade A (Sprite Zero Hit cycle precision) — 16 tests gated

**Hypothesis:** the AccuracyCoin `Sprite 0 Hit behavior` Test 1 fails
not because our sprite-eval FSM is wrong (it passes the 1013-case
equivalence harness) but because there's a cycle-precision issue
specifically around the AccuracyCoin test's setup pattern:

1. Background tile at nametable `$2001` (column 1, row 0).
2. Sprite zero at (X=8, Y=0, CHR=$FC, attribute 0).
3. EnableRendering_S followed by OAM DMA in VBlank.
4. Wait ~3000 CPU cycles for a full frame.
5. Read `$2002`, expect bit 6 (SPRITE_ZERO_HIT) to be set.

**Implementation pointers:**

- `crates/rustynes-ppu/src/ppu.rs:1188` — sprite-zero detection gate
  (`if i == 0 && self.spr_zero_in_line`).
- `crates/rustynes-ppu/src/ppu.rs:1204-1212` — actual `SPRITE_ZERO_HIT`
  bit set (guarded by `pixel_x < 255` + 8-pixel-mask predicate).
- `crates/rustynes-ppu/src/ppu.rs:1349` — FSM commit of
  `spr_zero_in_line` at dot 256.

**Investigation steps (before any code change):**

1. Instrument the test. Add a debug print path in
   `crates/rustynes-test-harness/tests/accuracycoin.rs` that captures the
   `$2002` byte and key PPU state (`spr_count`, `spr_zero_in_line`,
   `spr_x[0]`, `bg_shift_lo`) at scanline 1 dot 8-15 during the test
   ROM's Test 1.
2. Compare with Mesen2 (the reference implementation) by running the
   same test ROM in both and capturing the same state.
3. The divergence point reveals the specific cycle-precision bug.

**Risk profile:** medium-high. Changes to sprite-zero or BG-shifter
timing risk regression on the recently-recovered B8 FSM
SMB / Excitebike / Kid Icarus state. Bisect tooling at
`scripts/regression-bisect/` is the safety net.

**Expected outcome:** if cascade A closes cleanly, flips 16 tests
(9 Sprite Evaluation + 7 PPU Misc sprite-zero-gated). Pass rate
~75% → ~87%.

### Step 3 — C1 IRQ-timing rework — 4 strict residuals + 3 AccuracyCoin tests

**Status:** 10 prior independent fix attempts rolled back across
multiple sessions. The infrastructure is landed:

- `M2Phase::Low / High` enum (`crates/rustynes-cpu/src/scheduler.rs`).
- `LockstepBus::current_m2_phase()` accessor.
- `Bus::poll_irq_at_phase(M2Phase)` trait method.
- Two-phase IRQ trace fixture (gated on `irq-timing-trace` feature).
- 6 golden baseline traces at `crates/rustynes-test-harness/golden/irq_trace/`.

**The remaining structural fix:** the canonical CPU `T_last - 1`
IRQ-sample-point rework on the `cpu_interrupts_v2` axis. The MMC3
Sharp/NEC reload-pending discriminator (Phase B4) closed sub-test #2
of `mmc3_test_2/4`; sub-test #3 + 3 `cpu_interrupts_v2/*` tests remain.

**Implementation pointer:**

- `crates/rustynes-cpu/src/cpu.rs` — IRQ-sample logic (`irq_first_tick`,
  `idle_tick`, `service_interrupt`).
- ADR-0002 `Decision (revised)` section for the proposed coordinated
  change.

**Risk profile:** very high. This axis has 10 documented rollbacks.
A successful fix requires either (a) an empirical breakthrough on
the per-cycle IRQ-sample-point discriminator, or (b) modeling the
2A03 silicon's specific sub-cycle latches more precisely than the
M2-phase boundary captures.

**Recommendation:** treat as multi-week investigation. Defer to a
dedicated branch (e.g., `c1-irq-axis-attempt-11`) with the
`accuracy-stabilization` recovery pattern (rollback the whole branch
if it regresses real-game smoke).

### Step 4 — Multi-OS release-artifact smoke test (T-60-004)

**Not closeable in CLI by Claude.** Requires the user to:

1. Run the GitHub Actions release workflow with a manual trigger
   (`.github/workflows/release.yml`).
2. Download the produced artifacts for Linux, macOS, and Windows.
3. Smoke-test each on a representative ROM (e.g., nestest.nes).
4. Confirm crash-free playback for ~60 seconds.

### Step 5 — `v1.0.0` tag (T-60-005)

Gated on steps 1-4. The tag bump requires:

1. `cargo workspaces version` (or manual `Cargo.toml` edit) from
   `0.9.0` → `1.0.0`.
2. CHANGELOG.md `[Unreleased]` → `[1.0.0] - YYYY-MM-DD` rename.
3. README.md version badge update.
4. `docs/STATUS.md` version-policy section update.
5. `git tag -s v1.0.0` (or `git tag v1.0.0` if no signing).
6. `git push origin v1.0.0`.

---

## What CAN be done in a single CLI session

If the goal is to make the v1.0.0 work *progressively safer*, the
single-session options are:

1. **Incremental silicon-correctness improvements that don't flip
   AccuracyCoin tests but are nesdev-spec-aligned and produce no
   regression.** Session 2 lands `feat(bus): DMC DMA halt-cycle
   replay for $2002 / $4016 / $4017` as an example. These are
   defensible "the code is more correct than before, even if no
   specific test moves" commits.

2. **Audit doc additions** that structure the remaining work so
   future sessions can pick it up. Session 2 lands
   `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`.

3. **Instrumentation additions** (gated on cargo features so CI
   stays fast). Adding per-cycle PPU trace output for the
   sprite-zero hit test would help close Cascade A.

What CANNOT be done in a single CLI session:

- Multi-OS smoke (T-60-004).
- C1 IRQ-timing breakthrough (10 prior rollbacks; structural).
- 20-percentage-point AccuracyCoin push (42 cycle-precise fixes).

---

## Cross-references

- [`docs/audit/accuracycoin-readme-analysis-2026-05-17.md`](../../docs/audit/accuracycoin-readme-analysis-2026-05-17.md)
  — README cascade-diagnostic analysis (session 2).
- [`docs/audit/v1-closeout-progress-2026-05-17.md`](../../docs/audit/v1-closeout-progress-2026-05-17.md)
  — Session 1 progress audit.
- [`docs/adr/0002-irq-timing-coordination.md`](../../docs/adr/0002-irq-timing-coordination.md)
  — C1 IRQ-timing ADR (with Decision (revised) section + attempt
  differentiation).
- [`docs/STATUS.md`](../../docs/STATUS.md) — single source of truth
  for per-suite pass count, mapper coverage matrix, feature-flag
  state, version policy.
- `CHANGELOG.md` `[Unreleased]` → "Phase 6 v1.0.0 closeout — session
  1/2" subsections.

---

## Sprints (informal — formalize when each phase begins)

- **Sprint 6-1 — Cascade B + Cascade A** (Steps 1 + 2 above). Target:
  AccuracyCoin → ~87%.
- **Sprint 6-2 — C1 IRQ-timing rework** (Step 3). Target:
  `cpu_interrupts_v2` 3 residuals flipped + `mmc3_test_2/4` sub-test
  #3 flipped + AccuracyCoin → ≥ 90%.
- **Sprint 6-3 — Multi-OS smoke + v1.0.0 tag** (Steps 4 + 5).
  User-driven release validation.
