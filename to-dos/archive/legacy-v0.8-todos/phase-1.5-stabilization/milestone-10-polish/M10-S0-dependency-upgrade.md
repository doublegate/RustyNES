# M10-S0: Dependency Upgrade Sprint

**Milestone:** M10 (Final Polish)
**Sprint:** 0 (Pre-Sprint - Dependency Upgrades)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~2-3 days
**Status:** COMPLETE
**Version Target:** v0.8.0+
**Progress:** 100%
**Baseline:** v0.7.1 (GUI Framework Migration Complete)
**Created:** 2025-12-28
**Completed:** 2025-12-28

---

## Overview

This pre-sprint focuses on **dependency upgrades and integration of new features** available in updated packages. Upgrading dependencies ensures RustyNES benefits from bug fixes, performance improvements, new capabilities, and stays compatible with the evolving Rust ecosystem.

### Goals

1. **Upgrade all dependencies** to latest stable versions
2. **Integrate beneficial new features** from upgraded packages
3. **Ensure MSRV compatibility** with project requirements
4. **Validate no regressions** through comprehensive testing
5. **Document breaking changes** and migration steps

---

## Current vs Latest Versions

### GUI/Graphics Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **eframe** | 0.29 | 0.33.2 | +4 minor | Yes | **HIGH** | Major features: Plugin system, iOS SafeArea, Atoms |
| **egui** | 0.29 | 0.33.3 | +4 minor | Yes | **HIGH** | Major features: Atoms, kittest, popup rewrite |
| **egui_extras** | 0.29 | 0.33.x | +4 minor | Yes | **HIGH** | Follow egui version |
| **image** | 0.25 | 0.25.x | ~0 | No | Low | Already at latest minor |

### Audio Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **cpal** | 0.15 | 0.16.0 | +1 minor | Minor | **MEDIUM** | Buffer underrun reporting, ALSA improvements |
| **rubato** | 0.16 | 0.16.2 | +0.0.2 | No | Low | Already at latest minor |

### Input Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **gilrs** | 0.11 | 0.13.0 | +2 minor | Possible | **MEDIUM** | Improved platform support |

### Configuration/Serialization Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **ron** | 0.8 | 0.12.0 | +4 minor | Yes | **MEDIUM** | Significant API improvements |
| **serde** | 1.0 | 1.0.228 | +0.0.x | No | Low | Stable 1.x series |
| **directories** | 5.0 | 5.0.x | ~0 | No | Low | Already at latest |

### CLI Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **clap** | 4.5 | 4.5.x | ~0 | No | Low | Already at latest minor |

### Utility Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **thiserror** | 1.0 | 2.0.17 | +1 major | Yes | **HIGH** | `no_std` support, improved ergonomics |
| **anyhow** | 1.0 | 1.0.x | ~0 | No | Low | Stable 1.x series |
| **bitflags** | 2.4 | 2.10.0 | +0.6 | No | Low | Minor improvements |
| **log** | 0.4 | 0.4.x | ~0 | No | Low | Stable |
| **env_logger** | 0.11 | 0.11.8 | +0.0.x | No | Low | Bug fixes |

### Development Dependencies

| Package | Current | Latest | Delta | Breaking Changes | Priority | Notes |
|---------|---------|--------|-------|------------------|----------|-------|
| **criterion** | 0.5 | 0.5.x | ~0 | No | Low | Already at latest |
| **proptest** | 1.4 | 1.9.0 | +5 minor | Minor | Low | New strategies, improvements |
| **rfd** | 0.15 | 0.15.4 | +0.0.4 | No | Low | Bug fixes |

---

## New Features Available

### egui 0.30 -> 0.33 (HIGH PRIORITY)

#### egui 0.30.0 (2024-12-16)
- **Modal dialog support**: Native modal dialogs for alerts, confirmations
- **egui_kittest**: New testing framework for UI automation and screenshot testing
- **Layer management**: Improved layer system for complex UI layouts

#### egui 0.31.0 (2025-02-04)
- **SceneContainer**: Better rendering organization for complex scenes
- **Pixel-perfect rendering**: Enhanced visual quality
- **CornerRadius/Margin/Shadow optimizations**: Reduced memory usage

