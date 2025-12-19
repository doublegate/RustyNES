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

## Additional Test ROMs to Download

For comprehensive PPU validation, download these additional test suites:

### VBL/NMI Tests (complete set)

```bash
cd test-roms/ppu
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/04-nmi_control.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/05-nmi_timing.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/06-suppression.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/07-nmi_on_timing.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/08-nmi_off_timing.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/09-even_odd_frames.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/ppu_vbl_nmi/rom_singles/10-even_odd_timing.nes
```

### Sprite Hit Tests (complete set)

```bash
cd test-roms/ppu
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/03.corners.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/04.flip.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/05.left_clip.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/06.right_edge.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/07.screen_bottom.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/08.double_height.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/09.timing_basics.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/10.timing_order.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/sprite_hit_tests_2005.10.05/11.edge_timing.nes
```

### Other PPU Tests

```bash
cd test-roms/ppu
# Palette RAM test
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/blargg_ppu_tests_2005.09.15b/palette_ram.nes

# Sprite RAM (OAM) test
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/blargg_ppu_tests_2005.09.15b/sprite_ram.nes

# VRAM access test
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/blargg_ppu_tests_2005.09.15b/vram_access.nes
```

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
