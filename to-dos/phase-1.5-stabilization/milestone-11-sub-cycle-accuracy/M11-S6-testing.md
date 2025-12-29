# M11-S6: Testing and Validation

**Sprint:** S6 (Testing)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 1-2 weeks (8-12 hours)
**Status:** PLANNED
**Priority:** HIGH - Validation of all changes
**Depends On:** S1-S5 (All previous sprints)

---

## Overview

Comprehensive testing and validation of sub-cycle accuracy implementation, including VBlank timing tests, full Blargg suite regression, TASVideos accuracy suite, performance benchmarking, and game compatibility testing.

---

## Dependencies

### Required Before Starting
- **S1 (CPU Refactor)** - Cycle-by-cycle CPU must be complete
- **S2 (PPU Sync)** - VBlank timing must be implemented
- **S3 (APU)** - APU integration must be complete
- **S4 (Bus/DMA)** - DMA timing must be accurate
- **S5 (Mappers)** - Mapper IRQ timing must be accurate

### Blocks
- **v1.0.0 Release** - Cannot release until all tests pass

---

## Current Test Status

### Test Summary (v0.8.4)

| Category | Count | Status |
|----------|-------|--------|
| Unit tests | 517+ | PASS |
| Blargg CPU | 11 | PASS |
| Blargg PPU | 49 | PASS (2 ignored) |
| Blargg APU | 30 | PASS |
| **Total Blargg** | **90** | **100% (88 passing, 2 ignored)** |

### Currently Ignored Tests

| Test | Reason | Sprint to Fix |
|------|--------|---------------|
| `ppu_02-vbl_set_time` | Requires +/-2 cycle accuracy | S2 |
| `ppu_03-vbl_clear_time` | Requires +/-2 cycle accuracy | S2 |

---

## Required Testing

### Task S6.1: VBlank Timing Test Validation

**Priority:** P0 (Critical)
**Effort:** 3 hours
**Files:**
- `tests/blargg/ppu_vbl_nmi.rs`
- Test ROMs: `ppu_02-vbl_set_time`, `ppu_03-vbl_clear_time`

#### Subtasks
- [ ] Enable previously-ignored `ppu_02-vbl_set_time` test
- [ ] Enable previously-ignored `ppu_03-vbl_clear_time` test
- [ ] Run tests and capture results
- [ ] Debug any failures
- [ ] Document exact timing requirements
- [ ] Verify both tests PASS

#### Test Requirements

```text
ppu_02-vbl_set_time:
  Tests exact cycle when VBlank flag is set
  Requirement: $2002 bit 7 set at scanline 241, dot 1
  Tolerance: +/- 2 CPU cycles

ppu_03-vbl_clear_time:
  Tests exact cycle when VBlank flag is cleared
  Requirement: $2002 bit 7 clear at scanline 261, dot 1
  Tolerance: +/- 2 CPU cycles
```

#### Implementation Notes

```rust
// Update ignored test attributes
#[test]
fn test_ppu_02_vbl_set_time() {
    let test_rom = include_bytes!("../test-roms/blargg/ppu_vbl_nmi/02-vbl_set_time.nes");
    let result = run_blargg_test(test_rom);
    assert_eq!(result, BlarggResult::Pass);
}

#[test]
fn test_ppu_03_vbl_clear_time() {
    let test_rom = include_bytes!("../test-roms/blargg/ppu_vbl_nmi/03-vbl_clear_time.nes");
    let result = run_blargg_test(test_rom);
    assert_eq!(result, BlarggResult::Pass);
}
```

---

### Task S6.2: Full Blargg Test Suite Regression

**Priority:** P0 (Critical)
**Effort:** 2 hours
**Files:**
- `tests/blargg/*.rs`

#### Subtasks
- [ ] Run all 90 Blargg tests
- [ ] Verify 0 regressions from previous passing tests
- [ ] Document any new failures
- [ ] Create issue tickets for failures
- [ ] Update test status table

#### Test Categories

| Category | Tests | Expectation |
|----------|-------|-------------|
| cpu_instr_test | 11 | All PASS |
| ppu_vbl_nmi | 10 | All PASS (previously 8/10) |
| apu_test | 9 | All PASS |
| apu_mixer | 8 | All PASS |
| sprite_hit | 8 | All PASS |
| sprite_overflow | 5 | All PASS |
| oam_read | 2 | All PASS |
| ppu_read_buffer | 1 | All PASS |
| Other | 36 | All PASS |

---

### Task S6.3: nestest.nes Golden Log Validation

