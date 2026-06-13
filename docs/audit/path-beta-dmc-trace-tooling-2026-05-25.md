# Path β — DMC DMA trace-tooling foundation (Sprint 2.3 Step 3 prereq)

**Date:** 2026-05-25
**Path:** β — trace-tooling-first foundation work for Sprint 2.3 Step 3
(Implicit DMA Abort closure). Prerequisite for the multi-axis
recalibration of the four DMC compensating delays that Session-20
identified as load-bearing for closing the
`APU Registers and DMA tests :: Implicit DMA Abort` cascade.
**Outcome:** Three new artifacts landed, ready for paired RustyNES +
Mesen2 trace generation on the cascade-sentinel test. **No
production code changed; no AccuracyCoin / commercial-oracle /
sacred-trio invariants touched.**

---

## What landed

### 1. `crates/nes-test-harness/src/bin/trace_dmc_dma.rs` (NEW)

Per-CPU-cycle DMC-DMA scheduler trace binary. Modeled on the existing
`trace_apu_reg_activation.rs` template (Session-26). Reuses the
`irq-timing-trace` cargo feature and the `Nes::bus_mut().
enable_irq_trace(N)` / `take_irq_trace()` API; emits a focused CSV
filtered to DMC-relevant events:

* DMC DMA pending (pre or post sub-dot snapshot)
* `$4010-$4017` register reads/writes
* OAM-DMA + DMC-DMA bus fetches (`BusAccess::DmaRead` / `DmaWrite`)
* APU IRQ status transitions

CSV columns (16): `cpu_cycle, ppu_frame, ppu_scanline, ppu_dot,
m2_phase, access, bus_addr, bus_data, dmc_pending_pre,
dmc_pending_post, dmc_dma_short, dmc_abort_pending,
dmc_abort_delay, dmc_cooldown, mapper_irq_low, apu_irq_low`.

**Smoke-test** against `apu-reg-activation.nes` (80 frames):
captures 2.35M cycles, filters to 1778 interesting rows (905 DMA
reads, 768 DMA writes, 63 register reads, 38 register writes,
4 idle cycles) — efficient ~0.075% filter ratio.

Feature gating: the inner module is `#[cfg(feature = "irq-timing-
trace")]`; the default-feature build emits an error stub. Both
`cargo check -p nes-test-harness` (default) and `cargo check
-p nes-test-harness --features irq-timing-trace` pass cleanly.

### 2. `scripts/mesen2_dmc_dma_trace.lua` (NEW)

Mesen2 Lua counterpart. Modeled on the existing
`mesen2_apu_reg_activation_trace.lua` (Session-26) but pinned to the
DMC scheduler surface:

* Memory callbacks on `$4010-$4013` writes, `$4015` R/W
* On each callback: snapshot `apu.dmc.{bytesRemaining,sampleAddr,
  irqFlag,irqEnabled,silenceFlag}` from `emu.getState()`
* Infer `dmc_get` events (DMC DMA fetch) from `bytesRemaining`
  decrements between snapshots (Mesen2 does not expose
  `dmaPending` directly via Lua, but every fetch decrements
  `bytesRemaining` by 1)
* Emit per-frame heartbeat rows for cross-diff alignment

CSV columns (13): `cpu_cycle, ppu_frame, ppu_scanline, ppu_dot,
m2_phase, kind, addr, value, dmc_bytes_rem, dmc_sample_addr,
dmc_irq_flag, dmc_irq_en, dmc_silence`.

`kind` enum: `R / W / dmc_get / dmc_irq_set / dmc_irq_clr /
dmc_en_set / dmc_en_clr / frame`.

Env vars: `MESEN2_DMC_TRACE_OUT`, `MESEN2_DMC_TRACE_MAX_FRAMES`,
`MESEN2_DMC_TRACE_RESULT_ADDR`, `MESEN2_DMC_TRACE_DMC_EVENTS`.

### 3. `scripts/dmc_dma_trace_cross_diff.py` (NEW)

Cross-diff tool aligning the two CSVs at the first `$4015` write
(canonical DMC enable bootstrap) and walking forward emitting:

* **ALIGNMENT** block — first `$4015` W on each side + cycle offset
* **SUMMARY** block — fetch / `$4015` / IRQ counts on each side +
  first-event deltas
* **DMC FETCHES** walk — paired fetch events with cycle Δ (positive
  = Mesen2 later, negative = Mesen2 earlier); `ok`/`DIV` markers at
  the ±tolerance boundary (default ±2 cycles)
* **$4015 R/W** walk — paired register accesses with value
  agreement (`VAL` marker on bus-data mismatch)
* **DIVERGENCE** block — diverged-fetch count, largest signed Δ,
  value-mismatch count

**Interpretation guide** (printed at end of report):

* Positive Δ on DMC fetches ⇒ decrease `dmc_dma_cooldown` or shrink
  `dmc_abort_delay`
* Negative Δ on DMC fetches ⇒ increase those
* `$4015` value mismatch ⇒ `dmc_dma_pending` visibility axis

**Smoke-tested** against synthetic RustyNES + Mesen2 CSVs with a
deliberate +30-cycle Mesen2 offset: alignment correctly recovers
the +30 offset, walks both fetches and `$4015` accesses, reports
+2 cycle Δ on the first fetch as expected (the synthetic was
constructed so post-alignment fetches sit at +2 cycles).

