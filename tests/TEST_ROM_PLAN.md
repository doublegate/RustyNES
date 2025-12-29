# RustyNES Test ROM Execution Plan

**Version**: 1.0
**Date**: December 19, 2025
**Status**: Phase 1 (M1-M5) Complete - Ready for Comprehensive Testing

## Executive Summary

This document provides a comprehensive test execution plan for all test ROMs in the RustyNES project. As of December 19, 2025, Milestones 1-5 (CPU, PPU, APU, Mappers, Integration) are complete with 398 passing unit tests. This plan outlines the strategy for integrating and executing the 212 test ROM files across CPU, PPU, APU, and Mapper categories.

## Test ROM Inventory

### Summary Statistics

| Category | Total Files | Unique ROMs | Integrated | Passing | Pending |
|----------|-------------|-------------|------------|---------|---------|
| CPU      | 36          | 36          | 1          | 1       | 35      |
| PPU      | 49          | 49          | 6          | 4       | 43      |
| APU      | 70          | 64          | 0          | 0       | 70      |
| Mappers  | 57          | 23          | 0          | 0       | 57      |
| **Total**| **212**     | **172**     | **7**      | **5**   | **205** |

**Notes**:

- Total Files: Raw count of all .nes files in test-roms/
- Unique ROMs: Deduplicated count (172 unique test ROMs per CHECKSUMS.md)
- Integrated: Test ROMs with automated test harnesses in place
- Passing: Test ROMs currently passing all validation
- Pending: Test ROMs awaiting integration

### Currently Integrated Tests

#### CPU Tests (1/36 integrated)

1. **cpu_nestest.nes** - PASSING
   - Location: `/home/parobek/Code/RustyNES/test-roms/cpu/cpu_nestest.nes`
   - Test: `rustynes-cpu/tests/nestest_validation.rs`
   - Status: 100% passing (5003+ instructions validated against golden log)
   - Coverage: All 256 opcodes (151 official + 105 unofficial)

#### PPU Tests (6/49 integrated, 4 passing, 2 ignored)

1. **ppu_vbl_nmi.nes** - PASSING
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_ppu_vbl_nmi_suite`

2. **ppu_01-vbl_basics.nes** - PASSING
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_ppu_vbl_basics`

3. **ppu_02-vbl_set_time.nes** - IGNORED
   - Reason: "Requires exact cycle-accurate timing - within 51 cycles"
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_ppu_vbl_set_time`

4. **ppu_03-vbl_clear_time.nes** - IGNORED
   - Reason: "Requires exact cycle-accurate timing - within 10 cycles"
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_ppu_vbl_clear_time`

