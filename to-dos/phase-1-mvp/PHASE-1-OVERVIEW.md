# Phase 1: MVP - Overview

**Phase:** 1 (MVP)
**Duration:** Months 1-6 (December 2025 - June 2026)
**Status:** In Progress (17% Complete)
**Goal:** Playable emulator with 80% game compatibility

---

## Table of Contents

- [Overview](#overview)
- [Success Criteria](#success-criteria)
- [Milestones](#milestones)
- [Dependencies](#dependencies)
- [Risk Assessment](#risk-assessment)
- [Timeline](#timeline)

---

## Overview

Phase 1 delivers the **Minimum Viable Product (MVP)** - a fully functional NES emulator capable of running 80% of commercial games. This phase establishes the foundation for all future features and optimizations.

### Core Objectives

1. **Accuracy-First Implementation**
   - Cycle-accurate CPU (6502/2A03)
   - Dot-level PPU rendering (2C02)
   - Hardware-accurate APU synthesis (2A03)
   - Pass 85% of TASVideos accuracy test suite

2. **Essential Mappers**
   - Mapper 0 (NROM) - 9.5% of games
   - Mapper 1 (MMC1) - 27.9% of games
   - Mapper 2 (UxROM) - 10.6% of games
   - Mapper 3 (CNROM) - 6.3% of games
   - Mapper 4 (MMC3) - 23.4% of games
   - **Total Coverage:** 77.7% of licensed games

3. **Cross-Platform Desktop GUI**
   - egui-based interface
   - wgpu rendering backend
   - Audio output (SDL2 or cpal)
   - Controller support
   - Save states

---

## Success Criteria

### Technical Metrics

| Metric | Phase 1 Target | Measurement |
|--------|----------------|-------------|
| **Accuracy** | 85% TASVideos | Test ROM pass rate |
| **Game Compatibility** | 80%+ (500+ games) | Manual testing |
| **Test Coverage** | 75% code coverage | Tarpaulin/grcov |
| **Performance** | 100 FPS (1.67x real-time) | Benchmark suite |
| **Mapper Count** | 5 essential mappers | Implementation status |

### Quality Gates

- [x] ✅ All official CPU instructions implemented
- [x] ✅ nestest.nes golden log match (100%)
- [ ] All Blargg CPU tests pass
- [ ] All Blargg PPU tests pass
- [ ] 95%+ Blargg APU tests pass
- [ ] Super Mario Bros. playable (Mapper 0)
- [ ] Legend of Zelda playable (Mapper 1)
- [ ] Mega Man playable (Mapper 1)
- [ ] Castlevania playable (Mapper 2)
- [ ] Super Mario Bros. 3 playable (Mapper 4)

### Deliverables

- [ ] Functional emulator core (rustynes-core)
- [ ] Desktop GUI application (rustynes-desktop)
- [ ] User documentation (README, guides)
- [ ] Developer documentation (API docs)
- [ ] Build instructions (all platforms)
- [ ] CI/CD pipeline (GitHub Actions)
- [ ] Binary releases (Linux, Windows, macOS)

---

## Milestones

### Milestone 1: CPU Implementation ✅ COMPLETED

**Duration:** December 2025 - January 2026
**Status:** Complete (100%)
**Completed:** December 2025

**Achievements:**

- ✅ Cycle-accurate 6502 core
- ✅ All 256 opcodes (151 official + 105 unofficial)
- ✅ Complete interrupt handling (NMI, IRQ, BRK, RESET)
- ✅ nestest.nes automated mode passes
- ✅ Zero unsafe code
- ✅ Comprehensive unit tests

**Commits:**

- `506a810` - feat(cpu): implement complete cycle-accurate 6502 CPU emulation
- `f977a97` - feat(cpu): implement complete cycle-accurate 6502 CPU emulation

### Milestone 2: PPU Implementation 🔄 IN PROGRESS

**Duration:** January 2026 - March 2026
**Status:** Not Started (0%)
**Target:** March 2026

**Goals:**

- [ ] Dot-level rendering (341×262 scanlines)
- [ ] Background rendering with scrolling
- [ ] Sprite rendering (evaluation, priority, sprite 0 hit)
- [ ] Accurate VBlank/NMI timing
- [ ] Pass all Blargg PPU tests

**Key Files:**

- `crates/rustynes-ppu/` (currently empty)

### Milestone 3: APU Implementation ⏳ PENDING

**Duration:** February 2026 - April 2026
**Status:** Not Started (0%)
**Target:** April 2026

**Goals:**

- [ ] All 5 audio channels (2 pulse, triangle, noise, DMC)
- [ ] Frame counter (4-step, 5-step modes)
- [ ] Hardware-accurate mixing
- [ ] 48 kHz output with resampling
- [ ] 95%+ Blargg APU tests pass

**Key Files:**

- `crates/rustynes-apu/` (currently empty)

### Milestone 4: Mappers ⏳ PENDING

**Duration:** March 2026 - May 2026
**Status:** Not Started (0%)
**Target:** May 2026

**Goals:**

- [ ] Mapper trait infrastructure
- [ ] Mappers 0, 1, 2, 3, 4 fully functional
- [ ] iNES header parsing
- [ ] Battery-backed SRAM support
- [ ] 100+ games playable

**Key Files:**

- `crates/rustynes-mappers/` (currently empty)

### Milestone 5: Integration ⏳ PENDING

**Duration:** April 2026 - May 2026
**Status:** Not Started (0%)
**Target:** May 2026

**Goals:**

- [ ] Bus memory routing
- [ ] Console master coordinator
- [ ] ROM loading (iNES, NES 2.0)
- [ ] Save state system
- [ ] Input handling

**Key Files:**

- `crates/rustynes-core/` (currently empty)

### Milestone 6: Desktop GUI ⏳ PENDING

**Duration:** May 2026 - June 2026
**Status:** Not Started (0%)
**Target:** June 2026

**Goals:**

- [ ] egui-based interface
- [ ] wgpu rendering (60 FPS)
- [ ] Audio output (no crackling)
- [ ] Gamepad support
- [ ] Configuration system
- [ ] Cross-platform builds

**Key Files:**

- `crates/rustynes-desktop/` (currently empty)

---

## Dependencies

### Critical Path

```text
M1: CPU (DONE) → M2: PPU → M5: Integration → M6: GUI → MVP Release
                        ↓
                    M3: APU →
                        ↓
                    M4: Mappers →
```

### Milestone Dependencies

| Milestone | Depends On | Blocks |
|-----------|------------|--------|
| M1: CPU | None | M2, M5 |
| M2: PPU | M1 | M5 |
| M3: APU | M1 | M5 |
| M4: Mappers | M1 | M5 |
| M5: Integration | M1, M2, M3, M4 | M6 |
| M6: GUI | M5 | MVP Release |

### External Dependencies

- **Rust Toolchain:** 1.86+ (required by criterion 0.8)
- **Libraries:**
  - bitflags 2.x (CPU status flags)
  - egui 0.24+ (GUI framework)
  - wgpu 0.18+ (graphics backend)
  - cpal or SDL2 (audio output)
  - gilrs (gamepad support)
- **Tools:**
  - cargo (build system)
  - rustfmt (code formatting)
  - clippy (linting)
  - criterion (benchmarking)

---

## Risk Assessment

### High-Risk Items

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| PPU timing complexity | High | Medium | Reference Mesen2, TetaNES implementations |
| MMC3 IRQ accuracy | High | Medium | Dedicated test ROMs, community feedback |
| Audio crackling/latency | Medium | High | Use proven libraries (cpal), buffer tuning |
| Cross-platform builds | Medium | Medium | CI matrix testing, early platform validation |

### Technical Challenges

1. **PPU Sprite Evaluation**
   - 8-sprite limit enforcement
   - Sprite overflow flag quirks
   - Sprite 0 hit edge cases
   - **Mitigation:** Follow NesDev Wiki, test with sprite_hit_tests

2. **APU DMC Channel**
   - DMA conflicts with CPU
   - Sample buffer management
   - Accurate timing
   - **Mitigation:** Study FCEUX/Mesen implementations

3. **MMC3 Scanline Counter**
   - IRQ timing variations
   - A12 edge detection
   - Game-specific quirks
   - **Mitigation:** mmc3_test ROM, real game testing

---

## Timeline

### Month-by-Month Breakdown

#### Month 1: December 2025 ✅ COMPLETE

- ✅ CPU core implementation
- ✅ All 256 opcodes
- ✅ nestest.nes validation
- ✅ Comprehensive unit tests

#### Month 2: January 2026 🔄 CURRENT

- [ ] PPU core structure
- [ ] Background rendering
- [ ] Basic scrolling
- [ ] First frame rendered

#### Month 3: February 2026

- [ ] Sprite rendering
- [ ] Sprite 0 hit
- [ ] APU core structure
- [ ] Pulse channels

#### Month 4: March 2026

- [ ] PPU timing refinement
- [ ] APU triangle/noise/DMC
- [ ] Mapper infrastructure
- [ ] Mappers 0, 2 (NROM, UxROM)

#### Month 5: April 2026

- [ ] Mapper 1 (MMC1)
- [ ] Mapper 3 (CNROM)
- [ ] Mapper 4 (MMC3)
- [ ] Integration layer

#### Month 6: May-June 2026

- [ ] Desktop GUI
- [ ] Save states
- [ ] Input handling
- [ ] Polish and testing
- [ ] MVP Release

### Milestones Timeline

```text
Dec 2025  Jan 2026  Feb 2026  Mar 2026  Apr 2026  May 2026  Jun 2026
   |         |         |         |         |         |         |
   M1 ✅     M2 -----> M2 -----> M3 -----> M4 -----> M5 -----> M6
                       M3 -----> M3 -----> M4        M5        M6
                                 M4 -----> M4        M5        M6
                                                     M5        M6
                                                               MVP ✅
```

---

## Next Steps

### Immediate Actions (Week of 2025-12-19)

1. **Start Milestone 2: PPU**
   - Create `crates/rustynes-ppu/Cargo.toml`
   - Implement PPU register structure
   - Set up VRAM addressing (Loopy model)
   - Begin background tile fetching

2. **Continue Testing**
   - Run Blargg CPU tests
   - Document any failures
   - Create integration test framework

3. **Documentation**
   - Update CHANGELOG.md with CPU completion
   - Document CPU implementation decisions
   - Create PPU implementation plan

### Week 1-2: PPU Core (M2-S1)

- [ ] PPU registers (PPUCTRL, PPUMASK, PPUSTATUS)
- [ ] VRAM address registers (v, t, fine_x, w)
- [ ] Basic dot/scanline counting
- [ ] VBlank flag and NMI generation

### Week 3-4: Background Rendering (M2-S2)

- [ ] Tile fetching pipeline
- [ ] Pattern table reads
- [ ] Nametable reads
- [ ] Palette reads
- [ ] Pixel output

---

## Resources

### Reference Documentation

- [CPU Specification](../docs/cpu/CPU_6502_SPECIFICATION.md)
- [PPU Specification](../docs/ppu/PPU_2C02_SPECIFICATION.md)
- [APU Specification](../docs/apu/APU_2A03_SPECIFICATION.md)
- [Mapper Overview](../docs/mappers/MAPPER_OVERVIEW.md)

### External References

- [NesDev Wiki](https://www.nesdev.org/wiki/)
- [TASVideos Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)
- [Mesen2 Source](https://github.com/SourMesen/Mesen2)
- [TetaNES Source](https://github.com/lukexor/tetanes)

### Test ROMs

- `test-roms/cpu/nestest.nes` - CPU validation
- Blargg test suite (to be acquired)
- TASVideos accuracy suite (to be acquired)

---

**Last Updated:** 2025-12-19
**Maintained By:** Claude Code / Development Team
**Next Review:** Weekly
