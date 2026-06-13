# M11-S6: Desktop Integration

**Sprint:** S6 (Desktop Integration)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 1-2 weeks (10-15 hours)
**Status:** PLANNED
**Priority:** HIGH - Frontend integration with sub-cycle accuracy
**Depends On:** S1-S5 (Core sub-cycle implementation complete)

---

## Overview

Integration of sub-cycle accuracy improvements into the rustynes-desktop crate. This sprint ensures the desktop frontend properly leverages the cycle-accurate emulation core while maintaining 60fps performance, proper A/V sync, and responsive input handling.

---

## Dependencies

### Required Before Starting
- **S1 (CPU Refactor)** - Cycle-by-cycle CPU must be complete
- **S2 (PPU Sync)** - PPU synchronization must be implemented
- **S3 (APU Precision)** - APU integration must be complete
- **S4 (Bus/DMA)** - DMA timing must be accurate
- **S5 (Mappers)** - Mapper IRQ timing must be accurate

### Blocks
- **S7 (Testing)** - Cannot validate until desktop integration complete
- **v1.0.0 Release** - Cannot release without proper frontend integration

---

## Current State (v0.8.6)

### Desktop Architecture
- **Framework:** eframe 0.33 + egui 0.33
- **Audio:** cpal 0.16 with lock-free ring buffer (8192 samples)
- **Resampling:** rubato 0.16 (two-stage decimation)
- **Input:** gilrs 0.11 (gamepad), egui (keyboard)
- **Frame Timing:** Accumulator-based at 60.0988 Hz NTSC

### Known Integration Points
- `crates/rustynes-desktop/src/app.rs` - Main application loop
- `crates/rustynes-desktop/src/emulation.rs` - Emulation thread
- `crates/rustynes-desktop/src/audio.rs` - Audio output
- `crates/rustynes-desktop/src/video.rs` - Framebuffer rendering
- `crates/rustynes-desktop/src/input.rs` - Input handling
- `crates/rustynes-desktop/src/debug/` - Debug windows

---

## Sprint Tasks

### Task S6.1: Frame Timing Precision

**Priority:** P0 (Critical)
**Effort:** 3 hours
**Files:**
- `crates/rustynes-desktop/src/app.rs`
- `crates/rustynes-desktop/src/emulation.rs`

#### Current Implementation

```rust
// Current: Accumulator-based timing at 60.0988 Hz
const FRAME_DURATION: Duration = Duration::from_nanos(16_639_265); // ~60.0988 Hz
```

#### Required Changes

##### Subtasks
- [ ] Validate frame duration matches NES hardware (29780.5 CPU cycles average)
- [ ] Handle odd/even frame differences for PPU timing
- [ ] Ensure vsync doesn't introduce timing drift
- [ ] Add frame timing statistics to debug info
- [ ] Handle frame skip scenarios gracefully

#### Implementation Notes

```rust
// NES timing constants
const CPU_CYCLES_PER_FRAME: f64 = 29780.5;  // Average (odd frames: 29781, even: 29780)
const CPU_FREQUENCY_NTSC: f64 = 1_789_773.0;
const FRAME_RATE_NTSC: f64 = CPU_FREQUENCY_NTSC / CPU_CYCLES_PER_FRAME; // ~60.0988 Hz

// Frame timing validation
fn validate_frame_timing(frame_cycles: u32, is_odd_frame: bool) -> bool {
    let expected = if is_odd_frame { 29781 } else { 29780 };
    frame_cycles == expected
}
```

#### Validation Criteria
- Frame timing within 0.1% of hardware
- No visible frame jitter during gameplay
- Vsync properly synchronized
- Odd/even frame handling correct

---

### Task S6.2: Audio/Video Synchronization

**Priority:** P0 (Critical)
**Effort:** 4 hours
**Files:**
- `crates/rustynes-desktop/src/audio.rs`
- `crates/rustynes-desktop/src/emulation.rs`

#### Current Implementation

```rust
// Current: Separate audio buffer from frame rendering
// Two-stage decimation: 1.79MHz -> 192kHz -> 48kHz
// Lock-free ring buffer with 8192 samples
```

#### Required Changes

