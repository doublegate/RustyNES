# M10 Sprint 3: Release Preparation

**Sprint:** M10-S3 (Release Preparation)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~2-3 days
**Status:** Pending
**Prerequisites:** M10-S0 Complete, M10-S1 Complete, M10-S2 Complete
**Updated:** 2025-12-28

---

## Overview

Final testing, binary builds, version decision, release notes, and GitHub release to complete Phase 1.5 and transition to Phase 2.

## Current Implementation (Post M10-S0)

The following release infrastructure exists:

**Completed:**
- [x] GitHub repository with CI/CD basics
- [x] Cargo workspace with 6+ crates
- [x] Desktop binary: `rustynes-desktop`
- [x] Cross-platform dependencies: eframe+egui (OpenGL), cpal (audio), gilrs (gamepads)
- [x] Test suite: 508 unit tests passing, 100% Blargg ROM validation (90/90)
- [x] Dependency upgrade to latest stable versions (M10-S0)

**Build Dependencies (Post M10-S0):**
- eframe 0.33 (cross-platform window + rendering)
- egui 0.33 (immediate mode GUI with Modal, Atoms, Plugin)
- cpal 0.16 (cross-platform audio with buffer underrun reporting)
- gilrs 0.11 (gamepad support)
- rfd 0.15 (native file dialogs)
- ron 0.12 (configuration)
- thiserror 2.0 (error handling)

**Toolchain Requirements:**
- Rust Edition: 2024
- MSRV: 1.88 (required by egui 0.33)

**Platform Considerations:**
- Linux: Requires `libasound2-dev`, `libudev-dev`, `libxkbcommon-dev`, `libwayland-dev`
- macOS: Universal binary support (x86_64 + arm64)
- Windows: MSVC toolchain recommended

## Objectives

- [ ] Full regression testing (all test ROMs, games)
- [ ] Build release binaries (Linux, macOS, Windows)
- [ ] Decide version number (v0.9.0 or v1.0.0-alpha.1)
- [ ] Write comprehensive release notes
- [ ] Publish GitHub release
- [ ] Create Phase 1.5 completion report
- [ ] Plan Phase 2 kickoff

## Tasks

### Task 1: Regression Testing
- [ ] Run full test suite (508+ unit tests passing)
- [ ] Verify 100% Blargg pass rate maintained (90/90 tests)
- [ ] Test with 10+ different games (Super Mario Bros., Zelda, Mega Man, etc.)
- [ ] Verify save states (create, load, verify correctness)
- [ ] Test on all platforms (Linux, macOS, Windows)
- [ ] Performance benchmarking (verify 120+ FPS)
- [ ] Test edge cases (malformed ROMs, invalid save states)
- [ ] Verify no clippy warnings: `cargo clippy --workspace -- -D warnings`

### Task 2: Binary Builds
- [ ] Setup GitHub Actions for automated builds
- [ ] Build Linux binary (x86_64, musl or glibc)
- [ ] Build macOS binary (x86_64 and arm64/Apple Silicon)
- [ ] Build Windows binary (x86_64, MSVC)
- [ ] Package binaries with assets (README, LICENSE)
- [ ] Test binaries on fresh systems (no dev environment)

### Task 3: Version Decision
- [ ] Review Phase 1.5 completion criteria
- [ ] Assess test pass rate (95%+ → v1.0.0-alpha.1, 90-94% → v0.9.0)
- [ ] Assess performance (120+ FPS → v1.0.0-alpha.1, 110-119 FPS → v0.9.0)
- [ ] Assess bugs (0 critical → v1.0.0-alpha.1, 1-2 critical → v0.9.0)
- [ ] Decide version number
- [ ] Update Cargo.toml files with version

### Task 4: Release Notes
- [ ] Write release notes summary (highlights, improvements)
- [ ] List features (CPU, PPU, APU, mappers, desktop GUI)
- [ ] Document improvements from v0.5.0 (test pass rate, performance, audio)
- [ ] List known limitations (deferred to Phase 2)
- [ ] Include migration guide (v0.5.0 → v0.9.0/v1.0.0-alpha.1)
- [ ] Thank contributors

### Task 5: GitHub Release
- [ ] Create git tag (v0.9.0 or v1.0.0-alpha.1)
- [ ] Push tag to GitHub
- [ ] Create GitHub release (attach binaries)
- [ ] Publish release notes
- [ ] Announce on social media / forums (optional)

