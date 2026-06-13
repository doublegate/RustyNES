# Sprint 5 — C1 axis attempt 17 (CPU/PPU access ordering)

**Phase:** 6 — v1.0.0 final
**Status:** OPEN (gated on Sprint 4 completion).
**Cascade risk:** **HIGHEST** — 13 prior rollbacks (Attempts 1-4 in
v0.9.0-rc era; Phase B4 threshold prototype; mid-cycle snapshot
experiment; M2-low CPU IRQ sample; Sessions 12-16 prereq-only;
Session-17 per-instruction divergence; Session-18 PPU-axis predicate).

**Note:** this sprint is intentionally LATE in the priority order per
Session-19 strategic guidance. Sprints 1-4 are higher-leverage per
session investment.

## Target tests (4)

- `cpu_interrupts_v2/2-nmi_and_brk` (strict residual + AccuracyCoin)
- `cpu_interrupts_v2/3-nmi_and_irq` (strict residual + AccuracyCoin)
- `cpu_interrupts_v2/5-branch_delays_irq` (strict residual + AccuracyCoin)
- `mmc3_test_2/4-scanline_timing` sub-test #3 (strict residual + AccuracyCoin)

Estimated yield: **+1 to +4 AccuracyCoin tests + 3 strict residuals
flipped** (the strict ignored count drops from 5 to 2).

## Load-bearing axis (Session-18 empirical finding)

Per `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md`:

The actual load-bearing axis is **CPU-vs-PPU per-cycle access
interleaving**. RustyNES's `Cpu::read1` does:
```
bus.cpu_read(addr); idle_tick();  // read FIRST, PPU ticks 3 dots AFTER
```

Mesen2's `NesCpu::MemoryRead` does:
```
StartCpuCycle(advances PPU); Read(); EndCpuCycle(advances PPU more);
```

The 3-PPU-dot ordering difference makes RustyNES's read see the PPU
state at end-of-PRIOR-cycle, while Mesen2's read sees the PPU state at
mid-CURRENT-cycle. At scanline 240/dot 340 → scanline 241/dot 1 this is
the difference between "before VBL set" and "after VBL set". For the
failing reads in `cpu_interrupts_v2/{2,3}`, this puts RustyNES on the
wrong side of the documented `$2002` race window inside blargg's
`sync_vbl` precise-synchronization loop.

For `mmc3_test_2/4` sub-test #3, the same access-ordering shift affects
the IRQ assertion cycle at the boundary between scanline 261 (pre-render)
and scanline 0 (visible).

## Prerequisites (must be landed first)

1. **Per-PPU-dot trace infrastructure for the failing window**
   (Sprint 4 prerequisite if not already exercised). Extend
   `crates/rustynes-ppu/src/state_trace.rs` to capture every `cpu_read` /
   `cpu_write` event at sub-dot precision.

2. **Scoped access-ordering change (only for `$2002` register reads).**
   Test in isolation BEFORE applying to all register reads. The C1
   axis specifically affects PPU register reads; non-PPU register
   reads are NOT load-bearing per Session-18.

3. **Explicit user authorization to re-baseline the 60-ROM commercial-
   ROM oracle.** Access-ordering changes will shift framebuffer
   FNV-1a hashes by ≥ 1 dot. Audio + cycle invariants should remain
   byte-identical (verify).

## Sprint plan

### Step 1 — Confirm Session-18 finding via dot-precise oracle

Extend `crates/rustynes-ppu/src/state_trace.rs` with a `--track-cpu-accesses`
mode. For each of the 4 failing tests, capture:
- `(cpu_cycle, ppu_scanline, ppu_dot, sub_dot)` at every `cpu_read` /
  `cpu_write` to `$2002`.
- Compare against Mesen2 via an extended
  `scripts/mesen2_cpu_access_trace.lua`.

Land the comparison data in `docs/audit/sprint-5-cpu-access-trace.md`.

### Step 2 — Scoped fix: PPU register access ordering only

In `crates/rustynes-cpu/src/cpu.rs`, `read1` for the `$2000-$3FFF` range
(PPU register mirror), restructure to:
```
idle_tick();      // PPU advances 3 dots BEFORE the read
bus.cpu_read(addr);
```

All other reads keep the existing `bus.cpu_read; idle_tick;` order.

Gate on feature flag `cpu-c1-attempt-17-ppu-access-reorder` (default
off).

### Step 3 — Validation gauntlet

Standard 10-gate gauntlet. Special attention:

- **All 4 target tests must flip.** If only some flip, the fix is
  incomplete; do not land partial.
