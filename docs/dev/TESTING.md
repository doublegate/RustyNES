# Testing RustyNES

**Table of Contents**
- [Overview](#overview)
- [Test Suite Structure](#test-suite-structure)
- [Running Tests](#running-tests)
- [Test ROMs](#test-roms)
- [Unit Testing](#unit-testing)
- [Integration Testing](#integration-testing)
- [Accuracy Testing](#accuracy-testing)
- [Continuous Integration](#continuous-integration)

---

## Overview

RustyNES employs a comprehensive testing strategy combining unit tests, integration tests, and test ROM validation to ensure cycle-accurate emulation and compatibility.

### Testing Goals

- **100% TASVideos test suite pass rate** (156 tests)
- **Unit test coverage** for all components
- **Integration tests** for component interactions
- **Regression tests** for mapper edge cases
- **Game compatibility matrix** validation

---

## Test Suite Structure

```
tests/
├── unit/                 # Component-level tests
│   ├── cpu_tests.rs
│   ├── ppu_tests.rs
│   ├── apu_tests.rs
│   └── mapper_tests.rs
├── integration/          # Cross-component tests
│   ├── bus_tests.rs
│   └── system_tests.rs
├── roms/                 # Test ROM files
│   ├── nestest.nes
│   ├── blargg/
│   ├── ppu_tests/
│   └── mapper_tests/
└── accuracy/             # Accuracy validation
    └── tas_suite.rs
```

---

## Running Tests

### All Tests

```bash
cargo test
```

### Specific Test Suite

```bash
cargo test --test cpu_tests
cargo test --test ppu_tests
cargo test --test mapper_tests
```

### Specific Test

```bash
cargo test test_adc_immediate
cargo test test_ppu_vblank_timing
```

### Release Mode (Faster)

```bash
cargo test --release
```

### With Output

```bash
cargo test -- --nocapture
```

---

## Test ROMs

### Core Test ROMs

**nestest.nes** (CPU validation):
```bash
cargo test --test nestest
```

**Expected Result**: All 8000+ instructions pass golden log comparison

**blargg's Test Suite** (CPU, APU, PPU):
```bash
cargo test --test blargg_suite
```

**Tests**:
- cpu_exec_space
- cpu_interrupts_v2
- cpu_timing_test6
- apu_test
- ppu_vbl_nmi

### Acquiring Test ROMs

**Sources**:
- [NesDev Test ROMs](https://www.nesdev.org/wiki/Emulator_tests)
- [blargg's test suite](http://blargg.8bitalley.com/nes-tests/)
- [TASVideos Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)

**Installation**:
```bash
mkdir -p tests/roms
cd tests/roms
# Download test ROMs
curl -O https://www.nesdev.org/nestest.nes
```

---

## Unit Testing

### CPU Tests

```rust
#[test]
fn test_adc_immediate() {
    let mut cpu = Cpu::new();
    let mut bus = MockBus::new();

    cpu.a = 0x50;
    cpu.p.remove(Status::CARRY);

    // ADC #$10
    bus.write(0x0000, 0x69);  // ADC immediate
    bus.write(0x0001, 0x10);

    cpu.execute_instruction(&mut bus);

    assert_eq!(cpu.a, 0x60);
    assert!(!cpu.p.contains(Status::CARRY));
    assert!(!cpu.p.contains(Status::ZERO));
    assert!(!cpu.p.contains(Status::NEGATIVE));
}
```

### PPU Tests

```rust
#[test]
fn test_vblank_flag_set() {
    let mut ppu = Ppu::new();

    // Run to scanline 241 (VBlank start)
    for _ in 0..241 {
        for _ in 0..341 {
            ppu.step();
        }
    }

    assert!(ppu.status.contains(PpuStatus::VBLANK));
}
```

### Mapper Tests

```rust
#[test]
fn test_uxrom_bank_switching() {
    let rom = create_test_rom(8, 0); // 8 PRG banks
    let mut mapper = UxROM::new(rom, 0);

    mapper.write_prg(0x8000, 0x05);
    assert_eq!(mapper.prg_bank, 5);

    let addr = mapper.map_prg_addr(0x8000);
    assert_eq!(addr, 5 * 0x4000);
}
```

---

## Integration Testing

### System Integration

```rust
#[test]
fn test_cpu_ppu_timing_sync() {
    let mut console = Console::new(load_test_rom());

    for _ in 0..10 {
        console.step(); // Execute one CPU instruction
    }

    // Verify PPU ran 3x as many cycles
    assert_eq!(console.ppu.cycles, console.cpu.cycles * 3);
}
```

### NMI Timing

```rust
#[test]
fn test_nmi_generation() {
    let mut console = Console::new(load_test_rom());

    // Run until VBlank
    console.step_frame();

    // NMI should be pending
    assert!(console.cpu.nmi_pending);
}
```

---

## Accuracy Testing

### TASVideos Test Suite

**Automated validation**:
```bash
cargo test --test tas_accuracy_suite
```

**Categories**:
- APU Tests (25)
- CPU Tests (35)
- PPU Tests (45)
- Mapper Tests (51)

### Regression Testing

**Add new regression test**:
```rust
#[test]
fn test_regression_sprite_zero_hit_timing() {
    // Specific case that previously failed
    let mut console = Console::new(load_rom("sprite_hit_test.nes"));

    console.step_frame();

    assert!(console.ppu.sprite_zero_hit_occurred());
}
```

---

## Continuous Integration

### GitHub Actions Workflow

```yaml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - name: Run tests
        run: cargo test --all-features
      - name: Run test ROMs
        run: cargo test --test accuracy_suite
```

---

## References

- [NesDev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [blargg's Test Suite](http://blargg.8bitalley.com/nes-tests/)
- [TASVideos Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)

---

**Related Documents**:
- [BUILD.md](BUILD.md) - Building the project
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development workflow
- [DEBUGGING.md](DEBUGGING.md) - Debugging failing tests
