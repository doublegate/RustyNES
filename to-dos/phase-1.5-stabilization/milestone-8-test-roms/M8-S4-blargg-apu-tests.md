# M8 Sprint 4: Blargg APU Tests

## Overview

Systematically pass the Blargg APU test suite (70 tests) to validate audio channel behavior, frame counter timing, mixer output, and APU register behavior.

## Objectives

- [ ] Pass 67/70 APU tests (96%)
- [ ] Validate frame counter timing (4-step, 5-step)
- [ ] Verify all 5 audio channels (pulse 1/2, triangle, noise, DMC)
- [ ] Test length counter and linear counter behavior
- [ ] Validate mixer output levels
- [ ] Ensure IRQ flag timing correct

## Tasks

### Task 1: Comprehensive APU Tests (15 tests)
- [ ] Run apu_test/apu_test.nes (comprehensive APU test suite)
- [ ] Test 1-len_ctr.nes (length counter)
- [ ] Test 2-len_table.nes (length counter table values)
- [ ] Test 3-irq_flag.nes (IRQ flag behavior)
- [ ] Test 4-jitter.nes (APU jitter behavior)
- [ ] Test 5-len_timing.nes (length counter timing)
- [ ] Test 6-irq_flag_timing.nes (IRQ flag timing)
- [ ] Test 7-dmc_basics.nes (DMC channel basics)
- [ ] Test 8-dmc_rates.nes (DMC sample rates)

### Task 2: Frame Counter Tests (10 tests)
- [ ] Run apu_frame_counter/apu_frame_counter.nes
- [ ] Test 4-step mode timing (14,915 cycles, 29,829 cycles)
- [ ] Test 5-step mode timing (18,641 cycles, 37,282 cycles)
- [ ] Verify IRQ flag in 4-step mode
- [ ] Test quarter frame events (envelope, triangle linear)
- [ ] Test half frame events (length counter, sweep)
- [ ] Validate $4017 write timing (clear IRQ, set mode)
- [ ] Test frame counter reset behavior

### Task 3: Channel-Specific Tests (25 tests)

#### Pulse Channels (8 tests)
- [ ] Test pulse 1 duty cycle (12.5%, 25%, 50%, 75%)
- [ ] Test pulse 2 duty cycle
- [ ] Verify sweep unit behavior (pulse 1/2)
- [ ] Test envelope generator (volume, decay)
- [ ] Validate length counter interaction
- [ ] Test frequency sweep edge cases

#### Triangle Channel (6 tests)
- [ ] Run apu_lin_ctr/apu_lin_ctr.nes (linear counter)
- [ ] Test linear counter reload timing
- [ ] Verify halt flag behavior
- [ ] Test control flag interaction
- [ ] Validate length counter + linear counter
- [ ] Test triangle output waveform

#### Noise Channel (5 tests)
- [ ] Test noise mode 0 (long period LFSR)
- [ ] Test noise mode 1 (short period LFSR)
- [ ] Verify envelope generator
- [ ] Test length counter
- [ ] Validate noise output levels

#### DMC Channel (6 tests)
- [ ] Run apu_dmc/apu_dmc.nes (DMC comprehensive)
- [ ] Test DMC sample buffer (empty/refill timing)
- [ ] Verify DMC DMA conflicts with CPU
- [ ] Test all 16 DMC rates ($0-$F)
- [ ] Validate IRQ flag behavior
- [ ] Test memory reader timing

### Task 4: Mixer Tests (5 tests)
- [ ] Run apu_mixer/apu_mixer.nes (mixer output)
- [ ] Verify non-linear mixing formula
- [ ] Test pulse channel mixing (0-15 volume levels)
- [ ] Test TND mixing (triangle, noise, DMC)
- [ ] Validate output levels against hardware
- [ ] Compare mixer output to reference recordings