#### egui 0.32.0 (2025-07-10)
- **Atoms**: New indivisible UI building blocks combining text, images, or custom content
  - **Relevance:** Useful for debug window labels, status displays
- **Popup system rewrite**: Complete restructuring of popups, tooltips, menus
  - **Relevance:** Better menu behavior in desktop GUI
- **SVG improvements**: Enhanced SVG rendering
- **Crisper text rendering**: Improved visual quality
- **BREAKING:** Menus close on click by default

#### egui 0.33.0 (2025-10-09)
- **Plugin trait**: Replace `on_begin_pass`/`on_end_pass` with improved state handling
  - **Relevance:** Cleaner debug window integration
- **Kerning improvements**: Better font rendering quality
- **Rotation gestures**: Trackpad rotation support
- **SafeArea support (iOS)**: Important for future mobile port
- **BREAKING:** Default text size 12.5 -> 13.0
- **BREAKING:** `screen_rect` deprecated for `viewport_rect`/`content_rect`
- **MSRV:** Rust 1.88+

### eframe 0.30 -> 0.33 (HIGH PRIORITY)

#### eframe 0.30.0 (2024-12-16)
- **Android support**: New platform target
- **BREAKING:** Explicit `wayland`/`x11` features required for Linux
- **MSRV:** Rust 1.80+

#### eframe 0.31.0 (2025-02-04)
- **IME support re-enabled** on Linux
- **Window maximized state** serialized in WindowSettings
- **App state saved** on Android/iOS suspension
- **Web keyboard shortcuts**: cmd-S/O forwarding

#### eframe 0.32.0 (2025-07-10)
- **External event loop support**: Better integration with custom event loops
- **macOS `movable_by_window_background`**: Drag window by background
- **macOS `has_shadow`**: Window shadow control
- **BREAKING:** Rust 2024 edition required
- **MSRV:** Rust 1.85+

#### eframe 0.33.0 (2025-10-09)
- **iOS SafeArea support**: Important for future iOS port
- **Rotation gesture support**: Trackpad gestures
- **Migrated to `windows-sys`**: From `winapi` crate
- **MSRV:** Rust 1.88+

### cpal 0.15 -> 0.16 (MEDIUM PRIORITY)

#### cpal 0.16.0 (2025-06-07)
- **Buffer underrun/overrun reporting**: `StreamError::BufferUnderrun`
  - **Relevance:** Better audio diagnostics for debugging
- **ALSA improvements**: Updated to alsa 0.10, improved error handling
- **ASIO sharing**: Share `sys::Asio` instance across Host instances
- **Custom host implementations**: `custom_host` feature for non-native audio systems

### thiserror 1.0 -> 2.0 (HIGH PRIORITY)

#### thiserror 2.0.0 (2024-11-06)
- **`no_std` support**: Can use in `no_std` environments
  - **Relevance:** Better compatibility with `rustynes-core` no_std goals
- **Improved derive macro**: Better error messages
- **MSRV:** Rust 1.61+

### ron 0.8 -> 0.12 (MEDIUM PRIORITY)

#### ron 0.9.0 - 0.12.0 (2025)
- **Improved error messages**: Better parsing diagnostics
- **API improvements**: More flexible serialization options
- **Performance improvements**: Faster parsing and serialization
- **Note:** May require migration of existing config files

### gilrs 0.11 -> 0.13 (MEDIUM PRIORITY)

#### gilrs 0.12 - 0.13 (2024-2025)
- **Improved platform support**: Better Linux/Windows compatibility
- **New controller mappings**: Updated SDL gamepad database
- **Bug fixes**: Various platform-specific fixes

### proptest 1.4 -> 1.9 (LOW PRIORITY)

#### proptest 1.5 - 1.9 (2024-2025)
- **New strategies**: Additional test generation strategies
- **Performance improvements**: Faster shrinking
- **Better error reporting**: Improved failure diagnostics

---

## Recommended Upgrade Phases

