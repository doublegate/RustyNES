# Changelog

All notable changes to RustyNES will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Project Setup - 2025-12-18

#### Added

- Initial project structure with 10 workspace crates
- Comprehensive documentation suite (73 markdown files, 52,402 lines)
  - CPU documentation (6502 specification, timing, opcodes)
  - PPU documentation (2C02 specification, rendering, scrolling)
  - APU documentation (2A03 specification, all 5 channels)
  - Mapper documentation (NROM, MMC1, MMC3, etc.)
  - API reference documentation
  - Development guides (BUILD, CONTRIBUTING, TESTING, DEBUGGING)
  - Format specifications (iNES, NES 2.0, NSF, FM2)
- GitHub project templates
  - Issue templates (bug report, feature request)
  - Pull request template
  - Code of Conduct
  - Contributing guidelines
  - Security policy
  - Support documentation
- Development infrastructure
  - Dependabot configuration for automated dependency updates
  - CODEOWNERS file for code review assignments
- Project documentation
  - README.md with feature overview
  - ROADMAP.md with development timeline
  - ARCHITECTURE.md with system design
  - OVERVIEW.md with project philosophy
  - CHANGELOG.md (this file)

#### Documentation

- CPU specifications: 6502 instruction set, timing tables, unofficial opcodes
- PPU specifications: 2C02 rendering pipeline, dot-level timing, scrolling mechanics
- APU specifications: Audio channels, mixing, frame counter, DMC channel
- Mapper specifications: NROM, MMC1, MMC3, and 8 additional mappers
- API documentation: Core API, save states, configuration, Lua scripting
- Testing guides: Test ROM usage, nestest golden log, game testing strategy
- Build documentation: Multi-platform build instructions, cross-compilation
- File format documentation: iNES, NES 2.0, NSF, FM2, FDS, UNIF

---

## Development Phases

RustyNES follows a phased development approach. See [ROADMAP.md](ROADMAP.md) for complete details.

### Phase 1: MVP (Target: June 2026)

- [ ] Cycle-accurate 6502/2A03 CPU implementation
- [ ] Dot-level 2C02 PPU rendering
- [ ] Hardware-accurate 2A03 APU synthesis
- [ ] Mappers 0, 1, 2, 3, 4 (80% game coverage)
- [ ] Cross-platform desktop GUI (egui + wgpu)
- [ ] Save states and battery saves
- [ ] Gamepad support
- [ ] 85% TASVideos test suite pass rate

### Phase 2: Advanced Features (Target: December 2026)

- [ ] RetroAchievements integration (rcheevos)
- [ ] GGPO rollback netplay (backroll-rs)
- [ ] Lua 5.4 scripting API
- [ ] TAS recording/playback (FM2 format)
- [ ] Integrated debugger (CPU, PPU, APU viewers)
- [ ] Rewind, fast-forward, slow-motion
- [ ] 95% TASVideos test suite pass rate

### Phase 3: Expansion (Target: June 2027)

- [ ] WebAssembly build with PWA support
- [ ] Expansion audio (VRC6, VRC7, MMC5, FDS, N163, 5B)
- [ ] Additional mappers (target: 200+ total)
- [ ] Mobile platform support (Android, iOS)
- [ ] CRT/NTSC shader framework
- [ ] 98% TASVideos test suite pass rate

### Phase 4: Polish (Target: December 2027)

- [ ] Advanced video filters and shaders
- [ ] TAS editor with frame-by-frame editing
- [ ] Enhanced debugger features
- [ ] Performance optimizations
- [ ] 300+ mapper implementations
- [ ] 100% TASVideos accuracy test pass rate
- [ ] v1.0.0 release

---

## Version History

### [0.0.1] - 2025-12-18 (Project Initialization)

**Status**: Pre-implementation (Architecture & Documentation Complete)

This release represents the completion of the project planning phase. No executable emulator exists yet, but all architectural decisions have been made and comprehensively documented.

#### Added

- Project repository structure
- Workspace configuration with 10 crates
- Complete documentation suite (73 files)
- GitHub project infrastructure
- Development guidelines and policies
- Roadmap and milestone definitions

#### Notes

- No functional emulation code in this release
- Focus on planning, architecture, and documentation
- Ready to begin Phase 1 implementation (CPU core)

---

## Changelog Conventions

### Categories

We use the following categories for changes:

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Soon-to-be removed features
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security vulnerability fixes

### Versioning

RustyNES follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version (X.0.0): Incompatible API changes, major features
- **MINOR** version (0.X.0): Backward-compatible functionality additions
- **PATCH** version (0.0.X): Backward-compatible bug fixes

### Pre-1.0.0

During pre-1.0 development:

- Breaking changes may occur in any version
- APIs are not guaranteed to be stable
- Focus is on reaching feature completeness

---

## Links

- [Project Repository](https://github.com/doublegate/RustyNES)
- [Issue Tracker](https://github.com/doublegate/RustyNES/issues)
- [Discussions](https://github.com/doublegate/RustyNES/discussions)
- [Documentation](https://github.com/doublegate/RustyNES/tree/main/docs)

---

**Note**: This changelog will be actively maintained as the project progresses. Each significant change will be documented here following the Keep a Changelog format.
