# APU Test ROMs

This directory contains **92 test ROMs** for validating APU (Audio Processing Unit) accuracy.

## Complete Test ROM Collection

### Blargg APU Test Suite (blargg_apu_2005.07.30) - 11 tests

Primary test suite for APU validation:

1. `01.len_ctr.nes` - Length counter behavior
2. `02.len_table.nes` - Length counter lookup table
3. `03.irq_flag.nes` - Frame IRQ flag
4. `04.clock_jitter.nes` - Frame counter jitter
5. `05.len_timing_mode0.nes` - Length counter timing (4-step)
6. `06.len_timing_mode1.nes` - Length counter timing (5-step)
7. `07.irq_flag_timing.nes` - IRQ flag timing
8. `08.irq_timing.nes` - IRQ generation timing
9. `09.reset_timing.nes` - Reset timing
10. `10.len_halt_timing.nes` - Length halt timing
11. `11.len_reload_timing.nes` - Length reload timing

**Source**: [blargg_apu_2005.07.30](https://github.com/christopherpow/nes-test-roms/tree/master/blargg_apu_2005.07.30)

### APU Test Suite (apu_test) - 8 tests

General APU functionality tests:

- `apu_test.nes` - Complete APU test suite (all tests in one ROM)
- `1-len_ctr.nes` through `8-dmc_rates.nes` - Individual tests

Tests length counter, IRQ flag, jitter, timing, DMC basics/rates

**Source**: [apu_test](https://github.com/christopherpow/nes-test-roms/tree/master/apu_test)

### DMC (Delta Modulation Channel) Tests (dmc_tests) - 4 tests

Tests DMC sample playback and DMA behavior:

- `buffer_retained.nes` - Sample buffer retained after playback
- `latency.nes` - DMC start latency
- `status.nes` - DMC status register behavior
- `status_irq.nes` - DMC IRQ flag behavior

**Source**: [dmc_tests](https://github.com/christopherpow/nes-test-roms/tree/master/dmc_tests)

### Comprehensive APU Tests (64 tests)

From TetaNES test suite - exhaustive APU behavior tests:

#### Core Channel Tests

- `len_ctr.nes`, `len_table.nes`, `len_timing.nes` - Length counter
- `len_halt_timing.nes`, `len_reload_timing.nes` - Length counter edge cases
- `irq_flag.nes`, `irq_flag_timing.nes`, `irq_timing.nes` - Frame IRQ
- `clock_jitter.nes`, `pal_clock_jitter.nes` - Frame counter jitter
- `lin_ctr.nes` - Triangle linear counter

#### Square Wave Tests

- `square.nes`, `square_pitch.nes` - Square wave channels
- `square_sweep.nes` - Sweep unit behavior
- `square_timer_div2.nes` - Timer divider

#### Triangle Wave Tests

- `triangle.nes`, `triangle_pitch.nes` - Triangle channel

#### Noise Tests

- `noise.nes`, `noise_pitch.nes` - Noise channel

#### DMC Comprehensive Tests (17 tests)

- `dmc.nes`, `dmc_basics.nes`, `dmc_rates.nes` - Basic DMC operation
- `dmc_buffer_retained.nes` - Buffer behavior
- `dmc_latency.nes` - Start latency
- `dmc_pitch.nes` - Sample rate control
- `dmc_status.nes`, `dmc_status_irq.nes` - Status/IRQ flags
- `dmc_dma_2007_read.nes`, `dmc_dma_2007_write.nes` - DMA vs PPU reads/writes
- `dmc_dma_4016_read.nes` - DMA vs controller reads
- `dmc_dma_double_2007_read.nes` - Double DMA reads
- `dmc_dma_read_write_2007.nes` - Combined DMA operations
- `dpcmletterbox.nes` - DPCM sample playback visual test

#### APU Mixer Tests (4 tests)

- `apu_mixer_square.nes` - Square wave mixing
- `apu_mixer_triangle.nes` - Triangle wave mixing
- `apu_mixer_noise.nes` - Noise channel mixing
- `apu_mixer_dmc.nes` - DMC mixing

#### Other Tests

- `apu_env.nes` - Envelope generator
- `apu_reset.nes` - APU reset behavior
- `reset_timing.nes` - Reset timing

**Source**: [TetaNES test_roms/apu](https://github.com/lukexor/tetanes/tree/main/tetanes-core/test_roms/apu)

### Directory Structure

```text
test-roms/apu/
├── README.md (this file)
├── blargg_apu_2005.07.30/ (11 tests)
├── apu_test/ (8 tests)
├── dmc_tests/ (4 tests)
└── [64 individual test ROMs]
    ├── Channel tests (square, triangle, noise)
    ├── DMC tests (17 comprehensive DMC tests)
    ├── Mixer tests (4 channel mixer tests)
    ├── Timing tests (length, IRQ, jitter)
    └── Reset tests
```

## Test ROM Results

Test ROMs report results via memory location `$6000`:
- `0x00` = Pass
- Other values = Fail (error code)

Some tests also output text to screen or write ASCII results to `$6004+`.

## Test ROM Sources

All 92 test ROMs are included in this directory, obtained from:

1. **[christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)** - blargg test suites, DMC tests
2. **[TetaNES test_roms](https://github.com/lukexor/tetanes)** - Comprehensive APU tests, mixer tests
3. **[NESdev Wiki](https://www.nesdev.org/wiki/Emulator_tests)** - Test documentation

## Integration

Test ROM runner will be implemented in:
- `crates/rustynes-apu/tests/test_roms.rs`

## Documentation

For detailed test information, see:

- [docs/testing/TEST_ROM_GUIDE.md](../../docs/testing/TEST_ROM_GUIDE.md)
- [APU Test ROMs section](../../docs/testing/TEST_ROM_GUIDE.md#apu-test-roms)
- [NESdev Wiki - Emulator Tests](<https://www.nesdev.org/wiki/Emulator_tests>)

## Target: 95%+ Pass Rate

Goal for Milestone 3 completion: Pass 95% or more of Blargg APU test suite.
