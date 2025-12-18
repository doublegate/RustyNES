# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustyNES is a next-generation Nintendo Entertainment System (NES) emulator written in Rust. Target: 100% TASVideos accuracy test pass rate, 300+ mappers, RetroAchievements, GGPO netplay, TAS tools, Lua scripting.

**Status:** Architecture design complete, folder structure created, comprehensive documentation generated. Ready to begin Phase 1 implementation.

## Repository

- **GitHub**: https://github.com/doublegate/RustyNES
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

```
rustynes/
├── crates/
│   ├── rustynes-core/src/         # Core emulation engine (no_std compatible)
│   ├── rustynes-cpu/src/          # 6502 CPU (reusable for C64, Apple II)
│   ├── rustynes-ppu/src/          # 2C02 PPU
│   ├── rustynes-apu/src/          # 2A03 APU with expansion audio
│   ├── rustynes-mappers/src/      # All mapper implementations
│   ├── rustynes-desktop/src/      # egui/wgpu GUI frontend
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
├── benches/                       # Performance benchmarks
├── examples/                      # Usage examples
├── test-roms/                     # NES test ROM files (excluded from git)
├── assets/                        # Static resources
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

- **Graphics**: `wgpu` (cross-platform GPU), `egui` (GUI)
- **Audio**: `sdl2` or `cpal`
- **Netplay**: `backroll` (GGPO rollback)
- **Scripting**: `mlua` (Lua 5.4)
- **Achievements**: `rcheevos-sys` (FFI bindings)
- **Testing**: `criterion` (benchmarks), `proptest` (property-based)

## Implementation Phases

| Phase | Months | Deliverable |
|-------|--------|-------------|
| 1: MVP | 1-6 | 80% game compatibility, desktop GUI |
| 2: Features | 7-12 | RetroAchievements, netplay, TAS, Lua, debugger |
| 3: Expansion | 13-18 | Expansion audio, 98% mappers, WebAssembly |
| 4: Polish | 19-24 | Video filters, TAS editor, v1.0 release |

## Development Timeline

| Milestone | Target Date | Description |
|-----------|-------------|-------------|
| **Project Start** | December 2025 | Architecture & docs complete |
| **M1: CPU Complete** | January 2026 | nestest.nes passes |
| **M5: MVP Release** | June 2026 | 80% game compatibility |
| **M8: Feature Complete** | December 2026 | All advanced features |
| **M10: v1.0 Release** | December 2027 | 100% TASVideos accuracy |

## Accuracy Targets

- CPU: 100% nestest.nes golden log
- PPU: 100% blargg PPU tests, sprite_hit, ppu_vbl_nmi
- APU: 95%+ blargg APU tests
- Overall: 100% TASVideos accuracy suite (156 tests)

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

- **Edition**: Rust 2021
- **MSRV**: 1.75+ (for async traits in std)
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

### Phase 1: MVP (Current)

1. **CPU**: Complete 6502 implementation with all 256 opcodes
2. **PPU**: Basic rendering (backgrounds, sprites, scrolling)
3. **APU**: Square, triangle, noise channels
4. **Bus**: Memory mapping, DMA
5. **Mappers**: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4)
6. **ROM Loading**: iNES format support

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
