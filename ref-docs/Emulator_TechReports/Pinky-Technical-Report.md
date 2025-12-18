# Pinky Technical Report

**Repository:** [github.com/koute/pinky](https://github.com/koute/pinky)
**Author:** Koute (Jan Bujak)
**Language:** Rust
**License:** MIT/Apache-2.0
**Stars:** 800+ | **Status:** Maintenance Mode

---

## Executive Summary

Pinky is a research-focused NES emulator notable for its transistor-level PPU testsuite generated from Visual2C02 simulation data. The project provides reusable components including a standalone MOS 6502 emulator crate, making it valuable for projects requiring CPU-only emulation. While mapper support is limited, the accuracy-focused approach and unique testing methodology make it an excellent reference for PPU implementation.

---

## Architecture Overview

### Crate Organization

```
pinky/
├── emumisc/          # Shared emulation utilities
├── mos6502/          # Standalone 6502 emulator (reusable)
├── nes/              # Core NES emulator
├── nes-testsuite/    # ROM-based test framework
├── pinky-devui/      # SDL2 development frontend
├── pinky-libretro/   # Libretro core
├── pinky-web/        # WebAssembly build
└── rp2c02-testsuite/ # PPU transistor-level tests
```

**Key Design Decision:** The `mos6502` crate is completely standalone and can be used independently for other 6502-based system emulation (Apple II, Commodore 64, Atari, etc.).

### Dependency Philosophy

- **Minimal Dependencies:** Core emulation has very few dependencies
- **Modular Architecture:** Each crate serves a specific purpose
- **Multiple Frontends:** Supports libretro, SDL2, and WebAssembly

---

## Emulation Accuracy

### CPU (6502)

- Cycle-accurate execution ("accurate-ish")
- Official opcode implementation
- **Missing:** Most unofficial 6502 instructions
- Clean separation allows reuse in other projects

### PPU (2C02)

- Cycle-accurate rendering
- **Unique Feature:** Transistor-level testsuite from Visual2C02
- **Missing:** Accurate sprite overflow emulation
- NTSC only (no PAL support)

### APU

- Full audio implementation
- All standard channels supported

### Mappers

| Mapper | Name | Notes |
|--------|------|-------|
| 000 | NROM | Basic mapper |
| 001 | MMC1 | Battery save support |
| 002 | UxROM | Standard switching |
| 007 | AxROM | Standard switching |
| 030 | UNROM 512 | Homebrew mapper |

**Total Coverage:** ~52% of licensed NES library (5 mappers)

---

## Features

### Core Emulation
- [x] iNES ROM format support
- [ ] Save states (not implemented)
- [ ] Battery-backed RAM saves
- [ ] PAL support

### User Interface
- [x] Libretro core (RetroArch compatible)
- [x] SDL2 development frontend
- [x] WebAssembly browser support
- [ ] Standalone GUI application

### Developer Features
- [x] ROM-based testsuite framework
- [x] Transistor-level PPU tests (rp2c02-testsuite)
- [x] Reusable MOS 6502 crate
- [x] Web demo available

---

## Technical Highlights

### 1. Transistor-Level PPU Testsuite

The `rp2c02-testsuite` is auto-generated from [Visual2C02](https://www.qmtpro.com/~nes/chipimages/visual2c02/), a transistor-level simulation of an actual NES PPU. This provides hardware-accurate test vectors that go beyond traditional ROM-based testing.

### 2. Reusable 6502 Implementation

The `mos6502` crate can be used independently:

```rust
// Example: Using mos6502 for other projects
use mos6502::Cpu;
```

This makes Pinky valuable even for non-NES projects requiring 6502 emulation.

### 3. Emulator-Agnostic Testsuite

The `nes-testsuite` is designed to be hooked into any NES emulator by implementing a single trait:

```rust
// See nes/src/testsuite.rs for trait definition
```

### 4. WebAssembly Build

The project includes a working WebAssembly build available at [koute.github.io/pinky-web](http://koute.github.io/pinky-web/).

---

## Code Metrics & Structure

### Lines of Code Breakdown

| Component | Lines | Description |
|-----------|-------|-------------|
| **Total Project** | 32,763 | Complete Rust codebase |
| PPU Test (largest) | 3,963 | Transistor-level sprite rendering test |
| PPU Core (rp2c02) | 1,843 | Picture Processing Unit |
| PPU Scheduler | 1,266 | Cycle-accurate PPU timing |
| APU (virtual_apu) | 1,199 | Audio Processing Unit |
| MOS 6502 Crate | 2,163 | Standalone CPU emulator |

**Source Organization:**
- 51 Rust source files across 8 crates
- 94 unit tests
- 8 workspace members (modular design)

### Crate Breakdown

| Crate | Purpose | LOC (approx) |
|-------|---------|--------------|
| `mos6502` | Standalone 6502 CPU (reusable) | 2,163 |
| `nes` | Core NES emulator | ~12,000 |
| `rp2c02-testsuite` | Transistor-level PPU tests | ~15,000 |
| `nes-testsuite` | ROM-based test framework | ~1,500 |
| `emumisc` | Shared utilities | ~500 |
| `pinky-devui` | SDL2 frontend | ~800 |
| `pinky-libretro` | Libretro core | ~400 |
| `pinky-web` | WebAssembly build | ~400 |

### CPU Implementation (mos6502)

**Standalone 6502 Emulator:**
- 2,163 lines of pure CPU emulation
- Completely independent crate (can be used in other projects)
- Only dependency: `bitflags` + `emumisc`
- Feature flags: `default = ["std"]` for no_std compatibility

**Design Philosophy:**
```rust
// Clean trait-based memory interface
trait Interface {
    fn peek(&mut self, address: u16) -> u8;
    fn poke(&mut self, address: u16, value: u8);
}
```

This allows the 6502 crate to be used for:
- Apple II emulation
- Commodore 64 emulation
- Atari 2600/800 emulation
- Any MOS 6502-based system

**Limitations:**
- Official opcodes only (no illegal/unofficial instructions)
- "Accurate-ish" cycle timing (close but not perfect)

### PPU Implementation Details

**Three-Layer PPU Architecture:**

1. **rp2c02.rs** (1,843 lines) - Core PPU logic
2. **rp2c02_scheduler.rs** (1,266 lines) - Cycle-accurate scheduling
3. **rp2c02-testsuite** - Transistor-level validation

**Unique Feature: Visual2C02 Integration**

The `rp2c02-testsuite` is auto-generated from [Visual2C02](https://www.qmtpro.com/~nes/chipimages/visual2c02/), a transistor-level simulation of the actual 2C02 PPU die. This provides:
- Hardware-accurate test vectors
- Sub-cycle timing verification
- Transistor-level behavior validation
- Tests covering edge cases not in ROM test suites

**Test File Sizes:**
- test_vram_access_during_sprite_rendering_without_sprites.rs: 3,963 lines
- test_current_address_during_sprite_rendering_without_sprites.rs: 3,963 lines
- test_current_address_during_background_rendering.rs: 2,758 lines
- test_current_address_when_not_rendering.rs: 2,754 lines

These massive test files contain thousands of test cases generated directly from hardware simulation.

**Known Limitations:**
- NTSC only (no PAL/Dendy)
- Incomplete sprite overflow emulation

### Mapper Architecture

**Supported Mappers:**
```rust
// File: nes/src/mappers.rs
// Implementations:
mapper_axrom.rs     // Mapper 007 - AxROM
mapper_mmc1.rs      // Mapper 001 - MMC1
mapper_unrom512.rs  // Mapper 030 - UNROM 512 (homebrew)
```

**Generic Mapper Framework:**
- `generic_mapper.rs` provides base mapper trait
- Each mapper is a separate module
- Battery save support in MMC1

**Coverage:** Only 5 mappers = ~52% of commercial NES library

---

## Code Quality Indicators

### Build Configuration

```toml
[profile.test]
opt-level = 2
```

Optimizations enabled even in test builds for performance - critical for running extensive transistor-level tests efficiently.

### Testing Strategy

**Multi-Level Testing Approach:**

1. **Unit Tests:** 94 `#[test]` functions across crates
2. **ROM-Based Tests:** `nes-testsuite` with actual game ROMs
3. **Transistor-Level Tests:** `rp2c02-testsuite` with Visual2C02 data
4. **Integration Tests:** Full emulation validation

**Emulator-Agnostic Test Framework:**

The `nes-testsuite` is designed to be portable:
```rust
// See nes/src/testsuite.rs for trait definition
// Any NES emulator can hook into this by implementing a single trait
```

This allows other emulator projects to use Pinky's test suite.

### CI/CD Status

- No GitHub Actions workflows found in current repository
- Historical Travis CI integration mentioned in documentation
- Tests can be run locally with `cargo test`

---

## Performance Characteristics

### Optimization Techniques

- Test profile uses opt-level 2 for faster test execution
- Minimal allocations in hot paths
- Cycle-accurate scheduling without excessive overhead

### No_std Support

The `mos6502` crate supports no_std environments:
```toml
[features]
default = ["std"]
std = []
```

This enables embedded systems use cases.

---

## Accuracy Analysis

### Strengths

1. **Transistor-Level PPU Validation:** Unique testing methodology provides hardware-accurate verification
2. **Cycle-Accurate PPU:** Detailed scheduler implementation tracks exact cycle behavior
3. **Clean 6502 Implementation:** Well-structured CPU emulation
4. **Test-Driven Development:** Extensive test coverage

### Limitations

1. **No Unofficial Opcodes:** Many commercial games won't run
2. **Limited Mappers:** Only 52% of NES library supported
3. **Sprite Overflow:** Incomplete implementation
4. **NTSC Only:** No multi-region support

### Test ROM Results

- nestest.nes: Not explicitly mentioned
- Custom ROM suite in `nes-testsuite/roms/`
- Focus on PPU accuracy over game compatibility

---

## Technical Highlights for Research

### 1. Visual2C02 Methodology

The transistor-level test generation is groundbreaking:
- Simulates actual PPU hardware at die level
- Generates test vectors from simulation
- Validates emulator against hardware truth
- Goes beyond what ROM-based tests can verify

**Research Value:** This methodology could be applied to other chips (APU, mappers) if die simulations become available.

### 2. Reusable Component Architecture

Each crate is independently useful:
```
mos6502/          → Use in any 6502-based system emulator
emumisc/          → General emulation utilities
nes-testsuite/    → Portable test framework
rp2c02-testsuite/ → PPU accuracy verification
```

### 3. Multiple Frontend Support

**Deployment Targets:**
- **Libretro Core:** RetroArch integration (`pinky-libretro/`)
- **SDL2 Frontend:** Standalone development UI (`pinky-devui/`)
- **WebAssembly:** Browser deployment (`pinky-web/`)

**Live Demo:** [koute.github.io/pinky-web](http://koute.github.io/pinky-web/)

---

## Limitations

1. **Limited Mapper Support:** Only 5 mappers (~52% game coverage)
2. **No Unofficial Opcodes:** Many games requiring unofficial instructions won't work
3. **No Save States:** Missing save/load functionality
4. **NTSC Only:** No PAL or Dendy region support
5. **Maintenance Mode:** Less active development
6. **No Battery Saves:** Limited persistence support

---

## Recommendations for Reference

### For Accuracy Research

1. **Study the rp2c02-testsuite methodology** for hardware-accurate validation
2. **Use Visual2C02 integration approach** for transistor-level testing
3. **Reference the PPU scheduler** for cycle-accurate timing patterns

### For Clean Architecture

1. **Study the MOS 6502 implementation** for reusable component design
2. **Adopt the emulator-agnostic testsuite pattern** for test portability
3. **Reference the multi-crate architecture** for component separation
4. **Use the generic mapper framework** for extensible mapper support

### For Multi-Platform Deployment

1. **Study the libretro core** for RetroArch integration
2. **Reference the WebAssembly build** for browser deployment
3. **Use the no_std feature pattern** for embedded targets

---

## Use Cases

| Use Case | Suitability | Notes |
|----------|-------------|-------|
| Playing most NES games | Limited | Only 52% mapper coverage |
| Learning NES emulation | Excellent | Clean, well-structured code |
| 6502 emulation for other projects | Excellent | Standalone reusable crate |
| PPU accuracy research | Outstanding | Unique transistor-level tests |
| WebAssembly deployment | Good | Working web demo |
| RetroArch integration | Good | Libretro core available |
| Accuracy testing methodology | Outstanding | Visual2C02 approach |

---

## Community & Ecosystem

### Project Metrics

- **GitHub Stars:** 800+
- **Primary Author:** Jan Bujak (koute)
- **Status:** Maintenance mode (not production quality)
- **License:** Dual MIT/Apache-2.0

### Community Reception

Project is explicitly described as "not production quality emulator" but highly regarded for:
- Unique transistor-level testing approach
- Clean code for educational purposes
- Reusable 6502 implementation
- Accuracy-focused design

### Distribution Channels

1. **Web Demo:** [koute.github.io/pinky-web](http://koute.github.io/pinky-web/)
2. **Source Code:** GitHub repository
3. **Libretro Core:** Can be built for RetroArch
4. **Documentation:** Well-commented codebase

### Developer Resources

- **NESdev Reference:** Based on publicly available documentation
- **Code Comments:** Well-documented implementation
- **Test Framework:** Reusable by other emulators
- **Visual2C02:** External tool for test generation

---

## Comparison with Other Rust Emulators

### Unique Strengths

- **Only emulator with transistor-level PPU testing**
- Most reusable architecture (standalone mos6502 crate)
- Best educational resource for accuracy-focused design
- Emulator-agnostic test framework

### Trade-offs

- Accuracy over compatibility (limited mapper support)
- Research focus over production readiness
- Clean architecture over feature completeness

### Positioning

Pinky is a research-oriented emulator that prioritizes accuracy validation over game compatibility. It's invaluable for understanding PPU behavior at a hardware level and provides excellent reusable components for other projects.

---

## Version Information

- **Current Version:** 0.1.0 (all crates)
- **Status:** Maintenance mode
- **Last Major Updates:** Early development completed
- **Rust Edition:** Earlier edition (pre-2021)

---

*Report Generated: December 2024*
*Enhanced: December 2024 with deep code analysis and community research*
