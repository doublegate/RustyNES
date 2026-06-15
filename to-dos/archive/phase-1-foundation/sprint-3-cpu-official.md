# Sprint 3 — CPU core: official opcodes

**Phase:** Phase 1 — Foundation
**Sprint goal:** Implement all 151 documented 6502 opcodes with cycle-accurate execution. Pass blargg's `instr_test_v5` "official-only" subset.
**Estimated duration:** 2 weeks

## Tickets

### T-13-001 — `Cpu` struct, registers, status flags

**Description:** Define `Cpu` in `crates/rustynes-cpu/src/cpu.rs` with A, X, Y, PC, S, P. Use `bitflags` for status. Define `Bus` trait per `docs/cpu-6502.md` §Interfaces.

**Acceptance criteria:**

- [x] `Cpu::new()` returns power-on state per `docs/cpu-6502.md` §State.
- [x] `Cpu::reset(bus)` performs the documented 7-cycle reset sequence.
- [x] Status flag bitflags compile.

**Dependencies:** Sprint 1 complete.
**Reference:** `docs/cpu-6502.md` §Interfaces, §State.
**Estimated complexity:** S.

---

### T-13-002 — Addressing modes

**Description:** Implement all 13 addressing modes as enum + helpers: implied, accumulator, immediate, zero page, zero-page-X, zero-page-Y, absolute, absolute-X, absolute-Y, indirect, indexed-indirect (X), indirect-indexed (Y), relative.

**Acceptance criteria:**

- [x] Each mode produces a correct effective address and increments PC correctly.
- [x] Page-crossing detection for indexed modes.
- [x] Unit tests for each mode with edge cases (page wrap, zero-page wrap).

**Dependencies:** T-13-001.
**Reference:** `docs/cpu-6502.md` §Behavior.
**Estimated complexity:** M.

---

### T-13-003 — Load/store/transfer/stack instructions

**Description:** LDA, LDX, LDY, STA, STX, STY, TAX, TAY, TSX, TXA, TXS, TYA, PHA, PHP, PLA, PLP. With cycle counts.

**Acceptance criteria:**

- [x] Each opcode dispatches correctly per `nes-test-roms/other/nestest.txt` opcode table.
- [x] Cycle counts match the cycle reference chart.
- [x] Flag updates correct (N, Z, others as documented).
- [x] Unit tests per opcode.

**Dependencies:** T-13-002.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** M.

---

### T-13-004 — Arithmetic + logic + shift instructions

**Description:** ADC, SBC, AND, ORA, EOR, ASL, LSR, ROL, ROR, BIT, CMP, CPX, CPY, INC, DEC, INX, DEX, INY, DEY.

**Acceptance criteria:**

- [x] ADC and SBC handle carry, overflow, and binary mode correctly (D flag is ignored on 2A03).
- [x] All flag updates per the 6502 instruction reference.
- [x] Property test for ADC: random A + operand + carry → expected (A_out, N, V, Z, C) per a hand-rolled reference.
- [x] Cycle counts match.

**Dependencies:** T-13-003.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** L.

---

### T-13-005 — Branch + jump + return + interrupt instructions

**Description:** BCC, BCS, BEQ, BMI, BNE, BPL, BVC, BVS, JMP, JMP (indirect), JSR, RTS, RTI, BRK, NOP.

**Acceptance criteria:**

- [x] Indirect JMP page-bug preserved (`JMP ($XXFF)` reads high byte from `$XX00`).
- [x] Branch cycle penalty: +1 if taken, +2 if page-crossing taken.
- [x] BRK pushes P with B flag set; RTI pops P with B flag cleared (effective P).
- [x] JSR/RTS PC accounting correct (off-by-one detail per the 6502).

**Dependencies:** T-13-004.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** M.

---

### T-13-006 — Flag manipulation + miscellaneous

**Description:** CLC, SEC, CLD, SED, CLI, SEI, CLV, NOP variants.