**Priority:** P0 (Critical)
**Effort:** 1 hour
**Files:**
- `tests/nestest.rs`
- `docs/testing/NESTEST_GOLDEN_LOG.md`

#### Subtasks
- [ ] Run nestest.nes automated mode
- [ ] Compare output against golden log
- [ ] Verify all 256 opcodes pass
- [ ] Verify cycle counts match exactly
- [ ] Document any discrepancies

#### Validation Criteria

```text
nestest.nes Validation:
- All 151 official opcodes correct
- All 105 unofficial opcodes correct
- All cycle counts match NESdev reference
- Final result: $00 at $0002 and $0003 (no errors)
```

---

### Task S6.4: TASVideos Accuracy Suite

**Priority:** P1
**Effort:** 3 hours
**Files:**
- `tests/tasvideos/*.rs`
- Test ROMs from TASVideos suite

#### Subtasks
- [ ] Obtain TASVideos accuracy test suite
- [ ] Create test harness for TASVideos tests
- [ ] Run full suite (156 tests)
- [ ] Document results and failures
- [ ] Create tracking for gradual improvement

#### Test Categories

| Category | Tests | Target |
|----------|-------|--------|
| CPU timing | ~30 | 100% |
| PPU timing | ~50 | 95%+ |
| APU timing | ~20 | 90%+ |
| Mapper | ~30 | 90%+ |
| DMA | ~10 | 100% |
| Edge cases | ~16 | 85%+ |

---

### Task S6.5: Performance Benchmarking

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `benches/*.rs`

#### Subtasks
- [ ] Create benchmark for frame execution time
- [ ] Measure cycle-accurate vs instruction-level performance
- [ ] Profile hot paths (CPU step, PPU step, APU step)
- [ ] Identify optimization opportunities
- [ ] Document acceptable performance threshold

#### Benchmark Targets

```text
Performance Targets:
- Frame time (cycle-accurate): < 16.67ms (60 FPS)
- CPU instruction throughput: > 2M instructions/sec
- Memory usage: < 50MB for core emulation
- Maximum acceptable regression: 20% vs v0.8.4
```

#### Benchmark Implementation

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_frame_execution(c: &mut Criterion) {
    let rom_data = include_bytes!("../test-roms/nestest.nes");
    let mut console = Console::from_rom_bytes(rom_data).unwrap();

    c.bench_function("frame_accurate", |b| {
        b.iter(|| {
            console.step_frame_accurate();
        })
    });

    c.bench_function("frame_coarse", |b| {
        b.iter(|| {
            console.step_frame();
        })
    });
}

fn benchmark_cpu_step(c: &mut Criterion) {
    // ... CPU-focused benchmark
}

