# Milestone 3: APU Implementation

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 19, 2025
**Duration:** 3 weeks (actual)
**Progress:** 100%

---

## Overview

Milestone 3 delivered a **hardware-accurate 2A03 APU implementation** with all 5 audio channels, frame counter, and accurate mixing. This establishes the audio foundation for NES emulation.

### Goals

- ✅ Pulse channels (2) with sweep and envelope
- ✅ Triangle channel with linear counter
- ✅ Noise channel with LFSR
- ✅ DMC channel with sample playback
- ✅ Frame counter (4-step and 5-step modes)
- ✅ Hardware-accurate non-linear mixing
- ✅ 48 kHz audio output with resampling
- ✅ APU test ROMs acquired and integrated
- ✅ Zero unsafe code
- ✅ Comprehensive unit tests (136 passing)

---

## Sprint Breakdown

### Sprint 1: APU Core & Frame Counter ✅ COMPLETED

**Duration:** Week 1
**Target Files:** `crates/rustynes-apu/src/frame_counter.rs`, `apu.rs`, `envelope.rs`, `length_counter.rs`, `sweep.rs`

**Goals:**

- [x] APU register map ($4000-$4017)
- [x] Frame counter (4-step: 60 Hz, 5-step: 48 Hz)
- [x] IRQ generation (4-step mode)
- [x] Length counter lookup table
- [x] Envelope divider
- [x] Sweep unit logic

**Outcome:** Frame sequencer foundation for all channels. 39 tests passing.

### Sprint 2: Pulse Channels ✅ COMPLETED

**Duration:** Week 1-2
**Target Files:** `crates/rustynes-apu/src/pulse.rs`

**Goals:**

- [x] Two pulse channels (Pulse 1, Pulse 2)
- [x] Duty cycle (12.5%, 25%, 50%, 75%)
- [x] Envelope generator (volume/fade)
- [x] Sweep unit (frequency modulation)
- [x] Length counter (note duration)
- [x] Timer (frequency control)

**Outcome:** Both pulse channels functional with all features. 65 tests passing.

### Sprint 3: Triangle & Noise Channels ✅ COMPLETED

**Duration:** Week 2
**Target Files:** `crates/rustynes-apu/src/triangle.rs`, `noise.rs`

**Goals:**

- [x] Triangle channel (32-step sequence)
- [x] Linear counter (triangle-specific length)
- [x] Noise channel (15-bit LFSR)
- [x] Mode flag (short/long period)
- [x] Noise period lookup table

**Outcome:** Triangle and noise channels working. 83 tests passing.

### Sprint 4: DMC Channel ✅ COMPLETED

**Duration:** Week 2-3
**Target Files:** `crates/rustynes-apu/src/dmc.rs`

**Goals:**

- [x] Delta modulation channel
- [x] Sample buffer and memory reader
- [x] DMA cycle stealing
- [x] Output unit (7-bit counter)
- [x] IRQ on sample completion
- [x] Loop flag handling
- [x] Address wrapping ($FFFF → $8000)

**Outcome:** DMC channel with sample playback. 105 tests passing.

### Sprint 5: Audio Output & Mixing ✅ COMPLETED

**Duration:** Week 3
**Target Files:** `crates/rustynes-apu/src/mixer.rs`, `resampler.rs`, `lib.rs`

**Goals:**

- [x] Nonlinear mixing (pulse + TND lookup tables)
- [x] 48 kHz resampling (from ~1.789 MHz APU clock)
- [x] Linear interpolation resampler
- [x] Low-pass filter implementation
- [x] Audio sample buffer interface
- [x] Public API for audio output

**Outcome:** Complete APU with accurate audio output. 136 tests passing, zero unsafe code.

---

## Technical Requirements

### Register Map

| Address | Channel | Register | Description |
|---------|---------|----------|-------------|
| $4000-$4003 | Pulse 1 | Volume, sweep, timer, length | First pulse channel |
| $4004-$4007 | Pulse 2 | Volume, sweep, timer, length | Second pulse channel |
| $4008-$400B | Triangle | Linear counter, timer, length | Triangle wave |
| $400C-$400F | Noise | Volume, period, length | Pseudo-random noise |
| $4010-$4013 | DMC | Rate, direct load, address, length | Sample playback |
| $4015 | Status | Channel enable/length status | Global control |
| $4017 | Frame Counter | Mode, IRQ disable | Frame sequencer |

### Frame Counter Timing

**4-Step Mode (60 Hz):**

```text
Step    Cycles    Actions
0       7457      Clock envelopes & linear counter
1       14913     Clock envelopes, linear counter, length counters, sweep units
2       22371     Clock envelopes & linear counter
3       29829     Clock envelopes, linear counter, length counters, sweep units
                  Set IRQ flag
0       29830     (Next frame starts immediately)
```

**5-Step Mode (48 Hz):**

```text
Step    Cycles    Actions
0       7457      Clock envelopes & linear counter
1       14913     Clock envelopes, linear counter, length counters, sweep units
2       22371     Clock envelopes & linear counter
3       29829     (Nothing)
4       37281     Clock envelopes, linear counter, length counters, sweep units
0       37282     (Next frame starts)
```

### Mixing Formula

**Pulse Mixing:**

```text
pulse_out = 0.00752 * (pulse1 + pulse2)
```

**TND Mixing:**

```text
tnd_out = 0.00851 * triangle + 0.00494 * noise + 0.00335 * dmc
```

