# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustyNES is a next-generation Nintendo Entertainment System (NES) emulator written in Rust. Target: 100% TASVideos accuracy test pass rate, 300+ mappers, RetroAchievements, GGPO netplay, TAS tools, Lua scripting.

**Status:** v0.8.1 - Phase 1.5 Stabilization In Progress (M7-M8 Complete, M9 85% Complete). All Phase 1 milestones (M1-M6) done.

**Test Status:** 508+ tests passing (0 failures, 0 ignored). 100% Blargg pass rate (90/90 tests)

**Current Version:** v0.8.1 (December 28, 2025)
- M9 Known Issues Resolution: Core implementation 85% complete
- CPU performance: #[inline] hints on step(), execute_opcode(), handle_nmi(), handle_irq()
- PPU performance: #[inline] hints on step(), step_with_chr()
- Audio improvements: Dynamic resampling, A/V sync, buffer management (S1 complete)
- PPU edge cases: Sprite overflow, palette RAM, mid-scanline writes (S2 complete)
- Performance optimization: Hot path inline hints (S3 core complete)
- Zero accuracy regressions (508+ tests passing)

**Previous Version:** v0.8.0 (December 28, 2025)
- Rust 2024 Edition adoption (MSRV 1.88)
- Comprehensive dependency modernization
- eframe 0.33 + egui 0.33 immediate mode GUI
- OpenGL rendering via glow backend (replacing wgpu shader pipeline)
- cpal 0.16 for low-latency audio with lock-free ring buffer (8192 samples)
- Audio resampling via rubato 0.16 for flexible sample rate support
- Native file dialogs with rfd 0.15
- Gamepad support with gilrs 0.11 (hotplug detection)
- Configuration persistence with ron 0.12 format
- Debug windows: CPU, PPU, APU, Memory viewers
- Performance optimizations: inline hints, buffer reuse patterns

**Previous Version:** v0.7.1 (December 27, 2025)
- Desktop GUI framework migration: Iced+wgpu to eframe+egui
- eframe 0.29 + egui 0.29 immediate mode GUI
- cpal 0.15 for low-latency audio
- Configuration persistence with ron 0.8 format

**Previous Version:** v0.6.0 (December 20, 2025)
- M7: Accuracy Improvements complete (all 4 sprints)
- CPU cycle timing verified (all 256 opcodes, page boundary crossing)
- PPU VBlank/NMI timing functional, sprite 0 hit working
- APU frame counter precision fixed (22371 to 22372 cycles)
- Hardware-accurate non-linear audio mixer (NESdev TND formula)
- OAM DMA 513/514 cycle precision based on CPU cycle parity
- CPU cycle tracking added to bus for DMA alignment

## Repository

- **GitHub**: <https://github.com/doublegate/RustyNES>
- **License**: MIT / Apache-2.0 (dual-licensed)
- **Started**: December 2025

## Quick Start

