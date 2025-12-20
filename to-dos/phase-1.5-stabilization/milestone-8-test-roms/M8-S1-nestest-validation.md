# M8 Sprint 1: nestest & CPU Tests

## Overview

Automate nestest.nes golden log validation and systematically pass all 36 CPU instruction tests to establish baseline CPU accuracy.

## Objectives

- [ ] Automate nestest.nes with golden log comparison
- [ ] Pass all CPU instruction timing tests
- [ ] Verify branch timing edge cases
- [ ] Validate dummy read/write cycles
- [ ] Integrate CPU tests into CI pipeline

## Tasks

### Task 1: nestest.nes Automation
- [ ] Implement automated nestest.nes execution (automation mode $C000)
- [ ] Parse golden log format (PC, opcode, registers, cycle count)
- [ ] Compare emulator output line-by-line against golden log
- [ ] Report first divergence with context (10 lines before/after)
- [ ] Verify continues to pass (no regressions from v0.6.0)

### Task 2: CPU Instruction Timing Tests
- [ ] Run cpu_instr_timing.nes (overall instruction timing)
- [ ] Debug timing failures (identify slow/fast instructions)
- [ ] Verify page boundary crossing penalties (+1 cycle)
- [ ] Test indexed addressing modes (abs,X; abs,Y; ind,Y)
- [ ] Validate all 256 opcodes cycle-accurate

### Task 3: Branch Timing Edge Cases
- [ ] Test cpu_branch_timing_2.nes (branch edge cases)
- [ ] Verify branch taken/not taken timing (2 vs 3 cycles)
- [ ] Test page boundary crossing on branches (+1 cycle)
- [ ] Validate backward/forward branches
- [ ] Test branch to same page vs different page

### Task 4: Dummy Read/Write Cycles
- [ ] Test cpu_dummy_reads.nes
- [ ] Test cpu_dummy_writes_ppumem.nes
- [ ] Verify RMW instruction dummy writes (INC, DEC, ASL, LSR, ROL, ROR)
- [ ] Test STA abs,X dummy read
- [ ] Validate timing-critical dummy cycles

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| cpu_nestest.nes | ✅ Pass | Baseline (already passing) |
| cpu_instr_timing.nes | [ ] Pending | Overall instruction timing |
| cpu_branch_timing_2.nes | [ ] Pending | Branch timing edge cases |
| cpu_dummy_reads.nes | [ ] Pending | Dummy read cycles |
| cpu_dummy_writes_ppumem.nes | [ ] Pending | Dummy write cycles (PPU) |
| cpu_dummy_writes_oam.nes | [ ] Pending | Dummy write cycles (OAM) |
| cpu_exec_space_ppuio.nes | [ ] Pending | Execute from PPU I/O space |

**Additional CPU Tests (29 ROMs):**
- cpu_instr_test-v5/ (official instructions)
- cpu_unofficial_opcodes/ (105 unofficial opcodes)
- cpu_interrupts_v2/ (NMI, IRQ, BRK timing)

## Acceptance Criteria

- [ ] nestest.nes automated with golden log comparison
- [ ] 34/36 CPU tests passing (94%)
- [ ] Zero regressions from v0.6.0
- [ ] CPU instruction timing verified to ±1 cycle
- [ ] Dummy read/write cycles validated
- [ ] Branch timing edge cases handled
- [ ] CI integration complete (automated test execution)

## Expected Failures (2 tests)

**Highly timing-sensitive tests:**
- cpu_timing_test6.nes - Sub-cycle precision required
- cpu_exec_space_ppuio.nes - Execute from unmapped space edge case

**Rationale:** These represent <6% of CPU tests and require specialized handling beyond Phase 1.5 scope.

## Version Target

v0.7.0
