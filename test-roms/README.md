# NES Test ROMs

This directory contains test ROMs for validating NES emulator accuracy. Test ROMs verify CPU, PPU, APU, and mapper implementations against known-good behavior.

## Directory Structure

```text
test-roms/
├── cpu/         # 6502/2A03 CPU tests (nestest.nes, blargg tests)
├── ppu/         # 2C02 PPU tests (VBL/NMI, sprite hit, rendering)
├── apu/         # 2A03 APU tests (audio channels, timing)
└── mappers/     # Mapper tests (MMC1, MMC3, MMC5, NROM)
```

## Test Suites

### CPU Tests (`cpu/`)

Test ROMs for validating 6502 CPU implementation:

- **nestest.nes**: Gold standard CPU validation (all 256 opcodes)
- **Golden log**: 5003+ instruction trace for automated validation
- **Blargg tests**: Instruction timing, addressing modes, edge cases

See `cpu/README.md` for complete documentation.

**Key Tests**:

- All 151 official opcodes
- 105 unofficial opcodes
- All 13 addressing modes
- Flag behavior (N, Z, C, V)
- Interrupt handling

### PPU Tests (`ppu/`)

Test ROMs for validating PPU (Picture Processing Unit) implementation:

- **VBL/NMI tests**: VBlank flag timing, NMI generation
- **Sprite hit tests**: Sprite 0 collision detection
- **Palette tests**: Palette RAM behavior
- **VRAM tests**: VRAM access patterns

See `ppu/README.md` for complete documentation.

**Key Tests**:

- VBlank flag set/clear timing
- NMI timing and suppression
- Sprite 0 hit detection
- Even/odd frame behavior
- OAM (sprite) memory

### APU Tests (`apu/`)

Test ROMs for validating APU (Audio Processing Unit) implementation:

- **Channel tests**: Square, triangle, noise, DMC
- **Timing tests**: Frame counter, IRQ timing
- **Mixer tests**: Audio mixing and volume
- **DMC tests**: Delta modulation channel

See `apu/README.md` for complete documentation.

**Key Tests**:

- Square wave channels (duty cycle, envelope, sweep)
- Triangle wave channel
- Noise channel (period, length counter)
- DMC sample playback
- Frame counter (4-step, 5-step modes)

### Mapper Tests (`mappers/`)

Test ROMs for validating mapper implementations (bank switching, IRQ, mirroring):