```bash
# Build (once Cargo.toml files are created)
cargo build --workspace
cargo build --release

# Test
cargo test --workspace
cargo test -p rustynes-cpu

# Run desktop GUI
cargo run -p rustynes-desktop -- rom.nes

# Lint and format
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

## Architecture

### Workspace Structure (Created)

```text
rustynes/
├── crates/
│   ├── rustynes-core/src/         # Core emulation engine (no_std compatible)
│   ├── rustynes-cpu/src/          # 6502 CPU (reusable for C64, Apple II)
│   ├── rustynes-ppu/src/          # 2C02 PPU
│   ├── rustynes-apu/src/          # 2A03 APU with expansion audio
│   ├── rustynes-mappers/src/      # All mapper implementations
│   ├── rustynes-desktop/src/      # eframe/egui GUI frontend
│   ├── rustynes-web/src/          # WebAssembly frontend
│   ├── rustynes-tas/src/          # TAS recording/playback (FM2 format)
│   ├── rustynes-netplay/src/      # GGPO rollback netcode (backroll-rs)
│   └── rustynes-achievements/src/ # RetroAchievements (rcheevos FFI)
├── docs/                          # 40+ documentation files
│   ├── cpu/                       # 6502 CPU specification, timing, opcodes
│   ├── ppu/                       # 2C02 PPU rendering, timing, scrolling
│   ├── apu/                       # Audio channels, timing
│   ├── bus/                       # Memory map, bus conflicts
│   ├── mappers/                   # Mapper implementations (NROM, MMC1, MMC3, etc.)
│   ├── api/                       # Core API, save states, configuration
│   ├── testing/                   # Test ROM guide, nestest golden log
│   ├── input/                     # Controller handling
│   ├── dev/                       # Build, testing, contributing, debugging
│   ├── formats/                   # File format specifications
│   ├── features/                  # Advanced features documentation
│   └── platform/                  # Platform-specific build info
├── tests/                         # Integration tests
│   └── framework/                 # Test ROM validators and harness tools
├── benches/                       # Performance benchmarks
├── examples/                      # Usage examples
├── test-roms/                     # NES test ROM files (excluded from git)
├── assets/                        # Static resources
├── images/                        # Screenshots and visual documentation
├── temp/                          # Project-specific temporary files (gitignored)
├── ref-docs/                      # Reference documentation (architecture spec)
└── ref-proj/                      # Reference emulator projects (excluded from git)
```

### Core Design Principles

1. **Accuracy First**: Cycle-accurate CPU, dot-level PPU, pass all test ROMs before optimization
2. **Safe Rust**: Zero unsafe code except for FFI (rcheevos, platform APIs)
3. **Trait-Based Abstraction**: Strong typing with newtype patterns for registers/addresses
4. **Modular Crates**: Independent use of CPU/PPU/APU modules

### NES Timing Model

- Master clock: 21.477272 MHz (NTSC)
- CPU: 1.789773 MHz (master ÷ 12)
- PPU: 5.369318 MHz (master ÷ 4), 3 dots per CPU cycle
- Frame: 29,780 CPU cycles, 89,341 PPU dots

## Commands

### Build & Run

```bash
# Build
cargo build --workspace
cargo build --release --workspace

# Test (once implemented)
cargo test --workspace                    # All tests
cargo test -p rustynes-cpu                # Single crate
cargo test cpu_lda_immediate              # Single test

# Run
cargo run -p rustynes-desktop             # Desktop GUI
cargo run -p rustynes-desktop -- rom.nes  # With ROM

# Lint & Format
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo fmt                                 # Auto-format

# Benchmarks
cargo bench -p rustynes-core

# WebAssembly
wasm-pack build crates/rustynes-web --target web
```

### Test ROM Validation

```bash
# Run nestest automated mode (CPU validation)
cargo test nestest_rom

# Run blargg test suite
cargo test blargg_

# Full TASVideos accuracy suite
cargo test tasvideos_
```

### Development Workflow

```bash
# Pre-commit checks (recommended)
cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace

# Generate documentation
cargo doc --workspace --no-deps --open

# Check for unused dependencies
cargo +nightly udeps --workspace