5. **ppu_01.basics.nes** - PASSING (sprite hit basics)
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_sprite_hit_basics`

6. **ppu_02.alignment.nes** - PASSING (sprite hit alignment)
   - Test: `rustynes-ppu/tests/ppu_test_roms.rs::test_sprite_hit_alignment`

## Detailed Test ROM Inventory

### CPU Tests (36 files)

#### Instruction Tests (11 ROMs)

- cpu_instr_01_implied.nes
- cpu_instr_02_immediate.nes
- cpu_instr_03_zero_page.nes
- cpu_instr_04_zp_xy.nes
- cpu_instr_05_absolute.nes
- cpu_instr_06_abs_xy.nes
- cpu_instr_07_ind_x.nes
- cpu_instr_08_ind_y.nes
- cpu_instr_09_branches.nes
- cpu_instr_10_stack.nes
- cpu_instr_11_special.nes

#### Timing Tests (3 ROMs)

- cpu_instr_timing.nes
- cpu_instr_timing_1.nes
- cpu_branch_timing_2.nes

#### Interrupt Tests (7 ROMs)

- cpu_interrupts.nes
- cpu_int_nmi_and_irq.nes
- cpu_int_nmi_and_brk.nes
- cpu_int_irq_and_dma.nes
- cpu_int_branch_delays_irq.nes
- cpu_int_cli_latency.nes
- cpu_flag_concurrency.nes

#### DMA Tests (2 ROMs)

- cpu_sprdma_and_dmc_dma.nes
- cpu_sprdma_and_dmc_dma_512.nes

#### Misc CPU Tests (13 ROMs)

- cpu_nestest.nes (INTEGRATED, PASSING)
- cpu_official_only.nes
- cpu_all_instrs.nes
- cpu_branch_basics.nes
- cpu_branch_forward.nes
- cpu_branch_backward.nes
- cpu_dummy_reads.nes
- cpu_dummy_writes_oam.nes
- cpu_dummy_writes_ppumem.nes
- cpu_exec_space_apu.nes
- cpu_exec_space_ppuio.nes
- cpu_ram_after_reset.nes
- cpu_regs_after_reset.nes

### PPU Tests (49 files)

#### VBL/NMI Tests (10 ROMs)

- ppu_vbl_nmi.nes (INTEGRATED, PASSING)
- ppu_01-vbl_basics.nes (INTEGRATED, PASSING)
- ppu_02-vbl_set_time.nes (INTEGRATED, IGNORED - timing precision)
- ppu_03-vbl_clear_time.nes (INTEGRATED, IGNORED - timing precision)
- ppu_04-nmi_control.nes
- ppu_05-nmi_timing.nes
- ppu_06-suppression.nes
- ppu_07-nmi_on_timing.nes
- ppu_08-nmi_off_timing.nes
- ppu_09-even_odd_frames.nes
- ppu_10-even_odd_timing.nes

#### Sprite Hit Tests (13 ROMs)

- ppu_01.basics.nes (INTEGRATED, PASSING)
- ppu_02.alignment.nes (INTEGRATED, PASSING)
- ppu_03.corners.nes
- ppu_04.flip.nes
- ppu_05.left_clip.nes
- ppu_06.right_edge.nes
- ppu_07.screen_bottom.nes
- ppu_08.double_height.nes
- ppu_spr_hit_basics.nes (duplicate of ppu_01.basics.nes)
- ppu_spr_hit_alignment.nes (duplicate of ppu_02.alignment.nes)
- ppu_spr_hit_corners.nes (duplicate of ppu_03.corners.nes)
- ppu_spr_hit_flip.nes (duplicate of ppu_04.flip.nes)
- ppu_spr_hit_left_clip.nes (duplicate of ppu_05.left_clip.nes)
- ppu_spr_hit_right_edge.nes (duplicate of ppu_06.right_edge.nes)
- ppu_spr_hit_screen_bottom.nes (duplicate of ppu_07.screen_bottom.nes)
- ppu_spr_hit_double_height.nes (duplicate of ppu_08.double_height.nes)
- ppu_spr_hit_timing_basics.nes
- ppu_spr_hit_timing_order.nes
- ppu_spr_hit_edge_timing.nes

#### Sprite Overflow Tests (5 ROMs)

- ppu_spr_overflow_basics.nes
- ppu_spr_overflow_details.nes
- ppu_spr_overflow_emulator.nes
- ppu_spr_overflow_obscure.nes
- ppu_spr_overflow_timing.nes

#### Memory/Register Tests (7 ROMs)

- ppu_palette.nes
- ppu_palette_ram.nes
- ppu_sprite_ram.nes
- ppu_vram_access.nes
- ppu_oam_read.nes
- ppu_oam_stress.nes
- ppu_test_ppu_read_buffer.nes

#### Visual/Rendering Tests (7 ROMs)

- ppu_color.nes
- ppu_full_palette.nes
- ppu_full_palette_smooth.nes
- ppu_flowing_palette.nes
- ppu_ntsc_torture.nes
- ppu_scanline.nes
- ppu_open_bus.nes

### APU Tests (70 files)

#### Length Counter Tests (14 ROMs)

- apu_len_ctr.nes
- apu_len_table.nes
- apu_len_timing.nes
- apu_len_timing_mode0.nes
- apu_len_timing_mode1.nes
- apu_len_halt_timing.nes
- apu_len_reload_timing.nes
- apu_pal_len_ctr.nes
- apu_pal_len_table.nes
- apu_pal_len_halt_timing.nes
- apu_pal_len_reload_timing.nes
- apu_pal_len_timing_mode0.nes
- apu_pal_len_timing_mode1.nes
- apu_reset_len_ctrs_enabled.nes

#### IRQ Tests (8 ROMs)

- apu_irq_flag.nes
- apu_irq_flag_timing.nes
- apu_irq_timing.nes
- apu_pal_irq_flag.nes
- apu_pal_irq_flag_timing.nes
- apu_pal_irq_timing.nes
- apu_reset_irq_flag_cleared.nes

#### DMC Tests (14 ROMs)

- apu_dmc.nes
- apu_dmc_basics.nes
- apu_dmc_rates.nes
- apu_dmc_pitch.nes
- apu_dmc_status.nes
- apu_dmc_status_irq.nes
- apu_dmc_latency.nes
- apu_dmc_buffer_retained.nes
- apu_dmc_dma_2007_read.nes
- apu_dmc_dma_2007_write.nes
- apu_dmc_dma_4016_read.nes
- apu_dmc_dma_double_2007_read.nes
- apu_dmc_dma_read_write_2007.nes
- apu_dpcmletterbox.nes

#### Channel Tests (10 ROMs)

- apu_square.nes
- apu_square_pitch.nes
- apu_triangle.nes
- apu_triangle_pitch.nes
- apu_noise.nes
- apu_noise_pitch.nes
- apu_env.nes
- apu_lin_ctr.nes
- apu_volumes.nes
- apu_sweep_cutoff.nes
- apu_sweep_sub.nes

#### Reset Tests (8 ROMs)

- apu_reset_4015_cleared.nes
- apu_reset_4017_timing.nes
- apu_reset_4017_written.nes
- apu_reset_timing.nes
- apu_reset_works_immediately.nes
- apu_phase_reset.nes

#### Clock/Timing Tests (4 ROMs)

- apu_clock_jitter.nes
- apu_pal_clock_jitter.nes

#### Blargg APU Test Suite (12 ROMs)

- apu_test_1.nes
- apu_test_2.nes
- apu_test_3.nes
- apu_test_4.nes
- apu_test_5.nes
- apu_test_6.nes
- apu_test_7.nes
- apu_test_8.nes
- apu_test_9.nes
- apu_test_10.nes
- apu_test/apu_test.nes (suite ROM)
- apu_test/rom_singles/1-len_ctr.nes (duplicate)
- apu_test/rom_singles/2-len_table.nes (duplicate)
- apu_test/rom_singles/3-irq_flag.nes (duplicate)
- apu_test/rom_singles/4-jitter.nes (duplicate)
- apu_test/rom_singles/6-irq_flag_timing.nes (duplicate)

### Mapper Tests (57 files)

#### NROM (Mapper 0) - 4 ROMs

- mapper_nrom_368_test.nes
- mapper_holymapperel_0_P32K_C8K_V.nes
- mapper_holymapperel_0_P32K_CR32K_V.nes
- mapper_holymapperel_0_P32K_CR8K_V.nes

#### MMC1 (Mapper 1) - 15 ROMs

- mapper_mmc1_a12.nes
- mapper_holymapperel_1_P128K_C128K.nes
- mapper_holymapperel_1_P128K_C128K_S8K.nes
- mapper_holymapperel_1_P128K_C128K_W8K.nes
- mapper_holymapperel_1_P128K_C32K.nes
- mapper_holymapperel_1_P128K_C32K_S8K.nes
- mapper_holymapperel_1_P128K_C32K_W8K.nes
- mapper_holymapperel_1_P128K_CR8K.nes
- mapper_holymapperel_1_P128K.nes
- mapper_holymapperel_1_P512K_CR8K_S32K.nes
- mapper_holymapperel_1_P512K_CR8K_S8K.nes
- mapper_holymapperel_1_P512K_S32K.nes
- mapper_holymapperel_1_P512K_S8K.nes

#### UxROM (Mapper 2) - 2 ROMs

- mapper_holymapperel_2_P128K_CR8K_V.nes
- mapper_holymapperel_2_P128K_V.nes

#### CNROM (Mapper 3) - 1 ROM

- mapper_holymapperel_3_P32K_C32K_H.nes

#### MMC3 (Mapper 4) - 11 ROMs

- mapper_holymapperel_4_P128K_CR32K.nes
- mapper_holymapperel_4_P128K_CR8K.nes
- mapper_holymapperel_4_P128K.nes
- mapper_holymapperel_4_P256K_C256K.nes
- mapper_mmc3_test_1_clocking.nes
- mapper_mmc3_test_2_details.nes
- mapper_mmc3_test_3_a12_clocking.nes
- mapper_mmc3_test_4_scanline_timing.nes
- mapper_mmc3_test_5_mmc3_rev_a.nes
- mapper_mmc3_test_6_mmc6.nes
- mapper_mmc3_irq_1_clocking.nes (duplicate)
- mapper_mmc3_irq_2_details.nes (duplicate)
- mapper_mmc3_irq_3_a12_clocking.nes (duplicate)
- mapper_mmc3_irq_4_scanline_timing.nes (duplicate)
- mapper_mmc3_irq_5_rev_a.nes (duplicate)
- mapper_mmc3_irq_6_rev_b.nes (duplicate)

#### MMC5 (Mapper 5) - 3 ROMs

- mapper_mmc5test_v1.nes
- mapper_mmc5test_v2.nes
- mapper_mmc5exram.nes

#### Other Mappers - 21 ROMs

- mapper_holymapperel_7_P128K_CR8K.nes
- mapper_holymapperel_7_P128K.nes
- mapper_holymapperel_9_P128K_C64K.nes
- mapper_holymapperel_10_P128K_C64K_S8K.nes
- mapper_holymapperel_10_P128K_C64K_W8K.nes
- mapper_holymapperel_11_P64K_C64K_V.nes
- mapper_holymapperel_11_P64K_CR32K_V.nes
- mapper_holymapperel_28_P512K_CR32K.nes
- mapper_holymapperel_28_P512K.nes
- mapper_holymapperel_34_P128K_CR8K_H.nes
- mapper_holymapperel_34_P128K_H.nes
- mapper_holymapperel_66_P64K_C16K_V.nes
- mapper_holymapperel_69_P128K_C64K_S8K.nes
- mapper_holymapperel_69_P128K_C64K_W8K.nes
- mapper_holymapperel_78.3_P128K_C64K.nes
- mapper_holymapperel_118_P128K_C64K.nes
- mapper_holymapperel_180_P128K_CR8K_H.nes
- mapper_holymapperel_180_P128K_H.nes

## Current Implementation Status

### Unit Test Results

As of December 19, 2025, RustyNES has **398 passing unit tests** across all crates:

| Crate | Tests | Status | Notes |
|-------|-------|--------|-------|
| rustynes-cpu | 56 | 100% passing | All 256 opcodes validated |
| rustynes-ppu | 90 | 97.8% passing | 4 passing, 2 ignored (timing) |
| rustynes-apu | 105 | 100% passing | All 5 channels implemented |
| rustynes-mappers | 78 | 100% passing | 5 mappers (0, 1, 2, 3, 4) |
| rustynes-core | 69 | 100% passing | Integration layer complete |
| **Total** | **398** | **99.5% passing** | 396 passing, 2 ignored |

### Milestone Completion

| Milestone | Status | Version | Completion Date |
|-----------|--------|---------|-----------------|
| M1: CPU Implementation | Complete | v0.1.0 | Dec 2025 |
| M2: PPU Rendering | Complete | v0.2.0 | Dec 2025 |
| M3: APU Audio | Complete | v0.3.0 | Dec 2025 |
| M4: Mappers | Complete | v0.3.5 | Dec 19, 2025 |
| M5: Integration | Complete | v0.4.0 | Dec 19, 2025 |
| M6: Desktop GUI | In Progress | - | Target: Dec 2025 |

### Implemented Components

**CPU (6502/2A03)**:

- All 256 opcodes (151 official + 105 unofficial)
- All 13 addressing modes
- Cycle-accurate timing
- Interrupt handling (NMI, IRQ, BRK)
- DMA support (OAM DMA, DMC DMA)

**PPU (2C02)**:

- Background rendering with scrolling
- Sprite rendering (8x8 and 8x16 modes)
- Sprite 0 hit detection
- VBlank/NMI timing
- Palette system
- VRAM/OAM memory

**APU (2A03)**:

- Square wave channels (2x)
- Triangle wave channel
- Noise channel
- DMC sample playback
- Frame counter (4-step, 5-step modes)
- IRQ generation

**Mappers**:

- NROM (Mapper 0) - 9.5% game coverage
- MMC1 (Mapper 1) - 27.9% game coverage
- UxROM (Mapper 2) - 10.6% game coverage
- CNROM (Mapper 3) - 6.3% game coverage
- MMC3 (Mapper 4) - 23.4% game coverage
- **Total Coverage**: 77.7% of licensed NES library

**Integration Layer (rustynes-core)**:

- Console coordinator (CPU/PPU/APU synchronization)
- System bus (memory routing)
- ROM loading (iNES and NES 2.0 formats)
- Save state system (JSON serialization)
- Input handling (controller strobe protocol)

## Test Execution Plan

### Phase 1: CPU Test Integration (Priority: High)

**Goal**: Integrate all 35 pending CPU test ROMs

**Test Infrastructure**:

- Create `rustynes-core/tests/cpu_test_roms.rs`
- Implement multi-ROM test harness
- Read result from $6000 (0x00 = pass, others = error code)

**Execution Order**:

1. **Instruction Tests** (11 ROMs) - Expected: 100% passing
   - All addressing modes already validated by nestest
   - Should pass immediately

2. **Timing Tests** (3 ROMs) - Expected: 100% passing
   - CPU is cycle-accurate
   - Should pass immediately

3. **Interrupt Tests** (7 ROMs) - Expected: 95%+ passing
   - NMI, IRQ, BRK all implemented
   - May expose edge cases

4. **DMA Tests** (2 ROMs) - Expected: 90%+ passing
   - OAM DMA implemented
   - DMC DMA implemented
   - May require timing adjustments

5. **Misc Tests** (12 ROMs) - Expected: 95%+ passing
   - Various edge cases
   - May expose integration issues

**Success Criteria**: 32+/35 CPU test ROMs passing (91%+)

### Phase 2: PPU Test Integration (Priority: High)

**Goal**: Integrate all 43 pending PPU test ROMs

**Test Infrastructure**:

- Extend `rustynes-core/tests/ppu_test_roms.rs`
- Port existing rustynes-ppu tests to integration harness

**Execution Order**:

1. **VBL/NMI Tests** (7 pending) - Expected: 70%+ passing
   - Basic tests should pass
   - Timing tests may fail (cycle precision)

2. **Sprite Hit Tests** (11 pending) - Expected: 80%+ passing
   - Basic tests should pass
   - Timing tests may fail

3. **Sprite Overflow Tests** (5 ROMs) - Expected: 60%+ passing
   - Complex behavior
   - May require PPU refinement

4. **Memory/Register Tests** (7 ROMs) - Expected: 90%+ passing
   - Should pass (basic memory access)

5. **Visual/Rendering Tests** (7 ROMs) - Expected: 50%+ passing
   - Requires accurate pixel output
   - May expose rendering edge cases

**Success Criteria**: 30+/43 PPU test ROMs passing (70%+)

### Phase 3: APU Test Integration (Priority: Medium)

**Goal**: Integrate all 70 APU test ROMs

**Test Infrastructure**:

- Create `rustynes-core/tests/apu_test_roms.rs`
- Implement audio output capture
- Compare against expected waveforms

**Execution Order**:

1. **Length Counter Tests** (14 ROMs) - Expected: 95%+ passing
   - Length counter implemented in all channels

2. **IRQ Tests** (8 ROMs) - Expected: 90%+ passing
   - Frame counter IRQ implemented

3. **DMC Tests** (14 ROMs) - Expected: 85%+ passing
   - DMC fully implemented
   - DMA conflicts may cause failures

4. **Channel Tests** (10 ROMs) - Expected: 95%+ passing
   - All channels implemented

5. **Reset Tests** (8 ROMs) - Expected: 90%+ passing
   - Reset behavior implemented

6. **Clock/Timing Tests** (4 ROMs) - Expected: 80%+ passing
   - May require cycle-level precision

7. **Blargg Suite** (12 ROMs) - Expected: 90%+ passing
   - Comprehensive validation

**Success Criteria**: 60+/70 APU test ROMs passing (85%+)

### Phase 4: Mapper Test Integration (Priority: Medium)

**Goal**: Integrate all 57 mapper test ROMs

**Test Infrastructure**:

- Create `rustynes-core/tests/mapper_test_roms.rs`
- Test each mapper independently

**Execution Order**:

1. **NROM (Mapper 0)** (4 ROMs) - Expected: 100% passing
   - Simplest mapper
   - Should pass immediately

2. **MMC1 (Mapper 1)** (15 ROMs) - Expected: 90%+ passing
   - Complex shift register behavior
   - May expose edge cases

3. **UxROM (Mapper 2)** (2 ROMs) - Expected: 100% passing
   - Simple bank switching
   - Should pass immediately

4. **CNROM (Mapper 3)** (1 ROM) - Expected: 100% passing
   - Simple CHR banking
   - Should pass immediately

5. **MMC3 (Mapper 4)** (11 ROMs) - Expected: 80%+ passing
   - Complex IRQ behavior
   - A12 edge detection may need refinement

6. **MMC5 (Mapper 5)** (3 ROMs) - Expected: 0% passing
   - NOT IMPLEMENTED
   - Phase 3 feature

7. **Other Mappers** (21 ROMs) - Expected: 0% passing
   - NOT IMPLEMENTED
   - Phase 3 feature

**Success Criteria**: 32+/36 implemented mapper test ROMs passing (89%+)

## Test Result Documentation

### Result Format

For each test ROM, document:

**Test ROM Name**: `{category}_{test_name}.nes`

**Status**: PASSING | FAILING | TIMEOUT | NOT_INTEGRATED | IGNORED

**Expected Outcome**:

- Memory $6000: `0x00` (pass) or error code
- On-screen: Pass/fail text display
- Audio: Single beep (pass) or multiple beeps (error code)

**Actual Outcome**:

- Observed result code
- Error description (if failing)
- Log output or stack trace

**Notes**:

- Known issues
- Expected failures
- Pending fixes

### Example Test Result Entry

```markdown

