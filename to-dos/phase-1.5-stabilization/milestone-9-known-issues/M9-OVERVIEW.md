# Milestone 9: Known Issues Resolution

**Milestone:** M9 (Known Issues Resolution)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~2 weeks (March 2026)
**Status:** Ready to Start
**Version Target:** v0.9.0
**Progress:** 0%
**Baseline:** v0.8.0 (Rust 2024 Edition, Dependency Modernization Complete)

---

## Overview

Milestone 9 focuses on **resolving all known issues** identified during Phase 1.5 development, including audio quality improvements, PPU edge cases, performance optimization, and bug fixes. This milestone ensures a polished, production-ready emulator before the final Phase 1.5 polish sprint.

### Prerequisites (Completed in v0.7.1-v0.8.0)

The following foundational work was completed in v0.7.1-v0.8.0, providing a stable base for M9:

- [x] **GUI Framework Migration** (v0.7.1): Desktop frontend migrated from Iced+wgpu to eframe+egui
- [x] **Rust 2024 Edition** (v0.8.0): MSRV 1.88+, modern Rust idioms
- [x] **Audio Backend** (v0.8.0): cpal 0.16 with lock-free ring buffer, rubato 0.16 for resampling
- [x] **Configuration System**: RON 0.12 format with VideoConfig, AudioConfig, InputConfig, DebugConfig
- [x] **Debug Windows**: CPU, PPU, APU, Memory viewers implemented in egui 0.33
- [x] **Input System**: Keyboard and gamepad support via gilrs 0.11
- [x] **Frame Timing**: Accumulator-based timing at 60.0988 Hz NTSC
- [x] **Dependency Modernization** (v0.8.0): eframe/egui 0.33, cpal 0.16, ron 0.12, thiserror 2.0

### Goals

1. **Audio Quality Improvements**
   - Implement dynamic resampling (variable input → 48kHz output)
   - Add audio/video synchronization
   - Optimize buffer management (reduce latency)
   - Fix audio glitches and pops

2. **PPU Edge Cases**
   - Handle sprite overflow flag correctly
   - Fix palette RAM edge cases
   - Improve scrolling split-screen effects
   - Handle mid-scanline register writes

3. **Performance Optimization**
   - Profile CPU, PPU, APU hot paths
   - Optimize critical rendering loops
   - Reduce memory allocations
   - Benchmark and verify improvements

4. **Bug Fixes & Polish**
   - Close all open GitHub issues
   - Fix edge case crashes
   - Improve error handling and logging
   - Validate save state robustness

---

## Success Criteria

### Quality Gates

- [ ] Audio quality matches reference emulators (Mesen2)
- [ ] Dynamic resampling implemented (no audio glitches)
- [ ] Audio/video sync accurate (no drift)
- [ ] Sprite overflow behavior correct
- [ ] Performance: 100+ FPS (1.67x real-time) on mid-range hardware
- [ ] Zero known crashes or critical bugs
- [ ] All GitHub issues closed or triaged

### Metrics

| Category | v0.8.0 Baseline | v0.9.0 Target | Improvement |
|----------|-----------------|---------------|-------------|
| Audio Quality | Foundation Ready | High | Dynamic resampling, A/V sync |
| Performance | ~100 FPS | 120+ FPS | +20% optimization |
| Bugs (Critical) | 5 | 0 | All resolved |
| Bugs (Minor) | 15 | 5 | 66% reduction |

---

## Sprint Breakdown

### Sprint 1: Audio Improvements ⏳ PENDING

**Duration:** Week 1
**Focus:** Audio quality and synchronization

**Objectives:**
- [ ] Implement dynamic resampling (variable rate → 48kHz)
- [ ] Add audio/video synchronization
- [ ] Optimize buffer management
- [ ] Fix audio glitches and pops

**Deliverable:** High-quality audio with A/V sync

[M9-S1 Details](M9-S1-audio-improvements.md)

---

### Sprint 2: PPU Edge Cases ⏳ PENDING

**Duration:** Week 1
**Focus:** PPU edge case handling

**Objectives:**
- [ ] Fix sprite overflow flag behavior
- [ ] Handle palette RAM edge cases
- [ ] Improve scrolling split-screen effects
- [ ] Handle mid-scanline register writes

**Deliverable:** PPU edge cases resolved

[M9-S2 Details](M9-S2-ppu-edge-cases.md)

---

### Sprint 3: Performance Optimization ⏳ PENDING

**Duration:** Week 2
**Focus:** Profiling and optimization

**Objectives:**
- [ ] Profile CPU, PPU, APU hot paths
- [ ] Optimize critical rendering loops
- [ ] Reduce memory allocations
- [ ] Benchmark improvements

**Deliverable:** 20%+ performance improvement

[M9-S3 Details](M9-S3-performance-optimization.md)

---

### Sprint 4: Bug Fixes & Polish ⏳ PENDING

**Duration:** Week 2
**Focus:** Bug triage and resolution

**Objectives:**
- [ ] Close all critical GitHub issues
- [ ] Fix edge case crashes
- [ ] Improve error handling
- [ ] Validate save state robustness