### Task 6: Phase 1.5 Completion
- [ ] Write Phase 1.5 completion report (achievements, metrics, lessons learned)
- [ ] Update ROADMAP.md (Phase 1.5 complete, Phase 2 next)
- [ ] Update VERSION-PLAN.md (v0.9.0/v1.0.0-alpha.1 released)
- [ ] Archive Phase 1.5 documentation
- [ ] Plan Phase 2 kickoff (advanced features)

## Regression Testing Checklist

### Test ROMs (202/212, 95%+)

- [ ] CPU Tests: 34/36 passing (94%)
- [ ] PPU Tests: 47/49 passing (96%)
- [ ] APU Tests: 67/70 passing (96%)
- [ ] Mapper Tests: 54/57 passing (95%)
- [ ] Overall: 202/212 passing (95%+)

### Game Compatibility (10+ games)

| Game | Mapper | Test | Status |
|------|--------|------|--------|
| Super Mario Bros. | 0 | Basic gameplay, scrolling | [ ] Pass |
| Donkey Kong | 0 | Graphics, collision | [ ] Pass |
| Zelda | 1 | Save states, large ROM | [ ] Pass |
| Metroid | 1 | Scrolling, exploration | [ ] Pass |
| Mega Man | 2 | Bank switching, boss fights | [ ] Pass |
| Castlevania | 2 | Scrolling, stages | [ ] Pass |
| Contra | 2 | 2-player, scrolling | [ ] Pass |
| Super Mario Bros. 3 | 4 | IRQ timing, status bar | [ ] Pass |
| Mega Man 2 | 4 | Bank switching, weapons | [ ] Pass |
| Kirby's Adventure | 4 | Large ROM, graphics | [ ] Pass |

### Performance Benchmarking

| System | Spec | Target FPS | Actual FPS | Status |
|--------|------|------------|------------|--------|
| Mid-Range PC | i5-8400, GTX 1060 | 120+ FPS | [ ] _____ | [ ] Pass |
| Low-End PC | i3-6100, Integrated GPU | 100+ FPS | [ ] _____ | [ ] Pass |
| MacBook Pro | M1 Pro | 150+ FPS | [ ] _____ | [ ] Pass |
| Steam Deck | Custom APU | 100+ FPS | [ ] _____ | [ ] Pass |

## Binary Build Process

### GitHub Actions Workflow

```yaml
name: Release Builds

on:
  push:
    tags:
      - 'v*'

env:
  CARGO_TERM_COLOR: always
  # MSRV for egui 0.33
  RUST_VERSION: "1.88"

jobs:
  build-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Use specific Rust version for MSRV 1.88
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ env.RUST_VERSION }}

      # Install dependencies for eframe/egui (OpenGL) and cpal (ALSA)
      - name: Install Linux dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libasound2-dev \
            libudev-dev \
            libxkbcommon-dev \
            libwayland-dev \
            libgtk-3-dev

      - name: Build
        run: cargo build --release -p rustynes-desktop

      - name: Run Tests
        run: cargo test --workspace

      - name: Package
        run: |
          mkdir rustynes-linux-x64
          cp target/release/rustynes-desktop rustynes-linux-x64/
          cp README.md LICENSE rustynes-linux-x64/
          tar -czf rustynes-linux-x64.tar.gz rustynes-linux-x64/

      - uses: actions/upload-artifact@v4
        with:
          name: linux-binary
          path: rustynes-linux-x64.tar.gz

  build-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      # Use specific Rust version for MSRV 1.88
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ env.RUST_VERSION }}
          targets: x86_64-apple-darwin,aarch64-apple-darwin

      - name: Build (x86_64)
        run: cargo build --release -p rustynes-desktop --target x86_64-apple-darwin

      - name: Build (arm64)
        run: cargo build --release -p rustynes-desktop --target aarch64-apple-darwin

      - name: Create Universal Binary
        run: |
          lipo -create \
            target/x86_64-apple-darwin/release/rustynes-desktop \
            target/aarch64-apple-darwin/release/rustynes-desktop \
            -output rustynes-desktop

      - name: Package
        run: |
          mkdir -p RustyNES.app/Contents/MacOS
          mkdir -p RustyNES.app/Contents/Resources
          cp rustynes-desktop RustyNES.app/Contents/MacOS/
          cat > RustyNES.app/Contents/Info.plist << 'EOF'
          <?xml version="1.0" encoding="UTF-8"?>
          <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
          <plist version="1.0">
          <dict>
            <key>CFBundleExecutable</key>
            <string>rustynes-desktop</string>
            <key>CFBundleIdentifier</key>
            <string>com.doublegate.rustynes</string>
            <key>CFBundleName</key>
            <string>RustyNES</string>
            <key>CFBundleVersion</key>
            <string>${{ github.ref_name }}</string>
          </dict>
          </plist>
          EOF
          hdiutil create -volname RustyNES -srcfolder RustyNES.app -ov -format UDZO rustynes-macos-universal.dmg

      - uses: actions/upload-artifact@v4
        with:
          name: macos-binary
          path: rustynes-macos-universal.dmg

  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      # Use specific Rust version for MSRV 1.88
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ env.RUST_VERSION }}

      - name: Build
        run: cargo build --release -p rustynes-desktop

      - name: Package
        shell: pwsh
        run: |
          New-Item -ItemType Directory -Force -Path rustynes-windows-x64
          Copy-Item target/release/rustynes-desktop.exe rustynes-windows-x64/
          Copy-Item README.md rustynes-windows-x64/
          Copy-Item LICENSE rustynes-windows-x64/
          Compress-Archive -Path rustynes-windows-x64 -DestinationPath rustynes-windows-x64.zip

      - uses: actions/upload-artifact@v4
        with:
          name: windows-binary
          path: rustynes-windows-x64.zip

  create-release:
    needs: [build-linux, build-macos, build-windows]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4

      - name: Create Release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            linux-binary/rustynes-linux-x64.tar.gz
            macos-binary/rustynes-macos-universal.dmg
            windows-binary/rustynes-windows-x64.zip
          generate_release_notes: true
```

