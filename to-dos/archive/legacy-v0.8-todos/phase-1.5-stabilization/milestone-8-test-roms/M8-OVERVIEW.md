# Milestone 8: Test ROM Validation

**Milestone:** M8 (Test ROM Validation)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~4 weeks (February-March 2026)
**Status:** Not Started
**Version Target:** v0.7.0
**Progress:** 0%

---

## Overview

Milestone 8 focuses on **systematic test ROM validation** to achieve 95%+ pass rate across all test categories. This milestone leverages the accuracy improvements from M7 to comprehensively validate CPU, PPU, APU, and mapper implementations against the full test ROM catalog (212 tests).

### Goals

1. **Comprehensive Test ROM Coverage**
   - All 212 test ROMs cataloged and executed
   - Automated test harness with pass/fail reporting
   - Regression prevention through CI integration
   - Golden log comparisons for deterministic tests

2. **CPU Validation (36 tests)**
   - nestest.nes automation (continues to pass)
   - Blargg instruction timing tests
   - Branch timing edge cases
   - Dummy read/write verification

3. **PPU Validation (49 tests)**
   - Blargg VBL/NMI test suite
   - Sprite 0 hit comprehensive tests
   - Palette RAM edge cases
   - Open bus behavior

4. **APU Validation (70 tests)**
   - Blargg APU test suite (comprehensive)
   - Channel-specific tests (pulse, triangle, noise, DMC)
   - Frame counter timing
   - Mixer output validation

5. **Mapper Validation (57 tests)**
   - Holy Mapperel comprehensive suite
   - Per-mapper validation (NROM, MMC1, UxROM, CNROM, MMC3)
   - Bank switching edge cases
   - IRQ timing for MMC3

---

## Success Criteria

### Test Pass Rate Targets

| Category | v0.6.0 Baseline | v0.7.0 Target | Gain |
|----------|-----------------|---------------|------|
| CPU | 15/36 (42%) | 34/36 (94%) | +52% |
| PPU | 20/49 (41%) | 47/49 (96%) | +55% |
| APU | 10/70 (14%) | 67/70 (96%) | +82% |
| Mappers | 5/57 (9%) | 54/57 (95%) | +86% |
| **Total** | **50/212 (24%)** | **202/212 (95%)** | **+71%** |

### Quality Gates

- [ ] Automated test harness integrated into CI
- [ ] 95%+ overall test pass rate (202+/212)
- [ ] Zero regressions from v0.6.0 baseline
- [ ] Test ROM execution time <10 minutes (full suite)
- [ ] Comprehensive failure analysis for remaining 10 tests
- [ ] Documentation of known limitations and workarounds

---

## Sprint Breakdown

### Sprint 1: nestest & CPU Tests ⏳ PENDING

**Duration:** Week 1
**Focus:** CPU instruction and timing validation

**Objectives:**
- [ ] Automate nestest.nes (golden log comparison)
- [ ] Pass all 36 CPU instruction tests
- [ ] Verify branch timing edge cases
- [ ] Validate dummy read/write cycles

**Deliverable:** CPU validation complete, 34/36 tests passing

[M8-S1 Details](M8-S1-nestest-validation.md)

---

### Sprint 2: Blargg CPU Tests ⏳ PENDING

**Duration:** Week 1-2
**Focus:** Blargg CPU instruction timing suite

**Objectives:**
- [ ] Pass cpu_instr_timing.nes (overall timing)
- [ ] Pass cpu_branch_timing_2.nes (branch edge cases)
- [ ] Pass cpu_dummy_reads.nes
- [ ] Pass cpu_dummy_writes_ppumem.nes

**Deliverable:** Blargg CPU tests complete, 14/14 passing

[M8-S2 Details](M8-S2-blargg-cpu-tests.md)

---

### Sprint 3: Blargg PPU Tests ⏳ PENDING

**Duration:** Week 2-3
**Focus:** Blargg PPU VBL/NMI and sprite tests

**Objectives:**
- [ ] Pass all VBL/NMI timing tests (ppu_vbl_nmi suite)
- [ ] Pass sprite 0 hit suite (11 tests)
- [ ] Pass palette RAM tests
- [ ] Pass open bus tests

**Deliverable:** Blargg PPU tests complete, 47/49 passing

[M8-S3 Details](M8-S3-blargg-ppu-tests.md)

---

### Sprint 4: Blargg APU Tests ⏳ PENDING

**Duration:** Week 3
**Focus:** Blargg APU comprehensive test suite

**Objectives:**
- [ ] Pass apu_test.nes (comprehensive)
- [ ] Pass all channel-specific tests
- [ ] Pass frame counter timing tests
- [ ] Pass mixer output tests

**Deliverable:** Blargg APU tests complete, 67/70 passing

[M8-S4 Details](M8-S4-blargg-apu-tests.md)

---

### Sprint 5: Mapper Tests ⏳ PENDING

**Duration:** Week 3-4
**Focus:** Holy Mapperel and mapper-specific validation

**Objectives:**
- [ ] Pass Holy Mapperel comprehensive suite
- [ ] Validate NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4)
- [ ] Test bank switching edge cases
- [ ] Verify MMC3 IRQ timing

**Deliverable:** Mapper tests complete, 54/57 passing, v0.7.0 release

[M8-S5 Details](M8-S5-mapper-tests.md)

---

## Technical Focus Areas

### Automated Test Harness

**Requirements:**
- Execute all 212 test ROMs automatically
- Compare output against golden logs (where available)
- Report pass/fail with detailed failure analysis
- Integration with CI (GitHub Actions)
- Execution time <10 minutes for full suite