##### Subtasks
- [ ] Verify audio buffer timing matches APU cycle accuracy
- [ ] Implement adaptive sync to prevent audio underruns
- [ ] Handle DMC DMA stalls in audio generation
- [ ] Add audio buffer level monitoring
- [ ] Implement audio/video drift correction

#### Implementation Notes

```rust
// A/V sync with cycle-accurate emulation
pub struct AvSync {
    audio_buffer_level: Arc<AtomicUsize>,
    target_buffer_level: usize,
    speed_adjustment: f64,  // 0.99x - 1.01x
}

impl AvSync {
    pub fn calculate_speed_adjustment(&self) -> f64 {
        let level = self.audio_buffer_level.load(Ordering::Relaxed);
        let diff = level as f64 - self.target_buffer_level as f64;

        // Subtle adjustment to maintain sync
        1.0 - (diff / self.target_buffer_level as f64) * 0.01
    }
}

// DMC DMA stall handling
fn handle_dmc_stall(&mut self, stall_cycles: u8) {
    // Audio thread must account for DMC DMA stealing CPU cycles
    // This affects sample generation timing
    self.pending_dmc_stalls += stall_cycles;
}
```

#### Validation Criteria
- No audio crackling during normal gameplay
- No audio desync after 10+ minutes of play
- Proper DMC sample playback timing
- Buffer underrun rate < 0.1%

---

### Task S6.3: Input Latency Optimization

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `crates/rustynes-desktop/src/input.rs`
- `crates/rustynes-core/src/controller.rs`

#### Current Implementation

```rust
// Current: Per-frame input polling
fn update_input(&mut self) {
    // Input polled once per frame before emulation
}
```

#### Required Changes

##### Subtasks
- [ ] Verify controller read timing matches on_cpu_cycle() integration
- [ ] Ensure open bus behavior works with input system
- [ ] Test input timing with timing-sensitive games
- [ ] Measure and document input latency
- [ ] Consider sub-frame input polling if needed

#### Implementation Notes

```rust
// Controller read timing verification
fn verify_controller_timing(&mut self, bus: &Bus) {
    // $4016/$4017 reads should occur at correct cycle points
    // Open bus behavior: bits 4-0 from controller, bits 7-5 from bus

    // Verify strobe latch timing
    // Strobe write to $4016 bit 0 = 1 latches current button state
    // Strobe write to $4016 bit 0 = 0 allows shift register reads
}

// Input timing test pattern
#[test]
fn test_controller_read_open_bus() {
    let mut bus = Bus::new();
    bus.set_open_bus_value(0xE0);  // Bits 7-5

    let result = bus.read(0x4016);
    assert_eq!(result & 0xE0, 0xE0);  // Open bus bits preserved
}
```

#### Validation Criteria
- Input latency < 1 frame (16.67ms)
- Proper open bus behavior on controller reads
- Strobe latch timing accurate
- No input drops or duplicates

---

### Task S6.4: Debug Window Updates

**Priority:** P1
**Effort:** 3 hours
**Files:**
- `crates/rustynes-desktop/src/debug/cpu_debug.rs`
- `crates/rustynes-desktop/src/debug/ppu_debug.rs`
- `crates/rustynes-desktop/src/debug/apu_debug.rs`
- `crates/rustynes-desktop/src/debug/memory_debug.rs`

#### Current State

```text
Current debug windows:
- CPU: Registers, flags, disassembly (per-instruction)
- PPU: Pattern tables, nametables, OAM, palette
- APU: Channel visualization
- Memory: Hex viewer
```

#### Required Changes

##### Subtasks
- [ ] Add cycle counter to CPU debug window
- [ ] Show PPU dot/scanline in real-time
- [ ] Display APU frame counter state
- [ ] Add DMC DMA status indicator
- [ ] Add on_cpu_cycle() callback counter
- [ ] Show sub-instruction state for debugging

#### Implementation Notes

