# Phase 7 Sprint 2 — CPU, DMA, and internal-bus closure

**Date:** 2026-05-24 (v1.5.0)
**Scope:** close the CPU/DMA/`$4015`/NMI residuals that are *completable in the
v1.x horizon*; document the ones that require the v2.0 master-clock /
internal-bus rework. Additive only — AccuracyCoin held at 90.65%, oracle /
sacred trio / B4 byte-identical.

## Ticket disposition

### T-72-001 — C1 IRQ-sample-timing bundle — **DEFERRED to v2.0**

The coordinated CPU/Bus/PPU IRQ sample-point rework (`cpu_interrupts_v2/{2,3,5}`
+ `mmc3_test_2/4` sub-test #3) has been attempted and rolled back **17 times**.
Session-29 (`docs/audit/session-29-c1-axis-final-conclusion-2026-05-23.md`)
empirically falsified the last surgical option (global PPU-position shift):
closing C1 requires changing the per-cycle *phase relationship* between CPU and
PPU, which is the v2.0 master-clock-precise scheduling refactor (replace the
integer-3-PPU-dots-per-CPU-cycle model with Mesen2's fractional
12-master-clocks-per-CPU-cycle model). The `#[ignore]`'d probes + the
`cpu-c1-attempt-17-access-reorder` feature scaffold + the IRQ-trace fixture
remain as the v2.0 foundation. **No new attempt is made in Phase 7** — that
would be rollback #18.

### T-72-002 — NMI hijack and BRK vector evidence — **DONE (C1-independent part)**

Added `crates/nes-cpu/tests/opcodes.rs`:
- `nmi_pushes_status_with_b_clear_and_takes_fffa_vector`
- `irq_pushes_status_with_b_clear_and_takes_fffe_vector`

These pin the architectural B-flag stack-value contract — BRK/PHP push B **set**
(already covered by `brk_pushes_pc_plus_2_and_status_with_b`), IRQ/NMI push B
**clear**, bit 5 (unused) always set — and the correct vectors (`$FFFE` IRQ,
`$FFFA` NMI). The *cycle-precise* NMI-hijacks-BRK window is the
`cpu_interrupts_v2/2` residual on the deferred C1 axis; the B-flag/vector
semantics tested here are independent of it.

### T-72-003 — Internal-vs-external bus model — **DONE (split landed); SH\* residual DEFERRED**

The internal-vs-external 2A03 data-bus separation already landed in v1.0.0
Phase 1a/b: `LockstepBus` carries both `open_bus` (external) and
`internal_data_bus`, DMC DMA fetches drive only the external bus, and `$4015`
bit 5 is sourced from the internal bus (resolving Open Bus #9 vs Internal Data
Bus #2 simultaneously — see `bus.rs::raw_cpu_read`). The `$4015`-external-bus
behavior is now regression-guarded by
`reading_4015_does_not_refresh_external_open_bus` (T-72-006).

The **SH\* / TAS / LAS / XAA unstable-store residuals** (5 AccuracyCoin tests,
`[error 7]`) remain deferred: they need an explicit "RDY low for 2 cycles
corrupts the store's high address byte" model that a prior attempt could not
land without regressing Internal Data Bus #2. This is the internal-bus-model
rework reserved for v2.0 (it interacts with the master-clock DMA halt phasing).

### T-72-004 — DMC DMA side-effect bracket audit — **DONE (audit); completion DEFERRED**

blargg `dmc_dma_during_read4` stays **5/5 strict** (`dma_2007_read`,
`dma_2007_write`, `dma_4016_read`, `double_2007_read`, `read_write_2007`) — the
repeated-halted-read side effects on `$2007`/`$4015`/`$4016`/`$4017` are correct
for the load case. The remaining 4 AccuracyCoin DMA-cluster tests
(`DMA + $4015 Read`, `DMC DMA + OAM DMA`, `Explicit/Implicit DMA Abort`) need
the get/put cycle-alternation scheduler, which shipped behind the default-off
`dmc-get-put-scheduler` feature in v1.2.0 (ADR 0007) at 6/10 and is promoted to
default-on as part of the v2.0 master-clock absorption (ADR 0007 option c). No
default-build behavior change in Phase 7.

### T-72-005 — Power-on randomization mode — **DONE**

`Nes::from_rom_with_power_on_seed(bytes, seed)` +
`LockstepBus::randomize_power_on_ram(seed)`: a seeded `xorshift64` fill of the
2 KiB CPU work RAM + open-bus latch (models unreliable power-on RAM per nesdev
"CPU power up state"). Seeded → deterministic, so the determinism contract,
save-state round-trip, and the regression oracle are unaffected; the default
`from_rom` path (zeroed RAM) is what CI uses. CPU/PPU/DMA *phase* randomization
is intentionally not done (the lockstep scheduler phase is deterministic by
design and randomizing it is entangled with the v2.0 master-clock refactor).
Unit test: `power_on_randomization_is_opt_in_seeded_and_deterministic`.

### T-72-006 — `$4015` open-bus semantics — **DONE**

Documented in `bus.rs::raw_cpu_read` and regression-guarded by
`reading_4015_does_not_refresh_external_open_bus`: a `$4015` read returns APU
status but does not drive the external data bus, so the open-bus latch is
preserved across it (per nesdev "Open bus behavior" + AccuracyCoin Open Bus #7).

## Exit-checklist status

- `cpu_interrupts_v2` residuals: documented with updated trace evidence (C1
  axis, v2.0) — see above + `docs/STATUS.md`.
- AccuracyCoin CPU/APU/internal-bus residual list: unchanged at 90.65%; the
  completable items here are *coverage/guards*, the residual *fixes* are the
  v2.0 master-clock/internal-bus rework.
- No commercial-ROM oracle regression (additive tests + a new opt-in
  constructor only).