## Version Decision Matrix

### Decision Criteria

| Criterion | v0.9.0 Threshold | v1.0.0-alpha.1 Threshold |
|-----------|------------------|--------------------------|
| **Test Pass Rate** | 90-94% (190-199 tests) | 95%+ (202+ tests) |
| **Performance** | 110-119 FPS | 120+ FPS |
| **Critical Bugs** | 1-2 | 0 |
| **Minor Bugs** | 5-10 | <5 |
| **Documentation** | 80-90% complete | 95%+ complete |
| **Audio Quality** | Good | High |
| **Phase Completion** | Partial | Complete |

### Recommended Decision

**v1.0.0-alpha.1** if ALL of:
- Test pass rate ≥95% (202+ tests)
- Performance ≥120 FPS
- 0 critical bugs
- <5 minor bugs
- Documentation 95%+ complete

Otherwise → **v0.9.0**

## Release Notes Template

### v1.0.0-alpha.1 (Example)

```markdown
# RustyNES v1.0.0-alpha.1

**Release Date:** April 2026
**Phase:** 1.5 (Stabilization & Accuracy) Complete

## Highlights

- **95%+ Test ROM Pass Rate** - 202 out of 212 comprehensive test ROMs passing
- **High Audio Quality** - Dynamic resampling and audio/video synchronization
- **20%+ Performance Improvement** - Optimized to 120+ FPS on mid-range hardware
- **Polished Desktop GUI** - Responsive design, theme support, intuitive settings
- **Comprehensive Documentation** - User guide, API docs, developer guide

## Features

### CPU (6502)
- Cycle-accurate instruction timing (±1 cycle)
- All 256 opcodes implemented (official + unofficial)
- nestest.nes golden log 100% match
- 34/36 CPU test ROMs passing (94%)

### PPU (2C02)
- Dot-accurate rendering (3 PPU dots per CPU cycle)
- VBlank timing precision (±2 cycles)
- Sprite 0 hit edge cases handled
- 47/49 PPU test ROMs passing (96%)

### APU (2A03)
- All 5 audio channels (Pulse 1/2, Triangle, Noise, DMC)
- Frame counter timing precision (±1 cycle)
- Non-linear mixer with hardware-accurate output
- 67/70 APU test ROMs passing (96%)

### Mappers
- NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4)
- 54/57 mapper test ROMs passing (95%)
- IRQ timing precision (MMC3)

### Desktop GUI
- Responsive design (800x600 to 4K)
- Theme support (light/dark mode)
- Organized settings (Video, Audio, Input, Advanced)
- Save states, screenshots, keyboard shortcuts

## Improvements from v0.5.0

| Metric | v0.5.0 | v1.0.0-alpha.1 | Improvement |
|--------|--------|----------------|-------------|
| Test Pass Rate | 5/212 (2.4%) | 202/212 (95%) | +93% |
| Performance | ~100 FPS | 120+ FPS | +20% |
| Audio Quality | Basic | High | A/V sync, resampling |
| Documentation | Partial | Comprehensive | User guide, API docs |

## Known Limitations

- **Expansion Audio:** VRC6, FDS, MMC5 (deferred to Phase 2)
- **Rare Mappers:** 15, 19, 24+ (deferred to Phase 2)
- **Test ROMs:** 10 tests still failing (4.7%)
- **Netplay:** Not yet implemented (Phase 2)
- **TAS Tools:** Not yet implemented (Phase 2)

## Migration from v0.5.0

Save states from v0.5.0 are **not compatible** with v1.0.0-alpha.1 due to internal format changes. Please complete any in-progress games before upgrading.

## Downloads

- [Linux (x86_64)](https://github.com/doublegate/RustyNES/releases/download/v1.0.0-alpha.1/rustynes-linux-x64.tar.gz)
- [macOS (Universal)](https://github.com/doublegate/RustyNES/releases/download/v1.0.0-alpha.1/rustynes-macos-universal.dmg)
- [Windows (x86_64)](https://github.com/doublegate/RustyNES/releases/download/v1.0.0-alpha.1/rustynes-windows-x64.zip)

## What's Next

Phase 2 (Advanced Features) begins with Milestone 11:
- RetroAchievements integration
- GGPO netplay
- TAS tools (FM2 format)
- Lua scripting
- Expansion audio (VRC6, FDS, MMC5)

See [ROADMAP.md](ROADMAP.md) for full Phase 2 plan.

## Contributors

Thank you to all contributors who helped make v1.0.0-alpha.1 possible!
```