**Deliverable:** Zero critical bugs, v0.9.0 release

[M9-S4 Details](M9-S4-bug-fixes.md)

---

## Technical Focus Areas

### Audio Quality (M9-S1)

**Current Implementation (v0.8.0):**
- cpal 0.16 for cross-platform audio I/O
- rubato 0.16 for high-quality audio resampling
- Custom lock-free ring buffer (8192 samples) with atomic operations
- Mono samples converted to stereo in audio callback
- Volume/mute controls via atomic variables
- Sample rate: 44.1kHz (configurable in AudioConfig)

**Remaining Issues:**
- No dynamic resampling (NES ~1.79MHz APU output → device sample rate)
- No audio/video synchronization (potential audio drift over time)
- Fixed buffer size (no adaptive sizing based on system latency)
- Occasional pops/glitches under high system load

**Improvements Needed:**
1. **Dynamic Resampling:**
   - Use rubato crate for high-quality sinc interpolation
   - Handle variable input rate (NTSC 1.789773 MHz, PAL 1.662607 MHz)
   - Target output rate: 44.1kHz or 48kHz (device-dependent)
   - Consider replacing custom ring buffer with ringbuf crate (CachingProd/CachingCons)

2. **Audio/Video Sync:**
   - Track audio buffer fill level in real-time
   - Adjust emulation speed slightly to maintain sync (1.01x/0.99x)
   - Handle buffer underrun/overflow gracefully (insert silence, drop samples)
   - Reference: tetanes uses ringbuf crate with sophisticated sync logic

3. **Buffer Management:**
   - Adaptive buffer sizing based on system latency detection
   - Reduce latency (target <100ms, ideally ~50ms)
   - Consider dynamic latency adjustment (tetanes pattern)

### PPU Edge Cases (M9-S2)

**Current Implementation (v0.8.0):**
- PPU debug window implemented in egui 0.33 (pattern tables, nametables, OAM, palette)
- Basic sprite rendering with 8-sprite-per-scanline limit
- Palette RAM mirroring implemented
- VBlank/NMI timing functional with flag read handling

**Remaining Issues:**
- Sprite overflow flag not fully cycle-accurate (hardware quirks not emulated)
- Palette RAM writes during rendering not handled correctly
- Scrolling split-screen effects may have edge cases (mid-scanline writes)
- Some attribute handling edge cases remain

**Improvements Needed:**
1. **Sprite Overflow:**
   - Implement accurate sprite evaluation with hardware bug emulation
   - Set overflow flag correctly (including false positive/negative cases)
   - Use egui PPU debug window for visualization during development

2. **Palette RAM:**
   - Handle writes during rendering (immediate effect on output)
   - Validate mirroring edge cases ($3F10/$3F14/$3F18/$3F1C)
   - Add palette visualization to debug window

3. **Scrolling:**
   - Handle mid-scanline $2006 writes (split-screen effects)
   - Test with games using scrolling tricks (Super Mario Bros. 3)
   - Add scanline visualization to debug window

### Performance (M9-S3)

**Current State (v0.8.0):**
- eframe 0.33 + egui 0.33 rendering via OpenGL (glow backend)
- Accumulator-based frame timing (TARGET_FPS = 60.0988)
- Framebuffer: 256x240 RGBA with egui::TextureOptions::NEAREST
- Audio: Lock-free ring buffer with atomic operations, rubato resampling
- Inline hints and buffer reuse optimizations applied
- Performance not fully profiled (eframe overhead unknown)

**Potential Bottlenecks:**
- PPU rendering loop (pixel-by-pixel processing)
- egui texture updates every frame (ColorImage::from_rgba_unmultiplied)
- Audio callback overhead (mono-to-stereo conversion)
- Memory allocations in hot paths

**Improvements Needed:**
1. **Profiling:**
   - Use `cargo flamegraph` or `perf` with eframe workload
   - Profile egui rendering overhead (texture updates, layout)
   - Identify hot paths (CPU step, PPU scanline, APU sample)