### Phase 1: Non-Breaking Utility Updates (Day 1, ~2 hours) - COMPLETE

Low-risk updates that require minimal code changes:

- [x] **bitflags**: 2.4 -> 2.10.0 (workspace)
- [x] **proptest**: 1.4 -> 1.9.0 (workspace)

**Testing:** Run `cargo test --workspace` after each update.

### Phase 2: Audio System Upgrade (Day 1, ~3 hours) - COMPLETE

Upgrade audio stack with minor API changes:

- [x] **cpal**: 0.15 -> 0.16.0 (rustynes-desktop)
  - No code changes required - API compatible

**Testing:**
- Run audio playback tests
- Verify no audio crackling or buffer issues
- Test on multiple audio devices if available

### Phase 3: Input System Upgrade (Day 1-2, ~2 hours) - SKIPPED

Upgrade gamepad support:

- [x] **gilrs**: 0.11 is already the latest stable version (0.13 does not exist)
  - No upgrade needed

**Testing:**
- Test gamepad hotplug detection
- Verify button mappings
- Test multiple controller types if available

### Phase 4: Configuration System Upgrade (Day 2, ~3 hours) - COMPLETE

Upgrade serialization with potential config migration:

- [x] **ron**: 0.8 -> 0.12.0 (rustynes-desktop)
  - No code changes required - API compatible
- [x] **thiserror**: 1.0 -> 2.0 (workspace and rustynes-desktop)
  - No code changes required - API compatible

**Testing:**
- Load existing config files
- Save and reload configurations
- Verify error messages unchanged

### Phase 5: GUI Framework Major Upgrade (Day 2-3, ~8 hours) - COMPLETE

Major upgrade with breaking changes - requires careful migration:

- [x] **egui**: 0.29 -> 0.33 (rustynes-desktop)
- [x] **eframe**: 0.29 -> 0.33 (rustynes-desktop)
- [x] **egui_extras**: 0.29 -> 0.33 (rustynes-desktop)
- [x] **Rust Edition**: 2021 -> 2024
- [x] **MSRV**: 1.75 -> 1.88

**Code Changes Required:**
- Removed explicit `ref` and `ref mut` patterns (Rust 2024 implicit borrowing)
- Used `is_multiple_of()` instead of `% x == 0` (new clippy lint)
- Collapsed nested `if let` patterns (new clippy lint)
- Suppressed deprecated `egui::menu::bar` warning (functional API still works)

**Migration Steps:**

1. **Update Cargo.toml** with new versions
2. **Fix deprecated APIs:**
   - Replace `screen_rect` with `viewport_rect`/`content_rect`
   - Update popup/menu handling for new close-on-click behavior
3. **Handle text size change:**
   - Default text size increased from 12.5 to 13.0
   - Adjust UI layouts if needed
4. **Update event handling:**
   - Review `on_begin_pass`/`on_end_pass` for Plugin trait migration
5. **Test all UI elements:**
   - Menu bar
   - Debug windows (CPU, PPU, APU, Memory)
   - Settings dialogs
   - File dialogs
6. **Update Linux build:**
   - Ensure `wayland` and `x11` features explicitly enabled
7. **Evaluate new features:**
   - Consider `egui_kittest` for UI testing
   - Evaluate Atoms for status displays

**Testing:**
- Full GUI functionality test
- Menu navigation
- Debug window opening/closing
- ROM loading
- All scaling modes
- Keyboard and gamepad input

---

## Implementation Tasks

### Task 1: Prepare Upgrade Environment

- [ ] Create feature branch: `deps/upgrade-0.8.0`
- [ ] Document current test pass rate
- [ ] Run full test suite as baseline
- [ ] Backup current working configuration

### Task 2: Phase 1 - Utility Updates

- [ ] Update workspace Cargo.toml:
  ```toml
  bitflags = "2.10"
  proptest = "1.9"
  ```
- [ ] Update rustynes-desktop/Cargo.toml:
  ```toml
  env_logger = "0.11.8"
  rfd = "0.15.4"
  ```
- [ ] Run tests: `cargo test --workspace`
- [ ] Verify build: `cargo build --workspace`

