# Sprint 3-2 — DMC channel + DMC DMA + frame counter

**Phase:** Phase 3 — Audio + Polish
**Sprint goal:** DMC channel functional with DMA cycle stealing; frame counter operational with both modes and frame IRQ; 2A03 register-readout-during-DMA bug reproduced.
**Estimated duration:** 2 weeks

## Tickets

### T-32-001 — Frame counter (4-step + 5-step modes)

**Description:** Implement the frame counter sub-unit per `docs/apu-2a03.md` §Frame counter. 4-step mode (with frame IRQ); 5-step mode (no IRQ; immediate quarter+half-frame clock on write).

**Acceptance criteria:**
- [x] Both modes clock channel sub-units (envelope/linear, length/sweep) at correct cycles.
- [x] Writing `$4017` resets the counter with the documented 3- or 4-cycle delay.
- [x] Mode 1 selection immediately fires quarter+half-frame events.
- [x] Frame IRQ asserted at correct cycle of mode 0 (if not inhibited).
- [x] `apu_test/3-irq_flag` and `apu_test/4-jitter` pass.

**Reference:** `docs/apu-2a03.md` §Frame counter.
**Estimated complexity:** L.

---

### T-32-002 — DMC channel (timer, output unit, memory reader)

**Description:** Implement the DMC: bit-shift register, 7-bit output (deltas of ±2 clamped 0..=127), memory reader (sample address `$C000 + A*64`, length `L*16+1`, wrap `$8000` after `$FFFF`), loop, IRQ-on-end.

**Acceptance criteria:**
- [x] Direct write to `$4011` updates DAC immediately.
- [x] DMC IRQ asserted at end of sample (if enabled and not looping).
- [x] DMC IRQ flag cleared by writing `$4015`.
- [x] Sample address wraps correctly.

**Reference:** `docs/apu-2a03.md` §DMC channel.
**Estimated complexity:** M.

---

### T-32-003 — DMC DMA via scheduler

**Description:** Wire DMC's "buffer empty, request a byte" into the scheduler's DMA controller. Halt CPU on next read cycle; perform 1 memory read; total 3 or 4 CPU cycles per byte.

**Acceptance criteria:**
- [x] CPU halt accounting correct.
- [x] DMC DMA preempts OAM DMA when both pending (per `ref-docs/research-report.md` §DMA).
- [x] Cycle count matches expected for both load and reload variants.
- [x] `dmc_dma_during_read4/dma_2007_read` passes.

**Reference:** `docs/apu-2a03.md` §DMC and DMA interactions; `docs/scheduler.md` §DMA controller.
**Estimated complexity:** L.

---

### T-32-004 — 2A03 register-readout-during-DMA bug

**Description:** While CPU is halted by DMC DMA, repeats of the previously-addressed read cause extra reads of `$2007` (2-3 extra), `$4015`-`$4017` (1-4 extra). PAL 2A07 fixes these.

**Acceptance criteria:**
- [x] Bug reproduced for NTSC; fixed for PAL.
- [x] `dmc_dma_during_read4/dma_4016_read` passes; `4015_read` variant not in upstream rom set.

**Reference:** `docs/apu-2a03.md` §Edge cases item 1; `ref-docs/research-report.md` §DMA → Register conflict issues.
**Estimated complexity:** L.

---

### T-32-005 — `cpu_interrupts_v2` passes

**Description:** Re-validate CPU interrupt handling now that frame IRQ + DMC IRQ are real sources.

**Acceptance criteria:**
- [ ] All 5 sub-ROMs pass.  **Status: 1/5 after the per-cycle bus interleaving CPU refactor.**  `1-cli_latency` PASS (CLI / SEI / RTI I-flag delay model).  `2-nmi_and_brk` / `3-nmi_and_irq` / `4-irq_and_dma` / `5-branch_delays_irq` still fail by 1-2 cycles in places.  The `branch_delays_irq` quirk itself is now implemented in `rustynes-cpu` (branches suppress IRQ sampling on the operand-fetch / taken / page-cross cycles via `Cpu::skip_irq_sample`; covered by the `branch_taken_no_cross_delays_irq_one_instruction` unit test), but ROM 5 still fails on its `test_jmp` sub-test (an upstream IRQ-cycle-precision issue independent of branches), so the ROM never reaches the branch sub-tests.  `2/3/4` are likely a mix of finer PPU dot alignment and DMA cycle accounting.

**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

## Sprint review checklist

- [x] All tickets checked off (T-32-005 cpu_interrupts_v2 deferred to Phase 4 — see ticket).
- [x] DMC + DMA bugs reproduced; `dmc_dma_during_read4` test ROMs all pass.
- [x] CHANGELOG entry: "DMC channel + frame counter + DMA complete."
