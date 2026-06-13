# ADR 0007 — DMC DMA Get/Put Cycle Scheduler (Parallel-Impl Pattern)

**Status:** Superseded (2026-06-10, v2.0.1) — the R1 master-clock **unified DMA
engine** (`unified_dma_cycle`) replaced this model; the `dmc-get-put-scheduler`
feature flag and its `dmc_get_put_equivalence.rs` test were removed in v2.0.1
along with the rest of the legacy scheduler. Retained as a historical record.
**Date:** 2026-05-26
**Author:** RustyNES maintainers
**Relates to:**
- [ADR 0002 — IRQ Timing Coordination](0002-irq-timing-coordination.md) (the
  `cpu-implied-dummy-reads` companion feature that motivated this work)
- [ADR 0003 — Save-State Migration](0003-save-state-migration.md) (no version
  bump in v1.2; flag is default-off so the snapshot wire format is unchanged)
- The v2.0.0 release plan
  (`/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`)
  Sprint A (master-clock-precise scheduling refactor) which the get/put model
  is a foundation for.

---

## Context

### The cascade

v1.1.0 ships with two DMC-related AccuracyCoin residuals that were
**deferred to v1.2** in the post-release residuals plan:

* **`CPU Behavior 2 :: Implied Dummy Reads [error 3]`** — the cycle-2
  PC dummy read of implied/accumulator/transfer/flag opcodes (23
  opcodes total) must be emitted on the bus to satisfy the test's
  cycle-by-cycle behaviour check. Sprint 2.3 (v1.0.0-rc era) landed
  the dummy reads behind the `cpu-implied-dummy-reads` cargo feature
  flag, default-off.

* **`APU Registers and DMA tests :: Implicit DMA Abort`** — the
  cascade-sentinel. With `cpu-implied-dummy-reads` ON, this previously
  passing test flips to FAIL. Six independent single-axis recalibrations
  of the four compensating delays (`dmc_dma_short`, `dmc_dma_cooldown`,
  `dmc_abort_delay_for`, `dmc_dma_delay`) were rolled back across
  Session-19 → Session-21, then a longer hiatus, then the Sprint 2.3
  Step 3 iter 1+2 audit in 2026-05-25 confirmed empirically that
  **single-axis recalibration is insufficient**.

### The structural finding

`docs/audit/path-beta-paired-oracle-captured-2026-05-25.md` documents the
root cause via paired per-CPU-cycle traces captured against Mesen2
(canonical) and RustyNES (baseline + with implied-dummy-reads):

* **Mesen2's `NesCpu.cpp::RunDma` (lines 399-447)** uses a unified
  get/put cycle alternation loop keyed on `(_state.CycleCount & 0x01)`,
  with three independent state flags (`_dmcDmaRunning`, `_needHalt`,
  `_needDummyRead`) consumed across multiple cycle pairs. The
  `processCycle` lambda (lines 384-397) clears one flag per cycle in
  the order: `_needHalt` → `_needDummyRead`. When both are clear, the
  next GET cycle delivers the DMC byte.

* **RustyNES's `service_dmc_dma`** (pre-Sprint-3.2) used a
  phase-agnostic loop: `noop_cycles = 2` (load DMA, `short=true`) or
  `noop_cycles = 3` (reload DMA, `short=false`), followed by a single
  deliver tick. The four compensating delays (`dmc_dma_short`,
  `dmc_dma_cooldown`, `dmc_abort_delay_for`, `dmc_dma_delay`) were
  carefully tuned to make this phase-agnostic model approximate the
  canonical get/put behaviour for the bus-cycle patterns seen under
  RustyNES's pre-`cpu-implied-dummy-reads` baseline. They were NOT
  tuned for the bus-cycle patterns produced when `cpu-implied-dummy-
  reads` is ON.

* **Within-RustyNES cycle-precise diff** (baseline vs implied-dummy-
  reads-ON) shows **435 divergent same-cycle events**. Every one is
  Idle bus → `R $FF4D=$EA` (the cycle-2 PC dummy read of an implied
  opcode). The DMC sample-fetch lands one cycle earlier under
  implied-dummy-reads, cascading through subsequent test iterations.

**No delay-tweak can match Mesen2 because the scheduler model is
structurally different.** This was empirically proven by six prior
rollback attempts.

### Strategy: parallel-implementation pattern

