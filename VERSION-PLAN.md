# RustyNES Version Plan

## Semantic Versioning Strategy

RustyNES follows [Semantic Versioning 2.0.0](https://semver.org/) with a phased approach to v1.0.0:

- **v0.x.y** (Phase 1: MVP) - Pre-release development, breaking changes expected
- **v0.6.0 - v1.0.0-alpha.1** (Phase 1.5: Stabilization) - Accuracy improvements, test ROM validation
- **v1.1.0 - v1.5.0** (Phase 2: Features) - Feature additions, API stabilization
- **v1.6.0 - v1.9.0** (Phase 3: Expansion) - Feature complete, polish and optimization
- **v1.10.0 - v1.0.0-rc.x** (Phase 4: Polish) - Release candidates, final testing
- **v1.0.0** (Milestone 22) - Production release, TASVideos 100% accuracy

### Version Number Components

```
MAJOR.MINOR.PATCH[-PRERELEASE]
```

- **MAJOR**: Increments at v1.0.0 release (currently 0)
- **MINOR**: Increments with each milestone completion in Phase 1 (v0.x.y)
- **PATCH**: Increments for bug fixes between milestones (v0.x.y)
- **PRERELEASE**: alpha/beta/rc identifiers for Phases 2-4

## Milestone-to-Version Mapping

### Phase 1: MVP Development (v0.1.0 - v0.5.0)

Phase 1 focuses on core emulation engine development with a working desktop GUI.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M1** | CPU Complete | v0.1.0 | January 2026 | ✅ Done |
| **M2** | PPU Complete | v0.1.0 | January 2026 | ✅ Done |
| **M3** | APU Complete | v0.2.0 | December 2025 | ✅ Done |
| **M4** | Mappers Complete (NROM, MMC1, UxROM, CNROM, MMC3) | v0.3.0 | December 2025 | ✅ Done |
| **M5** | Integration/Core Complete | v0.4.0 | December 2025 | ✅ Done |
| **M6** | GUI Complete (MVP Release) | v0.5.0 | December 2025 | ✅ Done |

**Phase 1 Rationale:**
- M1+M2 combined into single v0.1.0 as they were completed together
- M3 (APU) warrants v0.2.0 as major emulation component
- Each subsequent milestone represents significant functionality increase
- v0.5.0 marks MVP completion: playable emulator with GUI

### Phase 1.5: Stabilization & Accuracy (v0.6.0 - v1.0.0-alpha.1)

Phase 1.5 bridges Phase 1 MVP to Phase 2 Advanced Features, focusing on accuracy, stability, and comprehensive test ROM validation.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M7** | Accuracy Improvements | v0.6.0 | January 2026 | Pending |
| **M8** | Test ROM Validation (95%+ pass rate) | v0.7.0 | February 2026 | Pending |
| **M9** | Known Issues Resolution | v0.8.0 | March 2026 | Pending |
| **M10** | Final Polish | v0.9.0 or v1.0.0-alpha.1 | April 2026 | Pending |

**Phase 1.5 Rationale:**
- v0.6.0 focuses on timing accuracy refinements (CPU, PPU, APU, Bus)
- v0.7.0 integrates all 212 test ROMs with 95%+ pass rate target (202+/212 tests)
- v0.8.0 resolves known issues (audio quality, PPU edge cases, performance)
- v0.9.0/v1.0.0-alpha.1 decision based on quality gates:
  - **v0.9.0**: If 90-94% test pass rate, minor issues remaining
  - **v1.0.0-alpha.1**: If 95%+ test pass rate, zero critical bugs, documentation complete
- Establishes production-ready foundation for Phase 2 advanced features
- Version jump to v1.0.0-alpha.1 signals transition to feature development phase

### Phase 2: Advanced Features (v1.1.0 - v1.5.0)

Phase 2 adds advanced features for competitive and achievement-focused users.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M11** | RetroAchievements Integration | v1.1.0 | May-June 2026 | Pending |
| **M12** | Netplay (GGPO) | v1.2.0 | July-September 2026 | Pending |
| **M13** | TAS Tools | v1.3.0 | August-September 2026 | Pending |
| **M14** | Lua Scripting | v1.4.0 | September-October 2026 | Pending |
| **M15** | Advanced Debugger | v1.5.0 | October-November 2026 | Pending |

**Phase 2 Rationale:**
- Minor version increments for each major feature addition
- Each release represents significant user-facing functionality
- API stabilization for v1.0.0 final release
- Backward compatible with v1.0.0-alpha.1 save states
- Focus on community features (achievements, netplay, TAS tools)

### Phase 3: Platform Expansion (v1.6.0 - v1.9.0)

Phase 3 expands platform support and completes mapper library.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M16** | Expansion Audio (VRC6, VRC7, MMC5, N163, FDS) | v1.6.0 | January-March 2027 | Pending |
| **M17** | Additional Mappers (50 total, 98% coverage) | v1.7.0 | February-May 2027 | Pending |
| **M18** | WebAssembly Port | v1.8.0 | April-May 2027 | Pending |
| **M19** | TAS Editor UI | v1.9.0 | May-June 2027 | Pending |

**Phase 3 Rationale:**
- Continued minor version increments for major features
- Focus on platform expansion and mapper completeness
- API remains stable from Phase 2
- WebAssembly brings browser-based emulation
- TAS Editor completes tool-assisted speedrun workflow

### Phase 4: Release Polish (v1.0.0-rc.1 - v1.0.0)

Phase 4 focuses on polish, optimization, and v1.0.0 release readiness.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M20** | Video Filters & Shaders | v1.0.0-rc.1 | September 2027 | Pending |
| **M21** | Performance Optimization | v1.0.0-rc.2 | October 2027 | Pending |
| **M22** | v1.0 Release Readiness | v1.0.0 | November-December 2027 | Pending |

**Phase 4 Rationale:**
- RC (Release Candidate) designation for final testing
- No new features, only polish and optimization
- TASVideos 100% accuracy requirement for v1.0.0
- API frozen, exhaustive compatibility testing

## Versioning Guidelines

### When to Bump Minor Version (v0.x.0)

Phase 1 only:
- Milestone completion (M3, M4, M5, M6)
- Major emulation component addition (CPU, PPU, APU, Mappers)
- Significant functionality increase (50+ games newly playable)

### When to Bump Patch Version (v0.x.y)

All phases:
- Bug fixes that don't add new features
- Performance improvements
- Documentation updates
- Dependency updates (security patches)
- Clippy/rustfmt compliance fixes

### When to Bump Prerelease (alpha/beta/rc)

Phases 1.5-4:
- **Phase 1.5 (v0.6.0-v0.9.0)**: Minor version increments for accuracy milestones (M7-M10)
- **Phase 2 (v1.1.0-v1.5.0)**: Minor version increments for feature additions (M11-M15)
- **Phase 3 (v1.6.0-v1.9.0)**: Minor version increments for platform expansion (M16-M19)
- **Phase 4 (RC)**: Release candidates for polish milestones (M20-M22)

### Breaking Changes Policy

- **Phase 1 (v0.x.y)**: Breaking changes allowed, documented in CHANGELOG
- **Phase 2 (alpha)**: Breaking changes minimized, require strong justification
- **Phase 3 (beta)**: Breaking changes forbidden, API is stable
- **Phase 4 (rc)**: Breaking changes forbidden, only bug fixes

## Version History

### Released Versions

- **v0.1.0** (2025-12-XX) - M1 (CPU) + M2 (PPU) complete
  - 6502 CPU emulation (all 256 opcodes)
  - 2C02 PPU emulation (backgrounds, sprites, scrolling)
  - 150 tests passing
  - nestest.nes golden log validation

- **v0.2.0** (2025-12-19) - M3 (APU) complete
  - 2A03 APU emulation (all 5 audio channels)
  - Frame Counter, Envelope, Length Counter, Sweep
  - Pulse, Triangle, Noise, DMC channels
  - Non-linear mixer and resampler
  - 150 tests passing (136 unit + 14 doc)
  - Zero unsafe code

- **v0.3.0** (2025-12-19) - M4 (Mappers) complete
  - NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4) mappers
  - 77.7% game compatibility (5 mappers covering majority of library)
  - 78 mapper-specific tests
  - iNES and NES 2.0 ROM format support