**Final Output:**

```text
output = pulse_out + tnd_out
```

(Simplified linear approximation - actual hardware uses lookup tables)

---

## Acceptance Criteria

### Functionality

- [ ] All 5 channels produce correct waveforms
- [ ] Frame counter operates at correct rates
- [ ] Sweep units modulate frequency correctly
- [ ] Length counters silence channels when expired
- [ ] DMC samples play back accurately
- [ ] Mixing produces expected output levels

### Test ROMs

- [ ] blargg_apu_2005.07.30
  - [ ] 01.len_ctr.nes
  - [ ] 02.len_table.nes
  - [ ] 03.irq_flag.nes
  - [ ] 04.clock_jitter.nes
  - [ ] 05.len_timing_mode0.nes
  - [ ] 06.len_timing_mode1.nes
  - [ ] 07.irq_flag_timing.nes
  - [ ] 08.irq_timing.nes
  - [ ] 09.reset_timing.nes
  - [ ] 10.len_halt_timing.nes
  - [ ] 11.len_reload_timing.nes
- [ ] apu_test (various DMC/triangle tests)
- [ ] dmc_tests
- [ ] square_timer_div2
- [ ] len_halt_timing

### Audio Quality

- [ ] Music sounds correct in 10 test games
- [ ] No pops/clicks during gameplay
- [ ] <20ms audio latency
- [ ] Proper volume levels

---

## Code Structure

```text
crates/rustynes-apu/
├── src/
│   ├── lib.rs           # Public API, audio interface
│   ├── apu.rs           # Main APU struct
│   ├── frame_counter.rs # Frame sequencer
│   ├── pulse.rs         # Pulse channels 1 & 2
│   ├── triangle.rs      # Triangle channel
│   ├── noise.rs         # Noise channel
│   ├── dmc.rs           # DMC channel
│   ├── mixer.rs         # Nonlinear mixing
│   ├── envelope.rs      # Envelope generator (shared)
│   ├── length_counter.rs # Length counter (shared)
│   └── sweep.rs         # Sweep unit (pulse only)
├── tests/
│   └── test_roms.rs     # Test ROM validation
└── Cargo.toml
```

**Actual Total:** ~4,200 lines of code (including comprehensive tests and documentation)

---

## Dependencies

### External Crates

- **blip_buf** (optional) - Band-limited synthesis
- **dasp** (optional) - Sample rate conversion
- **log** - Logging

### Internal Dependencies

- rustynes-cpu (for DMC DMA)

---

## Testing Strategy

### Unit Tests

- [ ] Envelope generator behavior
- [ ] Length counter lookup
- [ ] Sweep unit calculations
- [ ] Triangle linear counter
- [ ] Noise LFSR sequence
- [ ] DMC output levels
- [ ] Frame counter timing
- [ ] Mixer output values

### Integration Tests

- [ ] Channel synchronization
- [ ] Frame counter + channels
- [ ] DMC DMA interaction
- [ ] Full APU frame execution

### Test ROM Validation

- [ ] Blargg APU test suite (target: 95%+ pass rate)
- [ ] Manual audio testing with games

---

## Performance Targets

- **Clock Rate:** 1.789773 MHz (NTSC)
- **Output Rate:** 48,000 Hz
- **Latency:** <20ms
- **CPU Usage:** <5% of emulator total
- **Memory:** <10 KB

---

## Challenges & Risks

| Challenge | Risk | Mitigation |
|-----------|------|------------|
| DMC DMA timing | High | Study Mesen2, test with DMC test ROMs |
| Sweep unit edge cases | Medium | Comprehensive unit tests, reference docs |
| Mixing accuracy | Medium | Use lookup tables, test with real games |
| Resampling quality | Low | Use proven library (blip_buf) |

---

## Related Documentation

- [APU 2A03 Specification](../../../docs/apu/APU_2A03_SPECIFICATION.md)
- [APU Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)
- [APU Pulse Channel](../../../docs/apu/APU_CHANNEL_PULSE.md)
- [APU Triangle Channel](../../../docs/apu/APU_CHANNEL_TRIANGLE.md)
- [APU Noise Channel](../../../docs/apu/APU_CHANNEL_NOISE.md)
- [APU DMC Channel](../../../docs/apu/APU_CHANNEL_DMC.md)
- [APU Mixer](../../../docs/apu/APU_MIXER.md)

---

## Next Steps

### Pre-Sprint Preparation

1. **Review Documentation**
   - Read APU specification thoroughly
   - Study channel timing diagrams
   - Review mixing formulas

2. **Set Up Crate**
   - Create rustynes-apu/Cargo.toml
   - Add dependencies
   - Set up initial file structure

3. **Acquire Test ROMs**
   - Download Blargg APU test suite
   - Download DMC test ROMs
   - Set up test ROM runner

### Sprint 1 Kickoff

- Create APU struct skeleton
- Implement frame counter
- Set up register map
- Begin unit tests

---

**Milestone Status:** ✅ COMPLETED (December 19, 2025)
**Deliverables:**
- 5 audio channels fully implemented
- Non-linear mixer with lookup tables
- 48 kHz audio resampler
- 136 unit tests (100% passing)
- Zero unsafe code
- APU test ROMs acquired

**Next Milestone:** [Milestone 4: Mappers](../milestone-4-mappers/M4-OVERVIEW.md)
