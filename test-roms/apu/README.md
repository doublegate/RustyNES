# APU Test ROMs

This directory contains test ROMs for validating APU (Audio Processing Unit) accuracy.

## Required Test ROMs

### Blargg APU Test Suite (blargg_apu_2005.07.30)

Primary test suite for APU validation. Download from:

- **Source**: <https://github.com/christopherpow/nes-test-roms>
- **Direct**: <https://github.com/christopherpow/nes-test-roms/tree/master/blargg_apu_2005.07.30>

Tests included:
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

### APU Additional Tests

Download from: <https://github.com/christopherpow/nes-test-roms/tree/master/apu_test>

- `apu_test.nes` - General APU functionality
- `square_timer_div2.nes` - Square wave timer divider test
- `dmc_tests/` - DMC channel tests (multiple ROMs)

### Expected Directory Structure

```text
test-roms/apu/
├── README.md (this file)
├── blargg_apu_2005.07.30/
│   ├── 01.len_ctr.nes
│   ├── 02.len_table.nes
│   ├── 03.irq_flag.nes
│   ├── 04.clock_jitter.nes
│   ├── 05.len_timing_mode0.nes
│   ├── 06.len_timing_mode1.nes
│   ├── 07.irq_flag_timing.nes
│   ├── 08.irq_timing.nes
│   ├── 09.reset_timing.nes
│   ├── 10.len_halt_timing.nes
│   └── 11.len_reload_timing.nes
├── apu_test/
│   └── apu_test.nes
├── square_timer_div2.nes
└── dmc_tests/
    └── (various DMC test ROMs)
```

## Test ROM Results

Test ROMs report results via memory location `$6000`:
- `0x00` = Pass
- Other values = Fail (error code)

Some tests also output text to screen or write ASCII results to `$6004+`.

## Acquisition Instructions

1. Clone the test ROM repository:
   ```bash
   cd /tmp
   git clone https://github.com/christopherpow/nes-test-roms.git
   ```

2. Copy APU test ROMs:
   ```bash
   cp -r /tmp/nes-test-roms/blargg_apu_2005.07.30 /home/parobek/Code/RustyNES/test-roms/apu/
   cp -r /tmp/nes-test-roms/apu_test /home/parobek/Code/RustyNES/test-roms/apu/
   ```

3. Cleanup:
   ```bash
   rm -rf /tmp/nes-test-roms
   ```

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