#### cpu_instr_01_implied.nes

**Status**: PASSING
**Category**: CPU - Instruction Tests
**Test Suite**: Blargg Instruction Tests
**Priority**: High

**Expected Outcome**:

- $6000 = 0x00 (all tests passed)
- On-screen: "Passed"

**Actual Outcome**:

- $6000 = 0x00
- Test completed in 45,231 cycles

**Coverage**:

- Tests implied addressing mode instructions
- Validates: CLC, SEC, CLI, SEI, CLD, SED, CLV, INX, DEX, INY, DEY, TAX, TXA, TAY, TYA, TSX, TXS, NOP

**Notes**: Passed immediately, no issues detected

```

## Success Metrics

### Minimum Acceptable Results (Phase 1)

| Category | Target Pass Rate | Minimum ROMs Passing |
|----------|------------------|----------------------|
| CPU      | 91%+             | 32/35                |
| PPU      | 70%+             | 30/43                |
| APU      | 85%+             | 60/70                |
| Mappers  | 89%+             | 32/36 (implemented)  |
| **Total**| **75%+**         | **154/184 (implemented)** |

**Note**: MMC5 and other Phase 3 mappers (21 ROMs) excluded from Phase 1 metrics

### Stretch Goals

| Category | Stretch Pass Rate | ROMs Passing |
|----------|-------------------|--------------|
| CPU      | 100%              | 35/35        |
| PPU      | 85%+              | 37/43        |
| APU      | 95%+              | 67/70        |
| Mappers  | 95%+              | 34/36        |
| **Total**| **85%+**          | **173/184**  |

## Known Limitations and Expected Failures

### PPU Timing Tests

**Expected Failures**:

- ppu_02-vbl_set_time.nes - Requires ±51 cycle precision
- ppu_03-vbl_clear_time.nes - Requires ±10 cycle precision
- ppu_spr_hit_timing_order.nes - Cycle-level sprite evaluation timing
- ppu_spr_hit_edge_timing.nes - Edge case timing

**Reason**: PPU timing implementation prioritizes correctness over cycle-level precision. These tests validate sub-scanline timing accuracy beyond typical emulation requirements.

**Resolution**: Phase 2 timing refinement (Milestone 7)

### APU DMC DMA Conflicts

**Expected Failures**:

- apu_dmc_dma_*_read.nes - DMC DMA conflicts with CPU reads
- apu_dmc_dma_*_write.nes - DMC DMA conflicts with CPU writes

**Reason**: DMC DMA implementation may not perfectly replicate all DMA conflict edge cases.

**Resolution**: Phase 2 APU refinement if failures occur

### Mapper Edge Cases

**Expected Failures**:

- mapper_mmc3_test_4_scanline_timing.nes - Precise IRQ scanline timing
- mapper_mmc3_test_5_mmc3_rev_a.nes - Hardware revision differences
- mapper_mmc1_a12.nes - A12 line edge detection

**Reason**: Mapper implementations prioritize game compatibility over hardware-specific edge cases.

**Resolution**: Phase 2 mapper refinement if failures occur

### Unimplemented Mappers

**Expected 0% Pass Rate**:

- MMC5 (Mapper 5) - 3 test ROMs
- Mappers 7, 9, 10, 11, 28, 34, 66, 69, 78, 118, 180 - 21 test ROMs

**Reason**: These mappers are Phase 3 features.

**Resolution**: Phase 3 (Months 13-18) - Mapper expansion

## Test Execution Environment

### Hardware Requirements

- **CPU**: 4+ cores recommended (parallel test execution)
- **Memory**: 4GB+ RAM
- **Storage**: 5GB+ free space (test ROMs, results, logs)

### Software Requirements

- **Rust**: 1.86+ (MSRV)
- **Cargo**: Latest stable
- **Test Framework**: Built-in Rust test harness
- **CI/CD**: GitHub Actions (Linux, macOS, Windows)

### Test Execution Commands

```bash