- `ppu_vbl_nmi/*` 10/10 strict pass.
- `vbl_race_window_2002_read_sweep` unit test (Session-18 oracle):
  the predicate window MAY shift by 1 dot post-fix; if so, update the
  oracle to the new expected window AND verify the new window matches
  Mesen2.
- B4 invariant: first MMC3 IRQ at cycle 1,370,110 / scanline 0 / dot
  257. If the C1 access-ordering shift moves this by 1-3 PPU dots,
  the invariant updates AND `mmc3_test_2/4` sub-test #2 must still
  pass.
- **60-ROM commercial-ROM oracle**: framebuffer FNV-1a hashes WILL
  shift. Re-baseline ONLY after explicit user authorization.

### Step 4 — Sacred-trio bisect

Run `scripts/regression-bisect/bisect-real-games.sh` against SMB /
Excitebike / Kid Icarus PAL. Boot-and-play must remain clean.

### Step 5 — Land OR rollback

This is attempt 17. If it cascades, rollback discipline is identical
to attempts 1-16: revert chip-stack code; land infrastructure /
diagnostic / audit-doc only; document in CHANGELOG `[Unreleased]` →
"Investigated and rolled back"; update ADR-0002 with a new
"Decision update" subsection.

## Cascade-risk callouts

1. **13 prior rollbacks**. The first 11 (Attempts 1-4 + Phase B4
   threshold prototype + mid-cycle snapshot + M2-low CPU IRQ +
   Sessions 14-15) targeted the wrong axis (per Session-17/18
   empirical findings). Attempts 16 (Session-18) and any subsequent
   target the correct axis but require a coordinated change, not a
   single-line predicate flip.

2. **PPU register access ordering affects MANY tests beyond the 4
   targets.** Specifically:
   - `ppu_vbl_nmi/*` — sensitive to VBL set/clear timing at register
     read boundaries.
   - `sprite_hit_tests/*` — sensitive to sprite-zero hit flag set
     timing.
   - Real games' boot sequences often poll `$2002` in tight loops;
     access-ordering changes can shift `(scanline, dot)` of the first
     observed VBL by 1-3 dots, affecting downstream timer setup.

3. **Re-baselining the 60-ROM commercial-ROM oracle requires user
   authorization.** The audio + cumulative cycle-count invariants
   SHOULD remain byte-identical (this is the regression sentinel that
   distinguishes "expected visual shift" from "real regression").

4. **mmc3_test_2/4 sub-test #2 invariant**: the Phase B4 fix flipped
   sub-test #2 (FAIL → PASS) via the reload-pending Sharp
   discriminator. Sub-test #3 is the target of this sprint. The fix
   must NOT regress sub-test #2.

## Estimated effort + yield

- **Effort:** 3-5 days (research-heavy in Step 1; surgical in Steps
  2-3; cautious in Step 4).
- **Yield:** +3 strict residuals flipped + 1-4 AccuracyCoin tests
  (the 4 C1 sub-tests are part of the 24 failing AccuracyCoin
  residuals).

## References

- `docs/adr/0002-irq-timing-coordination.md` (PRIMARY reference, all
  "Decision update" subsections through Session-18)
- `docs/audit/session-17-c1-attempt15-per-instruction-divergence-2026-05-22.md`
  (PPU-axis finding)
- `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md`
  (access-ordering finding)
- `docs/audit/session-15-c1-attempt13-mesen2-irq-oracle-2026-05-22.md`
  (Mesen2 oracle infrastructure)
- `docs/audit/session-16-c1-attempt14-prereq-infrastructure-2026-05-22.md`
  (vector-fetch events + START_CYCLE plumbing)
- `crates/rustynes-cpu/src/cpu.rs` `read1` / `write1` / `idle_tick`
  (production surface)
- `crates/rustynes-cpu/src/scheduler.rs` `M2Phase::Low/High` (infrastructure
  already plumbed)
- `crates/rustynes-core/src/bus.rs` `LockstepBus` (access-ordering surface
  for the bus layer)

## Exit criterion

- 3 `cpu_interrupts_v2/{2,3,5}` strict residuals flipped (ignore count
  drops 5 → 2).
- `mmc3_test_2/4` sub-test #3 flipped (1 more strict, 2nd ignore drops).
- AccuracyCoin pass rate increases.
- 60-ROM commercial-ROM oracle re-baselined (with user authorization);
  audio + cycle invariants preserved byte-identical.
- Sacred trio PAL boots cleanly.
- If pass rate reaches ≥ 90%, jump to v1.0.0 final tag. Otherwise
  proceed to Sprint 6 (last-resort SH* sprint).