## Acceptance Criteria

- [ ] Full regression testing complete (202/212 tests passing)
- [ ] Binaries built for Linux, macOS, Windows
- [ ] Version decided (v0.9.0 or v1.0.0-alpha.1)
- [ ] Release notes written (comprehensive, clear)
- [ ] Git tag created and pushed
- [ ] GitHub release published with binaries
- [ ] Phase 1.5 completion report written
- [ ] ROADMAP.md updated (Phase 1.5 complete, Phase 2 next)
- [ ] Ready for Phase 2 kickoff

## Phase 1.5 Completion Report Outline

```markdown
# Phase 1.5: Stabilization & Accuracy - Completion Report

## Executive Summary
- Duration: 12 weeks (January-April 2026)
- Milestones: M7-M10 (4 milestones, 16 sprints)
- Outcome: 95%+ test ROM pass rate, high-quality audio, optimized performance

## Achievements

### Milestone 7: Accuracy Improvements (v0.6.0)
- CPU timing refinements (±1 cycle)
- PPU VBlank precision (±2 cycles)
- APU frame counter precision (±1 cycle)
- Bus timing accuracy (OAM DMA)

### Milestone 8: Test ROM Validation (v0.7.0)
- 202/212 tests passing (95%+)
- Automated test harness
- CI integration

### Milestone 9: Known Issues Resolution (v0.8.0)
- Dynamic audio resampling
- Audio/video synchronization
- Performance optimization (20%+)
- Bug fixes and polish

### Milestone 10: Final Polish (v0.9.0/v1.0.0-alpha.1)
- UI/UX improvements
- Comprehensive documentation
- Multi-platform release

## Metrics

[Detailed metrics table as shown in M10-OVERVIEW.md]

## Lessons Learned
- Test-driven development crucial for accuracy
- Systematic approach to test ROM validation effective
- Performance profiling early prevents late optimization crunch
- Comprehensive documentation benefits users and contributors

## Next Steps: Phase 2
- Advanced features (RetroAchievements, netplay, TAS tools)
- Expansion audio (VRC6, FDS, MMC5)
- Rare mapper implementations
- Target: v1.0.0 (full release) by December 2027
```

## Version Target

v0.9.0 or v1.0.0-alpha.1 (Final Decision)

---

**Status:** Pending
**Depends On:** M10-S0 (Complete), M10-S1, M10-S2
**Blocks:** Phase 2 (Advanced Features)
**Last Updated:** 2025-12-28
