# RustyNES Version Plan

## Semantic Versioning Strategy

RustyNES follows [Semantic Versioning 2.0.0](https://semver.org/) with a phased approach to v1.0.0:

- **v0.x.y** (Phase 1: MVP) - Pre-release development, breaking changes expected
- **v1.0.0-alpha.x** (Phase 2: Features) - Feature additions, API stabilization
- **v1.0.0-beta.x** (Phase 3: Expansion) - Feature complete, polish and optimization
- **v1.0.0-rc.x** (Phase 4: Polish) - Release candidates, final testing
- **v1.0.0** (Milestone 18) - Production release, TASVideos 100% accuracy

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

### Phase 2: Advanced Features (v1.0.0-alpha.1 - v1.0.0-alpha.4)

Phase 2 adds advanced features for competitive and achievement-focused users.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M7** | RetroAchievements Integration | v1.0.0-alpha.1 | July 2026 | Pending |
| **M8** | Netplay (GGPO) | v1.0.0-alpha.2 | September 2026 | Pending |
| **M9** | Lua Scripting | v1.0.0-alpha.3 | October 2026 | Pending |
| **M10** | Advanced Debugger | v1.0.0-alpha.4 | December 2026 | Pending |

**Phase 2 Rationale:**
- Alpha designation indicates feature additions with possible API changes
- Each alpha increment represents a major user-facing feature
- API begins stabilization for v1.0.0
- No breaking changes to core emulation (backward compatible with v0.5.0 saves)

### Phase 3: Platform Expansion (v1.0.0-beta.1 - v1.0.0-beta.4)

Phase 3 expands platform support and completes mapper library.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M11** | WebAssembly Port | v1.0.0-beta.1 | February 2027 | Pending |
| **M12** | Expansion Audio (VRC6, VRC7, MMC5, N163, FDS) | v1.0.0-beta.2 | April 2027 | Pending |
| **M13** | Additional Mappers (300+ total) | v1.0.0-beta.3 | June 2027 | Pending |
| **M14** | Mobile Support (iOS/Android) | v1.0.0-beta.4 | August 2027 | Pending |

**Phase 3 Rationale:**
- Beta designation indicates feature completeness
- Focus shifts to platform support and polish
- API is stable, no breaking changes expected
- Compatibility testing across all platforms

### Phase 4: Release Polish (v1.0.0-rc.1 - v1.0.0)

Phase 4 focuses on polish, optimization, and v1.0.0 release readiness.

| Milestone | Description | Version | Target Date | Status |
|-----------|-------------|---------|-------------|--------|
| **M15** | Video Filters & Shaders | v1.0.0-rc.1 | September 2027 | Pending |
| **M16** | TAS Editor UI | v1.0.0-rc.2 | October 2027 | Pending |
| **M17** | Performance Optimization | v1.0.0-rc.3 | November 2027 | Pending |
| **M18** | v1.0 Release Readiness | v1.0.0 | December 2027 | Pending |

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

Phases 2-4:
- **Alpha**: Each major feature addition (M7-M10)
- **Beta**: Each platform expansion or major mapper group (M11-M14)
- **RC**: Each polish milestone (M15-M17)

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

- **v1.0.0-alpha.1** (Q3 2026) - M7 (RetroAchievements)
- **v1.0.0-alpha.2** (Q3 2026) - M8 (Netplay)
- **v1.0.0-alpha.3** (Q4 2026) - M9 (Lua Scripting)
- **v1.0.0-alpha.4** (Q4 2026) - M10 (Debugger)

- **v1.0.0-beta.1** (Q1 2027) - M11 (WebAssembly)
- **v1.0.0-beta.2** (Q2 2027) - M12 (Expansion Audio)
- **v1.0.0-beta.3** (Q2 2027) - M13 (Additional Mappers)
- **v1.0.0-beta.4** (Q3 2027) - M14 (Mobile)

- **v1.0.0-rc.1** (Q3 2027) - M15 (Video Filters)
- **v1.0.0-rc.2** (Q4 2027) - M16 (TAS Editor)
- **v1.0.0-rc.3** (Q4 2027) - M17 (Optimization)
- **v1.0.0** (Q4 2027) - M18 (Release) - PRODUCTION RELEASE

## Accuracy Milestones

Version requirements for accuracy:

- **v0.2.0**: APU functional tests passing ✅
- **v0.3.0**: 77.7% game compatibility (5 core mappers) ✅
- **v0.5.0 (MVP)**: 77.7% game compatibility achieved ✅
- **v1.0.0-alpha.4**: 95% game compatibility (target: 15 mappers)
- **v1.0.0-beta.4**: 98% game compatibility (target: 50+ mappers)
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
2026 Q3: v1.0.0-alpha.1 (M7), v1.0.0-alpha.2 (M8)
2026 Q4: v1.0.0-alpha.3 (M9), v1.0.0-alpha.4 (M10)
2027 Q1: v1.0.0-beta.1 (M11)
2027 Q2: v1.0.0-beta.2 (M12), v1.0.0-beta.3 (M13)
2027 Q3: v1.0.0-beta.4 (M14), v1.0.0-rc.1 (M15)
2027 Q4: v1.0.0-rc.2 (M16), v1.0.0-rc.3 (M17), v1.0.0 (M18)
```

**Note**: Phase 1 MVP was completed 6+ months ahead of original schedule (December 2025 vs June 2026).

## Related Documentation

- [README.md](README.md) - Project overview
- [ROADMAP.md](ROADMAP.md) - Development roadmap
- [CHANGELOG.md](CHANGELOG.md) - Version history
- [CONTRIBUTING.md](docs/dev/CONTRIBUTING.md) - Contribution guidelines
- [ARCHITECTURE.md](ARCHITECTURE.md) - System architecture

---

**Last Updated**: 2025-12-19
**Current Version**: v0.5.0 (Phase 1 MVP Complete)
**Next Release**: v1.0.0-alpha.1 (M7 - RetroAchievements Integration)