- **NROM (Mapper 0)**: Basic passthrough mapper
- **MMC1 (Mapper 1)**: Shift register, bank switching
- **MMC3 (Mapper 4)**: IRQ counter, A12 detection (13 test ROMs)
- **MMC5 (Mapper 5)**: Advanced features, ExRAM (3 test ROMs)
- **Missing**: UxROM (Mapper 2), CNROM (Mapper 3) - Available via [Holy Mapperel](https://github.com/pinobatch/holy-mapperel)

See `mappers/README.md` for complete documentation.

**Key Tests**:

- Bank switching (PRG/CHR)
- IRQ counter operation (MMC3)
- A12 line edge detection
- Shift register behavior (MMC1)
- Mirroring control

## Test ROM Inventory

As of December 2025, this collection contains **172 unique test ROMs** (31 duplicates removed):

- **CPU Tests**: 36 ROMs (instruction validation, timing, interrupts, DMA)
- **PPU Tests**: 49 ROMs (VBL/NMI, sprite hit, sprite overflow, OAM, palette)
- **APU Tests**: 64 ROMs (all channels, timing, mixer, DMC, frame counter)
- **Mapper Tests**: 17 ROMs (NROM, MMC1, MMC3, MMC5, bank switching, IRQ)

**Organization**: All test ROMs follow standardized naming: `{category}_{test_name}.nes`
**Verification**: See `CHECKSUMS.md` for MD5 checksums of all test ROMs

## Test ROM Sources

All test ROMs are obtained from verified, community-maintained collections:

### Primary Sources

1. **[christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)**
   - Comprehensive collection of community test ROMs
   - Includes blargg's test suites, nestest, sprite tests
   - 263 test ROMs in complete archive

2. **[TetaNES Test ROMs](https://github.com/lukexor/tetanes)**
   - High-quality Rust NES emulator test suite
   - Extensive PPU, CPU, and APU coverage
   - Source for many edge case tests

3. **[NESdev Wiki - Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)**
   - Authoritative list of test ROMs
   - Links to original test ROM downloads
   - Test ROM documentation and expected behavior

### Test ROM Authors

- **Kevin Horton (kevtris)**: nestest.nes - Gold standard CPU validation
- **Shay Green (blargg)**: CPU, PPU, APU test suites - Comprehensive timing tests
- **Brad Smith (rainwarrior)**: Various sprite and PPU tests
- **Community Contributors**: MMC3 IRQ tests, mapper tests, edge case tests

## Running Tests

```bash
# Run all tests
cargo test --workspace

# Run specific component tests
cargo test -p rustynes-cpu
cargo test -p rustynes-ppu
cargo test -p rustynes-apu
cargo test -p rustynes-mappers

# Run individual test ROM (desktop GUI)
cargo run -p rustynes-desktop -- test-roms/cpu/nestest.nes

# Run with verbose output
cargo test -p rustynes-cpu -- --nocapture
```

## Test Result Format

Most test ROMs report results via multiple methods:

### On-screen Display

Visual text output showing:

- Test name
- Pass/fail status
- Error codes for failures

### Memory Output ($6000)

| Address | Meaning |
|---------|---------|
| $6000 | Test status: $80 = running, $00 = passed, $01-$7F = error code |
| $6001-$6003 | Magic bytes: $DE $B0 $61 (identifies test ROM) |
| $6004+ | Text output (null-terminated string) |

### Audible Output

Beeps indicate test result:

- Single low tone = Passed
- Multiple tones = Error code in binary (low = 0, high = 1)

## Accuracy Targets

Based on TASVideos accuracy test suite and community standards:

| Component | Target | Critical Tests |
|-----------|--------|----------------|
| **CPU** | 100% | nestest.nes golden log match |
| **PPU** | 100% | blargg VBL/NMI, sprite hit tests |
| **APU** | 95%+ | blargg APU tests (timing-sensitive) |
| **Mappers** | 100% | MMC1/MMC3/MMC5 test suites |
| **Overall** | 100% | TASVideos 156-test accuracy suite |

## Implementation Order

Recommended test ROM validation order:

### Phase 1: MVP (Months 1-6)

1. **CPU**: nestest.nes (automated mode, PC = $C000)
2. **PPU**: Basic VBL/NMI tests (01-vbl_basics.nes)
3. **Mappers**: NROM (mapper 0) basic functionality
4. **APU**: Basic square wave tests

### Phase 2: Advanced Features (Months 7-12)

1. **PPU**: Complete VBL/NMI suite, sprite 0 hit tests
2. **Mappers**: MMC1, MMC3 IRQ tests
3. **APU**: All 5 channels, timing tests
4. **CPU**: Blargg instruction timing tests

### Phase 3: 100% Accuracy (Months 13-24)

1. **Mappers**: MMC5, rare mappers
2. **PPU**: Edge case tests, PPU open bus
3. **APU**: DMC DMA conflicts, frame counter edge cases
4. **TASVideos**: Full 156-test accuracy suite

## Current Status

| Test Suite | Implemented | Passing | Notes |
|------------|-------------|---------|-------|
| nestest.nes | Yes | 100% | All 5003+ instructions match golden log |
| CPU timing | No | 0% | Awaiting implementation |
| PPU VBL/NMI | Partial | ~40% | Basic timing working |
| Sprite hit | No | 0% | Awaiting sprite rendering |
| APU channels | No | 0% | Awaiting implementation |
| NROM (0) | No | 0% | Foundation mapper |
| MMC1 (1) | No | 0% | Test ROM available |
| MMC3 (4) | No | 0% | 12 test ROMs available |

## Additional Resources

### Documentation

- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [Test ROM Documentation](https://github.com/christopherpow/nes-test-roms/blob/master/status.txt)
- [TASVideos Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)

### Reference Implementations

See `ref-proj/` directory for reference emulator implementations:

- **Mesen2**: Gold standard accuracy, excellent debugger
- **FCEUX**: TAS tools, extensive mapper support
- **TetaNES**: Rust implementation with test suite integration

### Automated Testing

The project includes automated test harness for ROM validation:

```rust
// Example: CPU test integration
#[test]
fn test_nestest_automated() {
    let mut nes = Nes::new();
    nes.load_rom("test-roms/cpu/nestest.nes");
    nes.cpu.pc = 0xC000; // Automated mode

    let result = run_until_complete(&mut nes);
    assert_eq!(result, 0x00); // $6000 should contain 0x00
}
```

## Organization and Deduplication

**Date**: December 19, 2025
**Action**: Comprehensive deduplication and standardization

### Changes Made

1. **Duplicate Removal**: 31 duplicate files removed (203 → 172 unique ROMs)
   - Removed duplicates from subdirectories (blargg_apu_2005.07.30/, apu_test/rom_singles/, dmc_tests/)
   - Kept files in root category directories with standardized names
   - Space saved: ~2.5 MB

2. **Standardized Naming Convention**:
   - All CPU tests: `cpu_*.nes`
   - All PPU tests: `ppu_*.nes`
   - All APU tests: `apu_*.nes`
   - All Mapper tests: `mapper_*.nes`

3. **Checksum Verification**: Created `CHECKSUMS.md` with MD5 hashes for all 172 files

4. **Source Archives Preserved**: Original test suite directories retained for source code reference

### Duplicate Analysis Summary

Duplicates identified by MD5 checksum matching across directories:

- **APU Tests**: 22 duplicates (blargg suite files copied to root)
- **PPU Tests**: 5 duplicates (numbered sprite hit tests)
- **CPU Tests**: 1 duplicate (registers test)
- **Mapper Tests**: 3 duplicates (MMC5 test versions)

All duplicates verified before removal via checksum matching.

## Contributing

When adding new test ROMs:

1. Place ROM in appropriate subdirectory (`cpu/`, `ppu/`, `apu/`, `mappers/`)
2. Follow naming convention: `{category}_{test_name}.nes`
3. Update subdirectory README.md with test description
4. Add automated test in corresponding crate
5. Update `CHECKSUMS.md` with MD5 checksum
6. Document expected behavior and known issues

## License

Test ROMs are created by their respective authors:

- **nestest.nes**: Kevin Horton (kevtris)
- **blargg tests**: Shay Green (blargg)
- **Various tests**: Community contributors

All test ROMs are used for educational and emulator development purposes.