# Security audit
cargo audit
```

## Reference Materials

### Documentation (`ref-docs/`)

- `RustyNES-Architecture-Design.md` - **Primary reference**: 3,400+ line comprehensive design spec
- `Claude-NES_Emulator_Compare-Opus4.5.md` - Comparison of reference emulators
- `Emulator_TechReports/` - 12 individual emulator technical reports

### Reference Projects (`ref-proj/`)

Cloned emulators for study and pattern reference:

| Project | Language | Key Reference For |
|---------|----------|-------------------|
| **Mesen2** | C++ | Gold standard accuracy, debugger |
| **FCEUX** | C++ | TAS tools, FM2 format |
| **puNES** | C++ | 461+ mapper implementations |
| **TetaNES** | Rust | Rust patterns, wgpu, egui |
| **Pinky** | Rust | PPU rendering, Visual2C02 tests |
| **Rustico** | Rust | Expansion audio |
| **DaveTCode/nes-emulator-rust** | Rust | Zero unsafe patterns |
| **RetroAchievements/** | C | rcheevos integration |

## Key Dependencies

- **Graphics**: `eframe` 0.33 (egui + window + OpenGL via glow)
- **GUI**: `egui` 0.33 (immediate mode GUI)
- **Audio**: `cpal` 0.16 (cross-platform audio I/O with lock-free ring buffer)
- **Resampling**: `rubato` 0.16 (high-quality audio resampling)
- **Input**: `gilrs` 0.11 (gamepad support with hotplug detection)
- **File Dialogs**: `rfd` 0.15 (native cross-platform file dialogs)
- **Configuration**: `ron` 0.12 (Rust Object Notation), `directories` 5.0 (platform paths)
- **CLI**: `clap` 4.5 (argument parsing)
- **Netplay**: `backroll` (GGPO rollback) - planned for Phase 2
- **Scripting**: `mlua` (Lua 5.4) - planned for Phase 2
- **Achievements**: `rcheevos-sys` (FFI bindings) - planned for Phase 2
- **Testing**: `criterion` (benchmarks), `proptest` (property-based)
- **Serialization**: `serde`, `ron` (configuration format)

## Architectural Decisions

### GUI Framework: eframe + egui (v0.7.1+, upgraded v0.8.0)
- **Changed from**: Iced+wgpu (v0.5.0-v0.6.0)
- **Current**: eframe 0.33 + egui 0.33 (as of v0.8.0)
- **Rationale**: Immediate mode GUI ideal for debug windows, simpler event loop, better game loop integration, integrated window management
- **Implementation**: eframe provides window + egui + OpenGL rendering; egui for menus/debug windows

### Window Management: eframe (glow backend)
- **Rationale**: Integrated with egui, cross-platform, OpenGL backend simpler than wgpu
- **Implementation**: eframe handles window creation and event loop with accumulator-based frame timing

### Framebuffer Rendering: egui textures
- **Rationale**: Direct integration with egui, efficient texture updates via OpenGL
- **Implementation**: 256x240 RGBA buffer with nearest-neighbor scaling via egui::Image

### Audio Backend: cpal 0.16 (upgraded v0.8.0)
- **Chosen over**: SDL2, rodio
- **Rationale**: Cross-platform, low-latency, direct device access, no runtime dependencies
- **Implementation**: Lock-free ring buffer (8192 samples) with atomic operations
- **Resampling**: rubato 0.16 for high-quality sample rate conversion

### Configuration: RON Format
- **Chosen over**: TOML, JSON
- **Rationale**: Rust native, type-safe, supports complex structures
- **Location**: Platform-specific config directory

### Test Framework
- Standalone validators in `tests/framework/`
- Enhanced ROM validator for detailed diagnostics
- Test ROM runner for automated validation
- Preserved for reference but not actively maintained

## Implementation Phases

| Phase | Status | Deliverable |
|-------|--------|-------------|
| **1: MVP** | ✅ **COMPLETE** | 80% game compatibility, desktop GUI, 5 mappers, audio |
| **1.5: Stabilization** | 🔄 **IN PROGRESS** | M7-M8 complete (100% Blargg), M9-M10 planned |
| **2: Features** | 📋 PLANNED | RetroAchievements, netplay, TAS, Lua, debugger |
| **3: Expansion** | 📋 PLANNED | Expansion audio, 98% mappers, WebAssembly |
| **4: Polish** | 📋 PLANNED | Video filters, TAS editor, v1.0 release |

## Development Timeline

| Milestone | Status | Description |
|-----------|--------|-------------|
| **Project Start** | ✅ December 2025 | Architecture & docs complete |
| **M1: CPU Complete** | ✅ v0.1.0 | nestest.nes passes (all 256 opcodes) |
| **M2: PPU Rendering** | ✅ v0.2.0 | Background & sprite rendering |
| **M3: APU Audio** | ✅ v0.3.0 | All 5 audio channels |
| **M4: Mappers** | ✅ v0.4.0 | 5 core mappers (0-4) |
| **M5: Input** | ✅ v0.4.0 | Controller support |
| **M6: Desktop GUI** | ✅ v0.5.0 | Iced + wgpu + audio integration |
| **M7: Accuracy** | ✅ v0.6.0 | CPU/PPU/APU timing, OAM DMA precision, hardware mixer |
| **M8: Test ROMs** | ✅ v0.7.0 | 100% Blargg pass rate (90/90 tests) |
| **GUI Migration** | ✅ v0.7.1 | eframe + egui desktop reimplementation |
| **Dependency Upgrade** | ✅ v0.8.0 | Rust 2024, eframe 0.33, egui 0.33, cpal 0.16, ron 0.12 |
| **M9: Known Issues** | 🔄 v0.8.1 (85%) | Audio S1, PPU S2, Performance S3 complete; S4 pending |
| **M10: Final Polish** | 📋 PLANNED | UI/UX improvements, documentation, v1.0.0-alpha.1 |
| **Phase 2+** | 📋 TBD | Advanced features |

## Recent Accomplishments (v0.8.1 - Dec 28, 2025)

### M9 Known Issues Resolution (85% Complete)

Systematic resolution of known issues identified during Phase 1.5 development:

#### Sprint 1: Audio Improvements (Complete)
- Two-stage decimation via rubato: 1.79MHz -> 192kHz -> 48kHz
- A/V sync with adaptive speed adjustment (0.99x-1.01x)
- Dynamic buffer sizing (2048-16384 samples)
- Hardware-accurate mixer with NES filter chain

#### Sprint 2: PPU Edge Cases (Complete)
- Sprite overflow bug with false positive/negative matching hardware
- Palette RAM mirroring at $3F10/$3F14/$3F18/$3F1C
- Mid-scanline write detection for split-screen effects
- Attribute byte extraction verified for all quadrants

#### Sprint 3: Performance Optimization (Core Complete)
- Added `#[inline]` to CPU hot paths: step(), execute_opcode(), handle_nmi(), handle_irq()
- Added `#[inline]` to PPU hot paths: step(), step_with_chr()
- Verified 68+ existing inline annotations in CPU/PPU crates
- Zero accuracy regressions (508+ tests passing)