### Task 3: Phase 2 - Audio Upgrade

- [ ] Update rustynes-desktop/Cargo.toml:
  ```toml
  cpal = "0.16"
  ```
- [ ] Review cpal 0.16 CHANGELOG for API changes
- [ ] Update audio.rs for new error types
- [ ] Add buffer underrun logging
- [ ] Test audio playback with sample ROM

### Task 4: Phase 3 - Input Upgrade

- [ ] Update rustynes-desktop/Cargo.toml:
  ```toml
  gilrs = "0.13"
  ```
- [ ] Review gilrs CHANGELOG for API changes
- [ ] Update input.rs if needed
- [ ] Test gamepad detection and input

### Task 5: Phase 4 - Configuration Upgrade

- [ ] Update Cargo.toml files:
  ```toml
  ron = "0.12"
  thiserror = "2.0"
  ```
- [ ] Test config file loading/saving
- [ ] Update error derive macros if needed
- [ ] Verify config migration path

### Task 6: Phase 5 - GUI Framework Upgrade

- [ ] Update rustynes-desktop/Cargo.toml:
  ```toml
  eframe = { version = "0.33", default-features = false, features = ["default_fonts", "glow", "wayland", "x11"] }
  egui = "0.33"
  egui_extras = { version = "0.33", features = ["image"] }
  ```
- [ ] Fix compilation errors
- [ ] Update deprecated API usage
- [ ] Adjust UI layouts for text size change
- [ ] Test all GUI functionality

### Task 7: Documentation and Cleanup

- [ ] Update CLAUDE.md with new versions
- [ ] Update README.md dependency section
- [ ] Document any configuration migration steps
- [ ] Update rust-version in Cargo.toml if needed

---

## MSRV Considerations

### Current MSRV: Rust 1.75

### Required MSRV After Upgrades:

| Package | Required MSRV | Impact |
|---------|---------------|--------|
| egui 0.33 | 1.88+ | **Requires MSRV bump** |
| eframe 0.33 | 1.88+ | **Requires MSRV bump** |
| eframe 0.32 | 1.85+ | Alternative if 1.88 too new |
| thiserror 2.0 | 1.61+ | Compatible |
| cpal 0.16 | TBD | Likely compatible |

### Recommendation

#### Option A: Full Upgrade (Recommended)

- Upgrade to egui/eframe 0.33.x
- Bump MSRV to 1.88+
- Get all latest features and fixes

#### Option B: Partial Upgrade (Conservative)
- Upgrade to egui/eframe 0.31.x
- Keep MSRV at 1.80+
- Miss some features but maintain compatibility

### Action Required

Update `Cargo.toml`:
```toml
[workspace.package]
rust-version = "1.88"
```

---

## Risk Assessment

### High Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| egui/eframe breaking changes | High | High | Incremental testing, rollback plan |
| UI layout changes from text size | Medium | High | Adjust layouts, test all windows |
| Config file incompatibility | Medium | Medium | Implement migration, backup configs |

### Medium Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Audio issues from cpal upgrade | Medium | Low | Extensive audio testing |
| Gamepad compatibility | Low | Low | Test multiple controllers |
| MSRV increase blocks users | Low | Low | Document requirements clearly |

### Low Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Utility crate updates | Low | Very Low | Simple version bumps |
| Build time increase | Low | Low | Monitor compile times |

---

## Testing Plan

### Pre-Upgrade Baseline

1. Run full test suite: `cargo test --workspace`
2. Document test count and pass rate
3. Run manual GUI smoke test
4. Record audio playback quality

### Post-Phase Testing

After each phase, verify:

