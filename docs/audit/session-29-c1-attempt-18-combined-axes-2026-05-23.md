# Session-29 — C1 Attempt 18: Combined φ1/φ2 Split + `$2002` Predicate Narrow

**Date**: 2026-05-23
**Status**: Phase 1 + 2 (feature-flagged combined CPU-side φ1/φ2 split + PPU-side predicate narrow) ATTEMPTED and ROLLED BACK as default. The C1-attempt-17 cargo feature is preserved as the next-iteration scaffold; default OFF.
**Predecessor**: `session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md` (PPU-axis predicate narrowing alone — rolled back because RustyNES's reads land at dot 0, not dot 1).

---

## Hypothesis tested

Per Session-18 §"Key empirical findings" line 238:
> The PPU-axis predicate (`dot == 0` vs `dot <= 1`) and the CPU-axis interleaving (read-then-tick vs tick-then-read) are **independent**. **Both must be aligned with Mesen2** to make the failing `sync_vbl` polls converge.

Attempt 17 (Session-26+) had landed the φ1/φ2 split alone (cpu-side intra-cycle reorder) — gauntlet with feature ON showed `cpu_interrupts_v2/4` + `ppu_vbl_nmi/5` + `full_palette_*` regressions and the target `cpu_interrupts_v2/{2,3,5}` STILL FAILED.

Attempt 18 tests the Session-18 prediction directly: combine the φ1/φ2 split AND the predicate narrow under the same `cpu-c1-attempt-17-access-reorder` feature flag. With BOTH active:

* CPU-side: `Cpu::read1` ticks PPU 1 dot BEFORE the bus access (mirroring Mesen2's `StartCpuCycle`), so the BIT $2002 read lands at PPU dot 1 instead of dot 0.
* PPU-side: `$2002` race-window predicate narrows from `dot <= 1` to `dot == 0`, so the dot-1 read no longer triggers suppression and observes the just-set VBL flag.

Together this prediction said the read at scanline 241 dot 1 should now return $80 (VBL set, not suppressed), satisfying the blargg `sync_vbl` precise polling loop.

---

## Implementation

Two files modified:

* `crates/nes-ppu/Cargo.toml` — added `cpu-c1-attempt-17-access-reorder` feature on `nes-ppu` (forwarded from `nes-core`).
* `crates/nes-ppu/src/ppu.rs::cpu_read_register` case 2 — `in_race_window` predicate now `cfg`-gated: `dot == 0` under the feature, `dot <= 1` legacy.

The CPU-side φ1/φ2 split (`Cpu::read1` / `write1` + `LockstepBus::tick_cpu_cycle_phi1` / `tick_cpu_cycle_phi2`) is the same code that landed in commit `370f486`.

---

## Result — ATTEMPT 18 ROLLED BACK

Gauntlet with feature ON (`cargo test --workspace --features test-roms,nes-core/cpu-c1-attempt-17-access-reorder --release`):

| Test | Status |
|------|--------|
| Workspace strict pass | **540** (was 545) → **5 regressions** |
| Regression 1 | `cpu_interrupts_v2_4_irq_and_dma_strict` FAILED (was strict-pass) |
| Regression 2 | `ppu_vbl_nmi_05_nmi_timing` FAILED (was 10/10 strict) |
| Regression 3 | `vbl_race_window_2002_read_sweep` FAILED (Session-18 oracle needs predicate update under feature) |
| Regression 4 | `full_palette_frame_60` FAILED |
| Regression 5 | `full_palette_frame_180` FAILED |
| Target | `cpu_interrupts_v2/{2,3,5}_strict` — **STILL `#[ignore]`'d** (DID NOT FLIP) |
| Target | `mmc3_test_2/4` sub-test #3 — STILL FAIL (orthogonal axis) |
| AccuracyCoin | 90.65% → **89.21%** (-2 tests) |

The target tests do NOT flip even with both changes combined. Per Session-17's empirical trace:
> RustyNES is **+1 CPU cycle behind** on absolute cycle anchor [on tests 2/3/5]

The 1-PPU-dot intra-cycle shift my φ1/φ2 split provides is insufficient to close the 3-PPU-dot (= 1 CPU cycle) gap. The 3-dot flat-shift variant (commit `3abb7d2`) cascades 19 tests because it shifts EVERY read's PPU sample point by 3 dots. The φ1/φ2 split only shifts by 1 dot, which doesn't reach Mesen2's read position.

---

## Empirical conclusion

The load-bearing axis is the **absolute CPU cycle anchor** for IRQ-heavy boot sequences (`cpu_interrupts_v2/{2,3,5}`), **not** the intra-cycle PPU/CPU access ordering. Per Session-17 Phase 1.3 outcomes:

* 4 of 6 trace ROMs show RustyNES is `+1 CPU cycle behind` Mesen2 on absolute cycle (`1-cli_latency`, `2-nmi_and_brk`, `3-nmi_and_irq`, `5-branch_delays_irq`, `mmc3_test_2/4`).
* Only `4-irq_and_dma` is byte-identical at every common-cycle CPU instruction.
* The PASSING test 1 has the +1 offset but still PASSES because its execution doesn't depend on PPU race-window reads.
* The FAILING tests 2/3/5 fail because the +1 offset puts their BIT $2002 read at the wrong PPU dot relative to VBL set.

The Session-13 cold-boot alignment (`Cpu::power_on` SP `$FD` → `$00` + 8-cycle reset + PPU scheduler power-up dot 0 → 340) closed the +344-dot drift but left a residual +1 CPU cycle offset that varies between test ROMs. The residual is likely:

* A 1-cycle difference in our `Cpu::reset` sequence vs Mesen2's `NesCpu::Reset` (line ~157-165 of `Core/NES/NesCpu.cpp`).
* Or a 1-cycle difference in the first NMI service after reset (tests 2/3 are NMI-heavy).
* Or a 1-cycle difference in how the first frame's VBL is reached by the test ROM's boot routine.

Sessions 13-18 chased CPU-side IRQ-sample-point timing (12 attempts) — none closed the absolute cycle offset because the offset comes from BEFORE any IRQ event. Sessions 16-18 reframed to the PPU axis but the predicate / position narrow alone can't close a 3-dot absolute-position gap.

---

## Recommended next attempts (attempt 19+)

1. **Per-CPU-instruction boot trace at the very-first 100k cycles** (extend Session-17's cpu_boot_trace fixture to cycles 0..100,000 instead of 250k..350k). Identify the EXACT CPU instruction where RustyNES diverges from Mesen2 by 1 cycle. This is the load-bearing surface; once identified, a 1-line CPU fix at that instruction should close all 4 cpu_interrupts_v2 sub-ROMs simultaneously.

2. **Compare `Cpu::power_on` / `Cpu::reset` vs Mesen2's `NesCpu::PowerUp` / `Reset`** line-by-line. The 8-cycle reset sequence in Mesen2 (`Core/NES/NesCpu.cpp` lines ~160-165 — the "CPU takes 8 cycles before it starts executing the ROM's code after a reset/power up" comment block) should be EXACTLY mirrored by our `Cpu::power_on`. Verify the 8 StartCpuCycle/EndCpuCycle pairs match our 8 idle_tick calls cycle-for-cycle.

3. **First NMI service path comparison**. Mesen2's `NesCpu::IRQ` (lines ~183-220) has a specific sequence: ProcessPendingDma → DummyRead → DummyRead → Push PCH → Push PCL → Push P → SetPC(vector). Compare to our `Cpu::service_interrupt`. Any 1-cycle deviation in the NMI service compounds across every NMI in the test.

4. **MMC3 sub-test #3 is on a different axis**: the canonical `T_last - 1` CPU IRQ-sample-point axis. Independent of the cpu_interrupts_v2 work; needs its own focused attempt.

---

## What landed in this session

* `crates/nes-ppu/Cargo.toml` — `cpu-c1-attempt-17-access-reorder` feature added on `nes-ppu` (forwarding chain complete).
* `crates/nes-core/Cargo.toml` — feature forward now includes `nes-ppu/cpu-c1-attempt-17-access-reorder`.
* `crates/nes-ppu/src/ppu.rs::cpu_read_register` case 2 — `in_race_window` predicate becomes `cfg`-gated.
* `docs/audit/session-29-c1-attempt-18-combined-axes-2026-05-23.md` — this audit document.

Production behavior at default feature state is UNCHANGED. Workspace gauntlet at default-off: 545 strict + 5 ignored. AccuracyCoin pass rate: 90.65%. Commercial-ROM oracle: 60/60. Sacred trio preserved.

---

## C1-axis rollback count

15th C1 axis rollback (attempts 1-4 + Phase B4 prototype + mid-cycle snapshot + M2-low CPU IRQ sample + Sessions 14-15 prereq + Session-17 hypothesis-only + Session-18 PPU-axis predicate + Session-26+ attempt-17 φ1/φ2 v1 (3-dot flat) + Session-26+ attempt-17 φ1/φ2 v2 (1+2 split) + this Session-29 combined attempt 18).

The C1 axis remains the **single open gate** between v1.0.0-rc2 and v1.0.0 final. AccuracyCoin ≥ 90% gate is CLEARED. The remaining 4 C1 IRQ-timing residuals (`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3) require either:

* (Per Recommended next attempts above) the +1 CPU cycle absolute-anchor diagnosis on tests 2/3/5 + the canonical CPU `T_last - 1` axis for mmc3 #3, OR
* Deferral to v1.x with documented rationale and per-test `#[ignore]` probes preserved.
