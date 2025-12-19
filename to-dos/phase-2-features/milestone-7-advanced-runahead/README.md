# Milestone 7: Advanced Run-Ahead System

**Phase:** 2 (Advanced Features)
**Duration:** Months 7-8 (2 months)
**Status:** Planned
**Target:** August 2026
**Prerequisites:** M6 MVP Complete (Basic RA=1 implemented)

---

## Overview

Milestone 7 implements the **Advanced Run-Ahead System** building on the basic RA=1 foundation from M6-S5. This milestone delivers configurable run-ahead (RA=0-4) with auto-detection, dual-instance mode for pristine audio quality, frame delay auto-tuning, and per-game profiles for optimal latency reduction.

**Run-Ahead Evolution:**
- **M6 (MVP):** Basic run-ahead (RA=1) - architectural foundation
- **M7 (This Milestone):** Advanced system (RA=0-4, auto-detect, dual-instance, frame delay)
- **M17 (Phase 4):** Full optimization (multi-threading, memory pools, profiling)

---

## Goals

### Core Features

- [ ] **Configurable Run-Ahead (RA=0-4)**
  - User-selectable frames (0 = disabled, 1-4 = aggressive)
  - Real-time switching without restart
  - Validated state management

- [ ] **Auto-Detection System**
  - Per-game analysis of optimal RA setting
  - Heuristic based on game characteristics:
    - Platformers: RA=2-3 (input-critical)
    - RPGs/Adventure: RA=0-1 (less input-sensitive)
    - Action: RA=1-2 (balanced)
  - ROM database with community-verified settings

- [ ] **Dual-Instance Mode**
  - Separate emulator instance for audio
  - Non-speculative audio (perfect quality)
  - Synchronized state sharing
  - Fallback to single-instance if performance insufficient

- [ ] **Frame Delay Auto-Tuning**
  - Measure monitor latency
  - Delay rendering to compensate
  - Configurable 0-15 frames
  - Per-game profiles

- [ ] **Just-In-Time Input Polling**
  - Poll input at last possible moment before rendering
  - Minimize input-to-display latency
  - Thread-safe input buffer

- [ ] **Preemptive Frames Mode (Alternative)**
  - Run-ahead alternative for games with deterministic AI
  - Pre-compute next N frames
  - Instant frame switching on input change

---

## Architecture

### Dual-Instance Design

```
┌─────────────────────────────────────────────┐
│         Main Application (Iced)             │
│  ┌─────────────────┐    ┌─────────────────┐ │
│  │ Display Instance│◄───│ Audio Instance  │ │
│  │                 │    │                 │ │
│  │ • RA=0-4        │    │ • No RA         │ │
│  │ • Speculative   │    │ • Pristine      │ │
│  │ • Visual only   │    │ • Audio only    │ │
│  └─────────────────┘    └─────────────────┘ │
│         ▲                      ▲            │
│         │                      │            │
│    ┌────┴──────────────────────┴────┐       │
│    │ State Synchronization Manager  │       │
│    │ • Share savestate each frame   │       │
│    │ • Maintain consistency         │       │
│    └────────────────────────────────┘       │
└─────────────────────────────────────────────┘
```

### Auto-Detection Algorithm

```rust
fn detect_optimal_runahead(rom: &Rom) -> u8 {
    let game_characteristics = analyze_game(rom);

    match game_characteristics {
        // Platformers: High input sensitivity
        GameType::Platformer => {
            if game_characteristics.has_tight_controls() {
                3  // Aggressive RA for Super Mario Bros, Mega Man
            } else {
                2  // Moderate RA
            }
        }

        // RPGs: Low input sensitivity (menu-driven)
        GameType::RPG | GameType::Adventure => {
            1  // Conservative RA for Final Fantasy, Zelda
        }

        // Action: Balanced
        GameType::Action | GameType::Shooter => {
            2  // Moderate RA for Contra, Gradius
        }

        // Puzzle: Minimal
        GameType::Puzzle => {
            0  // No RA for Tetris (turn-based)
        }

        _ => 1  // Default: Conservative RA
    }
}
```

---

## Technical Details

### Run-Ahead Performance (RA=0-4)

| RA Setting | CPU Overhead | Latency Reduction | Use Case |
|------------|--------------|-------------------|----------|
| **RA=0** | 1x (no overhead) | 0ms (2 frames native) | Turn-based games, puzzles |
| **RA=1** | 2x | 16.67ms (1 frame) | RPGs, adventure games |
| **RA=2** | 3x | 33.33ms (0 frames) | Action games, platformers |
| **RA=3** | 4x | 50.00ms (-1 frames)* | Highly responsive platformers |
| **RA=4** | 5x | 66.67ms (-2 frames)* | Extreme (competitive speedrunning) |

*Negative frames = frame appears BEFORE input (predictive)

### Dual-Instance Synchronization

**State Sharing Strategy:**
1. Display instance saves state after each frame
2. Audio instance loads state and emulates SAME frame
3. Audio output from non-speculative instance (pristine)
4. Visual output from speculative instance (low latency)

**Performance:**
- State serialization: <1ms (bincode)
- State transfer: <0.1ms (in-memory copy)
- Total overhead: ~1-2ms per frame