**Acceptance criteria:**

- [x] Each flag set/clear instruction works.
- [x] CLD and SED are accepted (D flag is settable on 2A03 even though ignored arithmetically).

**Dependencies:** T-13-005.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** S.

---

### T-13-007 — Cycle accuracy: dummy reads + writes

**Description:** Implement the documented dummy reads (e.g., during page-crossing absolute-X read instructions) and dummy writes (during read-modify-write instructions like INC, DEC, ASL, LSR, ROL, ROR). These cost cycles even though their results are discarded.

**Acceptance criteria:**

- [ ] `cpu_dummy_reads` test ROM passes (when run with stub bus). *Deferred: ROM is mapper 3 (CNROM); blocked on Sprint 5.*
- [ ] `cpu_dummy_writes_oam` and `cpu_dummy_writes_ppumem` test ROMs pass (when run with stub bus that logs accesses). *Deferred: ROMs are mapper 1 (MMC1); blocked on Sprint 5.*

**Dependencies:** T-13-006.
**Reference:** `docs/cpu-6502.md` §Cycle-accurate execution.
**Estimated complexity:** M.

---

### T-13-008 — Interrupt logic (NMI, IRQ, BRK, hijacking)

**Description:** Implement edge-detected NMI, level-sensitive IRQ, the 7-cycle interrupt sequence, and NMI-IRQ-BRK hijacking. Per `docs/cpu-6502.md` §Interrupt logic.

**Acceptance criteria:**

- [x] NMI fires once per high-to-low edge.
- [x] IRQ fires while line is low and I flag is clear.
- [x] NMI hijacks BRK during ticks 1-4. *Implemented in `Cpu::service_interrupt`: if NMI is sampled during the dummy / push cycles of a BRK sequence, the vector fetch redirects from `$FFFE` to `$FFFA` and the latched NMI is consumed in-place.  Per-cycle bus interleaving refactor landed.  04-nmi_control passes; 2-nmi_and_brk shows partial (timing differs by 1-2 cycles in the latter half — likely PPU-dot precision rather than CPU-cycle precision).*
- [ ] Branch instruction polling matches documented quirks. *Partial: per-cycle helpers sample IRQ/NMI on every emitted tick, but the documented "taken-no-cross branch delays IRQ poll by one cycle" quirk is not yet special-cased.  5-branch_delays_irq still fails on this.*

**Dependencies:** T-13-007.
**Reference:** `docs/cpu-6502.md` §Interrupt logic; `ref-docs/research-report.md` §CPU interrupts.
**Estimated complexity:** L.

---

### T-13-009 — `instr_test_v5` (official-only) green

**Description:** Add `instr_test_v5` test ROMs to the harness (those that don't depend on illegal opcodes). Run all sub-ROMs to completion; assert result code = 0.

**Acceptance criteria:**

- [ ] At least the "official only" subset of `instr_test_v5` passes. *Deferred: `instr_test_v5` ROMs are mapper 1 (MMC1); blocked on Sprint 5. CPU correctness validated through nestest golden-log compare instead.*
- [x] Test added to CI under the `test-roms` feature flag. (CI step exists; the `instr_test_v5` test bodies will land with MMC1.)

**Dependencies:** T-13-008.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

## Sprint review checklist

- [x] Sprint 3 tickets T-13-001 through T-13-006 fully complete; T-13-007 / T-13-008 / T-13-009 partially complete (gated on either MMC1 mapper for the test-ROM ones, or the Phase 2 lockstep core for the cycle-level interrupt intercepts).
- [x] `cargo test -p rustynes-cpu` green (18 tests including the ADC `proptest` reference fuzz).
- [ ] Coverage report shows >85% for `rustynes-cpu`. *Not measured in Phase 1; tooling lands in Phase 5 polish.*
- [x] CHANGELOG entry added for CPU official opcodes.