- **v0.4.0** (2025-12-19) - M5 (Integration) complete
  - Full emulation core integration (CPU + PPU + APU + Mappers)
  - Test ROM validation framework
  - Configuration system foundation
  - PRG-RAM support for test ROMs
  - 398 tests passing across 5 crates

- **v0.5.0** (2025-12-19) - M6 (GUI) complete - MVP RELEASE
  - Desktop GUI with egui immediate-mode framework
  - GPU-accelerated rendering via wgpu
  - Cross-platform audio output via cpal (48 kHz stereo)
  - Keyboard and gamepad input via gilrs
  - Configuration persistence (JSON-based)
  - 400+ tests passing across 6 crates
  - Phase 1 MVP Complete

### Planned Versions

#### Phase 1.5: Stabilization & Accuracy (2026 Q1-Q2)

- **v0.6.0** (January 2026) - M7 (Accuracy Improvements)
- **v0.7.0** (February 2026) - M8 (Test ROM Validation)
- **v0.8.0** (March 2026) - M9 (Known Issues Resolution)
- **v0.9.0 or v1.0.0-alpha.1** (April 2026) - M10 (Final Polish)

#### Phase 2: Advanced Features (2026 Q2-Q4)

- **v1.1.0** (May-June 2026) - M11 (RetroAchievements)
- **v1.2.0** (July-September 2026) - M12 (Netplay/GGPO)
- **v1.3.0** (August-September 2026) - M13 (TAS Tools)
- **v1.4.0** (September-October 2026) - M14 (Lua Scripting)
- **v1.5.0** (October-November 2026) - M15 (Advanced Debugger)

