# NES Test ROM Guide

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete inventory of test ROMs with expected results and failure interpretation

---

## Table of Contents

- [Overview](#overview)
- [Test ROM Categories](#test-rom-categories)
- [nestest.nes](#nestest)
- [Blargg Test Suite](#blargg-test-suite)
- [Accuracy Test Suite](#accuracy-test-suite)
- [PPU Test ROMs](#ppu-test-roms)
- [APU Test ROMs](#apu-test-roms)
- [Mapper Test ROMs](#mapper-test-roms)
- [Timing Test ROMs](#timing-test-roms)
- [Interpreting Failures](#interpreting-failures)
- [Test Automation](#test-automation)

---

## Overview

Test ROMs are essential for verifying NES emulator accuracy. They range from basic CPU instruction tests to complex timing validation.

### Test ROM Sources

- **NESdev Wiki:** [Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- **GitHub Collections:**
  - [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)
  - [bisqwit/nes_tests](https://bisqwit.iki.fi/src/nes_tests)
- **TASVideos:** [NES Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)

### Testing Strategy

1. **Start with nestest:** Validates basic CPU operation
2. **CPU instruction tests:** Comprehensive opcode coverage
3. **PPU tests:** Rendering accuracy
4. **APU tests:** Audio generation
5. **Integration tests:** CPU/PPU/APU interaction

---

## Test ROM Categories

### Essential (Must Pass for Basic Compatibility)

| Test | Component | Difficulty | Priority |
|------|-----------|------------|----------|
| nestest | CPU | Easy | P0 |
| instr_test-v5 | CPU | Easy | P0 |
| cpu_dummy_reads | CPU | Medium | P0 |
| vbl_nmi_timing | CPU/PPU | Medium | P0 |
| sprite_hit_tests_2005 | PPU | Hard | P1 |
| ppu_vbl_nmi | PPU | Medium | P1 |

### Comprehensive (Required for High Accuracy)

| Test | Component | Difficulty | Priority |
|------|-----------|------------|----------|
| blargg_ppu_tests_2005 | PPU | Hard | P1 |
| sprite_overflow_tests | PPU | Hard | P1 |
| apu_test | APU | Medium | P1 |
| dmc_tests | APU | Hard | P2 |
| cpu_interrupts_v2 | CPU | Hard | P1 |
| cpu_timing_test6 | CPU | Medium | P1 |

### Advanced (Edge Cases and Obscure Behavior)

| Test | Component | Difficulty | Priority |
|------|-----------|------------|----------|
| oam_read | PPU | Very Hard | P2 |
| oam_stress | PPU | Very Hard | P2 |
| full_palette | PPU | Medium | P2 |
| mmc3_test_2 | Mapper | Hard | P2 |
| exram | MMC5 | Very Hard | P3 |

---

## nestest

**File:** `nestest.nes`
**Author:** Kevin Horton
**Component:** CPU (all 151 official opcodes)
**Mapper:** NROM (000)

### Description

nestest is the **gold standard** CPU test. It validates:
- All official instructions
- Addressing modes
- Flag behavior
- Cycle timing (via golden log)

### Running Modes

#### Automation Mode (Recommended for Development)

```
Start PC: $C000 (ignore RESET vector)
Expected Result: $00 written to $6000 (pass)
                 Non-zero value = error code
```

**Automation mode advantages:**
- No PPU/APU required
- No controller input needed
- Fast execution (~30,000 cycles)

#### Interactive Mode

```
Start PC: From RESET vector ($FFFC)
Expected Result: On-screen menu showing test results
Requires: Basic PPU rendering
```

### Expected Results

**Success Indicators:**
- Automation: Address $6000 contains $00
- Interactive: All tests show "PASS" on screen

**Failure Codes:**
```
$01 = First test failed
$02 = Second test failed
...
```

### Golden Log Comparison

nestest includes a golden log (`nestest.log`) showing exact CPU state after each instruction:

```
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
C5F5  A2 00     LDX #$00                        A:00 X:00 Y:00 P:24 SP:FD CYC:10
C5F7  86 00     STX $00 = 00                    A:00 X:00 Y:00 P:26 SP:FD CYC:13
```

**Log Format:**
```
PC    BYTES     DISASM                          A:xx X:xx Y:xx P:xx SP:xx CYC:xxx
```

### Common nestest Failures

| Error | Likely Cause | Fix |
|-------|-------------|-----|
| Log diverges at ADC | Carry/overflow flag logic | Fix flag calculation |
| Log diverges at branch | Branch timing wrong | Check page crossing penalty |
| Log diverges at indexed | Address calculation error | Verify page crossing detection |
| Wrong cycle count | Timing error | Check cycle table |
| Different at first instruction | PC initialization wrong | Start at $C000 not $FFFC |

---

## Blargg Test Suite

**Author:** Shay Green (blargg)
**License:** Various (see individual tests)
**Quality:** Industry standard, extremely thorough

### instr_test-v5

**Component:** CPU instructions
**Subtests:** 16 individual ROM files

```
01-basics.nes        - Basic instructions (LDA, STA, etc.)
02-implied.nes       - Implied addressing (TAX, INX, etc.)
03-immediate.nes     - Immediate addressing
04-zero_page.nes     - Zero page addressing
05-zp_xy.nes         - Zero page,X and zero page,Y
06-absolute.nes      - Absolute addressing
07-abs_xy.nes        - Absolute,X and absolute,Y
08-ind_x.nes         - (Indirect,X) addressing
09-ind_y.nes         - (Indirect),Y addressing
10-branches.nes      - Branch instructions
11-stack.nes         - Stack operations (PHA, PLA, PHP, PLP)
12-jmp_jsr.nes       - JMP and JSR
13-rts.nes           - RTS
14-rti.nes           - RTI
15-brk.nes           - BRK
16-special.nes       - Special cases (page crossing, etc.)
```

**Expected Output:**
```
All tests passed
```

**Failure Example:**
```
T4 10 0F
```
- T4: Test 4 failed
- 10 0F: Error code (see source for details)

### cpu_dummy_reads

**Component:** CPU dummy read behavior
**Tests:** Indexed addressing dummy reads

**What it tests:**
- Zero page,X/Y: Reads from base address before adding index
- Absolute,X/Y: Reads from incorrect page on page crossing
- (Indirect),Y: Reads from incorrect address on page crossing

**Expected Output:** "Passed"

### cpu_interrupts_v2

**Component:** Interrupt timing and behavior

**Subtests:**
```
1-cli_latency.nes        - CLI/SEI timing
2-nmi_and_brk.nes        - NMI vs BRK interaction
3-nmi_and_irq.nes        - NMI vs IRQ priority
4-irq_and_dma.nes        - IRQ during DMA
5-branch_delays_irq.nes  - Branch instruction IRQ timing
```

**Expected Output:** "Passed" for each test

### cpu_timing_test6

**Component:** Per-instruction cycle timing

**Tests:** Every instruction's cycle count including page crossing penalties

**Expected Output:** All tests passed

---

## Accuracy Test Suite

### vbl_nmi_timing

**Author:** blargg
**Component:** VBlank and NMI timing

**Subtests:**
```
1-frame_basics.nes    - Basic frame timing
2-vbl_timing.nes      - VBlank flag timing
3-even_odd_frames.nes - Odd frame skip
4-vbl_clear_timing.nes - VBlank flag clear timing
5-nmi_suppression.nes - NMI suppression edge case
6-nmi_disable.nes     - NMI enable/disable timing
7-nmi_timing.nes      - Exact NMI timing
```

**Expected Output:** "Passed" for each

**Critical for:** Mid-frame PPU register writes, sprite 0 hit timing

---

## PPU Test ROMs

### sprite_hit_tests_2005

**Author:** Quietust
**Component:** Sprite 0 hit detection

**Subtests:**
```
01-basics.nes          - Basic sprite 0 hit
02-alignment.nes       - Pixel-perfect alignment
03-corners.nes         - Edge cases (corners, clipping)
04-flip.nes            - Horizontal/vertical flip
05-left_clip.nes       - Left 8-pixel clipping
06-right_edge.nes      - Right edge of screen
07-screen_bottom.nes   - Bottom scanline
08-double_height.nes   - 8×16 sprites
09-timing.nes          - Exact cycle timing
10-timing_order.nes    - Multiple sprites
```

**Expected Output:** "Passed" for each

**Difficulty:** Very hard (requires cycle-accurate PPU)

### sprite_overflow_tests

**Author:** Quietust
**Component:** Sprite overflow bug emulation

**What it tests:**
- Secondary OAM overflow behavior
- Hardware bug in sprite evaluation
- Overflow flag timing

**Expected Output:** All tests pass

### ppu_vbl_nmi

**Author:** blargg
**Component:** PPU VBlank and NMI

**Subtests:**
```
01-vbl_basics.nes      - VBlank flag basics
02-vbl_set_time.nes    - Exact VBlank set timing
03-vbl_clear_time.nes  - VBlank clear timing
04-nmi_control.nes     - NMI enable/disable
05-nmi_timing.nes      - NMI trigger timing
06-suppression.nes     - NMI suppression
07-nmi_on_timing.nes   - Enabling NMI during VBlank
08-nmi_off_timing.nes  - Disabling NMI during VBlank
09-even_odd_frames.nes - Frame length variation
10-even_odd_timing.nes - Odd frame skip timing
```

**Expected Output:** "Passed" for each

### blargg_ppu_tests_2005

**Author:** blargg
**Component:** PPU rendering details

**Subtests:**
```
palette_ram.nes        - Palette RAM behavior
sprite_ram.nes         - OAM behavior
vbl_clear_time.nes     - VBlank clear timing
vram_access.nes        - VRAM access during rendering
```

**Expected Output:** "Passed" for each

---

## APU Test ROMs

### apu_test

**Author:** blargg
**Component:** APU channels and frame counter

**Subtests:**
```
01-len_ctr.nes         - Length counter
02-len_table.nes       - Length counter table
03-irq_flag.nes        - IRQ flag
04-jitter.nes          - Frame counter jitter
05-len_timing.nes      - Length counter timing
06-irq_flag_timing.nes - IRQ flag timing
07-dmc_basics.nes      - DMC basics
08-dmc_rates.nes       - DMC sample rates
```

**Expected Output:** "Passed" for each

### apu_mixer

**Author:** blargg
**Component:** APU channel mixing

**Tests:** Correct volume levels and mixing for all channels

**Expected Output:** "Passed"

### dmc_tests

**Author:** Quietust
**Component:** DMC (Delta Modulation Channel)

**Subtests:**
```
01-basics.nes          - DMC basics
02-loop.nes            - Loop flag behavior
03-irq.nes             - DMC IRQ
04-rates.nes           - Sample rate accuracy
05-dma.nes             - DMA timing
06-odd_even.nes        - Odd/even cycle behavior
```

**Expected Output:** All tests pass

**Difficulty:** Very hard (requires cycle-accurate DMA)

---

## Mapper Test ROMs

### mmc3_test

**Component:** MMC3 mapper (004)

**Subtests:**
```
1-clocking.nes         - IRQ counter clocking
2-details.nes          - IRQ counter details
3-A12_clocking.nes     - A12 edge detection
4-scanline_timing.nes  - Scanline IRQ timing
5-MMC3_rev_A.nes       - Revision A behavior
6-MMC3_rev_B.nes       - Revision B behavior
```

**Expected Output:** "Passed" for each

**Critical for:** Most commercial games

### mmc5_test

**Component:** MMC5 mapper (005)

**Tests:** Banking, ExRAM, expansion audio, split screen

**Difficulty:** Very hard (MMC5 is extremely complex)

---

## Timing Test ROMs

### ppu_read_buffer

**Component:** PPU read buffer behavior

**Tests:** $2007 buffered reads, palette read exception

**Expected Output:** "Passed"

### oam_read

**Component:** OAM read behavior

**Tests:** $2004 reads during rendering

**Expected Output:** All tests pass

**Difficulty:** Very hard (requires accurate OAM fetch emulation)

### oam_stress

**Component:** OAM corruption during rendering

**Tests:** Writes to $2003/$2004 during rendering

**Expected Output:** All tests pass

**Difficulty:** Very hard

---

## Interpreting Failures

### Common Failure Patterns

#### "T1 02 04" Error Format

```
T[test_num] [byte1] [byte2] ...
```

Check test source code for error code meanings.

#### On-Screen Text Output

Many tests output text to screen:
```
"Passed" = Success
Error code = Failure (check source)
```

#### Address $6000 Status Codes

Some tests write result codes to $6000:
```
$00 = Success
$01-$FF = Error codes (test-specific)
```

### Debugging Failed Tests

1. **Identify failed test:** Check test name and error code
2. **Read test documentation:** Often in ROM archive
3. **Compare with golden log:** If available (nestest)
4. **Check test source:** Available for blargg tests
5. **Isolate component:** CPU vs PPU vs APU vs timing

### Test Dependencies

Some tests require prior tests to pass:

```
instr_test-v5: Must pass in order (01 before 02, etc.)
ppu_vbl_nmi: Requires accurate CPU timing
sprite_hit: Requires accurate PPU rendering
```

---

## Test Automation

### Automated Test Runner

```rust
pub fn run_test_rom(rom_path: &str) -> TestResult {
    let mut nes = Nes::new();
    nes.load_rom(rom_path)?;

    // For nestest automation mode
    nes.cpu.pc = 0xC000;

    // Run for max cycles
    for _ in 0..100_000_000 {
        nes.step();

        // Check for completion
        if let Some(result) = check_test_result(&nes) {
            return result;
        }

        // Timeout after ~30 seconds
        if nes.cpu.cycles > 50_000_000 {
            return TestResult::Timeout;
        }
    }

    TestResult::Timeout
}

fn check_test_result(nes: &Nes) -> Option<TestResult> {
    // Check $6000 for status
    let status = nes.bus.read(0x6000);

    match status {
        0x00 => Some(TestResult::Pass),
        0x01..=0xFF => Some(TestResult::Fail(status)),
        _ => None,
    }
}
```

### Log Comparison

```rust
pub fn compare_with_golden_log(emulator_log: &str, golden_log: &str) -> Result<(), LogDiff> {
    let emu_lines: Vec<&str> = emulator_log.lines().collect();
    let gold_lines: Vec<&str> = golden_log.lines().collect();

    for (i, (emu, gold)) in emu_lines.iter().zip(gold_lines.iter()).enumerate() {
        if emu != gold {
            return Err(LogDiff {
                line: i + 1,
                expected: gold.to_string(),
                actual: emu.to_string(),
            });
        }
    }

    Ok(())
}
```

---

## Related Documentation

- [NESTEST_GOLDEN_LOG.md](NESTEST_GOLDEN_LOG.md) - nestest golden log format
- [BLARGG_TEST_MATRIX.md](BLARGG_TEST_MATRIX.md) - Complete blargg test results
- [ACCURACY_VALIDATION.md](ACCURACY_VALIDATION.md) - TASVideos suite methodology
- [../dev/TESTING.md](../dev/TESTING.md) - Overall testing strategy

---

## References

- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [TASVideos: NES Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)
- [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)
- [blargg test ROMs documentation](https://github.com/christopherpow/nes-test-roms)
- Nintendulator test logs

---

**Document Status:** Complete test ROM inventory with expected results and failure interpretation.
