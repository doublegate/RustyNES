# DMC-RDY-sub-cycle rewrite — canonical DMA halt/arming model

> Supersedes the prior C1/master-clock plan (BG Serial In was REFUTED as a
> master-clock issue this session; see `docs/audit/v2.0-c1-phase0-writedot-oracle-2026-06-01.md`).
> The remaining high-yield axis is the DMA internal-bus cluster, which converges on
> the DMC-arming blocker. Substance of prior plans preserved in git + memory.

## Context

The AccuracyCoin DMA/bus cluster (~12 tests: DMA + {Open Bus, $2002, $2007R/W, $4015,
$4016}, DMC Bus Conflicts, DMC+OAM, Explicit/Implicit Abort, $2007 Stress, Internal Data
Bus) PASSES on `main` (legacy lockstep `drain_dma`, 13 fail total) and REGRESSED to ~29 fail
on the R1 master-clock branch. This session traced the root precisely (committed `9520cb6`,
scope `docs/audit/v2.0-dma-internal-bus-cluster-scope-2026-06-01.md`):

**The DMC-fetch-drives-`open_bus` behaviour is already wired + default-on** (`bus.rs:3322`,
the §72 register-conflict). The cluster fails because the **DMASync conflict never lands**:
to drive `open_bus = DMC-sample` (so a `$4000`/`$4015`/`$2002` read concurrent with a DMC
fetch sees the fetched byte), the DMC DMA must **halt the CPU during the in-progress read** —
but RustyNES arms the DMC **~1-2 CPU cycles early**, so the held address is the *operand
fetch*, not the data read, and the halt never coincides. A clean sub-test repro: the
`DMA + Open Bus` test hangs forever in an `FF4E LDA $4000 / BNE` poll (open_bus stuck at
$40) because the DMC fetch never lands on the `$4000` read. **Three localized fixes are
already refuted** (post-tick latch, prev-address, delay-latch — `project_r1_dma_regression`).
The fix is the canonical **RDY-stall arming model**, done as a parallel implementation.

Intended outcome: RustyNES's DMC DMA halts on the correct (read) cycle with the held address
= the in-progress read, so the register-conflict / open-bus / abort semantics land exactly,
closing the DMA cluster — validated against the now-corrected Mesen oracle.

## The canonical model (nesdev `DMA.xhtml` + `APU_DMC.xhtml`; Mesen `ProcessPendingDma`)

- **RDY-stall.** The CPU is halted via RDY. When RDY deasserts, the 6502 **repeats the last
  read cycle** indefinitely (no forward progress, no interrupts). The held bus address = the
  read the CPU was attempting. When DMA completes, the CPU performs that read for real.
- **Halt only on read cycles.** DMA can only halt on a CPU *read* cycle; on a write it
  retries next cycle (delays up to 3: RMW = 2 writes, interrupts = 3). The halt itself is
  1 cycle of no useful work.
- **DMC timing.** DMC DMA = halt + dummy cycle + optional alignment + 1 get (3-4 cycles).
  The first "load" DMC DMA (after `$4015`/`$4010` start) halts on a **get** cycle during the
  2nd following APU cycle; "reload" DMC DMAs halt on a **put** cycle. Get/put align to
  apu_clk1/apu_clk2 (NOT reliably CPU even/odd — depends on the power-on alignment).
- **Register conflicts.** The repeated reads during no-op DMA cycles are externally visible:
  if the held read is a side-effect register ($2002/$4015/$4016/$4017) or open-bus
  ($4000-$4013), the DMC fetch drives the external bus and the CPU sees the fetched byte
  (the §72 behaviour, already wired — it just needs the halt to land on the right cycle).

## Plan — parallel implementation behind a new feature `dmc-rdy-stall-exact`

The R1 default path uses `drain_dma` (`bus.rs:2432`); the r4 feature uses
`Cpu::process_pending_dma` (`cpu.rs`, with `r4_need_halt`/`r4_need_dummy_read`/get_cycle). The
prior `mc-dmc-rdy-stall` (§80) is the closest attempt. Build the exact model behind a NEW
feature so the default stays byte-identical and A/B is clean.

### Phase R-0 — Oracle the exact halt cycle (no production change; DE-RISK GATE)
With the **corrected** Mesen oracle (`RestrictPpuAccessOnFirstFrame=true` + `RamState::AllZeros`,
already in `mesen2-irq-oracle.full.patch`), capture on the `DMA + Open Bus` + `DMC DMA Bus
Conflicts` sub-tests: per-CPU-cycle `(cpu_cycle, RDY, held_addr, dmc_running, get/put,
apu_clk_half, open_bus)`. The Mesen reference: DMC halts mid-instruction on the in-progress
`$4000` read ~896×. Capture the RustyNES equivalent (`dma_loop_trace`/`reg_read_trace` + the
`RUSTYNES_PC_TRACE`/`W2001` channels). **Cross-diff: pin the exact CPU cycle + apu_clk half
where Mesen asserts the halt vs RustyNES.** Prior cross-diffs predate the corrected oracle and
may have been boot-path-confounded — this re-capture is the load-bearing new data.