```rust
// Enhanced CPU debug state
pub struct CpuDebugState {
    // Existing fields...
    pub cycle_count: u64,           // Total CPU cycles executed
    pub current_instruction_cycle: u8,  // Cycle within current instruction
    pub dma_active: bool,
    pub dma_cycles_remaining: u16,
}

// Enhanced PPU debug state
pub struct PpuDebugState {
    // Existing fields...
    pub dot: u16,                   // 0-340
    pub scanline: u16,              // 0-261
    pub frame: u64,
    pub vblank_flag: bool,
    pub nmi_occurred: bool,
}

// APU frame counter debug
pub struct ApuDebugState {
    // Existing fields...
    pub frame_counter_cycle: u32,
    pub frame_counter_mode: FrameCounterMode,
    pub irq_inhibit: bool,
    pub dmc_dma_pending: bool,
    pub dmc_stall_cycles: u8,
}
```

#### UI Layout

```text
+-- CPU Debug ----------------------------------+
| PC: $8000  A: $42  X: $00  Y: $00  SP: $FD   |
| Flags: NV-BDIZC = 00110100                    |
| Cycle: 123,456,789 | Instr Cycle: 3/4        |
| DMA: Inactive                                 |
+-----------------------------------------------+

+-- PPU Debug ----------------------------------+
| Scanline: 120  Dot: 240  Frame: 1,234        |
| VBlank: OFF  NMI: OK  Sprite 0: HIT          |
+-----------------------------------------------+

+-- APU Debug ----------------------------------+
| Frame Counter: 14913/29830 (4-step)          |
| DMC: Active (Stall: 0 cycles)                |
| IRQ Inhibit: NO  Frame IRQ: NO               |
+-----------------------------------------------+
```

#### Validation Criteria
- Debug windows update at 60fps
- Cycle counters accurate and synchronized
- No performance impact when debug windows closed
- Clear visual indication of DMA activity

---

### Task S6.5: Performance Optimization

**Priority:** P0 (Critical)
**Effort:** 4 hours
**Files:**
- `crates/rustynes-desktop/src/emulation.rs`
- `crates/rustynes-core/src/console.rs`
- `crates/rustynes-cpu/src/cpu.rs`
- `crates/rustynes-ppu/src/ppu.rs`

#### Current Performance Targets

```text
Performance Targets:
- Frame time: < 16.67ms (60 FPS)
- CPU instruction throughput: > 2M instructions/sec
- Memory usage: < 50MB
- Maximum regression vs v0.8.4: 20%
```

#### Required Changes

##### Subtasks
- [ ] Profile tick() loop with cycle callbacks
- [ ] Optimize hot paths in on_cpu_cycle()
- [ ] Consider SIMD for PPU rendering
- [ ] Benchmark against Mesen2 performance
- [ ] Add frame time profiling overlay
- [ ] Identify and optimize allocation hot spots

#### Implementation Notes

```rust
// Hot path optimization in on_cpu_cycle()
impl CpuBus for Bus {
    #[inline(always)]
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3 times (critical hot path)
        // Use unchecked operations where safe
        for _ in 0..3 {
            self.ppu.step_with_chr_unchecked(|addr| {
                self.mapper.read_chr_unchecked(addr)
            });
        }

        // Step APU once
        self.apu.clock_unchecked();

        // Clock mapper (only if needed)
        if self.mapper.needs_clock() {
            self.mapper.clock(1);
        }
    }
}

// Performance monitoring
pub struct PerformanceMetrics {
    frame_times: CircularBuffer<Duration, 60>,
    cpu_cycles_per_frame: u32,
    on_cpu_cycle_calls: u64,
}

impl PerformanceMetrics {
    pub fn average_frame_time(&self) -> Duration {
        self.frame_times.iter().sum::<Duration>() / self.frame_times.len() as u32
    }

    pub fn fps(&self) -> f64 {
        1.0 / self.average_frame_time().as_secs_f64()
    }
}
```

#### Benchmark Comparison

| Metric | v0.8.4 Baseline | Sub-Cycle Target | Max Regression |
|--------|-----------------|------------------|----------------|
| Frame time | 8ms | 10ms | 20% |
| CPU throughput | 3M/sec | 2.4M/sec | 20% |
| Memory | 35MB | 42MB | 20% |

#### Validation Criteria
- 60fps maintained on reference hardware
- No visible stuttering during gameplay
- Performance regression < 20% vs v0.8.4
- Memory usage stable (no leaks)

