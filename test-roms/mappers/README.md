# Mapper Test ROMs

This directory contains **57 test ROMs** for validating NES mapper implementations (bank switching, IRQ, mirroring).

## Test ROM Inventory

### Standard Tests (17 ROMs)

- **Mapper 0 (NROM)**: 1 test - Basic passthrough mapper
- **Mapper 1 (MMC1)**: 1 test - Shift register, bank switching
- **Mapper 4 (MMC3)**: 12 tests - IRQ counter, A12 detection (two complete suites)
- **Mapper 5 (MMC5)**: 6 tests - Advanced features, ExRAM modes

### Holy Mapperel Multi-Mapper Suite (40 ROMs)

Comprehensive bank switching validation covering 16 mapper types:

- **Mapper 0 (NROM)**: 3 tests - CHR-ROM/RAM variants
- **Mapper 1 (MMC1)**: 12 tests - 128KB/512KB PRG, 32KB/128KB CHR, SRAM variants
- **Mapper 2 (UxROM)**: 2 tests - 128KB PRG with CHR-RAM
- **Mapper 3 (CNROM)**: 1 test - 32KB PRG, 32KB CHR
- **Mapper 4 (MMC3)**: 4 tests - 128KB/256KB PRG, CHR-ROM/RAM
- **Mapper 7 (AxROM)**: 2 tests - 128KB PRG with CHR-RAM
- **Mapper 9 (MMC2)**: 1 test - 128KB PRG, 64KB CHR
- **Mapper 10 (MMC4)**: 2 tests - 128KB PRG, SRAM/WRAM
- **Mapper 11 (Color Dreams)**: 2 tests - 64KB PRG, CHR variants
- **Mapper 28 (Action 53)**: 2 tests - 512KB PRG, CHR-RAM
- **Mapper 34 (BNROM)**: 2 tests - 128KB PRG, CHR-RAM
- **Mapper 66 (GxROM)**: 1 test - 64KB PRG, 16KB CHR
- **Mapper 69 (FME-7)**: 2 tests - 128KB PRG, SRAM/WRAM
- **Mapper 78.3 (Jaleco)**: 1 test - 128KB PRG, 64KB CHR
- **Mapper 118 (TxSROM)**: 1 test - 128KB PRG, 64KB CHR
- **Mapper 180 (Holy Diver)**: 2 tests - 128KB PRG, CHR-RAM

**Total**: 57 mapper test ROMs

## Test ROM Sources

All test ROMs are obtained from community-maintained collections:

- **Primary Source**: [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)
- **Holy Mapperel**: [pinobatch/holy-mapperel](https://github.com/pinobatch/holy-mapperel) v0.02
- **Additional Source**: [TetaNES test_roms](https://github.com/lukexor/tetanes)
- **Authors**: Shay Green (blargg), Pino (Holy Mapperel), community contributors

## Downloaded Test ROMs

### Mapper 0 (NROM) Tests

| File | Author | Tests | Expected Result |
|------|--------|-------|-----------------|
| `nrom_368_test.nes` | Various | NROM 368 variant (16KB+16KB PRG, 8KB CHR) | Pass all tests |

**Description**: Tests basic NROM mapper functionality with no bank switching.

- **Source**: <https://github.com/christopherpow/nes-test-roms/tree/master/nrom368>
- **Features**: Basic memory mapping validation for games that don't use mappers

### Mapper 1 (MMC1) Tests

| File | Author | Tests | Expected Result |
|------|--------|-------|-----------------|
| `mmc1_a12.nes` | Various | A12 line behavior, shift register, banking modes | Pass all tests |

**Description**: Tests MMC1 mapper shift register behavior and PRG/CHR bank switching.

- **Source**: <https://github.com/christopherpow/nes-test-roms/tree/master/MMC1_A12>
- **Features**: Tests 5-bit shift register writes, banking modes (16KB/32KB PRG), CHR banking, mirroring control

### Mapper 4 (MMC3) Tests - Comprehensive Suite

Tests all aspects of MMC3 behavior including IRQ counter operation, bank switching, and register behavior.

| File | Author | Tests | Expected Result |
|------|--------|-------|-----------------|
| `mmc3_test_1_clocking.nes` | Shay Green (blargg) | Counter operation and clocking | Pass |
| `mmc3_test_2_details.nes` | Shay Green (blargg) | Counter details and edge cases | Pass |
| `mmc3_test_3_a12_clocking.nes` | Shay Green (blargg) | A12 line edge detection | Pass |
| `mmc3_test_4_scanline_timing.nes` | Shay Green (blargg) | Scanline counter timing | Pass |
| `mmc3_test_5_mmc3_rev_a.nes` | Shay Green (blargg) | MMC3 revision A behavior | Pass (Rev A only) |
| `mmc3_test_6_mmc6.nes` | Shay Green (blargg) | MMC6 variant behavior | Pass (MMC6 only) |

**Description**: Comprehensive test suite covering MMC3 scanline counter, IRQ generation, and bank switching.

- **Source**: <https://github.com/christopherpow/nes-test-roms/tree/master/mmc3_test>
- **Documentation**: See `ref-proj/nes-test-roms/mmc3_test/readme.txt`
- **Run Order**: Tests should be run in sequence (1-6) as later tests assume earlier ones pass

#### Test Details:

1. **Clocking**: Tests basic counter operation, manual toggling via $2006, IRQ generation
2. **Details**: Tests counter behavior with edge cases (counter at 255, disabled IRQ, flag clearing)
3. **A12 Clocking**: Tests clocking via bit 12 of VRAM address changes (read/write via $2006/$2007)
4. **Scanline Timing**: Tests timing for scanlines 0, 1, and 240
5. **MMC3 Rev A**: Tests revision A specific behavior (Crystalis cartridge)
6. **MMC6**: Tests MMC6 variant with battery-backed RAM

### Mapper 4 (MMC3) Tests - IRQ Counter Suite

Alternative MMC3 test suite focused specifically on IRQ counter behavior.

| File | Author | Tests | Expected Result |
|------|--------|-------|-----------------|
| `mmc3_irq_1_clocking.nes` | Shay Green (blargg) | IRQ counter clocking | Pass |
| `mmc3_irq_2_details.nes` | Shay Green (blargg) | IRQ counter details | Pass |
| `mmc3_irq_3_a12_clocking.nes` | Shay Green (blargg) | A12 edge detection | Pass |
| `mmc3_irq_4_scanline_timing.nes` | Shay Green (blargg) | Scanline timing | Pass |
| `mmc3_irq_5_rev_a.nes` | Shay Green (blargg) | MMC3 revision A | Pass (Rev A only) |
| `mmc3_irq_6_rev_b.nes` | Shay Green (blargg) | MMC3 revision B | Pass (Rev B only) |

**Description**: Focused test suite for MMC3 IRQ counter behavior on NTSC NES PPU.

- **Source**: <https://github.com/christopherpow/nes-test-roms/tree/master/mmc3_irq_tests>
- **Documentation**: See `ref-proj/nes-test-roms/mmc3_irq_tests/readme.txt`
- **Tested Hardware**: Verified on actual NES with multiple MMC3 cartridges
- **Run Order**: Tests should be run in sequence (1-6)

#### MMC3 Revision Differences:

- **Revision A** (Crystalis): Counter reload behavior differs when $C000 is written with 0
- **Revision B** (SMB3, Mega Man 3): Reloads and sets IRQ every clock when reload value is 0

### Mapper 5 (MMC5) Tests - 5 tests

| File | Author | Tests | Expected Result |
|------|--------|-------|-----------------|
| `mmc5_test.nes` | Various | Basic MMC5 functionality | Pass all tests |
| `mmc5test_v1.nes` | Various | MMC5 test suite v1 | Pass all tests |
| `mmc5test_v2.nes` | Various | MMC5 test suite v2 (updated) | Pass all tests |
| `mmc5exram.nes` | Various | ExRAM functionality | Pass all tests |
| `basics.nes` | TetaNES | MMC5 basics | Pass all tests |
| `exram.nes` | TetaNES | ExRAM modes | Pass all tests |

**Description**: Comprehensive MMC5 mapper testing including ExRAM, bank switching, and advanced features.

- **Sources**:
  - <https://github.com/christopherpow/nes-test-roms/tree/master/mmc5test>
  - <https://github.com/lukexor/tetanes> (TetaNES mapper tests)
- **Features**: Tests MMC5 bank switching, ExRAM modes (NT/EXT/RAM), PRG banking modes, CHR banking, split screen capabilities, IRQ scanline counter

## Test Results Format

### Output Methods

All mapper test ROMs report results via multiple methods:

1. **On-screen display**: Visual text output showing test results
2. **Memory at $6000**: Test status written to specific addresses
3. **Audible beeps**: Number of beeps indicates error code

### Memory Output ($6000)

| Address | Meaning |
|---------|---------|
| $6000 | Test status: $80 = running, $00-$7F = done (result code) |
| $6001-$6003 | Magic bytes: $DE $B0 $61 (identifies test ROM) |
| $6004+ | Text output (null-terminated string) |

### Result Codes

- **$00**: All tests passed
- **$01-$7F**: Specific test failure (see individual test documentation)

### Audible Output

Beeps report result code in binary:

- Low tone = 0 bit
- High tone = 1 bit
- Leading zeros skipped
- First tone always 0

Examples:

| Tones | Binary | Decimal | Meaning |
|-------|--------|---------|---------|
| low | 0 | 0 | Passed |
| low high | 01 | 1 | Failed |
| low high low | 010 | 2 | Error code 2 |

## Running Tests

```bash
# Run all mapper tests (when test harness is ready)
cargo test --package rustynes-mappers

# Run specific mapper tests
cargo test -p rustynes-mappers mapper_0_nrom
cargo test -p rustynes-mappers mapper_1_mmc1
cargo test -p rustynes-mappers mapper_4_mmc3

# Run with verbose output
cargo test -p rustynes-mappers -- --nocapture

# Run individual test ROM
cargo run -p rustynes-desktop -- test-roms/mappers/mmc3_test_1_clocking.nes
```

## Expected Behavior

### Passing Test

- Screen displays "Passed" or test number with "OK"
- Memory address $6000 contains $00
- Single low-tone beep

### Failing Test

- Screen displays error code or "Failed"
- Memory address $6000 contains non-zero error code
- Multiple beeps indicating error code in binary
- Specific failure message may indicate the issue

### Important Notes

- Some flashes or odd sounds during testing are normal - only final result matters
- Tests marked "done" (not "passed") require manual observation during execution
- MMC3 Rev A and Rev B tests are mutually exclusive - only one should pass per hardware

## Mapper Implementation Priority

Based on game compatibility and test ROM availability:

1. **Mapper 0 (NROM)**: Required first, simplest mapper (no bank switching)
2. **Mapper 4 (MMC3)**: Most important, ~25% of games, complex IRQ behavior
3. **Mapper 1 (MMC1)**: ~30% of games, shift register complexity
4. **Mapper 2 (UxROM)**: ~10% of games, simple PRG banking
5. **Mapper 3 (CNROM)**: ~8% of games, simple CHR banking
6. **Mapper 5 (MMC5)**: Advanced features, used by Castlevania 3, Koei games

## Current Status

| Mapper | Status | Test ROMs | Notes |
|--------|--------|-----------|-------|
| NROM (0) | Not implemented | 4 | Foundation for all mappers |
| MMC1 (1) | Not implemented | 13 | Comprehensive test coverage |
| UxROM (2) | Not implemented | 2 | Holy Mapperel PRG banking tests |
| CNROM (3) | Not implemented | 1 | Holy Mapperel CHR banking test |
| MMC3 (4) | Not implemented | 16 | Comprehensive IRQ + banking tests |
| MMC5 (5) | Not implemented | 6 | Basic test ROM available |
| AxROM (7) | Not implemented | 2 | Holy Mapperel tests |
| MMC2 (9) | Not implemented | 1 | Holy Mapperel test |
| MMC4 (10) | Not implemented | 2 | Holy Mapperel tests |

## Holy Mapperel Test Suite

The Holy Mapperel test suite (v0.02) provides comprehensive multi-mapper bank switching validation.

### Naming Convention

Files follow pattern: `mapper_holymapperel_{mapper}_{config}.nes`

| Suffix | Meaning |
|--------|---------|
| `P{size}K` | PRG-ROM size in KB |
| `C{size}K` | CHR-ROM size in KB |
| `CR{size}K` | CHR-RAM size in KB |
| `S{size}K` | Battery-backed SRAM |
| `W{size}K` | Work RAM (volatile) |
| `H` | Horizontal mirroring |
| `V` | Vertical mirroring |

### Example Files

| File | Description |
|------|-------------|
| `mapper_holymapperel_2_P128K_V.nes` | UxROM 128KB PRG, vertical mirroring |
| `mapper_holymapperel_3_P32K_C32K_H.nes` | CNROM 32KB PRG, 32KB CHR, horizontal |
| `mapper_holymapperel_1_P128K_C128K_S8K.nes` | MMC1 128KB PRG, 128KB CHR, 8KB battery SRAM |

### Test Features

- **PRG Banking**: Validates switchable/fixed bank configurations
- **CHR Banking**: Tests 4KB/8KB CHR bank switching
- **Mirroring**: Validates nametable mirroring modes
- **SRAM/WRAM**: Validates save RAM functionality
- **Visual Feedback**: Displays test results on screen

## MMC3 Implementation Notes

### Critical Behaviors

From blargg's testing on actual hardware:

1. **Manual Clocking**: Counter can be clocked via bit 12 of VRAM address even when $2000 = $00
2. **Flag Behavior**: IRQ flag NOT set when counter cleared by writing $C001
3. **Counter Reload**: Different behavior between Rev A and Rev B when $C000 = 0
4. **A12 Detection**: Counter clocks on rising edge of A12 (0→1 transition)

### Revision Differences

**Revision A (Crystalis)**:
- IRQ set when reloading to 0 after clear
- Counter frozen in pathological $C001 write sequences

**Revision B (SMB3, Mega Man 3)**:
- Writing 0 to $C000 causes reload every clock cycle
- IRQ set when counter is 0 after reloading
- Counter ORed with $80 in pathological cases

### Implementation Strategy

1. Implement basic counter decrement and reload
2. Add A12 edge detection (rising edge 0→1)
3. Implement IRQ flag set/clear behavior
4. Add $C000/$C001 register behavior
5. Choose Rev A or Rev B behavior (or make configurable)
6. Test with comprehensive test suite

## References

- [NESdev Wiki: Mappers](https://www.nesdev.org/wiki/Mapper)
- [NESdev Wiki: MMC1](https://www.nesdev.org/wiki/MMC1)
- [NESdev Wiki: MMC3](https://www.nesdev.org/wiki/MMC3)
- [NESdev Wiki: MMC5](https://www.nesdev.org/wiki/MMC5)
- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [blargg's Test ROMs](https://github.com/christopherpow/nes-test-roms)
- [MMC3 Scanline Counter](https://www.nesdev.org/wiki/MMC3_scanline_counter)

## License

Test ROMs are created by their respective authors:

- MMC3 tests: Created by Shay Green (blargg)
- MMC1 A12 test: Various contributors
- NROM 368 test: Various contributors
- MMC5 test: Various contributors
- Holy Mapperel: Created by Pino (pinobatch) - [GitHub](https://github.com/pinobatch/holy-mapperel)

All test ROMs are used for educational and emulator development purposes.
