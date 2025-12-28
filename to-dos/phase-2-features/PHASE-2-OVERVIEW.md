# Phase 2: Advanced Features - Overview

**Phase:** 2 (Advanced Features)
**Duration:** Months 7-12 (July 2026 - December 2026)
**Status:** Planned
**Goal:** Feature parity with modern emulators
**Last Updated:** 2025-12-28

---

## Table of Contents

- [Overview](#overview)
- [Technology Foundation](#technology-foundation-v071)
- [Success Criteria](#success-criteria)
- [Milestones](#milestones)
- [Dependencies](#dependencies)
- [Timeline](#timeline)

---

## Overview

Phase 2 transforms RustyNES from a functional emulator into a feature-rich platform. This phase adds advanced capabilities that distinguish modern emulators: online multiplayer, achievement tracking, scripting support, and comprehensive debugging tools.

### Technology Foundation (v0.7.1+)

Phase 2 builds upon the **eframe 0.33 + egui 0.33** desktop frontend established in v0.7.1:

| Component | Library | Version | Purpose |
|-----------|---------|---------|---------|
| GUI Framework | eframe/egui | 0.33 | Immediate mode GUI |
| Window/Render | eframe (glow) | 0.33 | OpenGL rendering backend |
| Audio | cpal | 0.16 | Low-latency audio with buffer underrun reporting |
| Input | gilrs | 0.11 | Gamepad support with hotplug |
| File Dialogs | rfd | 0.15 | Native file picker |
| Configuration | ron + serde | 0.12 | Settings persistence |
| Rust Edition | 2024 | MSRV 1.88 | Latest Rust features |

**Key Architectural Notes:**
- **Immediate Mode GUI**: egui's immediate mode paradigm simplifies debug windows and real-time displays
- **OpenGL Rendering**: glow backend via eframe (not wgpu) - simpler than shader pipeline
- **Lock-Free Audio**: cpal 0.16 with ring buffer and underrun detection
- **Native Dialogs**: Platform-native file dialogs via rfd integration

### Core Objectives

1. **RetroAchievements Integration**
   - rcheevos FFI bindings
   - Achievement detection and unlock system
   - Leaderboard support
   - Rich presence
   - **egui UI**: Achievement toasts via `egui::Window`, list panels, progress indicators

2. **GGPO Rollback Netplay**
   - backroll-rs (Rust GGPO port)
   - Minimal input lag
   - Robust synchronization
   - NAT traversal
   - **egui UI**: Lobby dialogs, connection status panels, spectator UI

3. **Lua 5.4 Scripting**
   - Runtime scripting API
   - Memory manipulation
   - Frame callbacks
   - GUI overlays
   - **egui Integration**: Script console, output log, overlay drawing layer

4. **Integrated Debugger**
   - CPU debugging (breakpoints, stepping)
   - PPU/APU visualization
   - Memory editing
   - Trace logging
   - **egui Native**: All debug windows use native egui (not overlay on separate framework)

---

## Success Criteria

### Technical Metrics

| Metric | Phase 2 Target | Measurement |
|--------|----------------|-------------|
| **Accuracy** | 95% TASVideos | Test ROM pass rate |
| **Features** | 4 advanced features | Implementation status |
| **Performance** | 500 FPS (8.3x real-time) | Benchmark suite |
| **Achievements** | 10 games tested | RetroAchievements validation |
| **Netplay** | <5 frame rollback @ 100ms | Latency testing |

### Quality Gates

- [ ] RetroAchievements unlock correctly in 10 games
- [ ] Netplay works with <2 frame input lag on LAN
- [ ] Lua scripts can read/write memory at 60 Hz
- [ ] Debugger useful for homebrew development
- [ ] All Phase 1 features remain functional

### Deliverables

- [ ] RetroAchievements support (rustynes-achievements)
- [ ] Netplay functionality (rustynes-netplay)
- [ ] Lua scripting API (integrated in rustynes-core)
- [ ] Advanced debugger (integrated in rustynes-desktop)
- [ ] Updated documentation for all features
- [ ] Tutorial videos for advanced features

---

## Milestones

### Milestone 7: RetroAchievements (Months 7-8)

**Duration:** July 2026 - August 2026
**Status:** Planned
**Target:** August 2026

**Goals:**

- [ ] rcheevos FFI integration
- [ ] Achievement detection logic
- [ ] UI notifications (toast popups)
- [ ] Login system
- [ ] Leaderboard support
- [ ] Rich presence

**Key Files:**

- `crates/rustynes-achievements/` (to be created)

**Acceptance Criteria:**

- [ ] Achievements unlock correctly in 10 test games
- [ ] No false positives/negatives
- [ ] Leaderboard submissions work
- [ ] <1% performance impact

### Milestone 8: GGPO Netplay (Months 7-9)

**Duration:** July 2026 - September 2026
**Status:** Planned
**Target:** September 2026

**Goals:**

- [ ] backroll-rs integration (Rust GGPO port)
- [ ] Save state serialization for rollback
- [ ] Input prediction/rollback
- [ ] Lobby system
- [ ] Spectator mode
- [ ] NAT traversal (STUN/TURN)

**Key Files:**

- `crates/rustynes-netplay/` (to be created)

**Acceptance Criteria:**

- [ ] 1-2 frame input lag over LAN
- [ ] <5 frame rollback on 100ms ping
- [ ] No desyncs in 30-minute sessions
- [ ] Works behind typical NAT setups

### Milestone 9: Lua Scripting (Months 9-10)

**Duration:** September 2026 - October 2026
**Status:** Planned
**Target:** October 2026

**Goals:**

- [ ] mlua 5.4 integration
- [ ] Memory read/write API
- [ ] Callback hooks (frame, scanline, instruction)
- [ ] Input injection
- [ ] GUI overlay support
- [ ] Example scripts (hitbox viewer, bot AI)

**Key Integration:**

- Integrated into `crates/rustynes-core/` and `crates/rustynes-desktop/`

**Acceptance Criteria:**

- [ ] Can read/write RAM from Lua
- [ ] Frame callbacks work at 60 Hz
- [ ] Drawing primitives render correctly
- [ ] <5% performance overhead

### Milestone 10: Advanced Debugger (Months 10-11)

**Duration:** October 2026 - November 2026
**Status:** Planned
**Target:** November 2026

**Goals:**

- [ ] CPU debugger (disassembly, breakpoints, stepping)
- [ ] PPU viewer (nametables, pattern tables, palettes, OAM)
- [ ] APU viewer (channel waveforms, volume meters)
- [ ] Memory viewer/editor (hex dump, search)
- [ ] Trace logger
- [ ] Code-data logger (CDL)

**Key Integration:**

- Integrated into `crates/rustynes-desktop/`

**Acceptance Criteria:**

- [ ] Breakpoints work reliably
- [ ] PPU viewer updates in real-time
- [ ] Trace logger captures execution
- [ ] Useful for homebrew debugging

---

## Dependencies

### Critical Path

```text
Phase 1 Complete → M7 (Achievements) → Phase 2 Complete
                 → M8 (Netplay) ↗
                 → M9 (Scripting) ↗
                 → M10 (Debugger) ↗
```

### Milestone Dependencies

| Milestone | Depends On | Blocks |
|-----------|------------|--------|
| M7: RetroAchievements | Phase 1 (MVP) | None |
| M8: Netplay | Phase 1 (MVP), Save states | None |
| M9: Scripting | Phase 1 (MVP) | None |
| M10: Debugger | Phase 1 (MVP) | None |

### External Dependencies

- **Core Libraries (v0.7.1 baseline):**
  - eframe 0.33 + egui 0.33 (immediate mode GUI framework)
  - cpal 0.16 (audio with buffer underrun reporting)
  - gilrs 0.11 (gamepad with hotplug detection)
  - ron 0.12 + serde (configuration persistence)
  - rfd 0.15 (native file dialogs)

- **Phase 2 Libraries:**
  - rcheevos (FFI bindings for RetroAchievements)
  - backroll-rs (GGPO rollback implementation)
  - mlua 0.10+ (Lua 5.4 bindings)

- **egui 0.33 Features for Phase 2:**
  - `egui::Window` for debug/dialog windows
  - `egui::Grid` for memory hex viewer
  - `egui::ScrollArea` for disassembly/trace logs
  - `egui::plot` for APU waveform visualization
  - `egui::TextEdit` for Lua console input
  - `egui::Modal` (0.33+) for confirmation dialogs
  - `egui_extras::TableBuilder` for structured data display

---

## Timeline

### Month-by-Month Breakdown

#### Month 7: July 2026

- [ ] M7: RetroAchievements core implementation
- [ ] M8: Netplay architecture design
- [ ] Save state serialization for netplay

#### Month 8: August 2026

- [ ] M7: RetroAchievements polish and testing
- [ ] M8: backroll-rs integration
- [ ] M8: Basic netplay functionality

#### Month 9: September 2026

- [ ] M8: Netplay lobby system and NAT traversal
- [ ] M9: Lua scripting API design
- [ ] M9: mlua integration

#### Month 10: October 2026

- [ ] M9: Lua scripting callbacks and overlays
- [ ] M10: CPU debugger implementation
- [ ] M10: Breakpoints and stepping

#### Month 11: November 2026

- [ ] M10: PPU/APU viewers
- [ ] M10: Memory editor and trace logger
- [ ] Phase 2 integration testing

#### Month 12: December 2026

- [ ] Documentation for all Phase 2 features
- [ ] Tutorial videos
- [ ] Community testing and feedback
- [ ] Phase 2 feature complete release

### Milestones Timeline

```text
Jul 2026  Aug 2026  Sep 2026  Oct 2026  Nov 2026  Dec 2026
   |         |         |         |         |         |
   M7 -----> M7 ✓
   M8 -----> M8 -----> M8 ✓
                       M9 -----> M9 ✓
                                 M10 ----> M10 ✓
                                                   Phase 2 ✓
```

---

## Risk Assessment

### High-Risk Items

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| rcheevos FFI complexity | Medium | Medium | Reference existing integrations (RetroArch) |
| Netplay desyncs | High | High | Deterministic emulation validation, extensive testing |
| Lua overhead | Medium | Medium | Profiling, selective callback usage |
| Debugger UI complexity | Medium | Low | Iterative design, user feedback |

### Technical Challenges

1. **Netplay Determinism**
   - Challenge: Perfect determinism required for rollback
   - Mitigation: Extensive save state testing, replay validation

2. **Achievement False Positives**
   - Challenge: Cheats/memory editing could trigger achievements
   - Mitigation: Standard rcheevos validation, user trust model

3. **Lua Performance**
   - Challenge: Scripting overhead at 60 Hz
   - Mitigation: Optimize callback frequency, JIT compilation

---

## Next Steps

### Phase 2 Kickoff (July 2026)

1. **Finalize Phase 1**
   - Complete MVP release
   - Address any critical bugs
   - Establish baseline performance

2. **Start Milestone 7: RetroAchievements**
   - Research rcheevos FFI integration
   - Design achievement detection architecture
   - Create test game list

3. **Plan Milestone 8: Netplay**
   - Research backroll-rs API
   - Design save state serialization format
   - Plan NAT traversal strategy

---

## Resources

### Reference Documentation

- [RetroAchievements API](https://docs.retroachievements.org/)
- [GGPO Whitepaper](https://www.ggpo.net/)
- [backroll-rs Documentation](https://github.com/HouraiTeahouse/backroll-rs)
- [mlua Documentation](https://github.com/khvzak/mlua)

### Reference Implementations

- RetroArch (rcheevos integration)
- FCEUX (Lua scripting, debugging)
- BizHawk (comprehensive debugging tools)
- Mesen2 (netplay, debugging)

---

## Technology Migration Notes

### GUI Framework Migration (v0.7.1)

Phase 2 documentation has been updated to reflect the GUI framework migration completed in v0.7.1:

| Previous (v0.5.0-v0.6.0) | Current (v0.7.1+) |
|--------------------------|-------------------|
| Iced 0.13 (Elm architecture) | eframe 0.33 + egui 0.33 (immediate mode) |
| wgpu shader pipeline | OpenGL via glow backend |
| pixels crate framebuffer | egui textures for framebuffer |
| Retained-mode widgets | Immediate-mode widgets |

**Implications for Phase 2:**
- Debug windows use native egui (simpler, no overlay complexity)
- Achievement toasts use `egui::Window` with custom positioning
- Netplay lobby uses `egui::Modal` for dialogs
- Lua drawing renders to egui overlay layer
- All UI code benefits from immediate mode simplicity

### Rust 2024 Edition

Phase 2 uses Rust 2024 edition with MSRV 1.88, enabling:
- Gen blocks (iterators)
- Enhanced pattern matching
- Improved async support
- Better diagnostics

---

**Last Updated:** 2025-12-28
**Maintained By:** Claude Code / Development Team
**Next Review:** Upon Phase 1.5 completion (M9-M10)
