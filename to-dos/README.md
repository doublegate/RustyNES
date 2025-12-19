# RustyNES Phase 1 TODO Tracker

**Version:** 1.0.0
**Last Updated:** 2025-12-19
**Phase:** Phase 1 - MVP (Months 1-6)
**Status:** In Progress (Milestone 1 Complete)

---

## Overview

This directory contains comprehensive TODO files tracking all tasks for Phase 1 of RustyNES development. Each milestone is broken down into sprints with detailed task lists, acceptance criteria, and progress tracking.

## Quick Navigation

### Milestone Overviews

- [Phase 1 Overview](PHASE-1-OVERVIEW.md) - Complete Phase 1 summary
- [Milestone 1: CPU](milestone-1-cpu/M1-OVERVIEW.md) - ✅ **COMPLETED**
- [Milestone 2: PPU](milestone-2-ppu/M2-OVERVIEW.md) - 🔄 In Progress
- [Milestone 3: APU](milestone-3-apu/M3-OVERVIEW.md) - ⏳ Pending
- [Milestone 4: Mappers](milestone-4-mappers/M4-OVERVIEW.md) - ⏳ Pending
- [Milestone 5: Integration](milestone-5-integration/M5-OVERVIEW.md) - ⏳ Pending
- [Milestone 6: GUI](milestone-6-gui/M6-OVERVIEW.md) - ⏳ Pending

### Milestone 1: CPU (COMPLETED ✅)

**Duration:** December 2025 - January 2026
**Status:** Complete

- [x] [Sprint 1: CPU Core](milestone-1-cpu/M1-S1-cpu-core.md) - Basic structure
- [x] [Sprint 2: Opcodes](milestone-1-cpu/M1-S2-opcodes.md) - All 256 opcodes
- [x] [Sprint 3: Addressing](milestone-1-cpu/M1-S3-addressing.md) - All addressing modes
- [x] [Sprint 4: Interrupts](milestone-1-cpu/M1-S4-interrupts.md) - NMI, IRQ, BRK
- [x] [Sprint 5: nestest](milestone-1-cpu/M1-S5-nestest.md) - Golden log validation

**Key Achievements:**

- Cycle-accurate 6502 implementation
- All 256 opcodes (151 official + 105 unofficial)
- Complete interrupt handling
- Zero unsafe code
- Comprehensive test suite

### Milestone 2: PPU (IN PROGRESS 🔄)

**Duration:** January 2026 - March 2026
**Target Date:** March 2026

- [ ] [Sprint 1: PPU Core](milestone-2-ppu/M2-S1-ppu-core.md) - Basic structure
- [ ] [Sprint 2: Background Rendering](milestone-2-ppu/M2-S2-background.md) - Backgrounds
- [ ] [Sprint 3: Sprite Rendering](milestone-2-ppu/M2-S3-sprites.md) - Sprites
- [ ] [Sprint 4: Scrolling](milestone-2-ppu/M2-S4-scrolling.md) - Loopy scrolling
- [ ] [Sprint 5: Timing](milestone-2-ppu/M2-S5-timing.md) - Dot-level accuracy

### Milestone 3: APU (PENDING ⏳)

**Duration:** February 2026 - April 2026
**Target Date:** April 2026

- [ ] [Sprint 1: APU Core](milestone-3-apu/M3-S1-apu-core.md) - Basic structure
- [ ] [Sprint 2: Pulse Channels](milestone-3-apu/M3-S2-pulse.md) - Square waves
- [ ] [Sprint 3: Triangle/Noise](milestone-3-apu/M3-S3-triangle-noise.md) - Triangle + Noise
- [ ] [Sprint 4: DMC](milestone-3-apu/M3-S4-dmc.md) - Delta modulation
- [ ] [Sprint 5: Mixing](milestone-3-apu/M3-S5-mixing.md) - Audio output

### Milestone 4: Mappers (PENDING ⏳)

**Duration:** March 2026 - May 2026
**Target Date:** May 2026

- [ ] [Sprint 1: Mapper Infrastructure](milestone-4-mappers/M4-S1-infrastructure.md) - Trait design
- [ ] [Sprint 2: NROM & UxROM](milestone-4-mappers/M4-S2-basic.md) - Mappers 0, 2
- [ ] [Sprint 3: MMC1](milestone-4-mappers/M4-S3-mmc1.md) - Mapper 1
- [ ] [Sprint 4: CNROM](milestone-4-mappers/M4-S4-cnrom.md) - Mapper 3
- [ ] [Sprint 5: MMC3](milestone-4-mappers/M4-S5-mmc3.md) - Mapper 4

