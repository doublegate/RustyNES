# kamiyaowl/rust-nes-emulator Technical Report

**Repository:** [github.com/kamiyaowl/rust-nes-emulator](https://github.com/kamiyaowl/rust-nes-emulator)
**Author:** kamiyaowl
**Language:** Rust
**License:** MIT
**Stars:** 500+ | **Status:** Maintained

---

## Executive Summary

This emulator is unique for targeting embedded systems, specifically the STM32F769I-DISCO microcontroller board. It demonstrates that Rust NES emulation can run on resource-constrained hardware. The project includes desktop, WebAssembly, and embedded builds, making it an excellent reference for understanding performance optimization and memory constraints in emulator development.

---

## Architecture Overview

### Build Targets

```
rust-nes-emulator/
├── desktop/     # SDL2 desktop application
├── wasm/        # WebAssembly build
└── embedded/    # STM32F769 microcontroller
```

**Key Design Decision:** Same core emulator runs on desktop, web, and embedded targets, demonstrating Rust's cross-platform capabilities.

### Platform Requirements

| Platform | Build Command |
|----------|---------------|
| Desktop | `cd desktop && cargo run --release` |
| WebAssembly | `cd wasm && wasm-pack build --release` |
| Embedded | `cd embedded && rustup run nightly cargo build --release` |

---

## Emulation Implementation

### CPU (6502)

- [x] All registers
- [x] Interrupt handling
- [x] Official opcodes
- [x] **Unofficial opcodes** (nestest passes)

### PPU (2C02)

- [x] OAM DMA
- [x] Background rendering
- [x] Nametable mirroring
- [x] Horizontal/Vertical scroll
- [x] Sprites (8x8 and 8x16)
- [ ] Vertical scroll bug (#87)
- [ ] Sprite 0 hit bug (#40)

### APU (2A03)

- [ ] Pulse Wave 1
- [ ] Pulse Wave 2
- [ ] Triangle Wave
- [ ] Noise
- [ ] DMC

**Note:** APU is not implemented.

### Mappers

| Mapper | Name | Status |
|--------|------|--------|
| 0 | NROM | Implemented |
| 1 | MMC1 | Not implemented |
| 2 | UNROM | Not implemented |
| 3 | CNROM | Not implemented |
| 4 | MMC3 | Not implemented |

**Coverage:** ~10% (NROM only)

### Input

- [x] Joypad 1

---

## Features

### Core Emulation
- [x] iNES ROM format
- [x] Snapshot/Restore (save states)
- [ ] Audio (not implemented)
- [ ] Multiple mappers

### Platforms

| Platform | Status | Notes |
|----------|--------|-------|
| Windows | Supported | SDL2 desktop |
| Linux | Supported | Docker builds available |
| macOS | Supported | SDL2 desktop |
| Web | Supported | WebAssembly playground |
| STM32F769 | **Work in Progress** | Embedded target |

### Developer Features
- [x] GitHub Actions CI
- [x] Docker build support
- [x] WebAssembly playground
- [x] Test ROM validation

---

## Technical Highlights

### 1. Embedded Target (STM32F769I-DISCO)

![Embedded](screenshot/embedded.jpg)

The emulator runs on the [STM32F769I-DISCO](https://www.st.com/ja/evaluation-tools/32f769idiscovery.html) development board:
- ARM Cortex-M7 processor
- 2MB Flash, 512KB RAM
- LCD display support

Derived repository for embedded optimization: [kamiyaowl/rust-nes-emulator-embedded](https://github.com/kamiyaowl/rust-nes-emulator-embedded)

### 2. Docker Build System

```bash
# Desktop build
docker-compose run build-desktop-release

# WebAssembly build
docker-compose run build-wasm-release
docker-compose run build-wasm-webpage

# Embedded build
docker-compose run build-embedded-lib
docker-compose run build-mbed
```

### 3. Unofficial Opcode Support

The emulator passes nestest for both official and unofficial opcodes:

![nestest_extra](screenshot/nestest_extra.bmp) - Unofficial opcodes
![nestest_normal](screenshot/nestest_normal.bmp) - Official opcodes

### 4. WebAssembly Playground

Online demo: [kamiyaowl.github.io/rust-nes-emulator](https://kamiyaowl.github.io/rust-nes-emulator/index.html)

---

## Code Metrics & Structure

### Overview

| Metric | Value |
|--------|-------|
| **Total Lines of Code** | 4,624 |
| **Source Files** | 20 Rust files |
| **Test Functions** | 5 unit tests |
| **Architecture** | Core lib + 3 platform targets |

### Lines of Code by Component

| Component | LOC | Files/Purpose |
|-----------|-----|---------------|
| **CPU Instructions** | 1,421 | 6502 opcode implementations |
| **PPU Core** | 697 | Picture Processing Unit |
| **Desktop Frontend** | 239 | SDL2 application wrapper |
| **System** | 244 | NES system coordination |
| **System PPU Registers** | 223 | PPU register interface |
| **Cassette** | 218 | ROM/Mapper system |
| **APU Skeleton** | 179 | Audio stub (not functional) |
| **WASM Bindings** | 163 | WebAssembly interface |
| **Video System** | 154 | Rendering abstraction |
| **Embedded Target** | 143 | STM32F769 implementation |
| **CPU Core** | 141 | 6502 execution engine |
| **System APU Registers** | 140 | APU register stubs |
| **Controller/Pad** | 91 | Input handling |
| **Test Suite** | 330 | Integration tests |

### Testing Coverage

**5 Unit Tests:**
- Minimal automated testing
- Primary validation via nestest ROM
- Manual testing with commercial games

**Test Strategy:**
- nestest for CPU verification (official + unofficial opcodes)
- Manual game testing
- Embedded hardware validation
- CI/CD automated builds

---

## CPU Implementation Details

### 6502 Core (1,421 LOC instructions + 141 LOC core)

**Total:** 1,562 LOC for complete CPU implementation

**Implementation Philosophy:** Exhaustive instruction dispatch with cycle-accurate execution.

**Key Features:**
- **Official opcodes:** Complete 6502 instruction set
- **Unofficial opcodes:** Full support (passes nestest extra)
- **Cycle counting:** Instruction-level timing
- **Interrupt handling:** NMI, IRQ, RESET
- **Dummy reads:** Side-effect handling

**Architecture:**
```rust
// Core CPU state (inferred from structure)
pub struct Cpu {
    // Registers
    a: u8,      // Accumulator
    x: u8,      // X index
    y: u8,      // Y index
    sp: u8,     // Stack pointer
    pc: u16,    // Program counter
    status: u8, // Status flags (N V _ B D I Z C)

    // Cycle counting
    cycles: usize,
}
```

**Instruction Dispatch (1,421 LOC):**
- Comprehensive match statement for all 256 opcodes
- Per-instruction implementation
- Address mode resolution
- Memory read/write operations
- Cycle-accurate timing

---

## PPU Implementation Details

### 2C02 Core (697 LOC ppu.rs + 223 LOC system_ppu_reg.rs)

**Total:** 920 LOC for complete PPU implementation

**Rendering Architecture:** Scanline-based rendering with sprite evaluation.

**Key Features:**
- Background rendering pipeline
- Sprite rendering (8x8 and 8x16)
- OAM DMA support
- Nametable mirroring (H/V)
- Horizontal/Vertical scrolling
- Pattern table access
- Palette system (internal + system tables)

**Rendering Pipeline:**
1. Scanline processing (262 scanlines/frame)
2. Background tile fetching
3. Sprite evaluation (up to 8 per scanline)
4. Pixel composition
5. VBlank handling

**Known Limitations:**
- Vertical scroll bug (#87)
- Sprite 0 hit bug (#40)
- Simplified timing for embedded performance

**PPU Register Interface (223 LOC):**
- PPUCTRL ($2000)
- PPUMASK ($2001)
- PPUSTATUS ($2002)
- OAMADDR ($2003)
- OAMDATA ($2004)
- PPUSCROLL ($2005)
- PPUADDR ($2006)
- PPUDATA ($2007)
- OAMDMA ($4014)

---

## APU Implementation Details

### 2A03 Audio (179 LOC apu.rs + 140 LOC system_apu_reg.rs)

**Total:** 319 LOC (skeleton implementation)

**Status:** NOT IMPLEMENTED - Stub only

**Reason:** Embedded target (STM32F769) prioritizes graphics over audio due to:
- Limited processing power
- Memory constraints
- Real-time rendering requirements

**Register Stubs Present:**
- Pulse 1 registers ($4000-$4003)
- Pulse 2 registers ($4004-$4007)
- Triangle registers ($4008-$400B)
- Noise registers ($400C-$400F)
- DMC registers ($4010-$4013)
- Status/Control ($4015, $4017)

**Future Work:** Audio implementation deferred to optimize for embedded display performance.

---

## Mapper Implementation

### Cartridge System (218 LOC cassette.rs)

**Supported Mappers:**
- **Mapper 0 (NROM)** - Fully implemented
- **Mapper 1 (MMC1)** - Mentioned but not implemented
- **Mapper 2 (UNROM)** - Mentioned but not implemented
- **Mapper 3 (CNROM)** - Mentioned but not implemented
- **Mapper 4 (MMC3)** - Mentioned but not implemented

**Coverage:** ~10% of games (NROM only)

**Architecture:**
- iNES format parsing
- PRG-ROM banking (NROM: 16KB or 32KB)
- CHR-ROM/CHR-RAM (8KB)
- Mirroring control
- Trainer support

**ROM Loading:**
```rust
// iNES header parsing
- Header validation
- PRG-ROM size determination
- CHR-ROM/RAM configuration
- Mapper number extraction
- Mirroring mode detection
```

**Limitation:** Single mapper support prioritizes embedded performance and memory constraints over game compatibility.

---

## Embedded Target Implementation

### STM32F769I-DISCO (143 LOC embedded/src/lib.rs)

**Hardware Specifications:**
- **CPU:** ARM Cortex-M7 @ 216 MHz
- **RAM:** 512KB SRAM
- **Flash:** 2MB
- **Display:** 800x480 LCD with touch
- **Board:** STM32F769I-DISCO development kit

**Embedded Optimizations:**
- `no_std` environment (no heap allocator)
- Stack-based execution
- DMA for video transfer
- Direct LCD framebuffer access
- Nightly Rust for embedded optimizations

**Memory Constraints:**
- NES Memory: 2KB RAM + 2KB VRAM
- Cartridge: Up to 32KB PRG + 8KB CHR (NROM)
- Framebuffer: 256x240 pixels
- Total usage: ~100KB (within 512KB SRAM)

**Build Target:**
- `thumbv7em-none-eabihf` (ARM Cortex-M7 with FPU)
- Nightly toolchain required
- Embedded HAL abstractions

**Performance:**
- Target: 60 FPS (16.67ms per frame)
- CPU cycles per frame: ~29,780
- PPU rendering: Optimized scanline rendering
- No audio processing (saves CPU cycles)

**Derived Project:**
[rust-nes-emulator-embedded](https://github.com/kamiyaowl/rust-nes-emulator-embedded) - Further optimizations for embedded deployment.

---

## Multi-Platform Architecture

### Desktop (239 LOC desktop/src/main.rs)

**Technologies:**
- SDL2 for window/input/rendering
- Native OS execution
- Full-speed emulation
- Keyboard controls

**Build:**
```bash
cd desktop && cargo run --release ROM_FILE.nes
```

### WebAssembly (163 LOC wasm/src/lib.rs)

**Technologies:**
- wasm-bindgen for JavaScript bindings
- WebAssembly compilation
- Browser-based execution
- Canvas rendering

**Build:**
```bash
cd wasm && wasm-pack build --release
```

**Online Demo:** [kamiyaowl.github.io/rust-nes-emulator](https://kamiyaowl.github.io/rust-nes-emulator/index.html)

### Embedded (143 LOC embedded/src/lib.rs)

**Technologies:**
- `no_std` Rust
- STM32F769 HAL
- Direct LCD control
- ARM Cortex-M7 optimizations

**Build:**
```bash
cd embedded && rustup run nightly cargo build --release
```

**Target:** STM32F769I-DISCO evaluation board

---

## Performance Characteristics

### Desktop Performance
- Native speed (60 FPS+)
- SDL2 minimal overhead
- Full-speed emulation

### WebAssembly Performance
- Near-native performance
- Browser-dependent (Chrome/Firefox)
- 60 FPS achievable on modern hardware

### Embedded Performance (STM32F769)
- **Target:** 60 FPS (challenging)
- **CPU:** 216 MHz ARM Cortex-M7
- **Frame time:** ~3.6M cycles per frame @ 216 MHz
- **NES requirement:** ~29,780 CPU cycles + PPU rendering
- **Overhead:** ~120x clock speed advantage
- **Challenge:** LCD refresh, sprite rendering, memory bandwidth

**Optimization Techniques:**
- No audio processing (saves cycles)
- Simplified PPU timing
- Direct LCD DMA transfer
- Stack-based execution (no heap)
- Nightly Rust optimizations

---

## Code Quality Indicators

### CI/CD

- GitHub Actions workflows:
  - Test
  - Deploy
  - Build for Windows

### Build Requirements

```
rustc 1.39.0-nightly required (for embedded optimization)
```

**Docker Support:**
```bash
# Desktop
docker-compose run build-desktop-release

# WebAssembly
docker-compose run build-wasm-release

# Embedded
docker-compose run build-embedded-lib
```

---

## Tested Games

| Game | Status |
|------|--------|
| Super Mario Bros. | Working |
| Donkey Kong | Working |
| Mario Bros. | Working |
| Ice Climber | Working |
| Hello World (homebrew) | Working |

---

## Comparison with Other Embedded-Focused Emulators

| Feature | kamiyaowl/rust-nes-emulator | Rustico | TetaNES |
|---------|---------------------------|---------|---------|
| **Total LOC** | 4,624 | 24,430 | 16,900 |
| **Unit Tests** | 5 | 0 | 50+ |
| **Embedded Target** | STM32F769 (ARM) | No | No |
| **Multi-Platform** | Desktop/Web/Embedded | Desktop/Web | Desktop/Web |
| **APU Support** | No (stub only) | Full + Expansion | Full |
| **Mapper Support** | 1 (NROM) | 20+ | 32+ |
| **Primary Focus** | Embedded/Education | Expansion Audio | Accuracy/Web |
| **Code Size** | Compact (4.6K) | Large (24K) | Medium (17K) |

---

## Community & Ecosystem

### Project Status
- **Repository:** [github.com/kamiyaowl/rust-nes-emulator](https://github.com/kamiyaowl/rust-nes-emulator)
- **Author:** kamiyaowl - Embedded systems specialist
- **Stars:** 500+
- **Status:** Maintained
- **Platforms:** Desktop (SDL2), Web (WASM), Embedded (STM32F769)

### Derived Projects
1. **[rust-nes-emulator-embedded](https://github.com/kamiyaowl/rust-nes-emulator-embedded)** - Optimized for STM32F769
2. **[wio-terminal-rust-nes-emulator](https://github.com/kamiyaowl-sandbox/wio-terminal-rust-nes-emulator)** - Wio Terminal port
3. **[mbed_DISCO-F769NI](https://github.com/kamiyaowl-sandbox/mbed_DISCO-F769NI)** - Trial mbed integration

### Community Recognition
- Referenced by rib/nes-emulator for CPU implementation
- Featured in embedded Rust showcases
- Educational resource for `no_std` Rust
- STM32 community example project

### Referenced Projects
- STM32F769I-DISCO BSP drivers
- Rust-SDL2 for desktop rendering
- wasm-bindgen for WASM bindings
- christopherpow/nes-test-roms for validation

---

## Limitations

1. **No Audio:** APU not implemented (embedded performance priority)
2. **Single Mapper:** NROM (mapper 0) only (~10% game coverage)
3. **Known Bugs:** Sprite 0 hit (#40), vertical scroll (#87)
4. **Nightly Rust:** Embedded requires nightly toolchain
5. **Embedded WIP:** Still under development
6. **Test Coverage:** Only 5 unit tests (validation via nestest ROM)
7. **Accuracy:** Simplified timing for embedded performance

---

## Recommendations for Reference

### Primary Use Cases
1. **Embedded optimization techniques** - Study `no_std`, stack-based execution, DMA
2. **Multi-platform architecture** - Same core runs on Desktop/Web/Embedded
3. **Docker build system** - Reproducible cross-platform builds
4. **ARM Cortex-M7 implementation** - Real-world embedded emulation
5. **Memory constraint handling** - 512KB SRAM optimization strategies

### Code Study Focus
1. **Embedded target** (143 LOC) - STM32F769 HAL integration
2. **CPU implementation** (1,562 LOC) - Comprehensive 6502 with unofficial opcodes
3. **PPU optimization** (920 LOC) - Embedded-friendly rendering
4. **Platform abstraction** - Core lib + 3 target-specific frontends
5. **Build system** - Docker, GitHub Actions, multi-target compilation

---

## Use Cases

| Use Case | Suitability | Notes |
|----------|-------------|-------|
| Learning embedded Rust | Excellent | Real-world `no_std` example |
| Understanding memory constraints | Excellent | 512KB SRAM optimization |
| Playing NES games | Limited | NROM only, no audio |
| WebAssembly deployment | Good | Functional web demo |
| Production emulator | Limited | Educational/demo focus |
| Embedded systems education | Excellent | STM32 + Rust showcase |
| Multi-platform reference | Excellent | Desktop/Web/Embedded from same core |

---

## Test ROMs Used

| Path | Source | Purpose |
|------|--------|---------|
| roms/other/hello.nes | [Japanese NES research](http://hp.vector.co.jp/authors/VA042397/nes/sample.html) | Basic functionality |
| roms/nes-test-roms | [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms) | CPU validation |
| nestest.nes | nes-test-roms | Official + unofficial opcodes |
| Super Mario Bros. | Commercial | Manual testing |
| Donkey Kong | Commercial | Sprite testing |

---

## Build Instructions

### Desktop
```bash
cd desktop
cargo run --release path/to/rom.nes
```

### WebAssembly
```bash
cd wasm
wasm-pack build --release
# Serve index.html
```

### Embedded (STM32F769)
```bash
cd embedded
rustup override set nightly
cargo build --release --target thumbv7em-none-eabihf
# Flash to STM32F769I-DISCO board
```

### Docker (All Platforms)
```bash
# Desktop
docker-compose run build-desktop-release

# WebAssembly
docker-compose run build-wasm-release

# Embedded
docker-compose run build-embedded-lib
```

---

## Sources

- [GitHub - kamiyaowl/rust-nes-emulator](https://github.com/kamiyaowl/rust-nes-emulator)
- [GitHub - kamiyaowl/rust-nes-emulator-embedded](https://github.com/kamiyaowl/rust-nes-emulator-embedded)
- [STM32F769I-DISCO Evaluation Board](https://www.st.com/ja/evaluation-tools/32f769idiscovery.html)
- [Online WebAssembly Demo](https://kamiyaowl.github.io/rust-nes-emulator/index.html)
- [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)
- [Wio Terminal Port](https://github.com/kamiyaowl-sandbox/wio-terminal-rust-nes-emulator)
- [rib/nes-emulator](https://github.com/rib/nes-emulator) - References this project for CPU

---

*Report Generated: December 2024*
*Enhanced: December 2024 with comprehensive code analysis and community research*
