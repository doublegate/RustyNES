# Milestone 7: Accuracy Improvements

**Milestone:** M7 (Accuracy Improvements)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~3 weeks (January-February 2026)
**Status:** Not Started
**Version Target:** v0.6.0
**Progress:** 0%

---

## Overview

Milestone 7 focuses on **refining the accuracy of CPU, PPU, and APU** to prepare for comprehensive test ROM validation in M8. This milestone addresses timing precision, edge case handling, and subsystem synchronization to achieve higher test pass rates.

### Goals

1. **CPU Cycle Timing Refinements**
   - Edge case instruction timing
   - Interrupt timing precision
   - Unofficial opcode verification
   - Page boundary crossing accuracy

2. **PPU Dot-Accurate Rendering**
   - VBlank timing precision (±2-10 cycles)
   - Sprite 0 hit edge cases
   - Attribute table handling
   - Palette RAM mirroring

3. **APU Timing & Mixing**
   - Frame counter precision
   - DMC channel edge cases
   - Triangle linear counter
   - Non-linear mixer calibration

4. **Bus Timing & Synchronization**
   - OAM DMA cycle accuracy (513-514 cycles)
   - CPU/PPU synchronization
   - Memory access timing
   - Bus conflicts

---

## Success Criteria

### Accuracy Targets

- [ ] CPU timing accuracy: ±1 cycle for all instructions
- [ ] PPU VBlank timing: ±2 cycle accuracy
- [ ] APU frame counter: ±1 cycle accuracy
- [ ] OAM DMA: exact 513-514 cycle timing
- [ ] Test ROM pass rate increase: 5/212 → 50/212 (24%)

### Quality Gates

- [ ] All CPU instruction timing verified
- [ ] VBlank flag timing accurate
- [ ] Sprite 0 hit edge cases handled
- [ ] DMC channel timing correct
- [ ] Frame counter modes (4-step, 5-step) precise
- [ ] Zero regressions in existing tests

---

## Sprint Breakdown

### Sprint 1: CPU Accuracy ⏳ PENDING

**Duration:** Week 1
**Focus:** CPU cycle timing and edge cases

**Objectives:**
- [ ] Refine instruction cycle timing
- [ ] Verify unofficial opcodes
- [ ] Interrupt timing precision
- [ ] Page boundary crossing accuracy

**Deliverable:** CPU accuracy improvements

[M7-S1 Details](M7-S1-cpu-accuracy.md)

---

### Sprint 2: PPU Accuracy ⏳ PENDING

**Duration:** Week 2
**Focus:** PPU dot-accurate rendering improvements

**Objectives:**
- [ ] VBlank timing precision (±2 cycles)
- [ ] Sprite 0 hit edge cases
- [ ] Attribute shift register verification
- [ ] Palette RAM mirroring edge cases

**Deliverable:** PPU accuracy improvements

[M7-S2 Details](M7-S2-ppu-accuracy.md)

---

### Sprint 3: APU Accuracy ⏳ PENDING

**Duration:** Week 2-3
**Focus:** APU timing and mixing calibration

**Objectives:**
- [ ] Frame counter precision
- [ ] DMC channel edge cases
- [ ] Triangle linear counter
- [ ] Mixer calibration and verification

**Deliverable:** APU accuracy improvements

[M7-S3 Details](M7-S3-apu-accuracy.md)

---

### Sprint 4: Timing & Synchronization ⏳ PENDING

**Duration:** Week 3
**Focus:** Bus timing and subsystem synchronization

**Objectives:**
- [ ] OAM DMA cycle precision
- [ ] CPU/PPU synchronization
- [ ] Bus timing accuracy
- [ ] Integration testing

**Deliverable:** Complete timing accuracy, v0.6.0 release

[M7-S4 Details](M7-S4-timing-polish.md)

---

## Technical Focus Areas

### CPU Accuracy

**Current State (v0.5.0):**
- nestest.nes passing (100% golden log match)
- All 256 opcodes implemented
- Cycle-accurate for standard cases

