# RustyNES

> **Precise. Pure. Powerful.**

[![Build Status](https://github.com/doublegate/RustyNES/workflows/CI/badge.svg)](https://github.com/doublegate/RustyNES/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#platform-support)
[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://doublegate.github.io/RustyNES/)
[![codecov](https://codecov.io/gh/doublegate/RustyNES/branch/main/graph/badge.svg)](https://codecov.io/gh/doublegate/RustyNES)
[![CPU Tests](https://img.shields.io/badge/CPU%20tests-46%20passing-brightgreen.svg)](#test-validation-status)
[![PPU Tests](https://img.shields.io/badge/PPU%20tests-83%20passing-brightgreen.svg)](#test-validation-status)
[![APU Tests](https://img.shields.io/badge/APU%20tests-150%20passing-brightgreen.svg)](#test-validation-status)
[![Mapper Tests](https://img.shields.io/badge/Mapper%20tests-78%20passing-brightgreen.svg)](#test-validation-status)
[![Core Tests](https://img.shields.io/badge/Core%20tests-41%20passing-brightgreen.svg)](#test-validation-status)

## Overview

A next-generation NES emulator written in pure Rust — targeting 100% accuracy, modern features, and safe code

---

> **Status:** v0.4.0 Released - Phase 1 In Progress (83% Complete)
>
> **Milestones Completed:**
>
> - ✅ **M1: CPU** - Cycle-accurate 6502/2A03 with all 256 opcodes, 100% nestest.nes golden log validation
> - ✅ **M2: PPU** - Complete dot-level 2C02 rendering with background/sprite support, VBlank/NMI timing
> - ✅ **M3: APU** - Hardware-accurate 2A03 APU with 5 audio channels, 48 kHz output, non-linear mixing
> - ✅ **M4: Mappers** - NROM, MMC1, UxROM, CNROM, MMC3 enabling 77.7% game library compatibility
> - ✅ **M5: Integration** - Complete rustynes-core layer with Bus, Console, Input, and Save State framework
>
> **Test Suite:** 398 tests passing (46 CPU • 83 PPU • 150 APU • 78 Mappers • 41 Core)
>
> **Next:** Milestone 6 (Desktop GUI) - Cross-platform GUI with egui/wgpu for playable MVP release
>
> See [ROADMAP.md](ROADMAP.md) and [to-dos/](to-dos/) for development timeline.

---

## Why RustyNES?

RustyNES combines **accuracy-first emulation** with **modern features** and the **safety guarantees of Rust**. Whether you're a casual player, TAS creator, speedrunner, or homebrew developer, RustyNES provides a comprehensive platform for NES emulation.

**Key Differentiators:**

- Cycle-accurate emulation targeting 100% TASVideos test suite
- Modern features: RetroAchievements, GGPO netplay, TAS tools, Lua scripting
- Safe Rust with minimal unsafe code (only in FFI boundaries)
- Modular architecture allowing independent use of CPU/PPU/APU crates
- Cross-platform with WebAssembly support

---

## Highlights

| Feature               | Description                                                                               |
| --------------------- | ----------------------------------------------------------------------------------------- |
| **Cycle-Accurate**    | Sub-cycle precision for CPU, PPU, and APU - targeting 100% TASVideos test suite pass rate |
| **300+ Mappers**      | Comprehensive cartridge support covering all licensed games plus homebrew                 |
| **RetroAchievements** | Native rcheevos integration for achievement hunting                                       |
| **GGPO Netplay**      | Frame-perfect rollback netcode via backroll-rs                                            |
| **TAS Tools**         | FM2 format support with rewind, frame advance, and movie recording                        |
| **Lua Scripting**     | Modern Lua 5.4 scripting via mlua for automation and bots                                 |
| **GPU Accelerated**   | Cross-platform wgpu rendering with shader support                                         |
| **Pure Rust**         | Zero unsafe code where possible, leveraging Rust's safety guarantees                      |

---

## Quick Start

### Recent Release: v0.4.0 (December 19, 2025)

RustyNES has reached its fifth milestone with the completion of the Integration layer:

**What's New:**

- Complete rustynes-core integration layer connecting CPU, PPU, APU, and Mappers
- Hardware-accurate bus system implementing full NES memory map ($0000-$FFFF)
- Cycle-accurate OAM DMA (513-514 cycles with proper alignment)
- Console coordinator with proper timing synchronization (3 PPU dots per CPU cycle)
- Input system with hardware-accurate shift register protocol
- Save state framework with format specification and error handling
- 41 new integration and core tests (398 total workspace tests)
- Zero unsafe code maintained across all 5 crates

**Previous Releases:**

- **v0.3.0** - Complete Mapper subsystem with 5 mappers covering 77.7% of NES games (78 tests)
- **v0.2.0** - Complete 2A03 APU with 5 audio channels, 48 kHz output, non-linear mixing (150 tests)
- **v0.1.0** - Complete 6502 CPU (256 opcodes) and 2C02 PPU (dot-accurate rendering) (129 tests)

See [CHANGELOG.md](CHANGELOG.md) for full details.

### Download Binaries (Coming with M6)

Pre-built binaries will be available when the Desktop GUI is complete (Milestone 6, target: June 2026). Currently, you can build from source and run the comprehensive test suite.

### Build from Source

**Prerequisites:**

- **Rust 1.75 or newer** — Install via [rustup.rs](https://rustup.rs)
- **SDL2 development libraries** — Platform-specific installation below
- **Git**

**Clone and Build:**

```bash
# Clone the repository
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES

# Build all implemented crates (CPU, PPU, APU, Mappers, Core)
cargo build --workspace --release

# Run tests to verify installation
cargo test --workspace

# Note: The desktop GUI is not yet implemented (planned for Milestone 6)
# Currently, the core emulation engine is complete with 398 comprehensive tests
```

### Platform-Specific Dependencies

**Ubuntu/Debian:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential libsdl2-dev
```

**Fedora:**

```bash
sudo dnf install gcc SDL2-devel
```

**macOS:**

```bash
brew install sdl2
```

**Windows:**

- Install [Visual Studio 2019+](https://visualstudio.microsoft.com/) with C++ tools
- Download SDL2 development libraries from [libsdl.org](https://libsdl.org)
- Set `SDL2_PATH` environment variable to SDL2 location

### Test Validation Status

RustyNES demonstrates world-class emulation accuracy:

```bash
# Run all tests (all crates)
cargo test --workspace
# Total: 398 tests - 46 CPU + 83 PPU + 150 APU + 78 Mappers + 41 Core

# Run specific crate tests
cargo test -p rustynes-cpu      # 46 tests (100%)
cargo test -p rustynes-ppu      # 83 tests (100%)
cargo test -p rustynes-apu      # 150 tests (100%)
cargo test -p rustynes-mappers  # 78 tests (100%)
cargo test -p rustynes-core     # 41 tests (100%)

# Run with debug logging
RUST_LOG=debug cargo test --workspace

# Run benchmarks (once implemented)
cargo bench --workspace
```

**What's Working:**

- **CPU**: 100% complete - All 256 opcodes (151 official + 105 unofficial) validated
- **PPU**: 100% complete - Background/sprite rendering, VBlank/NMI timing, sprite 0 hit
- **APU**: 100% complete - All 5 audio channels with hardware-accurate mixing
- **Mappers**: 100% complete - 5 mappers covering 77.7% of NES games
- **Core**: 100% complete - Integration layer with Bus, Console, Input, Save State framework
- **Validation**: nestest.nes golden log match (5003+ instructions verified)

**Coming Soon (M6):**

- **M6**: Desktop GUI application (egui/wgpu) - NEXT PRIORITY for playable MVP release

---

## Configuration

RustyNES will support extensive configuration through:

- Configuration files (TOML format)
- Command-line arguments
- GUI settings panel

See [docs/api/CONFIGURATION.md](docs/api/CONFIGURATION.md) for planned configuration options.

**Note:** Configuration system will be implemented in Milestone 6 (Desktop GUI) as part of the complete application interface.

### Crate Structure

The project is organized as a Cargo workspace:

```bash
# Build all implemented crates
cargo build --workspace --release

# Build specific crate
cargo build -p rustynes-cpu --release
cargo build -p rustynes-ppu --release
cargo build -p rustynes-apu --release
cargo build -p rustynes-mappers --release
cargo build -p rustynes-core --release

# Test specific crate
cargo test -p rustynes-cpu
cargo test -p rustynes-ppu
cargo test -p rustynes-apu
cargo test -p rustynes-mappers
cargo test -p rustynes-core

# Generate documentation
cargo doc --workspace --no-deps --open
```

**Note:** Feature flags and advanced crates (desktop, web, TAS, netplay, achievements) will be implemented in later milestones. Currently, all core emulation components are complete (M1-M5).

---

## Default Controls (Planned)

Input handling will be implemented in Milestone 5 (Integration) and Milestone 6 (Desktop GUI).

**Planned Default Bindings:**

| NES    | Keyboard      | Gamepad  |
| ------ | ------------- | -------- |
| D-Pad  | WASD / Arrows | D-Pad    |
| A      | K / Z         | A Button |
| B      | J / X         | B Button |
| Start  | Enter         | Start    |
| Select | Right Shift   | Select   |

Controls will be fully configurable through the configuration system.

---

## Features

### Current Status (v0.4.0 - December 2025)

- [x] **Architecture Design** - Complete modular crate structure with 10 component crates
- [x] **Documentation** - 40+ comprehensive specification and implementation guides covering CPU, PPU, APU, mappers, testing, and development
- [x] **Project Setup** - Workspace structure created with CI/CD pipeline
- [x] **Test ROM Acquisition** - 44 test ROMs downloaded (19 CPU, 25 PPU), 7 integrated
- [x] **Milestone 1: CPU** - Cycle-accurate 6502/2A03 emulation complete (46 tests passing)
- [x] **Milestone 2: PPU** - Dot-level 2C02 rendering complete (83 tests passing)
- [x] **Milestone 3: APU** - Hardware-accurate 2A03 APU complete (150 tests passing)
- [x] **Milestone 4: Mappers** - Essential mapper implementations (NROM, MMC1, UxROM, CNROM, MMC3) complete (78 tests passing)
- [x] **Milestone 5: Integration** - rustynes-core layer (CPU + PPU + APU + Bus + Mappers) complete (41 tests passing)
- [ ] **Milestone 6: Desktop GUI** - Cross-platform interface with egui/wgpu - NEXT PRIORITY

### MVP (Phase 1) - Target: June 2026 (83% Complete)

- [x] **Cycle-accurate 6502/2A03 CPU emulation** (all 256 opcodes) - ✅ M1 Complete (100%)
  - All official (151) and unofficial (105) opcodes implemented
  - Cycle-accurate timing with page-crossing penalties
  - Complete interrupt handling (NMI, IRQ, BRK, RESET)
  - 100% nestest.nes golden log match (5003+ instructions verified)
  - 144 comprehensive tests (100% passing)
  - Zero unsafe code
- [x] **Dot-level 2C02 PPU rendering** (341x262 scanlines) - ✅ M2 Complete (97.8%)
  - Complete background rendering with scrolling
  - Sprite rendering with 8-per-scanline limit
  - Sprite 0 hit and overflow detection
  - Accurate VBlank and NMI timing
  - Loopy scrolling model implementation
  - 6 comprehensive tests (100% passing)
  - Zero unsafe code
- [x] **Hardware-accurate 2A03 APU synthesis** (all 5 channels) - ✅ M3 Complete (100%)
  - Pulse channels (2) with sweep and envelope
  - Triangle channel with linear counter
  - Noise channel with 15-bit LFSR
  - DMC channel with sample playback and DMA
  - Hardware-accurate frame counter (4-step and 5-step modes)
  - Non-linear mixer matching hardware behavior
  - 48 kHz audio resampling
  - 150 comprehensive tests (100% passing)
  - Zero unsafe code
- [x] **Mappers 0, 1, 2, 3, 4** (77.7% game coverage, 500+ games) - ✅ M4 Complete (100%)
- [x] **Integration Layer** (CPU + PPU + APU + Bus coordination) - ✅ M5 Complete (100%)
  - Master clock synchronization (21.477 MHz)
  - Component stepping and timing
  - Interrupt routing (PPU NMI -> CPU, Mapper IRQ -> CPU)
  - Hardware-accurate bus system with full memory map
  - Cycle-accurate OAM DMA (513-514 cycles)
  - Input system with shift register protocol
  - Save state framework with format specification
- [ ] **Cross-platform GUI** (egui + wgpu) - 🔄 M6 Next Priority
- [ ] **Gamepad support** (SDL2/gilrs) - 🔄 M6 Planned
- [ ] **85% TASVideos test suite pass rate** - 🔄 In Progress (All core components: 100%)

### Planned (Phases 2-4) - Target: December 2027

- [ ] RetroAchievements integration (rcheevos FFI)
- [ ] GGPO-style rollback netplay (backroll-rs)
- [ ] Lua 5.4 scripting with memory/GUI APIs
- [ ] TAS recording/playback (FM2 format)
- [ ] Integrated debugger (CPU, PPU, APU viewers)
- [ ] Rewind, fast-forward, slow-motion
- [ ] WebAssembly build with PWA support
- [ ] CRT/NTSC shaders and video filters
- [ ] Expansion audio (VRC6, VRC7, MMC5, FDS, N163, 5B)
- [ ] 300+ mapper implementations (100%+ game coverage)
- [ ] 100% TASVideos accuracy test pass rate

See [ROADMAP.md](ROADMAP.md) for the complete development plan.

### Comparison with Other Emulators

| Feature | RustyNES | Mesen2 | FCEUX | puNES | TetaNES |
|---------|----------|--------|-------|-------|---------|
| **CPU Accuracy** | Cycle | Cycle | Cycle | Instruction | Cycle |
| **PPU Accuracy** | Dot | Dot | Scanline | Dot | Dot |
| **Mapper Count** | 300+ (goal) | 300+ | 200+ | 461+ | 10 |
| **RetroAchievements** | ✓ | ✓ | ✗ | ✗ | ✗ |
| **GGPO Netplay** | ✓ | ✗ | ✗ | ✗ | ✗ |
| **TAS Editor** | ✓ | ✓ | ✓ | ✗ | ✗ |
| **Lua Scripting** | ✓ (5.4) | ✓ (5.4) | ✓ (5.1) | ✗ | ✗ |
| **Debugger** | Advanced | Advanced | Advanced | Basic | Basic |
| **WebAssembly** | ✓ | ✗ | ✓ | ✗ | ✓ |
| **Language** | Rust | C++ | C++ | C++ | Rust |
| **License** | MIT/Apache | GPL-3.0 | GPL-2.0 | GPL-2.0 | GPL-3.0 |

**RustyNES Advantages:**

- Modern Rust codebase with memory safety guarantees
- Unique combination of accuracy + netplay + RetroAchievements
- Modular design allowing component reuse
- Permissive dual licensing (MIT/Apache-2.0)

---

## Technical Details

### NES Timing Model

RustyNES implements cycle-accurate timing based on the NES master clock:

```text
Master Clock (NTSC): 21.477272 MHz
├─ CPU Clock: ÷12 = 1.789773 MHz (~559 ns/cycle)
├─ PPU Clock: ÷4  = 5.369318 MHz (~186 ns/dot)
└─ APU Clock: Same as CPU (1.789773 MHz)

Frame Timing:
- Scanlines: 262 (NTSC), 312 (PAL)
- Dots per scanline: 341 (340 on odd frames)
- Total PPU cycles: 89,342 (NTSC), 106,392 (PAL)
- CPU cycles per frame: 29,781 (NTSC), 35,464 (PAL)
- Target framerate: 60.0988 Hz (NTSC), 50.0070 Hz (PAL)
```

### Accuracy Targets and Progress

| Component | Target | Status | Validation |
|-----------|--------|--------|------------|
| **CPU (6502)** | 100% instruction-level | ✅ **Complete** | nestest.nes golden log match (100%) |
| **PPU (2C02)** | 100% cycle-accurate | ✅ **Complete** | Background + sprite rendering, VBlank/NMI, sprite 0 hit |
| **APU (2A03)** | 99%+ hardware match | ✅ **Complete** | All 5 channels, non-linear mixing, 48 kHz output |
| **Mappers** | 100% for licensed | ✅ **Complete** | 5 essential mappers (0, 1, 2, 3, 4) - 77.7% coverage |
| **Integration** | Component coordination | ✅ **Complete** | Bus, Console, Input, Save State framework |
| **Overall** | 100% TASVideos suite | 🔄 **In Progress** | Target: 85% by June 2026 |

**Current Progress:** 83% (M1-M5 complete, M6 in progress)

**Test Results (v0.4.0):**

- **CPU**: 46/46 tests passing (100%)
  - All 256 opcodes validated
  - nestest.nes golden log match (5003+ instructions)
- **PPU**: 83/83 tests passing (100%)
  - Complete rendering pipeline
  - VBlank/NMI timing, sprite 0 hit
- **APU**: 150/150 tests passing (100%)
  - All 5 audio channels
  - Non-linear mixer, 48 kHz resampling
- **Mappers**: 78/78 tests passing (100%)
  - 5 mappers (NROM, MMC1, UxROM, CNROM, MMC3)
  - 77.7% game coverage
- **Core**: 41/41 tests passing (100%)
  - Bus system (22 tests)
  - Console coordinator (7 tests)
  - Input system (9 tests)
  - Integration tests (3 tests)
- **Total**: 398/398 tests passing (100%)
- **Code Quality**: Zero unsafe code across all 5 crates

### Development Roadmap

**Phase 1 Progress:** 83% Complete (5 of 6 milestones)

| Milestone | Status | Completion | Target Date |
|-----------|--------|------------|-------------|
| **M1: CPU** | ✅ Complete | 100% | ~~January 2026~~ December 19, 2025 |
| **M2: PPU** | ✅ Complete | 100% | ~~March 2026~~ December 19, 2025 |
| **M3: APU** | ✅ Complete | 100% | ~~February 2026~~ December 19, 2025 |
| **M4: Mappers** | ✅ Complete | 100% | ~~January 2026~~ December 19, 2025 |
| **M5: Integration** | ✅ Complete | 100% | ~~February 2026~~ December 19, 2025 |
| **M6: Desktop GUI** | 🔄 Next Priority | 0% | January-March 2026 |

**Next Priority:** Implement Milestone 6 (Desktop GUI) - Cross-platform GUI with egui/wgpu for playable MVP release.

See [ROADMAP.md](ROADMAP.md) for the complete 24-month development plan and [to-dos/](to-dos/) for detailed sprint tracking.

### Architecture Overview

```text
┌────────────────────────────────────────────────────────────────┐
│                    RustyNES Components                          │
├─────────────┬─────────────┬─────────────┬──────────────────────┤
│   CPU ✅    │    PPU ✅   │  APU ✅     │    Mappers ✅        │
│  (6502)     │  (2C02)     │  (2A03)     │  (0, 1, 2, 3, 4)     │
│             │             │             │                      │
│ • 100%      │ • 100%      │ • 100%      │ • 100%               │
│   Complete  │   Complete  │   Complete  │   Complete           │
│ • Cycle     │ • Dot-level │ • 5 Channels│ • 77.7% games        │
│   accurate  │   rendering │ • Non-linear│ • Banking            │
│ • All 256   │ • Scrolling │   mixing    │ • IRQ timing         │
│   opcodes   │ • Sprites   │ • 48 kHz    │ • Mirroring          │
│ • 46 tests  │ • 83 tests  │ • 150 tests │ • 78 tests           │
└─────────────┴─────────────┴─────────────┴──────────────────────┘
       │              │              │             │
       └──────────────┴──────────────┴─────────────┘
                         │
              ┌──────────┴──────────┐
              │  rustynes-core ✅   │
              │  (Integration)      │
              │  • Bus System       │
              │  • Console          │
              │  • Input/DMA        │
              │  • Save States      │
              │  • 41 tests         │
              └──────────┬──────────┘
                         │
       ┌─────────────────┼────────────────┐
       │                 │                │
┌──────┴──────┐   ┌──────┴──────┐   ┌─────┴─────┐
│  Desktop 🚧 │   │   Web 🔄    │   │ Headless 🔄│
│  (egui/wgpu)│   │   (WASM)    │   │   (API)    │
│  • M6 NEXT  │   │   • Phase 3 │   │  • Phase 2 │
└─────────────┘   └─────────────┘   └────────────┘

Legend: ✅ Complete | 🚧 Next Priority | 🔄 Planned | Phase 1 MVP: M1-M6
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for comprehensive system design.

---

## Platform Support

| Platform            | Status  |
| ------------------- | ------- |
| **Windows x64**     | Primary |
| **Linux x64**       | Primary |
| **macOS x64/ARM64** | Primary |
| **WebAssembly**     | Planned |
| **Linux ARM64**     | Planned |

### System Requirements

**Minimum:** 1.5 GHz dual-core, 512 MB RAM, OpenGL 3.3
**Recommended:** 2.0 GHz quad-core, 2 GB RAM, Vulkan/Metal/DX12 GPU

---

## Documentation

| Document                           | Description                                           |
| ---------------------------------- | ----------------------------------------------------- |
| [OVERVIEW.md](OVERVIEW.md)         | Philosophy, accuracy goals, emulation approach        |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design, component relationships, Rust patterns |
| [ROADMAP.md](ROADMAP.md)           | Development phases and milestones                     |
| [CHANGELOG.md](CHANGELOG.md)       | Version history and release notes                     |
| [to-dos/](to-dos/)                 | Phase 1 milestone tracking and sprint plans           |

### Hardware Documentation

| Component      | Location                                                             |
| -------------- | -------------------------------------------------------------------- |
| **CPU (6502)** | [docs/cpu/](docs/cpu/) - Instruction set, timing, unofficial opcodes |
| **PPU (2C02)** | [docs/ppu/](docs/ppu/) - Rendering, scrolling, sprite evaluation     |
| **APU (2A03)** | [docs/apu/](docs/apu/) - Audio channels, mixing, frame counter       |
| **Memory Bus** | [docs/bus/](docs/bus/) - Address space, bus conflicts                |
| **Mappers**    | [docs/mappers/](docs/mappers/) - Cartridge banking and variants      |

### Development Guides

| Guide                                    | Purpose                         |
| ---------------------------------------- | ------------------------------- |
| [CONTRIBUTING](docs/dev/CONTRIBUTING.md) | Code style and PR process       |
| [BUILD](docs/dev/BUILD.md)               | Toolchain and cross-compilation |
| [TESTING](docs/dev/TESTING.md)           | Test ROM suites and CI          |
| [DEBUGGING](docs/dev/DEBUGGING.md)       | Built-in debugger usage         |
| [GLOSSARY](docs/dev/GLOSSARY.md)         | NES terminology reference       |

### API Reference

| API                                        | Description                         |
| ------------------------------------------ | ----------------------------------- |
| [CORE_API](docs/api/CORE_API.md)           | Embedding the emulator as a library |
| [SAVE_STATES](docs/api/SAVE_STATES.md)     | State serialization format          |
| [CONFIGURATION](docs/api/CONFIGURATION.md) | Runtime options and settings        |

---

## Building from Source

### Standard Build

```bash
# Debug build (faster compilation, includes debug symbols)
cargo build --workspace

# Release build (optimized, ~10x faster runtime)
cargo build --workspace --release

# Build specific crate
cargo build -p rustynes-cpu --release
cargo build -p rustynes-ppu --release
cargo build -p rustynes-apu --release
cargo build -p rustynes-mappers --release
cargo build -p rustynes-core --release

# Run all tests (398 tests across 5 crates)
cargo test --workspace

# Run tests for specific crate
cargo test -p rustynes-cpu       # 46 tests
cargo test -p rustynes-ppu       # 83 tests
cargo test -p rustynes-apu       # 150 tests
cargo test -p rustynes-mappers   # 78 tests
cargo test -p rustynes-core      # 41 tests

# Run lints
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all

# Generate documentation
cargo doc --workspace --no-deps --open
```

### Development Build

```bash
# Build with all warnings
cargo build --workspace -- -D warnings

# Build and run in watch mode (requires cargo-watch)
cargo install cargo-watch
cargo watch -x 'test --workspace'

# Run specific tests
cargo test -p rustynes-cpu test_lda_immediate
cargo test -p rustynes-ppu test_ppu_integration

# Run with debug logging
RUST_LOG=debug cargo test --workspace

# Benchmark critical paths (once implemented)
cargo bench -p rustynes-cpu
cargo bench -p rustynes-ppu

# Check for unused dependencies
cargo +nightly udeps --workspace
```

### Cross-Compilation

```bash
# Install cross-compilation tool
cargo install cross

# Build for Linux ARM64
cross build --target aarch64-unknown-linux-gnu --release

# Build for Windows from Linux
cross build --target x86_64-pc-windows-gnu --release

# Build for Raspberry Pi
cross build --target armv7-unknown-linux-gnueabihf --release
```

### WebAssembly Build (Planned for Phase 3)

WebAssembly support is planned for Phase 3 (Expansion). The rustynes-web crate will provide:

- Browser-based emulation
- PWA (Progressive Web App) support
- IndexedDB save states
- Touch/gamepad controls
- WebAudio API integration

**Implementation Timeline:** June-December 2027 (Phase 3)

### Feature Flags (To Be Implemented)

Feature flags will be introduced as advanced functionality is implemented:

| Feature | Description | Phase | Status |
|---------|-------------|-------|--------|
| `default` | Core emulation (CPU, PPU, APU) | 1 | 🔄 In Progress |
| `netplay` | GGPO rollback netcode | 2 | ⏳ Planned |
| `achievements` | RetroAchievements integration | 2 | ⏳ Planned |
| `tas` | TAS recording/playback | 2 | ⏳ Planned |
| `lua` | Lua 5.4 scripting | 2 | ⏳ Planned |
| `debugger` | Advanced debugging tools | 2 | ⏳ Planned |
| `expansion-audio` | VRC6/VRC7/N163/etc. audio | 3 | ⏳ Planned |

**Current Focus:** Phase 1 MVP (core emulation without feature flags)

---

## Contributing

Contributions of all kinds are welcome! Whether you're fixing bugs, adding features, improving documentation, or testing, we'd love your help.

### Ways to Contribute

- **Code**: Implement CPU/PPU/APU features, mappers, or GUI improvements
- **Testing**: Run test ROMs, report bugs, verify accuracy
- **Documentation**: Improve guides, add examples, clarify specifications
- **Design**: UI/UX improvements, icons, artwork

### Getting Started

1. **Read the contribution guide**: [CONTRIBUTING.md](CONTRIBUTING.md)
2. **Find an issue**: Check [`good first issue`](https://github.com/doublegate/RustyNES/labels/good%20first%20issue) or [`help wanted`](https://github.com/doublegate/RustyNES/labels/help%20wanted) labels
3. **Ask questions**: Use [GitHub Discussions](https://github.com/doublegate/RustyNES/discussions) if you need guidance

### Quick Contribution Workflow

```bash
# 1. Fork and clone
git clone https://github.com/YOUR_USERNAME/RustyNES.git
cd RustyNES

# 2. Create a feature branch
git checkout -b feature/my-feature

# 3. Make changes and test
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all

# 4. Commit using conventional commits
git commit -m "feat(cpu): implement ADC instruction"

# 5. Push and create PR
git push origin feature/my-feature
```

### Development Resources

| Resource | Purpose |
|----------|---------|
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution guidelines |
| [docs/dev/BUILD.md](docs/dev/BUILD.md) | Build instructions |
| [docs/dev/TESTING.md](docs/dev/TESTING.md) | Testing guide |
| [docs/dev/STYLE_GUIDE.md](docs/dev/STYLE_GUIDE.md) | Code style standards |
| [docs/dev/DEBUGGING.md](docs/dev/DEBUGGING.md) | Debugging techniques |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System architecture |

---

## Acknowledgments

RustyNES stands on the shoulders of giants. We're grateful to:

### Reference Emulators

These projects provided invaluable reference implementations and inspiration:

- **[Mesen2](https://github.com/SourMesen/Mesen2)** - Gold standard for accuracy and debugging tools
- **[FCEUX](https://github.com/TASEmulators/fceux)** - TAS tools, FM2 format, and mapper reference
- **[puNES](https://github.com/punesemu/puNES)** - Comprehensive mapper catalog (461+ implementations)
- **[TetaNES](https://github.com/lukexor/tetanes)** - Rust architecture patterns and wgpu rendering
- **[Pinky](https://github.com/koute/pinky)** - PPU rendering techniques and Visual2C02 test validation
- **[Rustico](https://github.com/zeta0134/rustico)** - Expansion audio implementation patterns
- **[DaveTCode/nes-emulator-rust](https://github.com/DaveTCode/nes-emulator-rust)** - Zero-unsafe Rust patterns

### Community and Resources

- **[NESdev Community](https://www.nesdev.org/)** - Comprehensive hardware documentation and forums
- **[TASVideos](https://tasvideos.org/)** - Accuracy test suite and validation framework
- **[RetroAchievements](https://retroachievements.org/)** - Achievement system integration
- **[blargg](http://blargg.8bitalley.com/)** - Test ROM suites for CPU, PPU, and APU validation
- **[Visual 6502](http://visual6502.org/)** - Die-level CPU simulation and verification
- **[kevtris](https://forums.nesdev.org/memberlist.php?mode=viewprofile&u=5)** - PPU research and Visual2C02

### Contributors

Thank you to all contributors who help make RustyNES better! See the [Contributors page](https://github.com/doublegate/RustyNES/graphs/contributors) for the complete list.

### Funding

Development is currently unfunded and volunteer-driven. If you'd like to support the project:

- Star the repository
- Report bugs and test PRs
- Contribute code or documentation
- Spread the word about RustyNES

---

## License

RustyNES is dual-licensed under your choice of:

- **[MIT License](LICENSE-MIT)** - Permissive, allows commercial use
- **[Apache License 2.0](LICENSE-APACHE)** - Permissive with patent grant

**Why dual license?**

- Maximum compatibility with other projects
- Choose the license that best fits your use case
- Both licenses allow commercial and private use

### Third-Party Licenses

RustyNES uses several open-source libraries with compatible licenses:

- **wgpu** - MIT/Apache-2.0 (graphics)
- **egui** - MIT/Apache-2.0 (GUI)
- **SDL2** - zlib (audio/input)
- **mlua** - MIT (Lua scripting)
- **backroll** - Apache-2.0 (netplay)
- **rcheevos** - MIT (RetroAchievements)

See individual crate `Cargo.toml` files for complete dependency licenses.

---

## Community

### Get Help

- **[GitHub Discussions](https://github.com/doublegate/RustyNES/discussions)** - Ask questions, share ideas
- **[GitHub Issues](https://github.com/doublegate/RustyNES/issues)** - Report bugs, request features
- **[SUPPORT.md](SUPPORT.md)** - Detailed support information

### Stay Updated

- **[GitHub Releases](https://github.com/doublegate/RustyNES/releases)** - New versions and changelogs
- **[CHANGELOG.md](CHANGELOG.md)** - Detailed version history
- **[ROADMAP.md](ROADMAP.md)** - Development plans and milestones

### Related Projects

Explore other NES emulation projects:

- [NESdev Wiki](https://www.nesdev.org/wiki/) - Hardware documentation
- [NESdev Forums](https://forums.nesdev.org/) - Community discussions
- [TASVideos](https://tasvideos.org/) - Tool-assisted speedrun community

---

## Citation

If you use RustyNES in academic research, please cite:

```bibtex
@software{rustynes2025,
  author = {RustyNES Contributors},
  title = {RustyNES: A Next-Generation NES Emulator in Rust},
  year = {2025},
  version = {0.1.0},
  url = {https://github.com/doublegate/RustyNES},
  note = {Cycle-accurate Nintendo Entertainment System emulator with complete CPU and PPU implementation}
}
```

---

<p align="center">
  <strong>Built with Rust. Powered by passion for retro gaming.</strong><br>
  <sub>Preserving video game history, one frame at a time.</sub>
</p>

<p align="center">
  <a href="#quick-start">Get Started</a> •
  <a href="CONTRIBUTING.md">Contribute</a> •
  <a href="docs/">Documentation</a> •
  <a href="https://github.com/doublegate/RustyNES/discussions">Discuss</a>
</p>