---

### Task S6.6: Technical Debt Resolution

**Priority:** P2
**Effort:** 2 hours
**Files:**
- Various files in `crates/rustynes-desktop/src/`

#### Subtasks
- [ ] Address TODOs and FIXMEs in rustynes-desktop
- [ ] Clean up unused code from previous iterations
- [ ] Ensure consistent error handling
- [ ] Update documentation comments
- [ ] Remove deprecated patterns

#### Known Technical Debt

```rust
// TODO: Audit these areas for technical debt
// - Old Iced-era code remnants
// - Unused configuration fields
// - Inconsistent error handling patterns
// - Missing documentation on public APIs
// - Deprecated egui patterns from 0.29 -> 0.33 upgrade
```

#### Cleanup Checklist
- [ ] Remove unused imports
- [ ] Clean up dead code paths
- [ ] Standardize Result/Option usage
- [ ] Add doc comments to public functions
- [ ] Update egui deprecated API usage

#### Validation Criteria
- Zero clippy warnings
- All public APIs documented
- No unused code warnings
- Consistent error handling

---

## Research References

### Internal Documentation
- [M10-S0-dependency-upgrade.md](../milestone-10-polish/M10-S0-dependency-upgrade.md) - Dependency versions
- [M10-S1-ui-ux-improvements.md](../milestone-10-polish/M10-S1-ui-ux-improvements.md) - UI/UX patterns
- [M9-S1-audio-improvements.md](../milestone-9-known-issues/M9-S1-audio-improvements.md) - Audio architecture

### Reference Emulators
- **Mesen2** (`ref-proj/Mesen2/`) - GUI integration patterns, debug window design
- **TetaNES** (`ref-proj/TetaNES/`) - Rust + egui immediate mode patterns
- **FCEUX** (`ref-proj/FCEUX/`) - Frame timing implementation, debug tools

### External Resources
- [NESdev Wiki - PPU Timing](https://www.nesdev.org/wiki/PPU_frame_timing)
- [NESdev Wiki - APU Timing](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [egui Documentation](https://docs.rs/egui/latest/egui/)
- [cpal Audio Guide](https://docs.rs/cpal/latest/cpal/)

---

## Test Plan

### Test ROMs
All existing test ROMs in `test-roms/` should continue to pass:
- Blargg test suite (90 tests)
- nestest.nes (CPU validation)
- Timing-sensitive games

### Manual Testing
| Game | Focus Area | Expected Result |
|------|------------|-----------------|
| Super Mario Bros. | Frame timing | Smooth scrolling, no jitter |
| Mega Man 2 | Audio sync | Music in sync with gameplay |
| Battletoads | Input timing | Responsive controls |
| Ninja Gaiden | Status bar | IRQ-based effects correct |

### Automated Tests
- Frame timing validation
- Audio buffer level monitoring
- Input latency measurement
- Performance regression tests

---

## Success Criteria

### Sprint Complete When

| Criterion | Target | Validation |
|-----------|--------|------------|
| Frame timing | < 0.1% drift | Frame counter |
| Audio sync | No crackling | 10 min play test |
| Input latency | < 16.67ms | Measurement |
| Debug windows | Cycle-level | Visual inspection |
| Performance | < 20% regression | Benchmark |
| Technical debt | 0 clippy warnings | CI check |

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Performance regression > 20% | High | Medium | Profile continuously, optimize hot paths |
| Audio desync | Medium | Low | Adaptive sync implementation |
| Frame timing drift | Medium | Low | Accumulator correction |
| Debug window overhead | Low | Medium | Lazy evaluation when hidden |

---

## Acceptance Criteria

- [ ] Frame timing matches NES hardware within 0.1%
- [ ] Audio plays without crackling for 10+ minutes
- [ ] Input latency < 1 frame
- [ ] Debug windows show cycle-level state
- [ ] Performance regression < 20% vs v0.8.4
- [ ] All timing-sensitive games playable
- [ ] Zero clippy warnings in rustynes-desktop
- [ ] All 90 Blargg tests still pass

---

**Status:** PLANNED
**Created:** 2025-12-29
**Author:** Claude Code
