# M8 Sprint 2: Blargg CPU Tests

## Overview

Systematically pass the Blargg CPU instruction test suite (14 tests) to validate instruction timing, addressing modes, and edge case handling.

## Objectives

- [ ] Pass all 14 Blargg CPU tests
- [ ] Verify instruction timing accuracy
- [ ] Validate addressing mode edge cases
- [ ] Test page boundary crossing behavior
- [ ] Ensure zero regressions

## Tasks

### Task 1: Blargg Instruction Tests
- [ ] Run instr_test-v5/all_instrs.nes (comprehensive instruction test)
- [ ] Debug failures (identify incorrect instruction behavior)
- [ ] Test official_only.nes (151 official opcodes)
- [ ] Verify implied, immediate, zero page, absolute addressing modes
- [ ] Test indexed modes (zp,X; zp,Y; abs,X; abs,Y; ind,X; ind,Y)

### Task 2: Instruction Timing
- [ ] Run instr_timing/instr_timing.nes (overall timing)
- [ ] Test 1-instr_timing.nes (individual instruction timing)
- [ ] Test 2-branch_timing.nes (branch timing edge cases)
- [ ] Verify cycle counts match expected values
- [ ] Debug slow/fast instructions

### Task 3: Memory Access Tests
- [ ] Run instr_misc/instr_misc.nes (miscellaneous edge cases)
- [ ] Test 03-dummy_reads.nes (dummy read cycles)
- [ ] Test 04-dummy_reads_apu.nes (APU dummy reads)
- [ ] Verify RMW instruction behavior (INC, DEC, ASL, LSR, ROL, ROR)
- [ ] Test absolute indexed with page crossing

### Task 4: ROM Singles
- [ ] Run rom_singles/01-basics.nes
- [ ] Run rom_singles/02-implied.nes
- [ ] Run rom_singles/03-immediate.nes
- [ ] Run rom_singles/04-zero_page.nes
- [ ] Run rom_singles/05-zp_xy.nes
- [ ] Run rom_singles/06-absolute.nes
- [ ] Run rom_singles/07-abs_xy.nes
- [ ] Run rom_singles/08-ind_x.nes
- [ ] Run rom_singles/09-ind_y.nes
- [ ] Run rom_singles/10-branches.nes
- [ ] Run rom_singles/11-stack.nes
- [ ] Run rom_singles/12-jmp_jsr.nes
- [ ] Run rom_singles/13-rts.nes
- [ ] Run rom_singles/14-rti.nes
- [ ] Run rom_singles/15-brk.nes
- [ ] Run rom_singles/16-special.nes

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| instr_test-v5/all_instrs.nes | [ ] Pending | Comprehensive instruction test |
| instr_test-v5/official_only.nes | [ ] Pending | 151 official opcodes |
| instr_timing/instr_timing.nes | [ ] Pending | Overall timing |
| instr_timing/1-instr_timing.nes | [ ] Pending | Individual instruction timing |
| instr_timing/2-branch_timing.nes | [ ] Pending | Branch timing |
| instr_misc/instr_misc.nes | [ ] Pending | Miscellaneous edge cases |
| instr_misc/03-dummy_reads.nes | [ ] Pending | Dummy read cycles |
| instr_misc/04-dummy_reads_apu.nes | [ ] Pending | APU dummy reads |
| rom_singles/01-basics.nes | [ ] Pending | Basic instructions |
| rom_singles/02-implied.nes | [ ] Pending | Implied addressing |
| rom_singles/03-immediate.nes | [ ] Pending | Immediate addressing |
| rom_singles/04-zero_page.nes | [ ] Pending | Zero page addressing |
| rom_singles/05-zp_xy.nes | [ ] Pending | Zero page indexed (X/Y) |
| rom_singles/06-absolute.nes | [ ] Pending | Absolute addressing |
| rom_singles/07-abs_xy.nes | [ ] Pending | Absolute indexed (X/Y) |
| rom_singles/08-ind_x.nes | [ ] Pending | Indirect X addressing |
| rom_singles/09-ind_y.nes | [ ] Pending | Indirect Y addressing |
| rom_singles/10-branches.nes | [ ] Pending | Branch instructions |
| rom_singles/11-stack.nes | [ ] Pending | Stack operations |
| rom_singles/12-jmp_jsr.nes | [ ] Pending | JMP/JSR instructions |
| rom_singles/13-rts.nes | [ ] Pending | RTS instruction |
| rom_singles/14-rti.nes | [ ] Pending | RTI instruction |
| rom_singles/15-brk.nes | [ ] Pending | BRK instruction |
| rom_singles/16-special.nes | [ ] Pending | Special cases |

## Acceptance Criteria

- [ ] All 14 Blargg CPU tests passing
- [ ] Instruction timing verified (±1 cycle accuracy)
- [ ] All addressing modes validated
- [ ] Page boundary crossing behavior correct
- [ ] RMW dummy read/write cycles accurate
- [ ] Zero regressions from nestest.nes baseline
- [ ] All ROM singles (01-16) passing

## Debugging Strategy

1. **Identify Failure:**
   - Run test ROM, capture error code
   - Cross-reference error code with test source

2. **Isolate Instruction:**
   - Determine which instruction(s) failing
   - Review instruction implementation

3. **Trace Execution:**
   - Enable CPU trace logging
   - Compare against expected behavior

4. **Fix & Verify:**
   - Implement fix
   - Verify fix doesn't break other tests
   - Run full test suite

## Version Target

v0.7.0