### Phase R-1 — Model the RDY-stall as repeat-the-in-progress-read (the core change)
In `Cpu` (the access path) + `LockstepBus` DMA hooks, behind `dmc-rdy-stall-exact`:
- When a DMC DMA is pending and the CPU is on a **read** cycle, deassert RDY: the CPU
  **does not advance PC** — it re-issues the SAME read (the held address) for each no-op DMA
  cycle (halt + dummy + optional alignment), THEN the DMC get, THEN the CPU completes its real
  read. On a **write** cycle, the halt is deferred to the next cycle.
- The held address feeds the existing §72 conflict logic (`bus.rs:3290-3325`) unchanged, so
  `open_bus = sample` lands on the right read.
- Get/put alignment from the APU clock half (`Apu::apu_phase`), not a CPU-parity proxy
  (memory §15/§16: a parity proxy provably cannot substitute).

### Phase R-2 — Arming timing (load vs reload; the 2nd-APU-cycle get)
Drive the pending-assert from the APU's real DMC timer so the "load" halt lands on the get
cycle of the 2nd following APU cycle and "reload" on a put. Replace the `dmc_pend_delay` /
`dmc_pend_pre_tick(2)` latch heuristics (the refuted localized fixes) with the model-derived
assert point. Keep the legacy latch fields only for the A/B knob.

### Phase R-3 — DMC-during-OAM + the aborts
`service_dmc_dma_during_oam` (`bus.rs:3048/3118`): the DMC get takes precedence over the OAM
get (delays it, may force an OAM alignment cycle). Explicit/Implicit DMA Abort: model the
abort landing relative to the get/put (the §94/§95 abort-countdown phase). These ride the
R-1/R-2 model rather than the hand-counted `+2` hack.

### Phase R-4 — Validate the cluster (corrected oracle, BATTERY)
The DMA tests are NOT faithfully isolatable (the isolated sub-tests hang vs battery "error 2"),
so validate via the **battery** RAM-decode harness (`--test accuracycoin`) AND the corrected-
oracle per-cycle cross-diff. Target: the ~12 DMA/bus tests flip to pass; the cluster's shared
sync primitive also unblocks the APU-frame + sync-dependent-PPU tests (memory's "highest-
leverage lever").

### Phase R-5 — Promote (user-gated)
If the DMA cluster passes with the read-side milestone held: flip `dmc-rdy-stall-exact` toward
default, retiring the `dmc_pend_delay`/§80 heuristics. Update `docs/STATUS.md`, ADR-0002/0007.

## Cross-cutting invariants (run after EVERY step)
- `cpu_interrupts_v2` 5/5, `ppu_vbl_nmi` 10/10, `nes-ppu` 41/41 (the read-side milestone).
- AccuracyCoin RAM-decode fail-count **monotonically non-increasing** from the branch
  baseline (29 default).
- 60-ROM commercial oracle byte-identical (**no re-baseline without visual proof** — the
  masking-trap guard); default build byte-identical until R-5; `clippy -D warnings` + fmt.
- Each new behavior behind `dmc-rdy-stall-exact`; A/B vs the current path by toggling it.
- **ASK before rollback** on a regression (`feedback_adr0002_ask_before_rollback`) — report
  with evidence; do not auto-revert.

## Critical files
- `crates/nes-cpu/src/cpu.rs` — the access path / `process_pending_dma` / RDY-stall.
- `crates/nes-core/src/bus.rs` — `drain_dma` (2432), `service_dmc_dma`(2700/2799),
  `service_dmc_dma_during_oam`(3048/3118), the §72 conflict (3290-3325), the
  `dmc_pend_*` latches, `dma_halt_addr`.
- `crates/nes-apu/src/apu.rs` — `dmc_dma_pending`/`apu_phase` (the real DMC timer + clock half).
- Cargo features (nes-cpu/core/test-harness) — add `dmc-rdy-stall-exact`.
- Oracle/validation: `scripts/mesen2-irq-oracle/*` (corrected patch), `dma_loop_trace.rs`,
  `reg_read_trace.rs`, the sub-test extractor `scripts/accuracycoin-build/build_sub_test_rom.py`.

## Verification
- R-0: corrected-oracle per-cycle cross-diff pins the halt cycle; no production change.
- Per phase: `cargo test -p nes-test-harness --release --features test-roms --test accuracycoin`
  (RAM-decode count) + `--test cpu_interrupts_v2` (5/5) + `--test ppu_vbl_nmi` (10/10).
- Payoff: the DMA/bus cluster flips to pass (battery); cross-diff confirms the held address
  matches Mesen at the `$4000`/`$4015`/`$2002` reads.
- Determinism: save-state round-trip + 60-ROM oracle byte-identical; no_std `thumbv7em` clean.

## Risks & guardrails
- **17+ prior rollbacks on this surface** + 3 refuted localized DMC-arming fixes. Mitigation:
  oracle-derived (R-0 pins the exact cycle, no hand-tuning); parallel implementation behind
  `dmc-rdy-stall-exact` (revert = drop the flag); read-side milestone is a HARD invariant.
- The corrected oracle is the genuinely new advantage — prior attempts were boot-path-
  confounded; R-0's clean re-capture is the load-bearing difference vs the refuted attempts.
- This is a multi-week core; R-0 is the go/no-go gate (if the held-cycle offset is a clean,
  single, model-derived value the rewrite is bounded; if it is many-bodied, re-scope).