---

## What this is NOT

This is **trace tooling only**. No production code (no
`crates/nes-cpu/src/cpu.rs`, no `crates/nes-apu/src/apu.rs`, no
`crates/nes-core/src/bus.rs`) was modified.

* AccuracyCoin pass rate: **90.65% (126/139) preserved**
  (unchanged from v1.1.0)
* 60-ROM commercial-ROM oracle: **60/60 preserved**
* Sacred trio (SMB / Excitebike / Kid Icarus PAL): **preserved**
* B4 invariant (first MMC3 IRQ cycle 1,370,110): **preserved**
* All 10 v1.1.0 validation gates: **green**

The actual multi-axis recalibration of the four DMC compensating
delays — `dmc_dma_short`, `dmc_dma_cooldown`, `dmc_abort_delay_for`,
`dmc_dma_pending`/`in_dmc_dma` — is the next session's work, gated
on capturing a Mesen2 trace on a real Mesen2 install (the local
machine has no Mesen2 runtime yet).

---

## What the next session needs

1. **Mesen2 install** with Lua-scripting build (community CI builds
   work; the AppImage path `~/AppImages/mesen.appimage` is the
   project convention)
2. **Run** the Lua script against the same sub-test ROM (target:
   `apu-reg-activation.nes` first — has an existing oracle baseline
   from Session-26 — then move to a DMC-pinned harness once the
   tooling is validated end-to-end)
3. **Generate** matched RustyNES + Mesen2 CSVs and run the cross-
   diff to identify the per-cycle divergence pattern
4. **Recalibrate** the four compensating delays as a coordinated
   change (NOT single-axis guess-and-check — that was empirically
   proven insufficient by Sprint 2.3 Step 3 iter 1+2)
5. **Validate** under `--features cpu-implied-dummy-reads,
   test-roms` that the `Implicit DMA Abort` cascade sentinel flips
   PASS while preserving the 13-failure baseline on the rest of
   AccuracyCoin (no cascade in the inverse direction)
6. **Validate** the full 10-gate gauntlet before committing the
   recalibration

---

## Why trace-tooling-first matters

Sprint 2.3 Step 3 iter 1+2 (this session's predecessor audit at
`docs/audit/sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`)
attempted single-axis recalibration (`dmc_dma_cooldown ±1`) by
guess-and-check and produced **no improvement** in either
direction. Session-20's diagnosis named four interlocking delays;
no single-axis tweak can close the cascade.

A cycle-precise oracle is the **only viable next step**. RustyNES's
`irq_trace` infrastructure already exposes all four delays at every
cycle of interest — the bottleneck was the missing Mesen2 reference
trace. This session's three artifacts close that gap, so the next
session can begin with concrete cycle-by-cycle deltas rather than
hypotheses.

Lesson generalization: the same trace-tooling-first pattern closed
the C1 axis empirical finding (Session-29) and is the v2.0 Sprint A
master-clock refactor's foundation. Per the v2.0.0 plan
(`/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`),
trace-driven multi-axis recalibration is the prerequisite for every
axis closure in v1.2 and v1.6.

---

## Cross-references

* Sprint 2.3 Step 3 iter 1+2 audit:
  `sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
* Sprint 2.3 recon: `sprint-2.3-implied-dummy-dmc-recon-2026-05-25.md`
* Path D foundation audit: `path-d-foundation-work-2026-05-25.md`
* Session-20 (Sprint 1 DMC abort investigation, naming Finding 3
  on the four compensating delays):
  `session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
* Session-26 (apu-reg-activation oracle that this work models on):
  `session-26-sprint2-iter4-apu-reg-activation-2026-05-23.md`
* Trace template (Rust):
  `crates/nes-test-harness/src/bin/trace_apu_reg_activation.rs`
* Trace template (Lua):
  `scripts/mesen2_apu_reg_activation_trace.lua`
* CycleRecord DMC fields (already present pre-this-session, no
  change needed): `crates/nes-core/src/irq_trace.rs` lines 182-224
* Existing comprehensive IRQ cross-diff (alternative tool for the
  general IRQ surface): `scripts/irq_trace_cross_diff.py` with
  `--dmc` flag
* DMC scheduler code (the target of the next session's
  recalibration): `crates/nes-apu/src/apu.rs` lines 343 (post-load
  cooldown=4), 377 (post-get-tick cooldown=5), 103-108 (abort delay
  table), 549-561 (dma delay calibration)
* v2.0.0 release plan: `/home/parobek/.claude/plans/generate-a-new-
  plan-snug-starlight.md` — Sprint 2.3 listed as a v1.2 milestone

---

## Workspace state at end of this session

* Tests: 537 strict pass + 5 ignored across 34 suites with
  `--features test-roms`. **PRESERVED** (no test change).
* AccuracyCoin: **90.65% (126/139) PRESERVED**
* 60-ROM commercial-ROM oracle: **60/60 PRESERVED**
* Sacred trio: **PRESERVED**
* B4 invariant: **PRESERVED**
* New artifacts:
  - `crates/nes-test-harness/src/bin/trace_dmc_dma.rs`
    (215 LoC; auto-discovered by cargo; feature-gated)
  - `scripts/mesen2_dmc_dma_trace.lua` (162 LoC)
  - `scripts/dmc_dma_trace_cross_diff.py` (270 LoC, +x)
  - this audit doc