Per the user-selected option (c) from the v1.2 path analysis (see the
session transcript that scopes this ADR), the get/put refactor lands
behind a feature flag with a parallel-implementation equivalence
harness. This preserves the v1.1.0 baseline bit-identically on the
default-off path while making the new model available for iteration.

**Promotion strategy:**
- **v1.2.0** (this ADR): land the get/put model default-off; ship the
  equivalence harness; deliver concrete convergence metrics (Sprint 3.4
  closes 6 of 10 AccuracyCoin DMA-cluster tests).
- **v1.6.0 (fallback)**: if v2.0 Sprint A (master-clock refactor) slips
  past 6 months, promote `dmc-get-put-scheduler` to default-on in a
  focused accuracy patch, accepting the one-time 60-ROM commercial-
  oracle re-baseline.
- **v2.0.0 Sprint A**: the get/put alternation emerges naturally from
  master-clock-precise scheduling (`(master_clock / 6) & 1` is the
  get-cycle parity at 12 master clocks per CPU cycle, NTSC). At v2.0
  the feature flag is removed; the unified master-clock + get/put
  model becomes the only path.

---

## Decision

**Land the get/put cycle alternation DMC scheduler behind the
`dmc-get-put-scheduler` cargo feature flag (default-off).**

The flag propagates through three crates:
- `rustynes-apu/dmc-get-put-scheduler` — APU-side state for the new flags
  (`Apu::dmc_need_halt`, `Apu::dmc_need_dummy_read`); always-present
  fields (not `#[cfg]`-gated) so the snapshot wire format is forward-
  compatible with a future default-on flip.
- `rustynes-core/dmc-get-put-scheduler = ["rustynes-apu/dmc-get-put-scheduler"]`
  — bus-side scheduler implementation (`#[cfg]`-gated; the OLD
  scheduler lives in the `#[cfg(not(feature = ...))]` branch).
- `rustynes-test-harness/dmc-get-put-scheduler = ["rustynes-core/..."]` — for the
  parallel-implementation equivalence harness.

### Get/put scheduler shape

The new `rustynes-core::bus::service_dmc_dma` (under the feature):

```rust
loop {
    // RustyNES DMC-side parity: `(cycle & 1) == 0` is the GET half.
    // This is the INVERSE of the OAM-DMA parity convention in
    // `drain_dma` (where `cycle & 1 == 1` is "no alignment" / get-
    // aligned) — see the "Why DMC parity is inverse to OAM parity"
    // section in the source doc-comment.
    let get_cycle = (self.cycle & 1) == 0;
    let need_halt = self.apu.dmc_need_halt();
    let need_dummy = self.apu.dmc_need_dummy_read();

    if get_cycle && !need_halt && !need_dummy {
        // DELIVER cycle — the canonical DMC sample fetch.
        // ... read sample, tick, complete_dmc_dma{,_before_get_tick}
        return;
    }

    // Dummy / halt / alignment cycle.
    self.replay_dma_noop_read(halted_addr);

    // processCycle: clear ONE flag per cycle in Mesen2 order.
    if need_halt {
        self.apu.clear_dmc_need_halt();
    } else if need_dummy {
        self.apu.clear_dmc_need_dummy_read();
    }
    self.tick_one_cpu_cycle();
}
```

The OAM-conflict path (`service_dmc_dma_during_oam`) follows the same
unified shape, with `clock_oam_dma_cycle` substituted for the generic
dummy in cycles where OAM has work pending. Both paths are
feature-gated; the OLD compensating-delay loops are preserved as the
`#[cfg(not(...))]` branch for default-off behaviour.

### Parallel-impl equivalence harness

Two artifacts in Sprint 3.3:

- **`crates/rustynes-test-harness/tests/dmc_get_put_equivalence.rs`** —
  Rust CI gate, gated on
  `#![cfg(all(feature = "test-roms", feature = "dmc-get-put-scheduler"))]`.
  Two tests:
  - `accuracycoin_dma_cluster_progress_signal`: diagnostic, no
    assertions, prints per-DMA-test match count (the iteration
    metric Sprint 3.4 watches climb 1 → 10).
  - `accuracycoin_dma_cluster_matches_baseline_under_get_put`:
    strict byte-for-byte equivalence assertion, `#[ignore]`'d per
    the project's `*_currently_fails` pattern. Run via
    `cargo test ... -- --ignored` for actionable per-test diff.

