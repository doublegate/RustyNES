# RustyNES Comprehensive Test ROM Validation Results

**Generated**: 1766206065
**RustyNES Version**: v0.4.0 (Milestone 5 Complete)

## Executive Summary

| Metric | Count | Percentage |
|--------|-------|------------|
| **Total Test ROMs** | 207 | 100.0% |
| Passed | 0 | 0.0% |
| Failed | 75 | 36.2% |
| Timeout | 109 | 52.7% |
| Load Error | 2 | 1.0% |
| Not Implemented | 21 | 10.1% |

## Detailed Results by Category

### CPU Tests

**Total**: 36 ROMs
**Pass Rate**: 0.0%

| Status | Count |
|--------|-------|
| Pass | 0 |
| Fail | 30 |
| Timeout | 5 |
| Load Error | 1 |
| Not Implemented | 0 |

#### Detailed CPU Test Results

| Test ROM | Status | Time (ms) | Notes |
|----------|--------|-----------|-------|
| cpu_all_instrs.nes | ✗ FAIL | 21 | Test failed with error code: 0x80 |
| cpu_branch_backward.nes | ⏱ TIMEOUT | 803 | Test did not complete within timeout period |
| cpu_branch_basics.nes | ⏱ TIMEOUT | 805 | Test did not complete within timeout period |
| cpu_branch_forward.nes | ⏱ TIMEOUT | 800 | Test did not complete within timeout period |
| cpu_branch_timing_2.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_dummy_reads.nes | ⏱ TIMEOUT | 890 | Test did not complete within timeout period |
| cpu_dummy_writes_oam.nes | ✗ FAIL | 24 | Test failed with error code: 0x80 |
| cpu_dummy_writes_ppumem.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| cpu_exec_space_apu.nes | ✗ FAIL | 24 | Test failed with error code: 0x80 |
| cpu_exec_space_ppuio.nes | ✗ FAIL | 24 | Test failed with error code: 0x80 |
| cpu_flag_concurrency.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| cpu_instr_01_implied.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_02_immediate.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_03_zero_page.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_04_zp_xy.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_05_absolute.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_06_abs_xy.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_07_ind_x.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_08_ind_y.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_09_branches.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_10_stack.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| cpu_instr_11_special.nes | ⚠ LOAD_ERROR | 0 | ROM error: Invalid iNES magic number: expected [4E 45 53 1A], got [0A, 0A, 0A... |
| cpu_instr_timing.nes | ✗ FAIL | 25 | Test failed with error code: 0x80 |
| cpu_instr_timing_1.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| cpu_int_branch_delays_irq.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| cpu_int_cli_latency.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| cpu_int_irq_and_dma.nes | ✗ FAIL | 21 | Test failed with error code: 0x80 |
| cpu_int_nmi_and_brk.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| cpu_int_nmi_and_irq.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| cpu_interrupts.nes | ✗ FAIL | 18 | Test failed with error code: 0x80 |
| cpu_nestest.nes | ⏱ TIMEOUT | 865 | Test did not complete within timeout period |
| cpu_official_only.nes | ✗ FAIL | 34 | Test failed with error code: 0x80 |
| cpu_ram_after_reset.nes | ✗ FAIL | 26 | Test failed with error code: 0x80 |
| cpu_regs_after_reset.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| cpu_sprdma_and_dmc_dma.nes | ✗ FAIL | 17 | Test failed with error code: 0x80 |
| cpu_sprdma_and_dmc_dma_512.nes | ✗ FAIL | 17 | Test failed with error code: 0x80 |

### PPU Tests

**Total**: 49 ROMs
**Pass Rate**: 0.0%

| Status | Count |
|--------|-------|
| Pass | 0 |
| Fail | 23 |
| Timeout | 26 |
| Load Error | 0 |
| Not Implemented | 0 |

#### Detailed PPU Test Results

| Test ROM | Status | Time (ms) | Notes |
|----------|--------|-----------|-------|
| ppu_01-vbl_basics.nes | ✗ FAIL | 14 | Test failed with error code: 0x80 |
| ppu_01.basics.nes | ⏱ TIMEOUT | 829 | Test did not complete within timeout period |
| ppu_02-vbl_set_time.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_02.alignment.nes | ⏱ TIMEOUT | 833 | Test did not complete within timeout period |
| ppu_03-vbl_clear_time.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_03.corners.nes | ⏱ TIMEOUT | 830 | Test did not complete within timeout period |
| ppu_04-nmi_control.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| ppu_04.flip.nes | ⏱ TIMEOUT | 869 | Test did not complete within timeout period |
| ppu_05-nmi_timing.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_05.left_clip.nes | ⏱ TIMEOUT | 840 | Test did not complete within timeout period |
| ppu_06-suppression.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_06.right_edge.nes | ⏱ TIMEOUT | 831 | Test did not complete within timeout period |
| ppu_07-nmi_on_timing.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_07.screen_bottom.nes | ⏱ TIMEOUT | 864 | Test did not complete within timeout period |
| ppu_08-nmi_off_timing.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_08.double_height.nes | ⏱ TIMEOUT | 831 | Test did not complete within timeout period |
| ppu_09-even_odd_frames.nes | ✗ FAIL | 15 | Test failed with error code: 0x80 |
| ppu_10-even_odd_timing.nes | ✗ FAIL | 17 | Test failed with error code: 0x80 |
| ppu_color.nes | ⏱ TIMEOUT | 856 | Test did not complete within timeout period |
| ppu_flowing_palette.nes | ⏱ TIMEOUT | 385 | Test did not complete within timeout period |
| ppu_full_palette.nes | ⏱ TIMEOUT | 411 | Test did not complete within timeout period |
| ppu_full_palette_smooth.nes | ⏱ TIMEOUT | 416 | Test did not complete within timeout period |
| ppu_ntsc_torture.nes | ⏱ TIMEOUT | 859 | Test did not complete within timeout period |
| ppu_oam_read.nes | ✗ FAIL | 22 | Test failed with error code: 0x80 |
| ppu_oam_stress.nes | ✗ FAIL | 17 | Test failed with error code: 0x80 |
| ppu_open_bus.nes | ✗ FAIL | 23 | Test failed with error code: 0x80 |
| ppu_palette.nes | ⏱ TIMEOUT | 654 | Test did not complete within timeout period |
| ppu_palette_ram.nes | ⏱ TIMEOUT | 806 | Test did not complete within timeout period |
| ppu_scanline.nes | ⏱ TIMEOUT | 863 | Test did not complete within timeout period |
| ppu_spr_hit_alignment.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| ppu_spr_hit_basics.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| ppu_spr_hit_corners.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| ppu_spr_hit_double_height.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| ppu_spr_hit_edge_timing.nes | ⏱ TIMEOUT | 743 | Test did not complete within timeout period |
| ppu_spr_hit_flip.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| ppu_spr_hit_left_clip.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| ppu_spr_hit_right_edge.nes | ✗ FAIL | 20 | Test failed with error code: 0x80 |
| ppu_spr_hit_screen_bottom.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| ppu_spr_hit_timing_basics.nes | ⏱ TIMEOUT | 822 | Test did not complete within timeout period |
| ppu_spr_hit_timing_order.nes | ⏱ TIMEOUT | 813 | Test did not complete within timeout period |
| ppu_spr_overflow_basics.nes | ⏱ TIMEOUT | 815 | Test did not complete within timeout period |
| ppu_spr_overflow_details.nes | ⏱ TIMEOUT | 813 | Test did not complete within timeout period |
| ppu_spr_overflow_emulator.nes | ⏱ TIMEOUT | 815 | Test did not complete within timeout period |
| ppu_spr_overflow_obscure.nes | ⏱ TIMEOUT | 815 | Test did not complete within timeout period |
| ppu_spr_overflow_timing.nes | ⏱ TIMEOUT | 804 | Test did not complete within timeout period |
| ppu_sprite_ram.nes | ⏱ TIMEOUT | 806 | Test did not complete within timeout period |
| ppu_test_ppu_read_buffer.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| ppu_vbl_nmi.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| ppu_vram_access.nes | ⏱ TIMEOUT | 817 | Test did not complete within timeout period |

### APU Tests

**Total**: 65 ROMs
**Pass Rate**: 0.0%

| Status | Count |
|--------|-------|
| Pass | 0 |
| Fail | 15 |
| Timeout | 50 |
| Load Error | 0 |
| Not Implemented | 0 |

#### Detailed APU Test Results

| Test ROM | Status | Time (ms) | Notes |
|----------|--------|-----------|-------|
| apu_clock_jitter.nes | ⏱ TIMEOUT | 816 | Test did not complete within timeout period |
| apu_dmc.nes | ✗ FAIL | 22 | Test failed with error code: 0x80 |
| apu_dmc_basics.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| apu_dmc_buffer_retained.nes | ⏱ TIMEOUT | 348 | Test did not complete within timeout period |
| apu_dmc_dma_2007_read.nes | ⏱ TIMEOUT | 350 | Test did not complete within timeout period |
| apu_dmc_dma_2007_write.nes | ⏱ TIMEOUT | 357 | Test did not complete within timeout period |
| apu_dmc_dma_4016_read.nes | ⏱ TIMEOUT | 838 | Test did not complete within timeout period |
| apu_dmc_dma_double_2007_read.nes | ⏱ TIMEOUT | 844 | Test did not complete within timeout period |
| apu_dmc_dma_read_write_2007.nes | ⏱ TIMEOUT | 838 | Test did not complete within timeout period |
| apu_dmc_latency.nes | ⏱ TIMEOUT | 351 | Test did not complete within timeout period |
| apu_dmc_pitch.nes | ⏱ TIMEOUT | 358 | Test did not complete within timeout period |
| apu_dmc_rates.nes | ✗ FAIL | 22 | Test failed with error code: 0x80 |
| apu_dmc_status.nes | ⏱ TIMEOUT | 349 | Test did not complete within timeout period |
| apu_dmc_status_irq.nes | ⏱ TIMEOUT | 351 | Test did not complete within timeout period |
| apu_dpcmletterbox.nes | ✗ FAIL | 6 | Runtime panic: attempt to subtract with overflow |
| apu_env.nes | ⏱ TIMEOUT | 376 | Test did not complete within timeout period |
| apu_irq_flag.nes | ⏱ TIMEOUT | 801 | Test did not complete within timeout period |
| apu_irq_flag_timing.nes | ⏱ TIMEOUT | 815 | Test did not complete within timeout period |
| apu_irq_timing.nes | ⏱ TIMEOUT | 816 | Test did not complete within timeout period |
| apu_len_ctr.nes | ⏱ TIMEOUT | 798 | Test did not complete within timeout period |
| apu_len_halt_timing.nes | ⏱ TIMEOUT | 818 | Test did not complete within timeout period |
| apu_len_reload_timing.nes | ⏱ TIMEOUT | 805 | Test did not complete within timeout period |
| apu_len_table.nes | ⏱ TIMEOUT | 812 | Test did not complete within timeout period |
| apu_len_timing.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| apu_len_timing_mode0.nes | ⏱ TIMEOUT | 811 | Test did not complete within timeout period |
| apu_len_timing_mode1.nes | ⏱ TIMEOUT | 816 | Test did not complete within timeout period |
| apu_lin_ctr.nes | ⏱ TIMEOUT | 368 | Test did not complete within timeout period |
| apu_noise.nes | ✗ FAIL | 22 | Test failed with error code: 0x80 |
| apu_noise_pitch.nes | ⏱ TIMEOUT | 342 | Test did not complete within timeout period |
| apu_pal_clock_jitter.nes | ⏱ TIMEOUT | 829 | Test did not complete within timeout period |
| apu_pal_irq_flag.nes | ⏱ TIMEOUT | 799 | Test did not complete within timeout period |
| apu_pal_irq_flag_timing.nes | ⏱ TIMEOUT | 812 | Test did not complete within timeout period |
| apu_pal_irq_timing.nes | ⏱ TIMEOUT | 818 | Test did not complete within timeout period |
| apu_pal_len_ctr.nes | ⏱ TIMEOUT | 796 | Test did not complete within timeout period |
| apu_pal_len_halt_timing.nes | ⏱ TIMEOUT | 820 | Test did not complete within timeout period |
| apu_pal_len_reload_timing.nes | ⏱ TIMEOUT | 808 | Test did not complete within timeout period |
| apu_pal_len_table.nes | ⏱ TIMEOUT | 861 | Test did not complete within timeout period |
| apu_pal_len_timing_mode0.nes | ⏱ TIMEOUT | 817 | Test did not complete within timeout period |
| apu_pal_len_timing_mode1.nes | ⏱ TIMEOUT | 810 | Test did not complete within timeout period |
| apu_phase_reset.nes | ⏱ TIMEOUT | 365 | Test did not complete within timeout period |
| apu_reset_4015_cleared.nes | ✗ FAIL | 23 | Test failed with error code: 0x81 |
| apu_reset_4017_timing.nes | ✗ FAIL | 21 | Test failed with error code: 0x80 |
| apu_reset_4017_written.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| apu_reset_irq_flag_cleared.nes | ✗ FAIL | 23 | Test failed with error code: 0x81 |
| apu_reset_len_ctrs_enabled.nes | ✗ FAIL | 20 | Test failed with error code: 0x81 |
| apu_reset_timing.nes | ⏱ TIMEOUT | 819 | Test did not complete within timeout period |
| apu_reset_works_immediately.nes | ✗ FAIL | 19 | Test failed with error code: 0x80 |
| apu_square.nes | ✗ FAIL | 21 | Test failed with error code: 0x80 |
| apu_square_pitch.nes | ⏱ TIMEOUT | 361 | Test did not complete within timeout period |
| apu_sweep_cutoff.nes | ⏱ TIMEOUT | 374 | Test did not complete within timeout period |
| apu_sweep_sub.nes | ⏱ TIMEOUT | 362 | Test did not complete within timeout period |
| apu_test.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| apu_test_1.nes | ⏱ TIMEOUT | 799 | Test did not complete within timeout period |
| apu_test_10.nes | ⏱ TIMEOUT | 798 | Test did not complete within timeout period |
| apu_test_2.nes | ⏱ TIMEOUT | 799 | Test did not complete within timeout period |
| apu_test_3.nes | ⏱ TIMEOUT | 795 | Test did not complete within timeout period |
| apu_test_4.nes | ⏱ TIMEOUT | 801 | Test did not complete within timeout period |
| apu_test_5.nes | ⏱ TIMEOUT | 803 | Test did not complete within timeout period |
| apu_test_6.nes | ⏱ TIMEOUT | 807 | Test did not complete within timeout period |
| apu_test_7.nes | ⏱ TIMEOUT | 798 | Test did not complete within timeout period |
| apu_test_8.nes | ⏱ TIMEOUT | 796 | Test did not complete within timeout period |
| apu_test_9.nes | ⏱ TIMEOUT | 798 | Test did not complete within timeout period |
| apu_triangle.nes | ✗ FAIL | 22 | Test failed with error code: 0x80 |
| apu_triangle_pitch.nes | ⏱ TIMEOUT | 344 | Test did not complete within timeout period |
| apu_volumes.nes | ⏱ TIMEOUT | 378 | Test did not complete within timeout period |

### MAPPERS Tests

**Total**: 57 ROMs
**Pass Rate**: 0.0%

| Status | Count |
|--------|-------|
| Pass | 0 |
| Fail | 7 |
| Timeout | 28 |
| Load Error | 1 |
| Not Implemented | 21 |

#### Detailed MAPPERS Test Results

| Test ROM | Status | Time (ms) | Notes |
|----------|--------|-----------|-------|
| mapper_holymapperel_0_P32K_C8K_V.nes | ⏱ TIMEOUT | 352 | Test did not complete within timeout period |
| mapper_holymapperel_0_P32K_CR32K_V.nes | ⏱ TIMEOUT | 353 | Test did not complete within timeout period |
| mapper_holymapperel_0_P32K_CR8K_V.nes | ⏱ TIMEOUT | 350 | Test did not complete within timeout period |
| mapper_holymapperel_10_P128K_C64K_S8K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 10 (submapper 0) |
| mapper_holymapperel_10_P128K_C64K_W8K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 10 (submapper 0) |
| mapper_holymapperel_118_P128K_C64K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 118 (submapper 0) |
| mapper_holymapperel_11_P64K_C64K_V.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 11 (submapper 0) |
| mapper_holymapperel_11_P64K_CR32K_V.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 11 (submapper 0) |
| mapper_holymapperel_180_P128K_CR8K_H.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 180 (submapper 0) |
| mapper_holymapperel_180_P128K_H.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 180 (submapper 0) |
| mapper_holymapperel_1_P128K.nes | ⏱ TIMEOUT | 396 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C128K.nes | ⏱ TIMEOUT | 393 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C128K_S8K.nes | ⏱ TIMEOUT | 391 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C128K_W8K.nes | ⏱ TIMEOUT | 394 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C32K.nes | ⏱ TIMEOUT | 393 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C32K_S8K.nes | ⏱ TIMEOUT | 395 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_C32K_W8K.nes | ⏱ TIMEOUT | 396 | Test did not complete within timeout period |
| mapper_holymapperel_1_P128K_CR8K.nes | ⏱ TIMEOUT | 390 | Test did not complete within timeout period |
| mapper_holymapperel_1_P512K_CR8K_S32K.nes | ⏱ TIMEOUT | 396 | Test did not complete within timeout period |
| mapper_holymapperel_1_P512K_CR8K_S8K.nes | ⏱ TIMEOUT | 391 | Test did not complete within timeout period |
| mapper_holymapperel_1_P512K_S32K.nes | ⏱ TIMEOUT | 395 | Test did not complete within timeout period |
| mapper_holymapperel_1_P512K_S8K.nes | ⏱ TIMEOUT | 395 | Test did not complete within timeout period |
| mapper_holymapperel_28_P512K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 28 (submapper 0) |
| mapper_holymapperel_28_P512K_CR32K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 28 (submapper 0) |
| mapper_holymapperel_2_P128K_CR8K_V.nes | ⏱ TIMEOUT | 356 | Test did not complete within timeout period |
| mapper_holymapperel_2_P128K_V.nes | ⏱ TIMEOUT | 360 | Test did not complete within timeout period |
| mapper_holymapperel_34_P128K_CR8K_H.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 34 (submapper 0) |
| mapper_holymapperel_34_P128K_H.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 34 (submapper 0) |
| mapper_holymapperel_3_P32K_C32K_H.nes | ⏱ TIMEOUT | 353 | Test did not complete within timeout period |
| mapper_holymapperel_4_P128K.nes | ⏱ TIMEOUT | 389 | Test did not complete within timeout period |
| mapper_holymapperel_4_P128K_CR32K.nes | ⏱ TIMEOUT | 394 | Test did not complete within timeout period |
| mapper_holymapperel_4_P128K_CR8K.nes | ⏱ TIMEOUT | 390 | Test did not complete within timeout period |
| mapper_holymapperel_4_P256K_C256K.nes | ⏱ TIMEOUT | 393 | Test did not complete within timeout period |
| mapper_holymapperel_66_P64K_C16K_V.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 66 (submapper 0) |
| mapper_holymapperel_69_P128K_C64K_S8K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 69 (submapper 0) |
| mapper_holymapperel_69_P128K_C64K_W8K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 69 (submapper 0) |
| mapper_holymapperel_78.3_P128K_C64K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 78 (submapper 3) |
| mapper_holymapperel_7_P128K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 7 (submapper 0) |
| mapper_holymapperel_7_P128K_CR8K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 7 (submapper 0) |
| mapper_holymapperel_9_P128K_C64K.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 9 (submapper 0) |
| mapper_mmc1_a12.nes | ✗ FAIL | 35 | Test failed with error code: 0x9F |
| mapper_mmc3_irq_1_clocking.nes | ⏱ TIMEOUT | 877 | Test did not complete within timeout period |
| mapper_mmc3_irq_2_details.nes | ⏱ TIMEOUT | 870 | Test did not complete within timeout period |
| mapper_mmc3_irq_3_a12_clocking.nes | ⏱ TIMEOUT | 874 | Test did not complete within timeout period |
| mapper_mmc3_irq_4_scanline_timing.nes | ⏱ TIMEOUT | 861 | Test did not complete within timeout period |
| mapper_mmc3_irq_5_rev_a.nes | ⏱ TIMEOUT | 867 | Test did not complete within timeout period |
| mapper_mmc3_irq_6_rev_b.nes | ⏱ TIMEOUT | 866 | Test did not complete within timeout period |
| mapper_mmc3_test_1_clocking.nes | ✗ FAIL | 17 | Test failed with error code: 0x80 |
| mapper_mmc3_test_2_details.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| mapper_mmc3_test_3_a12_clocking.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| mapper_mmc3_test_4_scanline_timing.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| mapper_mmc3_test_5_mmc3_rev_a.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| mapper_mmc3_test_6_mmc6.nes | ✗ FAIL | 16 | Test failed with error code: 0x80 |
| mapper_mmc5exram.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 5 (submapper 0) |
| mapper_mmc5test_v1.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 5 (submapper 0) |
| mapper_mmc5test_v2.nes | ○ NOT_IMPLEMENTED | 0 | Mapper error: Unsupported mapper 5 (submapper 0) |
| mapper_nrom_368_test.nes | ⚠ LOAD_ERROR | 0 | Panic: NROM requires 16KB or 32KB PRG-ROM, got 49152 bytes |

## Failure Analysis

**Total Failures**: 77

### ROM Format Error

**Count**: 2

- **mapper_nrom_368_test.nes** (mappers): Panic: NROM requires 16KB or 32KB PRG-ROM, got 49152 bytes
- **cpu_instr_11_special.nes** (cpu): ROM error: Invalid iNES magic number: expected [4E 45 53 1A], got [0A, 0A, 0A, 0A]

### Other Error

**Count**: 75

- **ppu_01-vbl_basics.nes** (ppu): Test failed with error code: 0x80
- **ppu_02-vbl_set_time.nes** (ppu): Test failed with error code: 0x80
- **ppu_03-vbl_clear_time.nes** (ppu): Test failed with error code: 0x80
- **ppu_04-nmi_control.nes** (ppu): Test failed with error code: 0x80
- **ppu_05-nmi_timing.nes** (ppu): Test failed with error code: 0x80
- **ppu_06-suppression.nes** (ppu): Test failed with error code: 0x80
- **ppu_07-nmi_on_timing.nes** (ppu): Test failed with error code: 0x80
- **ppu_08-nmi_off_timing.nes** (ppu): Test failed with error code: 0x80
- **ppu_09-even_odd_frames.nes** (ppu): Test failed with error code: 0x80
- **ppu_10-even_odd_timing.nes** (ppu): Test failed with error code: 0x80
- **ppu_oam_read.nes** (ppu): Test failed with error code: 0x80
- **ppu_oam_stress.nes** (ppu): Test failed with error code: 0x80
- **ppu_open_bus.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_alignment.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_basics.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_corners.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_double_height.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_flip.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_left_clip.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_right_edge.nes** (ppu): Test failed with error code: 0x80
- **ppu_spr_hit_screen_bottom.nes** (ppu): Test failed with error code: 0x80
- **ppu_test_ppu_read_buffer.nes** (ppu): Test failed with error code: 0x80
- **ppu_vbl_nmi.nes** (ppu): Test failed with error code: 0x80
- **mapper_mmc1_a12.nes** (mappers): Test failed with error code: 0x9F
- **mapper_mmc3_test_1_clocking.nes** (mappers): Test failed with error code: 0x80
- **mapper_mmc3_test_2_details.nes** (mappers): Test failed with error code: 0x80
- **mapper_mmc3_test_3_a12_clocking.nes** (mappers): Test failed with error code: 0x80
- **mapper_mmc3_test_4_scanline_timing.nes** (mappers): Test failed with error code: 0x80
- **mapper_mmc3_test_5_mmc3_rev_a.nes** (mappers): Test failed with error code: 0x80
- **mapper_mmc3_test_6_mmc6.nes** (mappers): Test failed with error code: 0x80
- **cpu_all_instrs.nes** (cpu): Test failed with error code: 0x80
- **cpu_branch_timing_2.nes** (cpu): Test failed with error code: 0x80
- **cpu_dummy_writes_oam.nes** (cpu): Test failed with error code: 0x80
- **cpu_dummy_writes_ppumem.nes** (cpu): Test failed with error code: 0x80
- **cpu_exec_space_apu.nes** (cpu): Test failed with error code: 0x80
- **cpu_exec_space_ppuio.nes** (cpu): Test failed with error code: 0x80
- **cpu_flag_concurrency.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_01_implied.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_02_immediate.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_03_zero_page.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_04_zp_xy.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_05_absolute.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_06_abs_xy.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_07_ind_x.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_08_ind_y.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_09_branches.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_10_stack.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_timing.nes** (cpu): Test failed with error code: 0x80
- **cpu_instr_timing_1.nes** (cpu): Test failed with error code: 0x80
- **cpu_int_branch_delays_irq.nes** (cpu): Test failed with error code: 0x80
- **cpu_int_cli_latency.nes** (cpu): Test failed with error code: 0x80
- **cpu_int_irq_and_dma.nes** (cpu): Test failed with error code: 0x80
- **cpu_int_nmi_and_brk.nes** (cpu): Test failed with error code: 0x80
- **cpu_int_nmi_and_irq.nes** (cpu): Test failed with error code: 0x80
- **cpu_interrupts.nes** (cpu): Test failed with error code: 0x80
- **cpu_official_only.nes** (cpu): Test failed with error code: 0x80
- **cpu_ram_after_reset.nes** (cpu): Test failed with error code: 0x80
- **cpu_regs_after_reset.nes** (cpu): Test failed with error code: 0x80
- **cpu_sprdma_and_dmc_dma.nes** (cpu): Test failed with error code: 0x80
- **cpu_sprdma_and_dmc_dma_512.nes** (cpu): Test failed with error code: 0x80
- **apu_dmc.nes** (apu): Test failed with error code: 0x80
- **apu_dmc_basics.nes** (apu): Test failed with error code: 0x80
- **apu_dmc_rates.nes** (apu): Test failed with error code: 0x80
- **apu_dpcmletterbox.nes** (apu): Runtime panic: attempt to subtract with overflow
- **apu_len_timing.nes** (apu): Test failed with error code: 0x80
- **apu_noise.nes** (apu): Test failed with error code: 0x80
- **apu_reset_4015_cleared.nes** (apu): Test failed with error code: 0x81
- **apu_reset_4017_timing.nes** (apu): Test failed with error code: 0x80
- **apu_reset_4017_written.nes** (apu): Test failed with error code: 0x80
- **apu_reset_irq_flag_cleared.nes** (apu): Test failed with error code: 0x81
- **apu_reset_len_ctrs_enabled.nes** (apu): Test failed with error code: 0x81
- **apu_reset_works_immediately.nes** (apu): Test failed with error code: 0x80
- **apu_square.nes** (apu): Test failed with error code: 0x80
- **apu_test.nes** (apu): Test failed with error code: 0x80
- **apu_triangle.nes** (apu): Test failed with error code: 0x80

## Recommendations

- **Implemented ROMs Pass Rate**: 0.0% (0/186)
- **Action Required**: 109 test ROMs timed out - need to implement result checking mechanism
- **Future Work**: 21 test ROMs require unimplemented mappers (Phase 3 feature)

## Next Steps

1. Implement Console::read_memory() method to check $6000 result code
2. Add actual ROM execution and result validation
3. Integrate passing test ROMs into CI/CD pipeline
4. Prioritize fixing timeout cases
5. Plan Phase 3 mapper implementations based on test requirements