### Task 5: Miscellaneous APU Tests (15 tests)
- [ ] Test $4015 read/write behavior (channel enable)
- [ ] Verify APU register mirroring
- [ ] Test open bus behavior ($4000-$4017)
- [ ] Validate APU power-up state
- [ ] Test APU reset behavior
- [ ] Verify DMC DMA + OAM DMA conflicts

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| apu_test/1-len_ctr.nes | ❌ Fail | 0xFF |
| apu_test/2-len_table.nes | ❌ Fail | 0xFF |
| apu_test/3-irq_flag.nes | ❌ Fail | 0xFF |
| apu_test/4-jitter.nes | ❌ Fail | 0xFF |
| apu_test/5-len_timing.nes | ❌ Fail | First length too late |
| apu_test/6-irq_flag_timing.nes | ❌ Fail | 0xFF |
| apu_test/7-dmc_basics.nes | ❌ Fail | Reading IRQ flag shouldn't clear it |
| apu_test/8-dmc_rates.nes | ❌ Fail | Rate 0 period too long |
| apu_frame_counter/*.nes | [ ] Pending | |
| apu_lin_ctr/apu_lin_ctr.nes | ❌ Fail | 0xFF |
| apu_dmc/apu_dmc.nes | [ ] Pending | |
| apu_dmc/apu_dmc_latency.nes | [ ] Pending | |
| apu_mixer/apu_mixer.nes | [ ] Pending | |
| apu_sweep/apu_sweep.nes | ❌ Fail | 0xFF |
| apu_envelope/apu_envelope.nes | ❌ Fail | 0xFF |

**Additional APU Tests (40+ ROMs):**
- apu_pulse/ (pulse channel tests)
- apu_triangle/ (triangle channel tests)
- apu_noise/ (noise channel tests)
- apu_misc/ (miscellaneous APU behavior)

## Acceptance Criteria

- [ ] 67/70 APU tests passing (96%)
- [ ] Frame counter timing accurate (±1 cycle)
- [ ] Length counter behavior correct
- [ ] Linear counter timing precise
- [ ] DMC channel edge cases handled
- [ ] Mixer output validated
- [ ] IRQ flag timing correct
- [ ] Zero regressions from v0.6.0 baseline

## Expected Failures (3 tests)

**Expansion Audio Tests:**
- apu_vrc6/vrc6_test.nes - VRC6 expansion audio (not in Phase 1.5 scope)
- apu_fds/fds_test.nes - FDS expansion audio (not in Phase 1.5 scope)
- apu_mmc5/mmc5_test.nes - MMC5 expansion audio (not in Phase 1.5 scope)

**Rationale:** These tests require expansion audio implementation (VRC6, FDS, MMC5) which is deferred to Phase 2 (M13).

## Debugging Strategy

1. **Identify Failure:**
   - Run test ROM, capture error output
   - Cross-reference with test documentation

2. **Isolate Channel/Behavior:**
   - Determine which channel or behavior failing
   - Review APU implementation (frame counter, channels, mixer)

3. **Trace Execution:**
   - Enable APU trace logging
   - Log register writes, frame counter events, channel state

4. **Fix & Verify:**
   - Implement fix (adjust timing or behavior)
   - Verify no audio quality regressions
   - Run full APU test suite

## APU Timing Reference

### Frame Counter

| Mode | Quarter Frame | Half Frame | IRQ |
|------|---------------|------------|-----|
| **4-step** | 7,457 cycles | 14,913 cycles | 14,915 cycles |
| | 14,913 cycles | 29,829 cycles | 29,829 cycles |
| **5-step** | 7,457 cycles | 14,913 cycles | N/A |
| | 14,913 cycles | 22,371 cycles | N/A |
| | 18,641 cycles | 37,281 cycles | N/A |

### DMC Rates

| Value | NTSC Period | PAL Period |
|-------|-------------|------------|
| $0 | 428 | 398 |
| $1 | 380 | 354 |
| $2 | 340 | 316 |
| $3 | 320 | 298 |
| $4 | 286 | 276 |
| $5 | 254 | 236 |
| $6 | 226 | 210 |
| $7 | 214 | 198 |
| $8 | 190 | 176 |
| $9 | 160 | 148 |
| $A | 142 | 132 |
| $B | 128 | 118 |
| $C | 106 | 98 |
| $D | 84 | 78 |
| $E | 72 | 66 |
| $F | 54 | 50 |

## Version Target

v0.7.0