#### Sprint 4: Bug Fixes & Polish (Pending)
- GitHub issue triage and resolution
- Release preparation for v0.9.0

---

## Previous Accomplishments (v0.8.0 - Dec 28, 2025)

### Comprehensive Dependency Modernization

Complete modernization of the technology stack for long-term maintainability:

#### Rust 2024 Edition & MSRV 1.88
- Adopted Rust 2024 Edition with latest language features
- MSRV set to Rust 1.88 for latest stable compiler support
- Updated all workspace and crate configurations

#### Dependency Upgrades
- **eframe** 0.33 (from 0.29): Latest egui integration
- **egui** 0.33 (from 0.29): Improved widget system
- **cpal** 0.16 (from 0.15): Audio device improvements
- **rubato** 0.16: High-quality audio resampling
- **ron** 0.12 (from 0.8): Configuration format improvements
- **thiserror** 2.0: Error handling modernization

#### Performance Optimizations
- Added `#[inline]` hints on critical audio/rendering paths
- Buffer reuse patterns for reduced allocations
- PPU edge case handling improvements

---

## Previous Accomplishments (v0.7.1 - Dec 27, 2025)

### Desktop GUI Framework Migration

Complete rewrite of the desktop frontend from Iced+wgpu to eframe+egui:

#### Architecture
- **eframe** 0.29 for window management + OpenGL rendering via glow
- **egui** 0.29 immediate mode GUI for menus and debug windows
- **cpal** 0.15 for low-latency audio with lock-free ring buffer
- **gilrs** 0.11 for gamepad support with hotplug detection
- **rfd** 0.15 for native file dialogs
- **ron** 0.8 for configuration persistence

#### Core Features
- Menu bar with File, Emulation, Video, Audio, Debug, Help menus
- ROM loading via native file dialogs (rfd)
- Multiple scaling modes: PixelPerfect (8:7 PAR), FitWindow, Integer
- Keyboard and gamepad input with configurable mappings
- Configuration persistence (RON format)
- Accumulator-based frame timing at 60.0988 Hz NTSC

#### Debug Windows (egui)
- CPU debugger: registers, flags, disassembly
- PPU viewer: pattern tables, nametables, OAM, palette
- Memory viewer: hex display with navigation
- APU state: channel visualization

#### Technical Improvements
- Simpler event loop with full control over frame timing
- OpenGL backend (glow) replaces wgpu shader pipeline
- Lock-free ring buffer with atomic operations for audio
- Better separation of emulation and rendering threads
- Reduced dependencies and faster compile times

---

## Previous Accomplishments (v0.6.0 - Dec 20, 2025)

### Milestone 7: Accuracy Improvements (4 Sprints)

#### Sprint 1: CPU Accuracy
- All 256 opcodes verified against nestest.nes golden log
- Page boundary crossing timing accuracy confirmed
- Unofficial opcode cycle counts validated
- Interrupt timing precision verified

