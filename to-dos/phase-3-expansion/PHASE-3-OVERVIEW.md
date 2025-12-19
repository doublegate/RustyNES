# Phase 3: Expansion - Overview

**Phase:** 3 (Expansion)
**Duration:** Months 13-18 (January 2027 - June 2027)
**Status:** Planned
**Goal:** Comprehensive mapper support and platform expansion

---

## Table of Contents

- [Overview](#overview)
- [Success Criteria](#success-criteria)
- [Milestones](#milestones)
- [Dependencies](#dependencies)
- [Timeline](#timeline)

---

## Overview

Phase 3 expands RustyNES's reach and capabilities. This phase focuses on comprehensive mapper support (achieving 98% game coverage), expansion audio chips, WebAssembly deployment, and the advanced TAS editor.

### Core Objectives

1. **Expansion Audio**
   - VRC6, VRC7, MMC5 audio channels
   - Namco 163, Sunsoft 5B
   - FDS (Famicom Disk System) audio

2. **50 Total Mappers**
   - 98% game coverage
   - All common mappers (0-26, 69, etc.)
   - Proper IRQ timing for each

3. **WebAssembly Port**
   - Browser-based emulation
   - Touch controls for mobile
   - PWA (Progressive Web App) support

4. **TAS Editor**
   - Greenzone frame history
   - Piano roll input editing
   - Branch system
   - Competitive with FCEUX

---

## Success Criteria

### Technical Metrics

| Metric | Phase 3 Target | Measurement |
|--------|----------------|-------------|
| **Accuracy** | 98% TASVideos | Test ROM pass rate |
| **Mappers** | 50 (98% coverage) | Implementation count |
| **Game Compatibility** | 99%+ | Manual testing |
| **WebAssembly** | 60 FPS desktop | Browser performance |
| **Expansion Audio** | 6 chips | Audio implementation |

### Quality Gates

- [ ] Castlevania III (VRC6) audio accurate
- [ ] Lagrange Point (VRC7) audio accurate
- [ ] 50 mappers fully functional
- [ ] WebAssembly runs at 60 FPS on desktop browsers
- [ ] TAS editor can create/edit movies
- [ ] 99%+ of games playable

### Deliverables

- [ ] Expansion audio support (integrated in rustynes-apu)
- [ ] 45 additional mappers (total 50)
- [ ] WebAssembly build (rustynes-web)
- [ ] TAS editor (rustynes-tas enhanced)
- [ ] Web demo deployment
- [ ] Comprehensive mapper documentation

---

## Milestones

### Milestone 11: WebAssembly (Months 16-17)

**Duration:** April 2027 - May 2027
**Status:** Planned
**Target:** May 2027

**Goals:**

- [ ] wasm-pack build configuration
- [ ] Web frontend (HTML/CSS/JS)
- [ ] Browser audio/video APIs
- [ ] Virtual filesystem (for ROMs)
- [ ] Touch controls (mobile)
- [ ] PWA support

**Key Files:**

- `crates/rustynes-web/` (to be expanded)

**Acceptance Criteria:**

- [ ] Runs in Chrome, Firefox, Safari
- [ ] 60 FPS on desktop browsers
- [ ] 30+ FPS on mobile
- [ ] ROMs load from local files

### Milestone 12: Expansion Audio (Months 13-15)

**Duration:** January 2027 - March 2027
**Status:** Planned
**Target:** March 2027

**Goals:**

- [ ] VRC6 (2 pulse + sawtooth)
- [ ] VRC7 (FM synthesis)
- [ ] MMC5 (2 pulse + PCM)
- [ ] Namco 163 (8 wavetable channels)
- [ ] Sunsoft 5B (3 square + noise)
- [ ] FDS (wavetable + modulation)

**Test Games:**

- Castlevania III (VRC6)
- Lagrange Point (VRC7)
- Castlevania (FDS)

**Key Integration:**

- Integrated into `crates/rustynes-apu/`

**Acceptance Criteria:**

- [ ] Expansion audio sounds accurate
- [ ] Music matches hardware recordings
- [ ] Proper channel mixing

### Milestone 13: Additional Mappers (Months 14-17)

**Duration:** February 2027 - May 2027
**Status:** Planned
**Target:** May 2027

**Target:** 98% game coverage (50 total mappers)

**Priority Mappers:**

- [ ] Mapper 5 (MMC5) - ExROM
- [ ] Mapper 7 (AxROM) - Battletoads
- [ ] Mapper 9/10 (MMC2/4) - Punch-Out!!
- [ ] Mapper 11 (ColorDreams)
- [ ] Mapper 19 (Namco 163)
- [ ] Mapper 23/25 (VRC2/4)
- [ ] Mapper 24/26 (VRC6)
- [ ] Mapper 69 (Sunsoft FME-7)
- [ ] + 30 more common mappers

**Key Files:**

- `crates/rustynes-mappers/` (expansion)

**Acceptance Criteria:**

- [ ] All target games playable
- [ ] Mapper-specific test ROMs pass
- [ ] IRQ timing accurate

### Milestone 14: Mobile Support (Optional)

**Duration:** TBD
**Status:** Planned (Optional)
**Target:** TBD

**Note:** This milestone may be skipped if WebAssembly performance is sufficient for mobile browsers.

**Goals (if approved):**

- [ ] Android native app
- [ ] iOS native app
- [ ] Touch controls optimized for mobile
- [ ] Performance tuning for mobile CPUs
- [ ] Battery life optimization

---

## Dependencies

### Critical Path

```text
Phase 2 Complete → M12 (Expansion Audio) → Phase 3 Complete
                 → M13 (Extra Mappers) ↗
                 → M11 (WebAssembly) ↗
                 → (M14 Mobile - Optional) ↗
```

### Milestone Dependencies

| Milestone | Depends On | Blocks |
|-----------|------------|--------|
| M11: WebAssembly | Phase 2, no_std core | None |
| M12: Expansion Audio | Phase 1 APU, Mapper framework | None |
| M13: Extra Mappers | Phase 1 Mappers | None |
| M14: Mobile (Optional) | M11 (WebAssembly) | None |

### External Dependencies

- **Libraries:**
  - wasm-bindgen (WebAssembly bindings)
  - web-sys (Browser APIs)
  - Additional mapper implementations (reference: puNES)

---

## Timeline

### Month-by-Month Breakdown

#### Month 13: January 2027

- [ ] M12: VRC6, VRC7 expansion audio
- [ ] M13: Mappers 5, 7, 9, 10 implementation

#### Month 14: February 2027

- [ ] M12: MMC5, Namco 163 expansion audio
- [ ] M13: Mappers 11, 19, 23, 25 implementation

#### Month 15: March 2027

- [ ] M12: Sunsoft 5B, FDS expansion audio complete
- [ ] M13: Mappers 24, 26, 69 + 10 more

#### Month 16: April 2027

- [ ] M13: Additional 15 mappers
- [ ] M11: WebAssembly build configuration
- [ ] M11: Web frontend development

#### Month 17: May 2027

- [ ] M13: Final 5 mappers (50 total)
- [ ] M11: Touch controls and PWA
- [ ] M11: WebAssembly optimization

#### Month 18: June 2027

- [ ] Phase 3 integration testing
- [ ] Web demo deployment
- [ ] Documentation updates
- [ ] Phase 3 expansion complete release

### Milestones Timeline

```text
Jan 2027  Feb 2027  Mar 2027  Apr 2027  May 2027  Jun 2027
   |         |         |         |         |         |
   M12 ----> M12 ----> M12 ✓
   M13 ----> M13 ----> M13 ----> M13 ----> M13 ✓
                                 M11 ----> M11 ✓
                                                   Phase 3 ✓
```

---

## Risk Assessment

### High-Risk Items

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| VRC7 FM synthesis accuracy | Medium | High | Reference YM2413 documentation, Mesen2 |
| Mapper IRQ complexity | High | Medium | Extensive test ROMs, real game testing |
| WebAssembly performance | Medium | Medium | Profiling, optimization, SIMD where possible |
| Mobile performance (if pursued) | High | High | May defer/cancel if WebAssembly insufficient |

### Technical Challenges

1. **Expansion Audio Mixing**
   - Challenge: Proper channel balance across different chips
   - Mitigation: Hardware recordings reference, community feedback

2. **Mapper IRQ Timing**
   - Challenge: Game-specific quirks in IRQ behavior
   - Mitigation: Comprehensive test ROM suite, real game validation

3. **WebAssembly Performance**
   - Challenge: Maintaining 60 FPS in browser
   - Mitigation: Profiling, wasm-opt optimization, consider WebAssembly SIMD

---

## Next Steps

### Phase 3 Kickoff (January 2027)

1. **Finalize Phase 2**
   - Complete all advanced features
   - Validate 95% TASVideos accuracy
   - Address any critical bugs

2. **Start Milestone 12: Expansion Audio**
   - Research VRC6/VRC7 specifications
   - Design expansion audio architecture
   - Acquire test games with expansion audio

3. **Plan Milestone 13: Extra Mappers**
   - Prioritize mapper implementation order
   - Acquire test ROMs for each mapper
   - Study reference implementations (puNES)

---

## Resources

### Reference Documentation

- [NesDev Wiki - VRC6](https://www.nesdev.org/wiki/VRC6_audio)
- [NesDev Wiki - VRC7](https://www.nesdev.org/wiki/VRC7_audio)
- [NesDev Wiki - MMC5](https://www.nesdev.org/wiki/MMC5)
- [NesDev Wiki - FDS Audio](https://www.nesdev.org/wiki/FDS_audio)
- [WebAssembly Documentation](https://webassembly.org/)

### Reference Implementations

- puNES (461+ mappers)
- Mesen2 (expansion audio, mapper implementations)
- Rustico (Rust expansion audio reference)
- TetaNES (WebAssembly build)

### Test ROMs

- VRC6/VRC7 test ROMs
- MMC5 test ROMs
- FDS test suite
- Mapper-specific validation ROMs

---

**Last Updated:** 2025-12-19
**Maintained By:** Claude Code / Development Team
**Next Review:** Upon Phase 2 completion
