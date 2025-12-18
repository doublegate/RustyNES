# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustyNES is a next-generation Nintendo Entertainment System (NES) emulator written in Rust. The project is in **pre-implementation phase** with comprehensive architecture design complete. Target: 100% TASVideos accuracy test pass rate, 300+ mappers, RetroAchievements, GGPO netplay, TAS tools, Lua scripting.

**Status:** Architecture design complete, implementation not yet started.

## Architecture

### Workspace Structure (Planned)

```
rustynes/
├── crates/
│   ├── rustynes-core/         # Core emulation engine (no_std compatible)
│   ├── rustynes-cpu/          # 6502 CPU (reusable for C64, Apple II)
│   ├── rustynes-ppu/          # 2C02 PPU
│   ├── rustynes-apu/          # 2A03 APU with expansion audio
│   ├── rustynes-mappers/      # All mapper implementations
│   ├── rustynes-desktop/      # egui/wgpu GUI frontend
│   ├── rustynes-web/          # WebAssembly frontend
│   ├── rustynes-tas/          # TAS recording/playback (FM2 format)
│   ├── rustynes-netplay/      # GGPO rollback netcode (backroll-rs)
│   └── rustynes-achievements/ # RetroAchievements (rcheevos FFI)
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

## Commands (Planned)

Once implementation begins, standard Cargo workspace commands apply:

```bash
# Build
cargo build --workspace
cargo build --release

# Test
cargo test --workspace                    # All tests
cargo test -p rustynes-cpu                # Single crate
cargo test cpu_lda_immediate              # Single test

# Run
cargo run -p rustynes-desktop             # Desktop GUI
cargo run -p rustynes-desktop -- rom.nes  # With ROM

# Lint
cargo clippy --workspace -- -D warnings
cargo fmt --check

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