### Frame Delay Calculation

```rust
fn calculate_optimal_frame_delay(monitor_latency_ms: f32) -> u8 {
    // Target: Render frame as close to vsync as possible
    let frame_time_ms = 16.67;  // 60 FPS

    // Delay frames to compensate for monitor latency
    let delay_frames = (monitor_latency_ms / frame_time_ms).ceil() as u8;

    // Clamp to 0-15 range
    delay_frames.min(15)
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Run-ahead configurable (RA=0-4) via settings UI
- [ ] Auto-detection correctly identifies optimal RA for 50+ games
- [ ] Dual-instance mode works on all platforms
- [ ] Audio quality identical to non-RA mode (dual-instance)
- [ ] Frame delay reduces total latency measurably
- [ ] JIT input polling works without race conditions
- [ ] Per-game profiles load automatically

### Performance

- [ ] RA=2: Achieves 180+ FPS (3x emulation speed)
- [ ] RA=3: Achieves 240+ FPS (4x emulation speed)
- [ ] State serialization <1ms
- [ ] Dual-instance sync overhead <2ms
- [ ] Memory: <150 MB (dual-instance mode)

### User Experience

- [ ] Settings UI clearly explains RA benefits/trade-offs
- [ ] Auto-detection can be overridden per-game
- [ ] Performance insufficient warning (if CPU can't sustain 60 FPS)
- [ ] Visual indicator for RA status (overlay)

---

## Implementation Plan

### Sprint 1: Configurable Run-Ahead (RA=0-4)

**Duration:** 2 weeks

- [ ] Extend RunAheadManager to support RA=0-4
- [ ] Settings UI for RA selection
- [ ] Validation and fallback logic
- [ ] Performance testing at each RA level

### Sprint 2: Auto-Detection System

**Duration:** 2 weeks

- [ ] Game characteristic analyzer
- [ ] Heuristic algorithm implementation
- [ ] ROM database creation (50+ games)
- [ ] Per-game profile storage (TOML)

### Sprint 3: Dual-Instance Mode

**Duration:** 3 weeks

- [ ] Separate emulator instance for audio
- [ ] State synchronization manager
- [ ] Audio routing from non-speculative instance
- [ ] Performance optimization

### Sprint 4: Frame Delay & JIT Input

**Duration:** 2 weeks

- [ ] Monitor latency measurement
- [ ] Frame delay auto-tuning
- [ ] JIT input polling system
- [ ] Thread-safe input buffer

---

## Dependencies

### Prerequisites

- **M6 MVP Complete:** Basic RA=1 implemented
- **Save State System:** Fast serialization (<1ms)
- **Performance Baseline:** NES achieves 300+ FPS

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies]
bincode = "1.3"            # Fast state serialization
crossbeam-channel = "0.5"  # Thread-safe state sharing
parking_lot = "0.12"       # Fast locks for dual-instance sync
```

---

## Related Documentation

- [M6-S5-polish-runahead.md](../../phase-1-mvp/milestone-6-gui/M6-S5-polish-runahead.md) - Basic RA=1 implementation
- [M6-REORGANIZATION-SUMMARY.md](../../phase-1-mvp/milestone-6-gui/M6-REORGANIZATION-SUMMARY.md) - Feature rephasing details
- M17 Optimization (Phase 4) - Full run-ahead optimization with multi-threading

---

## Future Enhancements (Phase 4 M17)

Advanced optimizations deferred to M17:

1. **Multi-Threading:**
   - Parallel state serialization
   - Async state restoration
   - Thread pool for speculative frames

2. **Memory Pools:**
   - Pre-allocated state buffers
   - Zero-copy state transfers
   - Reduced GC pressure

3. **Performance Profiling:**
   - Per-component timing
   - Bottleneck identification
   - Auto-tuning CPU affinity

---

## Success Criteria

1. RA=0-4 configurable via settings UI
2. Auto-detection works for 50+ games with 90%+ accuracy
3. Dual-instance mode achieves perfect audio quality
4. Frame delay measurably reduces total latency
5. Performance targets met on target hardware
6. Zero regressions in M6 MVP functionality
7. Comprehensive documentation and user guide
8. M7 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete
**Next Milestone:** M8 (GGPO Netplay)

---

## Notes

### Design Decisions

**Why Dual-Instance?**
- Run-ahead introduces audio artifacts (speculative frames discarded)
- Separate audio instance ensures pristine audio quality
- Trade-off: 20-30% higher memory usage

**Why Auto-Detection?**
- Users shouldn't need to understand RA=0-4
- Game characteristics dictate optimal setting
- Power users can still override manually

**Why Frame Delay?**
- Compensates for monitor latency (5-15ms typical)
- Renders frame closer to vsync
- Combined with RA: Total latency <10ms

---

## Migration from M6

M6-S5 implemented basic RA=1. M7 extends this foundation:

**Preserved:**
- SaveState serialization system
- RunAheadManager architecture
- Performance metrics overlay

**Added:**
- Configurable RA frames (0-4)
- Auto-detection algorithm
- Dual-instance mode
- Frame delay system

**No Breaking Changes:** M6 RA=1 remains functional as default.
