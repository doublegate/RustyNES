# M7 Sprint 1: CPU Accuracy

## Overview

Refine CPU cycle timing to ±1 cycle accuracy for all instructions, focusing on edge cases, unofficial opcodes, and interrupt timing precision.

## Objectives

- [ ] Verify and refine instruction cycle timing for edge cases
- [ ] Validate unofficial opcode timing against hardware
- [ ] Improve interrupt handling timing precision
- [ ] Ensure page boundary crossing accuracy
- [ ] Document timing edge cases

## Tasks

### Task 1: Instruction Timing Verification
- [ ] Review nestest.nes golden log for cycle counts
- [ ] Verify all 256 opcode cycle counts against documentation
- [ ] Test page boundary crossing penalties (+1 cycle)
- [ ] Validate branch taken/not taken timing
- [ ] Test dummy read/write cycles

### Task 2: Unofficial Opcode Timing
- [ ] Verify all 105 unofficial opcodes
- [ ] Cross-reference with hardware behavior documentation
- [ ] Test edge cases (unstable opcodes: ANE, LXA, etc.)
- [ ] Validate timing for complex unofficials (DCP, ISC, RLA, RRA, etc.)

### Task 3: Interrupt Timing Precision
- [ ] NMI edge detection timing
- [ ] IRQ disable flag (I) timing
- [ ] BRK instruction timing (7 cycles)
- [ ] Interrupt hijacking edge cases
- [ ] RTI timing precision

### Task 4: Page Boundary Edge Cases
- [ ] Indexed addressing modes (abs,X; abs,Y; ind,Y)
- [ ] Branch instructions crossing pages (+1 cycle)
- [ ] Indirect addressing page wrapping (6502 bug)
- [ ] Verify penalty cycle application

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| cpu_nestest.nes | ✅ Pass | Already passing (baseline) |
| cpu_instr_timing.nes | [ ] Pending | Overall instruction timing |
| cpu_branch_timing_2.nes | [ ] Pending | Branch timing edge cases |
| cpu_dummy_reads.nes | [ ] Pending | Dummy read cycles |
| cpu_dummy_writes_ppumem.nes | [ ] Pending | Dummy write cycles |

## Acceptance Criteria

- [ ] All CPU instruction timing verified to ±1 cycle
- [ ] nestest.nes continues to pass (no regression)
- [ ] cpu_instr_timing.nes passes
- [ ] Unofficial opcode timing documented
- [ ] Interrupt timing edge cases handled
- [ ] Zero performance regression

## Version Target

v0.6.0