#### Sprint 2: PPU Accuracy
- VBlank/NMI timing functional with flag read/race condition handling
- Sprite 0 hit detection working (2/2 basic tests passing)
- Attribute shift register verification complete
- Palette RAM mirroring edge cases handled

#### Sprint 3: APU Accuracy
- Frame counter precision fixed: 4-step mode quarter frame at cycle 22372 (was 22371)
- Hardware-accurate non-linear mixer: NESdev TND formula implemented
- Triangle linear counter timing verified
- Mixer output validated against reference implementation

#### Sprint 4: Timing & Synchronization
- OAM DMA cycle precision: 513 cycles (even CPU start) vs 514 cycles (odd CPU start)
- CPU cycle parity tracking: `(cpu_cycles % 2) == 1` check for alignment
- CPU/PPU synchronization verified
- Bus timing accuracy confirmed

### Technical Specifications (v0.6.0)

- **APU Frame Counter (4-step):** Cycles 7457, 14913, 22372, 29830 (corrected)
- **TND Mixer Formula:** `159.79 / (100 + 1 / (triangle/8227 + noise/12241 + dmc/22638))`
- **OAM DMA Timing:** `513 + if (cpu_cycles % 2) == 1 { 1 } else { 0 }`
- **Test Results:** 429 tests passing, 0 failures, 6 ignored

## Known Issues & Limitations

### Audio
- Occasional audio crackling under high system load (buffer underrun)
- No resampling for non-44.1kHz output devices
- Fixed latency (no dynamic adjustment)

### PPU
- Some attribute table edge cases may have minor glitches
- Sprite overflow flag not fully cycle-accurate
- Mid-scanline updates not yet supported for all registers

### General
- WebAssembly frontend not yet implemented
- Save states not yet implemented
- Debugger interface planned for Phase 2
- Limited to NTSC timing (PAL support planned)

## Accuracy Targets

- CPU: 100% nestest.nes golden log ✅ **ACHIEVED**
- PPU: 100% blargg PPU tests, sprite_hit, ppu_vbl_nmi 🔄 **IN PROGRESS**
- APU: 95%+ blargg APU tests 🔄 **IN PROGRESS**
- Overall: 100% TASVideos accuracy suite (156 tests) 📋 **PLANNED**

## Code Patterns

### CPU Instruction (Table-Driven)

```rust
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    let opcode = self.read(bus, self.pc);
    let addr_mode = self.addressing_mode_table[opcode as usize];
    let instruction = self.instruction_table[opcode as usize];
    instruction(self, bus, addr_mode)
}
```

### Strong Typing (Newtype Pattern)

```rust
#[derive(Copy, Clone, Debug)]
struct VramAddress(u16);

impl VramAddress {
    fn coarse_x(&self) -> u8 { (self.0 & 0x1F) as u8 }
    fn coarse_y(&self) -> u8 { ((self.0 >> 5) & 0x1F) as u8 }
}
```

### Mapper Trait

```rust
pub trait Mapper: Send {
    fn read_prg(&self, addr: u16) -> u8;
    fn write_prg(&mut self, addr: u16, val: u8);
    fn read_chr(&self, addr: u16) -> u8;
    fn write_chr(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring;
    fn irq_pending(&self) -> bool { false }
    fn clock(&mut self, _cycles: u8) {}
}
```

## Code Style Guidelines

### Rust Conventions

- **Edition**: Rust 2024
- **MSRV**: 1.88 (for Rust 2024 Edition support)
- **Format**: `rustfmt` with default settings
- **Lints**: `clippy::pedantic` + `-D warnings`
- **Unsafe**: Only permitted in FFI (rcheevos) and platform-specific audio; must be documented

### Naming Conventions

```rust
// Types: PascalCase
pub struct StatusRegister(u8);
pub enum AddressingMode { ... }

// Functions/methods: snake_case
fn execute_instruction(&mut self) { ... }

// Constants: SCREAMING_SNAKE_CASE
const MASTER_CLOCK_NTSC: u32 = 21_477_272;

// Module files: snake_case
// cpu.rs, ppu.rs, memory_map.rs
```

### Error Handling