- **`scripts/dmc_equivalence_harness.sh`** — deep-iteration shell
  tool. Builds `trace_dmc_dma` twice (feature off + on), runs both
  against a ROM corpus (minimal / sub-tests / full), diffs the
  per-CPU-cycle traces via `scripts/dmc_dma_within_rusty_diff.py`,
  reports per-cycle-divergence counts.

### Convergence trajectory (Sprint 3.4)

| Iter | Change | DMA cluster matching baseline |
|---|---|---:|
| 3.2 landing | Initial get/put with OAM-DMA parity convention | **1/10** |
| 3.4 iter 1 | Flipped parity to `(cycle & 1) == 0 = get` for DMC | **6/10** |
| 3.4 iter 2 | Ported `service_dmc_dma_during_oam` to get/put | **6/10** (structural, no new test flips) |

The 4 remaining failures (`DMA + $4015 Read`, `DMC DMA + OAM DMA`,
`Explicit DMA Abort`, `Implicit DMA Abort`) all involve either the
DMC abort path (`service_dmc_abort`, NOT yet ported to get/put) or
the `$4015` open-bus interaction during DMC service. **Closing them
is Sprint 3.4 iter 3+** (v1.2.x patch series) OR absorbed naturally
into v2.0 Sprint A's master-clock refactor.

---

## Consequences

### Default-off (current `main` behaviour)

- **Bit-identical to v1.1.0**: same 599 strict tests + 6 ignored across
  48 workspace test suites; same `dump_battery_ram` byte values
  across the AccuracyCoin DMA cluster.
- 60-ROM commercial-ROM oracle: 60/60 preserved.
- Sacred trio (SMB / Excitebike / Kid Icarus PAL): preserved.
- B4 invariant (first MMC3 IRQ at cycle 1,370,110 / scanline 0 /
  dot 257): preserved.

### Default-on (under `--features dmc-get-put-scheduler`)

- AccuracyCoin DMA cluster: **6/10 match baseline** (was 1/10 at
  Sprint 3.2 landing).