**Improvements Needed:**
- Edge case timing verification
- Unofficial opcode timing validation
- Interrupt handling edge cases
- Page boundary crossing accuracy

### PPU Accuracy

**Current State (v0.5.0):**
- 85/87 tests passing (97.7%)
- 2 tests ignored (timing precision: ±51 cycle, ±10 cycle)
- VBL/NMI working, sprite 0 hit functional

**Improvements Needed:**
- VBlank timing: ±51 cycle → ±2 cycle precision
- VBlank clear timing: ±10 cycle → exact timing
- Sprite 0 hit edge cases
- Dot-level rendering edge cases

### APU Accuracy

**Current State (v0.5.0):**
- All 5 channels implemented
- 136/136 unit tests passing
- Non-linear mixing functional

**Improvements Needed:**
- Frame counter cycle precision
- DMC channel edge cases (DMA conflicts, buffer)
- Triangle linear counter timing
- Mixer calibration for hardware accuracy

### Bus & Synchronization

**Current State (v0.5.0):**
- OAM DMA: 513-514 cycles (range, not exact)
- CPU/PPU synchronization functional
- Basic bus timing

**Improvements Needed:**
- Exact OAM DMA timing (determine 513 vs 514)
- CPU/PPU synchronization precision
- Bus conflict handling
- Memory access timing accuracy

---

## Expected Outcomes

### Test ROM Pass Rate Improvement

| Category | v0.5.0 | v0.6.0 Target | Gain |
|----------|--------|---------------|------|
| CPU | 1/36 (2.8%) | 15/36 (42%) | +39% |
| PPU | 4/49 (8.2%) | 20/49 (41%) | +33% |
| APU | 0/70 (0%) | 10/70 (14%) | +14% |
| Mappers | 0/57 (0%) | 5/57 (9%) | +9% |
| **Total** | **5/212 (2.4%)** | **50/212 (24%)** | **+22%** |

### Performance Impact

- Target: <5% performance regression
- Focus: Accuracy over speed (optimization in M9-S4)
- Benchmark: Maintain 100+ FPS (1.67x real-time)

---

## Dependencies

### Blockers

- None (Phase 1 complete)

### Inputs

- v0.5.0 implementation report (known issues)
- Test ROM catalog (212 tests)
- NESdev Wiki timing diagrams
- Reference emulator source (Mesen2, TetaNES)

### Outputs

- v0.6.0 release (accuracy improvements)
- Updated test pass rate baseline
- Performance benchmarks
- Detailed timing documentation

---

## Risks & Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Performance regression | Medium | Medium | Continuous benchmarking, defer optimization |
| Edge case discovery | High | High | Incremental testing, reference emulator study |
| Timing precision limits | Medium | Low | Accept ±1-2 cycle tolerance for complex cases |
| Test ROM incompatibilities | Low | Low | Focus on blargg/nestest standard tests |

---

## Resources

### Documentation

- [CPU Timing Reference](../../../docs/cpu/CPU_TIMING_REFERENCE.md)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)
- [APU Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)
- [v0.5.0 Implementation Report](/tmp/RustyNES/v0.5.0-implementation-report.md)

### External References

- [NesDev Wiki - PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [NesDev Wiki - APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [Mesen2 Source](https://github.com/SourMesen/Mesen2)
- [TetaNES Source](https://github.com/lukexor/tetanes)

---

## Milestone Deliverables

1. **Code Improvements**
   - CPU timing refinements
   - PPU rendering improvements
   - APU mixing calibration
   - Bus timing precision

2. **Test Coverage**
   - Increased pass rate: 5/212 → 50/212
   - New test cases for edge conditions
   - Regression test suite

3. **Documentation**
   - Timing accuracy notes
   - Edge case documentation
   - Performance impact analysis

4. **Release**
   - v0.6.0 git tag
   - Release notes
   - CHANGELOG entry

---

**Status:** ⏳ PENDING
**Blocks:** M8 (Test ROM Validation)
**Next Milestone:** M8 (Test ROM Validation) - 95%+ test pass rate