```rust
// Use thiserror for library errors
#[derive(Debug, thiserror::Error)]
pub enum EmulatorError {
    #[error("Invalid ROM format: {0}")]
    InvalidRom(String),
    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),
}

// Return Result<T, EmulatorError> from fallible operations
// Use anyhow for application-level errors in desktop/web crates
```

### Documentation

- All public APIs must have doc comments
- Include examples for non-trivial functions
- Document panic conditions with `# Panics` section
- Cross-reference related documentation files

## Implementation Priorities

### Phase 1: MVP ✅ COMPLETE

1. **CPU**: Complete 6502 implementation with all 256 opcodes ✅
2. **PPU**: Background & sprite rendering with scrolling ✅
3. **APU**: All 5 channels (square1, square2, triangle, noise, DMC) ✅
4. **Bus**: Memory mapping, DMA, mapper integration ✅
5. **Mappers**: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4) ✅
6. **ROM Loading**: iNES format support ✅
7. **Desktop GUI**: eframe + egui with audio integration ✅ (v0.7.1)

### Phase 1.5: Stabilization 🔄 CURRENT

See `/to-dos/phase-1.5-stabilization/` for detailed milestone plans:
- **M7**: Accuracy Improvements ✅ **COMPLETE** (v0.6.0)
- **M8**: Test ROM Validation ✅ **COMPLETE** (v0.7.0) - 100% Blargg pass rate
- **GUI Migration**: ✅ **COMPLETE** (v0.7.1) - eframe + egui
- **M9**: Known Issues Resolution (audio quality, PPU edge cases)
- **M10**: Documentation and v1.0-alpha preparation

### Test-Driven Development

```bash
# Validation order
1. nestest.nes - CPU instruction accuracy
2. blargg_instr_test - CPU timing
3. blargg_ppu_tests - PPU behavior
4. blargg_apu_tests - APU timing
```

## Documentation Index

### Core Specifications

- `docs/cpu/CPU_6502_SPECIFICATION.md` - Complete 6502 reference
- `docs/cpu/CPU_TIMING_REFERENCE.md` - Cycle-accurate timing
- `docs/ppu/PPU_2C02_SPECIFICATION.md` - Complete PPU reference
- `docs/ppu/PPU_TIMING_DIAGRAM.md` - Dot-accurate timing
- `docs/apu/APU_OVERVIEW.md` - Audio system overview

### Implementation Guides

- `docs/mappers/MAPPER_OVERVIEW.md` - Mapper architecture
- `docs/testing/TEST_ROM_GUIDE.md` - Test ROM usage
- `docs/testing/NESTEST_GOLDEN_LOG.md` - CPU validation reference
- `docs/dev/BUILD.md` - Build instructions
- `docs/dev/CONTRIBUTING.md` - Contribution guidelines

### API Reference

- `docs/api/CORE_API.md` - Emulator core API
- `docs/api/SAVE_STATES.md` - Save state format
- `docs/api/CONFIGURATION.md` - Configuration options

### File Formats

- `docs/formats/INES_FORMAT.md` - iNES header parsing
- `docs/formats/NES20_FORMAT.md` - NES 2.0 extended format
- `docs/formats/NSF_FORMAT.md` - NES Sound Format
- `docs/formats/FM2_FORMAT.md` - TAS movie format

### APU Deep-Dive

- `docs/apu/APU_2A03_SPECIFICATION.md` - Complete APU reference
- `docs/apu/APU_CHANNEL_*.md` - Individual channel specs

## Related Files

- `ARCHITECTURE.md` - Detailed system architecture (20K+ lines)
- `OVERVIEW.md` - High-level project overview
- `ROADMAP.md` - Development roadmap with milestones
- `README.md` - GitHub landing page

## Quick Links

- [README](README.md) - Project landing page
- [ROADMAP](ROADMAP.md) - Development timeline
- [ARCHITECTURE](ARCHITECTURE.md) - System design
- [OVERVIEW](OVERVIEW.md) - Project philosophy
- [Documentation Index](docs/DOCUMENTATION_INDEX.md) - All docs

### External Resources

- [NESdev Wiki](https://www.nesdev.org/wiki/) - Hardware reference
- [TASVideos](https://tasvideos.org/) - Accuracy tests
- [NesDev Forums](https://forums.nesdev.org/) - Community