### Milestone 5: Integration (PENDING ⏳)

**Duration:** April 2026 - May 2026
**Target Date:** May 2026

- [ ] [Sprint 1: Bus Integration](milestone-5-integration/M5-S1-bus.md) - Memory routing
- [ ] [Sprint 2: Console](milestone-5-integration/M5-S2-console.md) - Master coordinator
- [ ] [Sprint 3: Cartridge](milestone-5-integration/M5-S3-cartridge.md) - ROM loading
- [ ] [Sprint 4: Save States](milestone-5-integration/M5-S4-save-states.md) - Serialization
- [ ] [Sprint 5: Input](milestone-5-integration/M5-S5-input.md) - Controllers

### Milestone 6: Desktop GUI (PENDING ⏳)

**Duration:** May 2026 - June 2026
**Target Date:** June 2026

- [ ] [Sprint 1: GUI Framework](milestone-6-gui/M6-S1-framework.md) - egui setup
- [ ] [Sprint 2: Video Output](milestone-6-gui/M6-S2-video.md) - wgpu rendering
- [ ] [Sprint 3: Audio Output](milestone-6-gui/M6-S3-audio.md) - Sound playback
- [ ] [Sprint 4: Input Handling](milestone-6-gui/M6-S4-input.md) - Controllers
- [ ] [Sprint 5: UI Features](milestone-6-gui/M6-S5-ui.md) - Menus, settings

---

## Progress Summary

### Overall Phase 1 Progress

| Milestone | Status | Progress | Target Date |
|-----------|--------|----------|-------------|
| M1: CPU | ✅ Complete | 100% | January 2026 |
| M2: PPU | 🔄 In Progress | 0% | March 2026 |
| M3: APU | ⏳ Pending | 0% | April 2026 |
| M4: Mappers | ⏳ Pending | 0% | May 2026 |
| M5: Integration | ⏳ Pending | 0% | May 2026 |
| M6: GUI | ⏳ Pending | 0% | June 2026 |
| **Phase 1 Total** | 🔄 In Progress | **17%** | **June 2026** |

### Sprint Status

- ✅ Completed: 5 sprints (Milestone 1)
- 🔄 In Progress: 0 sprints
- ⏳ Pending: 25 sprints (Milestones 2-6)

---

## How to Use This Directory

### For Developers

1. **Check milestone overview** - Understand goals and dependencies
2. **Read sprint TODO** - See detailed task breakdown
3. **Update status** - Mark tasks as completed with dates and commits
4. **Add notes** - Document decisions, blockers, and learnings

### For Project Management

1. **Track progress** - Monitor sprint completion rates
2. **Identify blockers** - Address dependencies and issues
3. **Plan sprints** - Use estimates for scheduling
4. **Review retrospectives** - Learn from completed work

### Status Markers

- ✅ **Complete** - All tasks done, tests passing
- 🔄 **In Progress** - Active development
- ⏳ **Pending** - Not started
- ⚠️ **Blocked** - Waiting on dependency
- 🐛 **Issues** - Bugs or problems

---

## Key Documentation

### Architecture & Design

- [ARCHITECTURE.md](../ARCHITECTURE.md) - System design
- [OVERVIEW.md](../OVERVIEW.md) - Project philosophy
- [ROADMAP.md](../ROADMAP.md) - Development timeline

### Component Specifications

- [CPU Specification](../docs/cpu/CPU_6502_SPECIFICATION.md)
- [PPU Specification](../docs/ppu/PPU_2C02_SPECIFICATION.md)
- [APU Specification](../docs/apu/APU_2A03_SPECIFICATION.md)

### Testing

- [Test ROM Guide](../docs/testing/TEST_ROM_GUIDE.md)
- [nestest Golden Log](../docs/testing/NESTEST_GOLDEN_LOG.md)
- [Testing Strategy](../docs/dev/TESTING.md)

---

## Contributing

When updating TODO files:

1. ✅ Mark completed tasks with [x]
2. 📅 Add completion dates for finished sprints
3. 🔗 Link to commit hashes for completed work
4. 📝 Document decisions and learnings
5. ⚠️ Note any blockers or issues

---

## Related Files

- [CHANGELOG.md](../CHANGELOG.md) - Project history
- [CONTRIBUTING.md](../docs/dev/CONTRIBUTING.md) - Contribution guidelines
- [BUILD.md](../docs/dev/BUILD.md) - Build instructions

---

**Last Updated:** 2025-12-19
**Maintained By:** Claude Code / Development Team
**Repository:** <https://github.com/doublegate/RustyNES>