#### Phase 3: Platform Expansion (2027 Q1-Q2)

- **v1.6.0** (January-March 2027) - M16 (Expansion Audio)
- **v1.7.0** (February-May 2027) - M17 (Additional Mappers)
- **v1.8.0** (April-May 2027) - M18 (WebAssembly Port)
- **v1.9.0** (May-June 2027) - M19 (TAS Editor UI)

#### Phase 4: Release Polish (2027 Q3-Q4)

- **v1.0.0-rc.1** (September 2027) - M20 (Video Filters & Shaders)
- **v1.0.0-rc.2** (October 2027) - M21 (Performance Optimization)
- **v1.0.0** (November-December 2027) - M22 (Release Readiness) - PRODUCTION RELEASE

## Accuracy Milestones

Version requirements for accuracy:

- **v0.2.0**: APU functional tests passing ✅
- **v0.3.0**: 77.7% game compatibility (5 core mappers) ✅
- **v0.5.0 (MVP)**: 77.7% game compatibility achieved ✅
- **v0.6.0**: CPU/PPU/APU timing refinements (±1-2 cycle accuracy)
- **v0.7.0**: 95%+ test ROM pass rate (202+/212 tests)
- **v0.9.0/v1.0.0-alpha.1**: Production-ready stability (Phase 1.5 complete)
- **v1.5.0**: 95% game compatibility (target: 15 mappers, Phase 2 complete)
- **v1.9.0**: 98% game compatibility (target: 50+ mappers, Phase 3 complete)
- **v1.0.0**: 100% TASVideos accuracy suite (156 tests)

## Git Tagging Strategy

### Tag Format

```
vMAJOR.MINOR.PATCH[-PRERELEASE]
```

Examples:
- `v0.2.0`
- `v1.0.0-alpha.1`
- `v1.0.0-rc.2`
- `v1.0.0`

### Tag Annotations

All tags MUST be annotated with:
- Release title (milestone name)
- Key features list
- Technical specifications
- Test coverage statistics
- Links to documentation
- Comparison link to previous version

### Tag Signing

Production releases (v1.0.0 and later) will be GPG-signed.

## Release Workflow

### Pre-Release Checklist

1. All tests passing (`cargo test --workspace`)
2. Zero clippy warnings (`cargo clippy --workspace -- -D warnings`)
3. Code formatted (`cargo fmt --check`)
4. Documentation updated (README, CHANGELOG, API docs)
5. Version bumped in all `Cargo.toml` files
6. CHANGELOG entry added with release notes

