# M7 Sprint 3: APU Accuracy

## Overview

Calibrate APU timing and mixing to achieve hardware-accurate audio synthesis and prepare for blargg APU test suite in M8.

## Objectives

- [x] Achieve frame counter cycle precision (±1 cycle) ✅
- [~] Fix DMC channel edge cases (cycle stealing documented for future enhancement)
- [ ] Refine triangle linear counter timing
- [x] Calibrate non-linear mixer for hardware accuracy ✅
- [ ] Validate channel interaction timing

## Tasks

### Task 1: Frame Counter Precision ✅ COMPLETE
- [x] Verify 4-step mode timing (7457, 14913, 22372, 29830 cycles) - **Fixed 22371→22372**
- [x] Verify 5-step mode timing (7457, 14913, 22371, 37281 cycles)
- [x] Test IRQ flag timing in 4-step mode
- [x] Validate quarter frame/half frame events
- [x] Handle write timing edge cases ($4017 writes)

### Task 2: DMC Channel Edge Cases
- [ ] Test DMC DMA conflicts with CPU
- [ ] Verify sample buffer behavior (empty/refill timing)
- [ ] Test DMC rates (16 rate values)
- [ ] Handle IRQ flag edge cases
- [ ] Validate memory reader timing

### Task 3: Triangle Linear Counter
- [ ] Verify linear counter reload timing
- [ ] Test halt flag behavior
- [ ] Validate control flag interaction
- [ ] Test with apu_lin_ctr.nes
- [ ] Handle edge cases (reload while counting)

### Task 4: Mixer Calibration ✅ COMPLETE
- [x] Verify non-linear mixing formula - **Implemented hardware-accurate NESdev formula**
- [x] Test output levels for all channels - **TND lookup table corrected**
- [x] Validate pulse channel mixing (0-15 volume)
- [x] Test TND mixing (triangle, noise, DMC) - **Proper weighted calculation: 3*tri + 2*noise + dmc**
- [ ] Compare output to hardware recordings (deferred to M8)

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| apu_test.nes | [ ] Pending | Comprehensive APU test suite |
| apu_len_ctr.nes | [ ] Pending | Length counter timing |
| apu_len_table.nes | [ ] Pending | Length counter table values |
| apu_irq_flag.nes | [ ] Pending | IRQ flag timing |
| apu_irq_flag_timing.nes | [ ] Pending | IRQ flag precise timing |
| apu_lin_ctr.nes | [ ] Pending | Triangle linear counter |
| apu_dmc.nes | [ ] Pending | DMC channel comprehensive |
| apu_dmc_latency.nes | [ ] Pending | DMC DMA latency |

## Acceptance Criteria

- [ ] Frame counter timing accurate to ±1 cycle
- [ ] apu_test.nes passes (basic APU test)
- [ ] apu_len_ctr.nes passes
- [ ] apu_lin_ctr.nes passes
- [ ] DMC channel edge cases handled
- [ ] Mixer output matches hardware recordings
- [ ] Audio quality verified in test games

## Version Target

v0.6.0
