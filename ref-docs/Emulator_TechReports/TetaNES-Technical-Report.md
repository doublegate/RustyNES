# TetaNES Technical Report

**Repository:** [github.com/lukexor/tetanes](https://github.com/lukexor/tetanes)
**Author:** Luke Petherbridge
**Language:** Rust
**License:** MIT OR Apache-2.0
**Stars:** 219+ | **Commits:** 1,281+ | **Status:** Actively Maintained

---

## Executive Summary

TetaNES is the most feature-complete and production-ready Rust NES emulator available. It demonstrates best practices for Rust emulator architecture with a clean separation between the emulation core (`tetanes-core`) and the UI binary (`tetanes`). The project targets cross-platform compatibility including WebAssembly, making it an excellent reference for modern Rust emulator development.

---

## Architecture Overview

### Crate Organization

```
tetanes/
├── tetanes/           # Main UI binary (wgpu + egui)
├── tetanes-core/      # Emulation library (minimal dependencies)
└── tetanes-utils/     # Shared utilities
```

**Key Design Decision:** The `tetanes-core` crate is designed as a standalone library that can be used by other projects. It publishes to crates.io and has documentation on docs.rs.

### Dependency Philosophy

- **Core:** Minimal dependencies - `bincode`, `bitflags`, `serde`, `tracing`, `rand`
- **UI:** Modern graphics stack with `wgpu` and `egui`
- **WebAssembly:** Uses `trunk` for WASM compilation targeting `wasm32-unknown-unknown`

### Rust Edition & Toolchain

- **Edition:** 2024 (cutting-edge)
- **Minimum Rust Version:** 1.85.0 (nightly features)
- **Profile Optimizations:** LTO enabled, codegen-units=1, panic=abort for release builds

---

## Emulation Accuracy

### CPU (6502)

- Full official opcode implementation
- **Unofficial opcodes supported**
- Cycle-accurate execution
- Proper dummy read/write handling

### PPU (2C02)

- Cycle-accurate rendering (3 PPU cycles per CPU cycle)
- NTSC, PAL, and Dendy region support
- NTSC filters and CRT shaders
- PPU warmup emulation (optional)
- Randomized power-on RAM state (hardware-accurate)

### APU

- Complete 2A03 audio implementation
- All five channels: Pulse 1, Pulse 2, Triangle, Noise, DMC
- Individual channel toggle support

### Mappers

| Mapper | Name | Coverage |
|--------|------|----------|
| 000 | NROM | ~10% of games |
| 001 | MMC1 | ~28% of games |
| 002 | UxROM | ~11% of games |
| 003 | CNROM | ~6% of games |
| 004 | MMC3/MMC6 | ~24% of games |
| 005 | MMC5 | <0.01% |
| 007 | AxROM | ~3% of games |
| + 20 more | Various | ~10% |

**Total Coverage:** ~92.2% of licensed NES library (30+ mappers)

---

## Features

### Core Emulation
- [x] iNES and NES 2.0 ROM format support
- [x] Save states (multiple slots)
- [x] Battery-backed RAM saves
- [x] Game Genie codes
- [x] Visual and instant rewind
- [x] Gameplay recording and playback
- [x] Audio recording

### User Interface
- [x] Cross-platform (Linux, macOS, Windows, Web)
- [x] Gamepad support (up to 4 players)
- [x] Zapper (Light Gun) support
- [x] Fullscreen mode
- [x] PPU Debugger
- [x] Runtime performance statistics
- [x] Configurable keybindings

### Developer Features
- [x] Headless mode for testing/AI
- [x] Profiling support (puffin)
- [x] Benchmark suite (criterion)
- [x] Comprehensive test suite with test ROMs

---

## Code Quality Indicators

### Linting Configuration

```toml
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
missing_const_for_fn = "warn"
print_literal = "warn"

[workspace.lints.rust]
future_incompatible = "warn"
rust_2018_idioms = "warn"
rust_2021_compatibility = "warn"
```

### Testing Strategy

- Unit tests integrated
- Integration tests using test ROMs in `tetanes-core/test_roms/`
- nestest.nes verification at $C000
- Blargg's test suite support

---

## Code Metrics & Structure

### Lines of Code Breakdown

| Component | Lines | Description |
|-----------|-------|-------------|
| **Total Project** | 37,873 | Complete Rust codebase |
| PPU | 1,708 | Picture Processing Unit core |
| CPU Instructions | 1,543 | Opcode implementations |
| CPU Core | 1,119 | Processor logic |
| MMC5 Mapper | 1,108 | Most complex mapper |
| Control Deck | 1,076 | System orchestration |
| Cartridge | 978 | ROM/RAM management |
| Bandai FCG | 896 | Advanced mapper |
| APU | 681 | Audio Processing Unit |
| Memory Bus | 545 | Address space management |

**Source Organization:**
- 113 Rust source files
- 27 mapper implementations (24 numbered + 3 support files)
- 37 unit tests with integration test ROMs

### CPU Implementation Details

**Instruction Set Coverage:**
```rust
pub const INSTRUCTIONS: [Instr; 256] = [
    // Complete 16x16 opcode matrix matching 6502 datasheet
    // Includes all official and unofficial opcodes
];
```

- Full 256-opcode lookup table
- Unofficial opcodes: SLO, RLA, SRE, RRA, SAX, LAX, DCP, ISB, ANC, ALR, ARR, XAA, AXS, AHX, SHY, SHX, TAS, LAS
- Cycle-accurate execution with dummy read/write handling
- Address modes: IMM, ZP0, ZPX, ZPY, ABS, ABX, ABY, IND, IDX, IDY, REL, ACC, IMP

**File:** `/tetanes-core/src/cpu/instr.rs` (1,543 lines)

### PPU Implementation Details

**Cycle-Accurate Rendering Engine:**
```rust
pub cycle: u32,           // (0, 340) cycles per scanline
pub scanline: u32,        // (0, 261) NTSC or (0, 311) PAL/Dendy
pub master_clock: u64,    // Clock divider for accurate timing
```

**Features:**
- 3 PPU cycles per CPU cycle (5.37 MHz PPU clock)
- 341 PPU cycles per scanline
- 262 scanlines per frame (NTSC) / 312 (PAL)
- Sprite evaluation with 8-sprite-per-scanline limit
- Sprite zero hit detection
- PPU warmup emulation (power-on state)
- Accurate VBlank timing with `prevent_vbl` flag

**File:** `/tetanes-core/src/ppu.rs` (1,708 lines)

### Mapper Architecture

Uses `enum_dispatch` for zero-cost abstraction:

```rust
#[enum_dispatch(Mapper)]
pub enum MapperImpl {
    M000(M000),  // NROM
    M001(M001),  // MMC1
    M004(M004),  // MMC3/MMC6
    M005(M005),  // MMC5
    // ... 24 total mappers
}
```

**Benefits:**
- Eliminates dynamic dispatch overhead
- Maintains clean trait-based API
- Compiler optimizes to direct function calls

---

## Testing & Quality Assurance

### Test Suite

- **Unit Tests:** 37 test functions across components
- **Integration Tests:** nestest.nes verification at $C000
- **Test ROMs:** Blargg's test suite in `tetanes-core/test_roms/`
- **Test Runner:** `cargo-nextest` for parallel execution

### CI/CD Pipeline

**5 GitHub Actions Workflows:**

1. **ci.yml** - Multi-dimensional testing matrix
   - Platforms: macOS, Ubuntu, Windows
   - Toolchains: nightly, stable, 1.85
   - Separate jobs for web (WASM) and native targets
   - Documentation generation with `-D warnings`

2. **cd.yml** - Continuous deployment
3. **security.yml** - Dependency auditing
4. **outdated.yml** - Dependency freshness checks
5. **release-pr.yml** - Automated release PRs

**Linting Configuration:**
```toml
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
missing_const_for_fn = "warn"
print_literal = "warn"

[workspace.lints.rust]
future_incompatible = "warn"
rust_2018_idioms = "warn"
rust_2021_compatibility = "warn"
```

**Environment Variables (CI):**
- `CARGO_INCREMENTAL=0` - Faster clean builds
- `CARGO_PROFILE_DEV_DEBUG=0` - Smaller cache size
- `RUST_BACKTRACE=1` - Debug information

---

## Performance Characteristics

### Optimization Techniques

**Release Profile:**
```toml
[profile.release]
codegen-units = 1      # Maximum optimization across crates
lto = true             # Link-time optimization
panic = "abort"        # Smaller binary, no unwind tables
```

**Additional Profiles:**
- `flamegraph` - Debug symbols enabled for profiling
- `dist` - Release with stripped symbols
- `dev` - opt-level = 1 for playable framerates during development

### Benchmarking

- **Framework:** Criterion with HTML reports
- **Profiling:** puffin integration (optional feature)
- **Benchmark:** `clock_frame` - measures full frame emulation timing

---

## Technical Highlights for Reference

### 1. Trait-Based Mapper System

TetaNES uses Rust's enum_dispatch for efficient mapper polymorphism, avoiding runtime overhead of dynamic dispatch while maintaining clean abstraction. The `Mapper` trait defines the interface, and enum_dispatch generates direct function calls at compile time.

### 2. WebAssembly Architecture

The WASM build uses `trunk` with `wasm32-unknown-unknown` target, enabling browser deployment without emscripten. Web storage is handled via `web-sys` Storage API for save states and battery-backed RAM.

**Crate Type:**
```toml
[lib]
crate-type = ["cdylib", "rlib"]  # Both C-compatible and Rust library
```

### 3. Frame Timing

Implements proper NTSC timing:
- CPU: 1.789773 MHz
- PPU: 5.37 MHz (3x CPU)
- 341 PPU cycles per scanline
- 262 scanlines per frame (NTSC)
- 312 scanlines per frame (PAL/Dendy)

### 4. Serialization

Uses `bincode` v2.0 with `serde` for efficient save state serialization, with `flate2` compression for reduced file sizes. The entire emulator state is serializable for save/load functionality.

### 5. Minimal Core Dependencies

**tetanes-core dependencies:**
- bincode, bitflags, cfg-if, dirs, enum_dispatch
- flate2, rand, serde, thiserror, tracing
- Platform-specific: web-sys, web-time (WASM)

No heavy dependencies in the core ensures library reusability.

---

## Accuracy Analysis

### Test ROM Results

- **nestest.nes:** Verified at $C000 (automated test start)
- **Blargg's Test Suite:** Supported in test_roms directory
- **Mapper Coverage:** 92.2% of licensed NES library (30+ mappers)

### Known Accuracy Features

- Cycle-accurate CPU execution
- Cycle-accurate PPU rendering (3:1 PPU:CPU ratio)
- Unofficial opcode support (required for some games)
- Proper dummy read/write cycles
- PPU power-on state emulation
- Randomized RAM state on power-up (hardware-accurate)

### Edge Cases Handled

- Sprite zero hit detection
- Sprite overflow flag
- PPU address wrapping
- VBlank suppression (prevent_vbl flag)
- Open bus behavior
- MMC5 advanced features (split-screen, extended RAM)

---

## Community & Ecosystem

### Project Metrics

- **GitHub Stars:** 219+
- **Total Commits:** 1,281+
- **Primary Author:** Luke Petherbridge (solo project)
- **Status:** Actively maintained
- **License:** Dual MIT OR Apache-2.0

### Distribution Channels

1. **crates.io** - Published Rust crate
2. **docs.rs** - API documentation
3. **GitHub Releases** - Binary releases
4. **Homebrew** - macOS package manager
5. **Web Demo** - Browser-based version

### Developer Resources

- **Website:** [lukeworks.tech/tetanes](https://lukeworks.tech/tetanes)
- **Web Demo:** [lukeworks.tech/tetanes-web](https://lukeworks.tech/tetanes-web)
- **Blog Series:** Detailed development blog at lukeworks.tech covering:
  - "Programming an NES Emulator from Scratch in Rust"
  - "NES Emulation in Rust: Designs and Frustrations"
- **API Docs:** Published on docs.rs with private items documented
- **NESdev Forum:** Active community engagement

### Community Reception

Described as "fairly accurate emulator that can play most NES titles" with focus on:
- Rust best practices demonstration
- Performance and memory safety
- Fearless concurrency features
- Clean, readable low-level code

---

## Recommendations for New Projects

1. **Adopt the crate separation pattern** - `*-core` for emulation, main binary for UI
2. **Use wgpu** for cross-platform graphics that works on web and native
3. **Consider egui** for rapid UI development with immediate-mode paradigm
4. **Target wasm32-unknown-unknown** for browser deployment
5. **Use trunk** for simplified WASM build tooling
6. **Enable LTO and single codegen-unit** for release performance
7. **Use enum_dispatch** for zero-cost polymorphism in mapper systems
8. **Implement headless mode** for testing and AI integration
9. **Use cargo-nextest** for faster test execution
10. **Profile with puffin** for frame-by-frame performance analysis

---

## Comparison with Other Rust Emulators

### Strengths

- Most complete feature set (rewind, recording, Game Genie, Zapper)
- Best WebAssembly support with live web demo
- Production-ready with multiple distribution channels
- Excellent code organization (3-crate workspace)
- Comprehensive CI/CD pipeline
- Strong documentation and developer blog

### Positioning

TetaNES represents the most mature Rust NES emulator available, balancing accuracy with usability. It serves as both a fully functional emulator and an educational resource for Rust systems programming.

---

## Version Information

- **Current Version:** 0.12.2
- **Rust Edition:** 2024 (cutting-edge)
- **Minimum Rust:** 1.85.0
- **Last Significant Update:** Active development (2024)
- **Release Channels:** GitHub Releases, crates.io, Homebrew

---

*Report Generated: December 2024*
*Enhanced: December 2024 with deep code analysis and community research*