criterion_group!(benches, benchmark_frame_execution, benchmark_cpu_step);
criterion_main!(benches);
```

---

### Task S6.6: Game Compatibility Testing

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `tests/games/*.rs` (if applicable)
- Manual testing

#### Subtasks
- [ ] Test Top 10 most popular NES games
- [ ] Test known timing-sensitive games
- [ ] Document any regressions
- [ ] Verify audio sync
- [ ] Verify input responsiveness

#### Games to Test

| Game | Mapper | Timing Sensitivity | Focus |
|------|--------|-------------------|-------|
| Super Mario Bros. | 0 | Low | Basic gameplay |
| The Legend of Zelda | 1 | Medium | Scrolling |
| Super Mario Bros. 3 | 4 | High | IRQ status bar |
| Mega Man 2 | 1 | Medium | Scrolling |
| Mega Man 3 | 4 | High | IRQ split screen |
| Castlevania III | 5 | High | MMC5 scanlines |
| Battletoads | 4 | Very High | DMA mid-frame |
| Kirby's Adventure | 4 | High | Raster effects |
| Ninja Gaiden | 4 | High | Status bar |
| Contra | 2 | Low | Basic gameplay |

---

### Task S6.7: Unit Test Updates

**Priority:** P0
**Effort:** 2 hours
**Files:**
- `crates/rustynes-cpu/tests/*.rs`
- `crates/rustynes-ppu/tests/*.rs`
- `crates/rustynes-apu/tests/*.rs`
- `crates/rustynes-core/tests/*.rs`

#### Subtasks
- [ ] Update CPU tests for new bus interface
- [ ] Add cycle callback verification tests
- [ ] Add VBlank timing edge case tests
- [ ] Add DMA cycle count tests
- [ ] Verify all unit tests pass

#### New Tests to Add

```rust
// CPU cycle callback verification
#[test]
fn test_lda_absolute_calls_on_cpu_cycle_4_times() {
    let mut cpu = Cpu::new();
    let mut bus = MockBus::new();

    // LDA $1234
    bus.write_memory(0x0000, &[0xAD, 0x34, 0x12]);
    bus.write_memory(0x1234, &[0x42]);

    let cycles = cpu.step(&mut bus);

    assert_eq!(cycles, 4);
    assert_eq!(bus.cycle_callback_count, 4);
}

// VBlank timing edge case
#[test]
fn test_vblank_read_at_exact_cycle() {
    let mut ppu = Ppu::new();

    // Advance to exactly VBlank set cycle
    advance_to_scanline_dot(&mut ppu, 241, 1);

    assert!(ppu.status().vblank());

    // Read should clear flag
    let status = ppu.read_status();
    assert!(status & 0x80 != 0);  // Was set
    assert!(!ppu.status().vblank());  // Now clear
}
```

---

### Task S6.8: Regression Test Suite

**Priority:** P0
**Effort:** 1 hour
**Files:**
- `.github/workflows/ci.yml`
- `tests/regression/*.rs`

#### Subtasks
- [ ] Create regression test runner script
- [ ] Add to CI/CD pipeline
- [ ] Test on Linux, macOS, Windows
- [ ] Set up nightly regression runs
- [ ] Document regression handling process

#### CI Configuration

```yaml
# .github/workflows/regression.yml
name: Regression Tests

on:
  push:
    branches: [main]
  pull_request:
  schedule:
    - cron: '0 0 * * *'  # Nightly

jobs:
  regression:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]

    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run Blargg Tests
        run: cargo test --test blargg -- --test-threads=1

      - name: Run nestest
        run: cargo test --test nestest

      - name: Run Benchmarks
        run: cargo bench --bench frame_timing -- --noplot
```

---

## Validation Criteria

### Sprint Complete When

| Criterion | Target | Validation |
|-----------|--------|------------|
| ppu_02-vbl_set_time | PASS | Test execution |
| ppu_03-vbl_clear_time | PASS | Test execution |
| Blargg suite | 90/90 PASS | Test execution |
| nestest.nes | 100% match | Golden log diff |
| Unit tests | 517+ PASS | cargo test |
| Performance | < 20% regression | Benchmark |
| Game compat | Top 10 working | Manual test |

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| VBlank tests still fail | High | Medium | Debug with cycle-level tracing |
| Performance regression > 20% | Medium | Medium | Optimize hot paths |
| Game regressions | High | Low | Extensive game testing |
| Test infrastructure issues | Low | Low | Use established test patterns |

---

## References

### Internal Documentation
- [Test ROM Guide](../../../docs/testing/TEST_ROM_GUIDE.md)
- [nestest Golden Log](../../../docs/testing/NESTEST_GOLDEN_LOG.md)

### External Resources
- [Blargg's Test ROMs](https://github.com/christopherpow/nes-test-roms)
- [TASVideos Accuracy Tests](https://tasvideos.org/)
- [NESdev Wiki - Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)

### Test ROM Locations
- `test-roms/blargg/` - Blargg test suite
- `test-roms/nestest/` - nestest CPU validation
- `test-roms/tasvideos/` - TASVideos accuracy suite

---

## Acceptance Criteria

- [ ] `ppu_02-vbl_set_time` test PASSES
- [ ] `ppu_03-vbl_clear_time` test PASSES
- [ ] All 90 Blargg tests PASS (0 regressions)
- [ ] All 517+ unit tests PASS (0 regressions)
- [ ] nestest.nes 100% golden log match
- [ ] Performance within 20% of v0.8.4
- [ ] Top 10 games fully playable
- [ ] CI/CD pipeline green on all platforms

---

## Success Metrics

### Milestone 11 Complete When

1. [x] CPU refactored to cycle-by-cycle execution (S1)
2. [ ] PPU stepped before each CPU memory access (S2)
3. [ ] APU stepped before each CPU memory access (S3)
4. [ ] `ppu_02-vbl_set_time` test PASSES (S2/S6)
5. [ ] `ppu_03-vbl_clear_time` test PASSES (S2/S6)
6. [ ] All 90 Blargg tests pass (0 regressions) (S6)
7. [ ] All 517+ unit tests pass (0 regressions) (S6)
8. [ ] Performance within 20% of v0.8.4 (S6)
9. [ ] OAM DMA timing verified (S4)
10. [ ] DMC DMA cycle stealing implemented (S3/S4)

---

**Status:** PLANNED
**Created:** 2025-12-28