- [ ] All tests pass: `cargo test --workspace`
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings`
- [ ] Build succeeds: `cargo build --release --workspace`
- [ ] GUI launches and functions
- [ ] Audio plays correctly
- [ ] Gamepad input works (if controller available)

### Final Validation

- [ ] Full test suite passes (500+ tests)
- [ ] All Blargg tests still pass (90/90)
- [ ] Manual playthrough of Super Mario Bros.
- [ ] Debug windows functional
- [ ] Configuration saves/loads correctly
- [ ] No audio crackling or dropouts
- [ ] Performance unchanged (120+ FPS)

---

## Rollback Plan

If critical issues arise:

1. **Immediate:** Revert to previous Cargo.lock
2. **Short-term:** Revert specific package to known-good version
3. **Long-term:** Create issue, investigate root cause

### Version Pinning

If rollback needed for specific package:
```toml
# Pin to specific version if issues
egui = "=0.29.1"
eframe = "=0.29.0"
```

---

## Success Criteria

### Required

- [ ] All tests pass (500+ tests)
- [ ] GUI fully functional
- [ ] Audio playback working
- [ ] Gamepad input working
- [ ] Configuration loads/saves
- [ ] No performance regression

### Desired

- [ ] MSRV updated to latest stable requirement
- [ ] New egui features evaluated for use
- [ ] Documentation updated
- [ ] Clean clippy output

---

## Timeline

| Day | Focus | Deliverable |
|-----|-------|-------------|
| Day 1 (AM) | Phase 1-2 | Utility + Audio upgrades |
| Day 1 (PM) | Phase 3-4 | Input + Config upgrades |
| Day 2 | Phase 5 | GUI framework upgrade |
| Day 3 | Testing + Docs | Validation, documentation |

---

## References

### Changelogs

- [egui CHANGELOG](https://github.com/emilk/egui/blob/master/CHANGELOG.md)
- [eframe CHANGELOG](https://github.com/emilk/egui/blob/master/crates/eframe/CHANGELOG.md)
- [cpal CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md)
- [thiserror 2.0 announcement](https://www.reddit.com/r/rust/comments/1glb3ya/psa_thiserror_200_released/)

### Migration Guides

- [cpal UPGRADING.md](https://github.com/RustAudio/cpal/blob/master/UPGRADING.md)
- [egui Migration Notes](https://github.com/emilk/egui/releases)

### Crates.io Links

- [eframe](https://crates.io/crates/eframe)
- [egui](https://crates.io/crates/egui)
- [cpal](https://crates.io/crates/cpal)
- [gilrs](https://crates.io/crates/gilrs)
- [ron](https://crates.io/crates/ron)
- [thiserror](https://crates.io/crates/thiserror)

---

## New Features Integration Opportunities

### Immediate Value

1. **egui_kittest**: Add UI automation tests for debug windows
2. **Buffer underrun reporting**: Improve audio diagnostics
3. **thiserror no_std**: Better core crate compatibility

### Future Consideration

1. **Atoms**: Explore for status displays and debug labels
2. **Plugin trait**: Cleaner debug window integration
3. **iOS SafeArea**: Foundation for future mobile port
4. **Modal dialogs**: Better error/confirmation UX

---

**Status:** COMPLETE
**Blocks:** M10-S1 (UI/UX Improvements)
**Completed:** 2025-12-28

## Summary of Changes

### Dependency Versions Updated

| Package | Before | After |
|---------|--------|-------|
| bitflags | 2.4 | 2.10 |
| proptest | 1.4 | 1.9 |
| cpal | 0.15 | 0.16 |
| ron | 0.8 | 0.12 |
| thiserror | 1.0 | 2.0 |
| eframe | 0.29 | 0.33 |
| egui | 0.29 | 0.33 |
| egui_extras | 0.29 | 0.33 |

### Toolchain Updates

| Setting | Before | After |
|---------|--------|-------|
| Rust Edition | 2021 | 2024 |
| MSRV | 1.75 | 1.88 |

### Code Changes

1. **Rust 2024 Pattern Matching**: Removed explicit `ref`/`ref mut` from patterns in favor of implicit borrowing
2. **Clippy Lints**: Updated code to use `is_multiple_of()` and collapsed nested `if let` patterns
3. **Deprecated API**: Suppressed `egui::menu::bar` deprecation warning (API still functional)

### Test Results

- **508 tests passing** (0 failed, 8 ignored)
- **100% Blargg pass rate** maintained (90/90 tests)
- **All clippy warnings resolved**