- Workspace tests: 600 strict + 7 ignored (+1 from
  `accuracycoin_dma_cluster_progress_signal` diagnostic, +1 from the
  `#[ignore]`'d strict equivalence probe). The new file gates on
  `cfg(all(feature = "test-roms", feature = "dmc-get-put-scheduler"))`,
  so it doesn't affect any other build.
- `cpu-implied-dummy-reads` cascade closure NOT yet validated under
  the new scheduler — the cascade probe ROM (`Implicit DMA Abort`)
  is in the 4-failure cluster that still regresses.
- Save-state format: unchanged in v1.2 (the new APU fields are
  always-present but currently 0/false-initialized; future
  promotion adds them to a new tagged save-state section, version
  bump deferred to v1.6 or v2.0).

### Forward-compatibility

- The APU fields `dmc_need_halt` and `dmc_need_dummy_read` are
  **always-present** (not `#[cfg]`-gated). Reason: serializing them
  unconditionally lets a future flag-flip migration restore them
  from a v1.2-era save-state without compatibility hacks.
- The `clear_dmc_need_halt()` / `clear_dmc_need_dummy_read()`
  mutators are always-present in the public API too. Under the
  default-off path they're no-ops semantically (the old scheduler
  doesn't call them); under the new path the bus calls them once
  per processed cycle.

### Carry-cost

- Two code paths now exist for `service_dmc_dma` +
  `service_dmc_dma_during_oam`. A future maintainer touching DMC
  logic must update both. The doc-comment on each
  `#[cfg]`-gated function points to this ADR and the convergence
  audit doc for context.
- The OAM-conflict path under the new model has identical
  test-outcome to the OLD path at v1.2 — landing it was a
  structural-cleanup decision, not a per-test-flip decision.
  Justification: future iterations on the DMC abort path
  (`service_dmc_abort`) will benefit from having BOTH service paths
  already on the same model.

### Iteration cost (for Sprint 3.4 iter 3+)

The equivalence harness is the iteration vehicle:

```text
edit crates/rustynes-core/src/bus.rs::service_dmc_dma{,_during_oam}
cargo test --features test-roms,dmc-get-put-scheduler \
    --test dmc_get_put_equivalence -- --ignored
# watch matching count climb 6 → 10
```

When the Rust gate reports a specific failing DMA test, run the shell
harness for cycle-level traces:

```text
scripts/dmc_equivalence_harness.sh --keep-traces sub-tests
```

The `dmc_dma_within_rusty_diff.py` output then identifies the EXACT
cycles where the two schedulers diverge.

---

## Alternatives considered

### (a) v1.2.0 standalone refactor

Land the get/put refactor as the default model in v1.2, paying for
the 60-ROM commercial-oracle re-baseline at v1.2 ship. Pro: closes
Sprint 2.3 Step 3 cleanly. Con: **double oracle re-baseline** —
v1.2's audio-FNV-1a regen, then v2.0's framebuffer + audio + IRQ
trace regen. The audit work paper trail doubles too.

### (b) v2.0.0 only

Defer the get/put model entirely until v2.0 Sprint A's master-clock
refactor. Pro: single oracle re-baseline, architecturally clean
(get/put alternation emerges naturally from master-clock scheduling).
Con: AccuracyCoin stuck at 90.65% for ~6 months (v1.2-v1.6); Sprint
2.3 Step 3 stays open; **concentration risk** if v2.0 Sprint A stalls
or slips.

### **(c) Hybrid — selected.** Parallel-impl pattern in v1.2; promotion at v1.6 (fallback) or v2.0 (planned).

Pro: zero oracle re-baseline at v1.2 (default-off); forward progress
visible (Sprint 3.1-3.4 ship real refactor + harness); risk-mitigation
via v1.6 fallback if v2.0 stalls; engineering reuse ~80% into v2.0
(the get/put loop is preserved; the master-clock change replaces the
parity source). Con: extra cargo feature in the build matrix for the
v1.x lifetime; two ADRs eventually (this one + a future promotion
ADR); some abstraction churn at the v2.0 absorption point.

### (d) Compensating-delay multi-axis recalibration

Try harder at the original Sprint 2.3 approach: recalibrate all four
delays simultaneously via multi-axis search. Rejected: empirically
proven insufficient by six prior rollbacks (Session-19 + Session-20 +
Sprint 2.3 Step 3 iter 1+2). The model is structurally wrong, not
mis-calibrated.

---

## Cross-references

* **Audit thread** (the empirical basis for this ADR):
  - `docs/audit/path-beta-dmc-trace-tooling-2026-05-25.md`
  - `docs/audit/path-beta-paired-trace-2026-05-25.md`
  - `docs/audit/path-beta-paired-oracle-captured-2026-05-25.md`
  - `docs/audit/sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
* **Commit chain** (v1.2 Sprint 3.1 → 3.4 iter 2):
  - Sprint 3.1: `feat(apu): scaffold dmc-get-put-scheduler feature flag + API`
  - Sprint 3.2: `feat(bus): get/put DMC scheduler initial landing — Sprint 3.2 (WIP)`
  - Sprint 3.3: `feat(test-harness): DMC get/put parallel-impl equivalence harness`
  - Sprint 3.4 iter 1: `feat(bus): Sprint 3.4 iter 1 — DMC parity convention fix (1/10 -> 6/10)`
  - Sprint 3.4 iter 2: `feat(bus): Sprint 3.4 iter 2 — service_dmc_dma_during_oam under get/put`
* **Mesen2 reference** (GPL-3.0, structural only — no verbatim code):
  - `Core/NES/NesCpu.cpp::RunDma` (lines 399-447) — unified loop
  - `Core/NES/NesCpu.cpp::RunDma`'s `processCycle` lambda (lines 384-397)
  - `Core/NES/NesCpu.cpp::StartDmcTransfer` (line 527) — flag arming
  - `Core/NES/NesCpu.h:41` — `_needHalt`/`_needDummyRead` field declaration
* **Source files (this repo)**:
  - `crates/rustynes-apu/src/apu.rs` — `Apu::dmc_need_halt`,
    `Apu::dmc_need_dummy_read` + flag arming/clearing wiring
  - `crates/rustynes-core/src/bus.rs::service_dmc_dma` (base path,
    `#[cfg]`-gated)
  - `crates/rustynes-core/src/bus.rs::service_dmc_dma_during_oam`
    (OAM-conflict path, `#[cfg]`-gated)
  - `crates/rustynes-test-harness/tests/dmc_get_put_equivalence.rs`
  - `scripts/dmc_equivalence_harness.sh`
* **External reference**:
  - nesdev wiki §DMA — get/put cycle terminology and alignment rules
  - nesdev wiki §APU DMC — DMC fetch cycle accounting