2. **Optimization:**
   - Inline critical functions (#[inline(always)] on hot paths)
   - Reduce memory allocations (reuse buffers, avoid Vec::new in loops)
   - Consider SIMD for pixel processing (palette lookup, pixel mixing)
   - Optimize CPU opcode dispatch (compile-time lookup tables)
   - Batch PPU rendering (render full scanline instead of dot-by-dot)

3. **Benchmarking:**
   - Measure FPS with various game complexity levels
   - Target 120+ FPS on mid-range hardware (headroom for vsync)
   - Compare before/after optimization
   - Ensure no accuracy regressions (test ROM pass rate)

### Bug Fixes (M9-S4)

**Current State (v0.8.0):**
- Configuration persistence via RON 0.12 format
- Error handling with anyhow + thiserror 2.0 in desktop crate
- Logging via log crate
- File dialog errors handled with rfd 0.15

**Known Issues:**
- Edge case crashes (malformed ROMs, invalid states)
- Save states not yet implemented
- Input handling edge cases (rapid key presses, gilrs edge cases)
- Error messages shown in console, not in GUI dialogs

**Improvements Needed:**
1. **Crash Prevention:**
   - Add bounds checking in core emulation
   - Validate ROM headers (iNES, NES 2.0 format checking)
   - Handle malformed ROM files gracefully (user-friendly errors)

2. **Error Handling:**
   - Show error dialogs in egui instead of console
   - Improve error messages (actionable guidance for users)
   - Add structured logging (consider tracing crate)

3. **Save States:**
   - Implement save state system (serialize CPU, PPU, APU, mapper state)
   - Add versioning for forward compatibility
   - Add checksum validation (detect corruption)
   - Handle mid-frame/mid-instruction saves correctly

4. **Input Handling:**
   - Test gilrs edge cases (controller hotplug, disconnect)
   - Add input debouncing if needed
   - Validate keyboard bindings on load

---

## Expected Outcomes

### Audio Quality

- Dynamic resampling: Variable rate → 48kHz
- Audio/video sync: <10ms drift
- Buffer latency: <100ms
- Zero pops/glitches in normal gameplay

### PPU Edge Cases

- Sprite overflow: 100% accurate
- Palette RAM: All edge cases handled
- Scrolling: Split-screen effects working
- Test with Super Mario Bros. 3, Zelda, Mega Man 3

### Performance

- FPS: 120+ (1.67x → 2.0x real-time)
- Memory: Reduced allocations (heap profiling)
- Optimization: 20%+ improvement over v0.8.0
- Zero performance regressions

### Bug Fixes

- Critical bugs: 0 (all resolved)
- Minor bugs: <5 (66% reduction)
- GitHub issues: All closed or triaged
- Save states: Robust and versioned

---

## Dependencies

### Blockers

- M8 (Test ROM Validation) must be complete

### Inputs

- v0.8.0 baseline (508+ tests, 100% Blargg pass rate)
- v0.7.1 implementation report (known issues list)
- GitHub issue tracker (open bugs)
- User feedback (audio quality, performance)

### Outputs

- v0.9.0 release (known issues resolved)
- Performance benchmarks (before/after)
- Audio quality comparison (Mesen2)
- Updated documentation (CHANGELOG, known limitations)

---

## Risks & Mitigation

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Audio resampling complexity | Medium | Medium | Use proven libraries (rubato, dasp) |
| Performance regression | High | Low | Continuous benchmarking, profile before/after |
| Edge case discovery | Medium | High | Systematic testing, user feedback |
| Time overrun (optimization) | Low | Medium | Timebox optimization, accept "good enough" |

---

## Resources

### Documentation

- [Desktop README](../../../crates/rustynes-desktop/README.md) - GUI architecture documentation
- [Audio Resampling Guide](../../../docs/apu/AUDIO_RESAMPLING.md)
- [Performance Profiling Guide](../../../docs/dev/PERFORMANCE_PROFILING.md)
- [Save State Format](../../../docs/api/SAVE_STATES.md)

### Reference Projects

- **tetanes** - Rust NES emulator using egui (primary reference)
  - Audio: ringbuf crate with sophisticated sync
  - GUI: Comprehensive egui implementation with PPU viewer
  - Location: `ref-proj/tetanes/`

### External References

- [rubato Resampling Library](https://github.com/HEnquist/rubato) - High-quality Rust resampling
- [ringbuf Lock-Free Ring Buffer](https://crates.io/crates/ringbuf) - CachingProd/CachingCons for audio
- [dasp Digital Signal Processing](https://github.com/RustAudio/dasp) - DSP primitives
- [cargo-flamegraph Profiling](https://github.com/flamegraph-rs/flamegraph) - CPU/memory profiling
- [Mesen2 Audio Implementation](https://github.com/SourMesen/Mesen2) - Reference for accuracy
- [egui Documentation](https://docs.rs/egui/) - GUI framework reference
- [eframe Documentation](https://docs.rs/eframe/) - Application framework reference
- [cpal Documentation](https://docs.rs/cpal/) - Audio I/O reference

---

## Milestone Deliverables

1. **Audio Improvements**
   - Dynamic resampling implementation
   - Audio/video synchronization
   - Optimized buffer management
   - Audio quality documentation

2. **PPU Edge Cases**
   - Sprite overflow implementation
   - Palette RAM edge case handling
   - Scrolling split-screen support
   - Test suite validation

3. **Performance Optimization**
   - Profiling reports (before/after)
   - Optimized hot paths
   - Benchmark results
   - Performance documentation

4. **Bug Fixes**
   - All critical bugs resolved
   - GitHub issues closed/triaged
   - Improved error handling
   - Save state robustness

5. **Release**
   - v0.9.0 git tag
   - Release notes
   - CHANGELOG entry
   - Updated documentation

---

**Status:** Ready to Start (v0.8.0 Baseline Complete)
**Blocks:** M10 (Final Polish)
**Next Milestone:** M10 (Polish) - UI/UX improvements, documentation, v1.0.0-alpha.1 release
