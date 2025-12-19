# PPU Test ROMs

This directory contains test ROMs for validating PPU (Picture Processing Unit) implementation.

## Test ROM Sources

All test ROMs are downloaded from the community-maintained collection:

- **Primary Source**: [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)

## Downloaded Test ROMs

### VBlank/NMI Tests (blargg)

Tests VBlank flag and NMI timing with PPU clock accuracy.

- **ppu_vbl_nmi.nes** - Complete test suite (all tests in one ROM)
  - Source: <https://github.com/christopherpow/nes-test-roms/tree/master/ppu_vbl_nmi>
  - Tests: VBlank basics, set/clear timing, NMI control, frame timing

- **01-vbl_basics.nes** - Basic VBlank flag behavior
  - Source: <https://github.com/christopherpow/nes-test-roms/blob/master/ppu_vbl_nmi/rom_singles/01-vbl_basics.nes>
  - Tests: VBL flag set/clear, $2002 read behavior, mirroring

- **02-vbl_set_time.nes** - Exact VBlank set timing
  - Source: <https://github.com/christopherpow/nes-test-roms/blob/master/ppu_vbl_nmi/rom_singles/02-vbl_set_time.nes>
  - Tests: VBL flag set at cycle 1 of scanline 241

- **03-vbl_clear_time.nes** - Exact VBlank clear timing
  - Source: <https://github.com/christopherpow/nes-test-roms/blob/master/ppu_vbl_nmi/rom_singles/03-vbl_clear_time.nes>
  - Tests: VBL flag clear at cycle 1 of pre-render scanline

### Sprite Hit Tests (Quietust, 2005)

Tests sprite 0 hit detection with pixel-perfect accuracy.

- **01.basics.nes** - Basic sprite 0 hit behavior
  - Source: <https://github.com/christopherpow/nes-test-roms/tree/master/sprite_hit_tests_2005.10.05>
  - Tests: Basic collision detection, background/sprite interaction

- **02.alignment.nes** - Sprite/background alignment
  - Source: <https://github.com/christopherpow/nes-test-roms/tree/master/sprite_hit_tests_2005.10.05>
  - Tests: Pixel-perfect alignment requirements

## Test Results Format

Most test ROMs report results via address $6000:

- **$00**: Test passed
- **$01-$FF**: Test failed (error code)

Some tests also display results on-screen and beep a number of times equal to the error code.

## Running Tests

```bash
# Run all PPU tests
cargo test -p rustynes-ppu --test ppu_test_roms

# Run specific test
cargo test -p rustynes-ppu --test ppu_test_roms test_ppu_vbl_basics

# Run with output
cargo test -p rustynes-ppu --test ppu_test_roms -- --nocapture
```

## Expected Results

### Current Status

| Test | Status | Notes |
|------|--------|-------|
| 01-vbl_basics.nes | Expected to pass | Basic VBlank timing |
| 02-vbl_set_time.nes | May fail | Requires exact cycle accuracy |
| 03-vbl_clear_time.nes | May fail | Requires exact cycle accuracy |
| 01.basics.nes | May fail | Requires sprite rendering |
| 02.alignment.nes | May fail | Requires sprite rendering |

### Known Limitations

The current PPU implementation has:

- Cycle-accurate timing framework
- VBlank/NMI generation
- Register implementation
- Sprite evaluation logic

Still implementing:

- Full tile fetching and rendering
- Sprite 0 hit detection
- Complete background rendering

## Complete Test ROM Collection

This directory contains **54 PPU test ROMs** covering:

### VBL/NMI Tests (10 tests)

Complete VBlank flag and NMI timing validation:

- 01-vbl_basics.nes - 10-even_odd_timing.nes (complete set)
- Tests VBL flag set/clear timing, NMI control, suppression, frame timing

### Sprite Hit Tests (11 tests)

Sprite 0 collision detection (Quietust, 2005):

- spr_hit_basics.nes through spr_hit_edge_timing.nes
- Tests collision detection, alignment, clipping, timing

### Sprite Overflow Tests (5 tests)

Tests PPU sprite evaluation bug behavior:

- spr_overflow_basics.nes - Basic overflow flag behavior
- spr_overflow_details.nes - Detailed overflow logic
- spr_overflow_timing.nes - Overflow timing
- spr_overflow_obscure.nes - Edge cases
- spr_overflow_emulator.nes - Emulator-specific behavior

**Source**: [blargg's sprite overflow tests](https://www.nesdev.org/wiki/Emulator_tests)

### OAM (Sprite Memory) Tests (2 tests)

Tests sprite RAM ($2003/$2004) behavior:

- oam_read.nes - OAM read behavior and open bus
- oam_stress.nes - OAM stress test

### PPU Bus Tests (3 tests)

Tests PPU data bus and buffer behavior:

- open_bus.nes - PPU open bus behavior
- read_buffer.nes - PPU read buffer ($2007)
- test_ppu_read_buffer.nes - Detailed read buffer tests

### Palette Tests (4 tests)

Tests palette RAM behavior:

- palette_ram.nes - Basic palette RAM test
- palette.nes - Palette mirroring
- color.nes - Color rendering test
- full_palette*.nes - Complete 400+ color NTSC palette display

### VBL/NMI Detailed Tests (10 tests)

Individual VBL/NMI tests (vbl_nmi_* prefix):

- vbl_nmi_basics.nes through vbl_nmi_timing.nes
- Comprehensive VBL flag and NMI timing tests

### Other Tests (9 tests)

- sprite_ram.nes - Sprite RAM access patterns
- vram_access.nes - VRAM ($2006/$2007) access patterns
- scanline.nes - Scanline rendering test
- ntsc_torture.nes - NTSC signal torture test
- And others

## Test ROM Sources

- **[christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)** - Primary source
- **[TetaNES test_roms](https://github.com/lukexor/tetanes)** - Additional comprehensive tests
- **[NESdev Wiki](https://www.nesdev.org/wiki/Emulator_tests)** - Test ROM documentation

## References

- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [nes-test-roms Repository](https://github.com/christopherpow/nes-test-roms)
- [PPU VBL/NMI Documentation](https://github.com/christopherpow/nes-test-roms/tree/master/ppu_vbl_nmi)
- [Sprite Hit Tests Documentation](https://github.com/christopherpow/nes-test-roms/tree/master/sprite_hit_tests_2005.10.05)

## License

Test ROMs are created by their respective authors:

- blargg's tests: Various (see individual test documentation)
- Sprite hit tests: Created by Quietust (2005)

All test ROMs are used for educational and emulator development purposes.