### Release Process

1. **Local Validation**
   ```bash
   cargo fmt --check
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```

2. **Version Update**
   - Update all `Cargo.toml` files
   - Update `CHANGELOG.md`
   - Update version references in documentation

3. **Git Operations**
   ```bash
   git add -A
   git commit -m "chore(release): bump version to vX.Y.Z"
   git tag -a vX.Y.Z -m "Release notes..."
   git push origin main
   git push origin vX.Y.Z
   ```

4. **GitHub Release**
   - Create release from tag via `gh` CLI or web UI
   - Upload binary artifacts (multi-platform builds)
   - Link to documentation
   - Link to CHANGELOG

5. **Crates.io Publication** (post-v1.0.0)
   ```bash
   cargo publish -p rustynes-cpu
   cargo publish -p rustynes-ppu
   cargo publish -p rustynes-apu
   cargo publish -p rustynes-core
   cargo publish -p rustynes-desktop
   ```

## Backward Compatibility

### Save States

- Save state format versioning: `RUSTYNES_SAVE_V1`
- Major version changes may break save compatibility
- Migration tools provided for v1.0.0+ saves
- Warning displayed when loading incompatible saves

### Configuration Files

- Configuration format versioning
- Automatic migration for minor/patch versions
- Manual migration guide for major versions

### API Stability

- **Phase 1 (v0.x.y)**: No API stability guarantees
- **Phase 2 (alpha)**: Core API stabilizing, peripheral APIs may change
- **Phase 3 (beta)**: API frozen, only additions allowed
- **Phase 4 (rc) & v1.0.0+**: Semantic versioning guarantees

## Version Support Policy

### Long-Term Support (LTS)

Post-v1.0.0:
- Latest major version: Full support (features + bug fixes)
- Previous major version: Security fixes only (6 months)
- Older versions: Community support only

### Security Updates

- Critical security fixes backported to current + previous major version
- Security advisories published via GitHub Security Advisories
- CVE registration for confirmed vulnerabilities

## Deprecation Policy

Post-v1.0.0:
- Deprecation warnings in code (1 minor version before removal)
- Deprecation notices in CHANGELOG
- Removal in next major version only
- Migration guide provided

## Version Timeline Summary

```
2025 Q4: v0.1.0 (M1+M2), v0.2.0 (M3), v0.3.0 (M4), v0.4.0 (M5), v0.5.0 (M6 - MVP) ✅ COMPLETE
2026 Q1: v0.6.0 (M7 - Accuracy), v0.7.0 (M8 - Test ROMs)
2026 Q2: v0.8.0 (M9 - Known Issues), v0.9.0/v1.0.0-alpha.1 (M10 - Polish)
2026 Q2-Q3: v1.1.0 (M11 - RetroAchievements), v1.2.0 (M12 - Netplay)
2026 Q3-Q4: v1.3.0 (M13 - TAS), v1.4.0 (M14 - Lua), v1.5.0 (M15 - Debugger)
2027 Q1-Q2: v1.6.0 (M16 - Expansion Audio), v1.7.0 (M17 - Mappers), v1.8.0 (M18 - WASM), v1.9.0 (M19 - TAS Editor)
2027 Q3-Q4: v1.0.0-rc.1 (M20 - Filters), v1.0.0-rc.2 (M21 - Optimization), v1.0.0 (M22 - Release)
```

**Note**: Phase 1 MVP was completed 6+ months ahead of original schedule (December 2025 vs June 2026).

## Related Documentation

- [README.md](README.md) - Project overview
- [ROADMAP.md](ROADMAP.md) - Development roadmap
- [CHANGELOG.md](CHANGELOG.md) - Version history
- [CONTRIBUTING.md](docs/dev/CONTRIBUTING.md) - Contribution guidelines
- [ARCHITECTURE.md](ARCHITECTURE.md) - System architecture

---

**Last Updated**: 2025-12-20
**Current Version**: v0.5.0 (Phase 1 MVP Complete)
**Next Release**: v0.6.0 (M7 - Accuracy Improvements, Phase 1.5 Start)