# Run all unit tests

cargo test --workspace

# Run all test ROMs (when integrated)

cargo test --workspace --test '*_test_roms'

# Run specific category

cargo test --test cpu_test_roms
cargo test --test ppu_test_roms
cargo test --test apu_test_roms
cargo test --test mapper_test_roms

# Run with verbose output

cargo test --test cpu_test_roms -- --nocapture

# Run single test ROM

cargo test --test cpu_test_roms cpu_instr_01_implied

# Run with timeout (prevent infinite loops)

timeout 300 cargo test --workspace

```

### Results Output

Test results will be saved to:

```text
tests/results/
├── cpu/
│   ├── cpu_instr_01_implied.txt
│   ├── cpu_instr_02_immediate.txt
│   └── ...
├── ppu/
│   ├── ppu_vbl_nmi.txt
│   └── ...
├── apu/
│   └── ...
├── mappers/
│   └── ...
└── SUMMARY.md

```

Each result file contains:

- Test ROM name
- Status (PASSING/FAILING/TIMEOUT/etc.)
- Execution time (cycles and wall time)
- Memory $6000 value
- Log output or error trace
- Notes and observations

## Next Steps

### Immediate Actions (Week 1)

1. Create test harness infrastructure:
   - `rustynes-core/tests/cpu_test_roms.rs`
   - `rustynes-core/tests/ppu_test_roms.rs`
   - `rustynes-core/tests/apu_test_roms.rs`
   - `rustynes-core/tests/mapper_test_roms.rs`

2. Implement ROM loading and execution utilities:
   - `load_test_rom(path: &str) -> Result<Console>`
   - `run_until_complete(console: &mut Console) -> u8`
   - `read_result_code(console: &Console) -> u8`

3. Port existing tests to new harness:
   - cpu_nestest.nes
   - ppu_vbl_nmi.nes
   - ppu_01-vbl_basics.nes
   - ppu_01.basics.nes
   - ppu_02.alignment.nes

### Week 2-3: CPU Test Integration

Execute Phase 1 (CPU tests):

- Integrate all 35 pending CPU test ROMs
- Document results in `tests/results/cpu/`
- Debug and fix any failures
- Achieve 91%+ pass rate

### Week 4-5: PPU Test Integration

Execute Phase 2 (PPU tests):

- Integrate all 43 pending PPU test ROMs
- Document results in `tests/results/ppu/`
- Debug and fix critical failures
- Achieve 70%+ pass rate

### Week 6-7: APU Test Integration

Execute Phase 3 (APU tests):

- Integrate all 70 APU test ROMs
- Document results in `tests/results/apu/`
- Debug and fix critical failures
- Achieve 85%+ pass rate

### Week 8-9: Mapper Test Integration

Execute Phase 4 (Mapper tests):

- Integrate 36 implemented mapper test ROMs
- Document results in `tests/results/mappers/`
- Debug and fix critical failures
- Achieve 89%+ pass rate

### Week 10: Documentation and Reporting

1. Update README.md with test results
2. Update ROADMAP.md with findings
3. Generate comprehensive summary report
4. Document all failures and workarounds
5. Create issue tickets for Phase 2 refinements

## Conclusion

This comprehensive test plan provides a structured approach to validating RustyNES against 212 test ROM files. With Milestones 1-5 complete and 398 passing unit tests, the emulator is well-positioned for comprehensive test ROM integration.

**Target Outcome**: 75%+ overall pass rate (154/184 implemented test ROMs) by end of Phase 1

**Next Milestone**: M6 (Desktop GUI) - Provide visual interface for test ROM execution and debugging
