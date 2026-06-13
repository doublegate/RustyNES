# Sprint 4 — CPU core: unofficial opcodes + nestest

**Phase:** Phase 1 — Foundation
**Sprint goal:** Implement the 105 unofficial / illegal 6502 opcodes; pass nestest's golden-log comparison and the full `instr_test_v5` suite.
**Estimated duration:** 1-2 weeks

## Tickets

### T-14-001 — Stable unofficial opcodes (LAX, SAX, DCP, ISC, RLA, SLO, SRE, RRA, ANC, ALR, ARR, AXS, NOPs)

**Description:** Implement the well-defined unofficial opcodes. Each behaves as the documented combination of legal opcodes (e.g., DCP = DEC then CMP).

**Acceptance criteria:**
- [x] Each opcode dispatches and executes per `nes-test-roms/other/nestest.txt`. Verified via the nestest golden-log compare in T-14-005.
- [x] Cycle counts match nestest expectations. Verified via the same.
- [x] Unit tests per opcode (representative coverage; full DCP / ISC / RLA / SLO / SRE / RRA fanout validated through nestest).

**Dependencies:** Sprint 3 complete.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** L.

---

### T-14-002 — Unstable unofficial opcodes (XAA, LAS, TAS, SHA, SHX, SHY)

**Description:** Implement the genuinely-unstable opcodes per nestest's expected behavior. Document deviations from real-hardware behavior in code comments.

**Acceptance criteria:**
- [x] XAA implements `A := (A | const) & X & operand` per nestest expectation.
- [x] SHX, SHY, SHA implement the documented "AND with high byte + 1" behavior.
- [x] LAS implements the documented `A := X := S := S & operand`.
- [x] Unit tests where determinism allows. (XAA / LAS / SHA / SHX / SHY are exercised through nestest. The dedicated unit tests live with the rest of the CPU integration tests in `crates/rustynes-cpu/tests/opcodes.rs`.)

**Dependencies:** T-14-001.
**Reference:** `docs/cpu-6502.md` §Open questions.
**Estimated complexity:** M.

---

### T-14-003 — JAM / KIL / STP halt opcodes

**Description:** Implement the opcodes that lock the CPU as a graceful halt. The CPU surfaces a `Cpu::is_jammed()` query; subsequent `tick()` calls do nothing until reset.

**Acceptance criteria:**
- [x] All 12 jam opcodes recognized.
- [x] CPU reports `is_jammed()` after executing one.
- [x] Reset clears jam state.

**Dependencies:** T-14-002.
**Reference:** `docs/cpu-6502.md` §Instruction set.
**Estimated complexity:** S.

---

### T-14-004 — Nestest golden-log harness

**Description:** Build the test that runs nestest with PC=$C000, captures `(PC, A, X, Y, P, SP, CYC, PPU dot, scanline)` after each instruction, and diffs against `nestest.log`.

**Acceptance criteria:**
- [x] Harness lives in `rustynes-test-harness`.
- [x] First-mismatch reporting prints expected vs. actual line + the diverging field.
- [x] Test runs as `cargo test -p rustynes-test-harness --features test-roms --test nestest`.

**Dependencies:** T-14-003.
**Reference:** `docs/testing-strategy.md` §Layer 2.
**Estimated complexity:** M.

---

### T-14-005 — Nestest passes

**Description:** Iterate on CPU bugs surfaced by nestest until the golden-log diff is empty.

**Acceptance criteria:**
- [x] Nestest test passes (zero diff against `nestest.log`; 8991 instructions compared).
- [x] No remaining `TODO` markers in CPU code for nestest-required behavior.

**Dependencies:** T-14-004.
**Reference:** `docs/testing-strategy.md` §Layer 2.
**Estimated complexity:** L.

---

### T-14-006 — Full `instr_test_v5` (official + unofficial) passes

**Description:** All 16 sub-ROMs of `instr_test_v5` pass with status code 0. Including illegal-opcode sub-ROMs.

**Acceptance criteria:**
- [x] All sub-ROMs reach completion (status `$00` at `$6000`). 16 sub-ROMs (NROM) + 2 aggregates (`all_instrs.nes` and `official_only.nes`, both MMC1) all pass after MMC1 landed in Phase-2 Sprint 4 / Checkpoint 1. The aggregates exercise the MMC1 consecutive-write bug.
- [x] All return result code 0.

**Dependencies:** T-14-005.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

### T-14-007 — `cpu_timing_test6` passes

**Description:** Cycle-accurate instruction timing for all official + unofficial opcodes (excluding branches).

**Acceptance criteria:**
- [ ] Test ROM completes with result code 0. *Partial: harness boots and runs the ROM cleanly; reaching the result-code report path requires the Phase 2 PPU lockstep (the ROM polls the PPU `$2002` VBlank flag to time its messages).*

**Dependencies:** T-14-006.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** M.

---

### T-14-008 — `branch_timing_tests` passes

**Description:** Branch instruction timing including page-crossing penalties.

**Acceptance criteria:**
- [ ] All 3 branch_timing sub-ROMs pass. *Partial: same gate as T-14-007 — the ROMs boot and run, but reaching the result code requires the Phase 2 PPU. The branch-cycle math itself is validated by the nestest golden log (which exercises taken / untaken / page-crossed branches with byte-exact CYC counts).*

**Dependencies:** T-14-007.
**Reference:** `docs/testing-strategy.md` §Layer 3.
**Estimated complexity:** S.

---

## Sprint review checklist

- [x] T-14-001 through T-14-005 fully complete; T-14-006/007/008 deferred (MMC1 gate or Phase-2 PPU gate as documented above).
- [x] CPU is feature-complete for the Phase-1 scope: 151 official + 105 unofficial + 12 JAM + NMI/IRQ/BRK + JMP indirect page-bug. Cycle-level interrupt intercepts (NMI hijacking ticks 1-4, branch-cycle IRQ poll quirks) land with the Phase-2 lockstep `tick()`.
- [ ] Tag a `v0.1.0-cpu` milestone for clear progress signaling. *Deferred to the parent's commit.*
- [x] CHANGELOG entry: "CPU core nestest-passing; instr_test_v5 acquired but blocked on MMC1; cycle timing validated through nestest."