**Implementation:**
```rust
#[test]
fn test_rom_suite() {
    let test_roms = discover_test_roms("test-roms/");
    for rom in test_roms {
        let result = execute_rom(&rom);
        assert!(result.passed(), "Test ROM failed: {}", rom.name);
    }
}
```

### Test ROM Categories

#### CPU Tests (36 total)
- **nestest.nes** (1) - Comprehensive instruction validation
- **Blargg CPU Tests** (14) - Timing, branches, dummy reads/writes
- **Edge Cases** (21) - Unofficial opcodes, interrupts, page crossing

#### PPU Tests (49 total)
- **Blargg VBL/NMI** (10) - VBlank flag timing, NMI control
- **Sprite 0 Hit** (11) - Edge cases, alignment, flip, timing
- **Palette RAM** (5) - Mirroring, edge cases
- **Open Bus** (3) - PPU register behavior
- **Misc PPU** (20) - Scrolling, rendering edge cases

#### APU Tests (70 total)
- **Blargg APU** (15) - Comprehensive suite
- **Channel Tests** (25) - Pulse, triangle, noise, DMC
- **Frame Counter** (10) - 4-step, 5-step, IRQ timing
- **Mixer** (5) - Non-linear mixing, output levels
- **Misc APU** (15) - Edge cases, timing

#### Mapper Tests (57 total)
- **Holy Mapperel** (40) - Comprehensive mapper suite
- **NROM (0)** (3) - Basic mapper
- **MMC1 (1)** (5) - Bank switching, mirroring
- **UxROM (2)** (3) - PRG bank switching
- **CNROM (3)** (2) - CHR bank switching
- **MMC3 (4)** (4) - Bank switching, IRQ timing

---

## Expected Outcomes

### Test Pass Rate Improvement

| Milestone | Total Pass Rate | CPU | PPU | APU | Mappers |
|-----------|-----------------|-----|-----|-----|---------|
| v0.5.0 | 5/212 (2.4%) | 1/36 | 4/49 | 0/70 | 0/57 |
| v0.6.0 | 50/212 (24%) | 15/36 | 20/49 | 10/70 | 5/57 |
| **v0.7.0** | **202/212 (95%)** | **34/36** | **47/49** | **67/70** | **54/57** |

### Remaining Failures (10 tests)

**Expected Failures:**
- CPU (2): Highly timing-sensitive tests requiring sub-cycle precision
- PPU (2): Rare edge cases (mid-scanline register access)
- APU (3): Expansion audio edge cases (FDS, VRC6, MMC5)
- Mappers (3): Rare mapper variants (15, 19, 24)

**Rationale:** These tests represent <5% of the suite and require specialized handling beyond Phase 1.5 scope.

---

## Dependencies

### Blockers

- M7 (Accuracy Improvements) must be complete

### Inputs

- v0.6.0 accuracy improvements (CPU, PPU, APU timing)
- Test ROM catalog (212 tests in `test-roms/`)
- Golden logs (nestest, Blargg tests)
- Reference emulator outputs (Mesen2, FCEUX)

### Outputs

- v0.7.0 release (95%+ test pass rate)
- Automated test harness (CI integration)
- Comprehensive failure analysis report
- Regression test suite baseline

---

## Risks & Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Test ROM incompatibilities | High | Medium | Focus on standard test suites (Blargg, nestest) |
| Timing precision limits | Medium | Medium | Accept ±1-2 cycle tolerance for edge cases |
| CI execution time | Low | Medium | Parallelize test execution, optimize ROM loading |
| False positives/negatives | Medium | Low | Manual verification of edge cases, golden log validation |
| Test ROM missing/corrupted | Low | Low | Verify checksums, re-download from trusted sources |

---

## Resources

### Documentation

- [Test ROM Guide](../../../docs/testing/TEST_ROM_GUIDE.md)
- [nestest Golden Log](../../../docs/testing/NESTEST_GOLDEN_LOG.md)
- [CPU Timing Reference](../../../docs/cpu/CPU_TIMING_REFERENCE.md)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)
- [APU Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)

### External References

- [NesDev Wiki - Test ROMs](https://www.nesdev.org/wiki/Emulator_tests)
- [Blargg Test ROMs](http://blargg.8bitalley.com/nes-tests/)
- [Holy Mapperel](https://github.com/TomHarte/HolyMapperel)
- [nestest.nes](https://github.com/christopherpow/nes-test-roms)

### Test ROM Sources

```bash
# Download test ROMs (not included in repo)
cd test-roms/
./download-test-roms.sh  # Script to fetch from trusted sources

# Verify checksums
sha256sum -c test-roms.sha256
```

---

## Milestone Deliverables

1. **Automated Test Harness**
   - CI integration (GitHub Actions)
   - Pass/fail reporting with detailed output
   - Golden log comparison (nestest)
   - Execution time <10 minutes

2. **Test Coverage**
   - 95%+ pass rate (202+/212 tests)
   - Comprehensive failure analysis for remaining tests
   - Regression test suite baseline

3. **Documentation**
   - Test ROM execution guide
   - Known limitations and workarounds
   - Failure analysis report
   - CI setup instructions

4. **Release**
   - v0.7.0 git tag
   - Release notes (test pass rate improvements)
   - CHANGELOG entry

---

**Status:** ⏳ PENDING
**Blocks:** M9 (Known Issues Resolution)
**Next Milestone:** M9 (Known Issues) - Audio/video sync, performance, edge case handling
