# RustyNES Architecture & Technical Design Document

**Version:** 1.0.0
**Project:** RustyNES - Next-Generation NES Emulator in Rust
**Document Generated:** 2025-12-18
**Status:** Comprehensive Design Specification

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Vision & Design Philosophy](#vision--design-philosophy)
3. [Technical Requirements](#technical-requirements)
4. [Architecture Overview](#architecture-overview)
5. [Core Engine Architecture](#core-engine-architecture)
6. [Memory Subsystem](#memory-subsystem)
7. [Graphics Pipeline](#graphics-pipeline)
8. [Audio Pipeline](#audio-pipeline)
9. [Input System](#input-system)
10. [Advanced Features](#advanced-features)
11. [Performance Optimizations](#performance-optimizations)
12. [Crate Structure](#crate-structure)
13. [Testing Strategy](#testing-strategy)
14. [Implementation Roadmap](#implementation-roadmap)
15. [Reference Matrix](#reference-matrix)
16. [Dependencies](#dependencies)
17. [Appendices](#appendices)

---

## Executive Summary

**RustyNES** is a next-generation Nintendo Entertainment System (NES) emulator written in pure Rust, designed to combine world-class accuracy with modern features and cross-platform deployment. This project synthesizes best practices from the finest NES emulators (Mesen2's 100% accuracy, FCEUX's TAS tools, puNES's mapper coverage, Ares's code clarity) while leveraging Rust's safety guarantees and zero-cost abstractions.

### Primary Goals

1. **Gold-Standard Accuracy** - Target 100% pass rate on TASVideos Accuracy Test suite (156 tests)
2. **Comprehensive Mapper Support** - 300+ mappers covering all licensed games plus extensive unlicensed/homebrew
3. **Modern Feature Set** - RetroAchievements, netplay, TAS tools, Lua scripting, advanced debugging
4. **Cross-Platform** - Native desktop (Windows/Linux/macOS), WebAssembly, potential embedded
5. **Developer-Friendly** - Clean architecture, comprehensive documentation, extensive testing
6. **Safe Rust** - Zero unsafe code where possible, following DaveTCode's safe patterns

### Key Differentiators

- **Rust-First Design**: Leveraging ownership, traits, and type safety for correctness
- **Modular Architecture**: Clean crate separation enabling library reuse
- **GPU-Accelerated Rendering**: wgpu-based pipeline for cross-platform graphics
- **GGPO Netplay**: Rollback netcode for frame-perfect online multiplayer
- **RetroAchievements Native**: First-class rcheevos integration
- **Lua 5.4 Scripting**: Modern Lua via mlua for automation and bots
- **TAS-Ready**: FM2 format support with deterministic execution

### Target Audience

1. **Emulation Enthusiasts** - Seeking accuracy and modern features
2. **TAS Community** - Frame-perfect tools and movie recording
3. **RetroAchievements Users** - Native achievement tracking
4. **Homebrew Developers** - Reliable testing platform
5. **Netplay Users** - Low-latency online multiplayer
6. **Rust Developers** - Reference implementation for emulator development

---

## Vision & Design Philosophy

### Core Principles

#### 1. Accuracy First, Speed Second

Following Mesen2's philosophy, accuracy is non-negotiable. Every component (CPU, PPU, APU) must pass all relevant test ROMs before optimization. We target cycle-accurate emulation with sub-cycle precision where required (PPU scrolling, sprite 0 hit, APU frame counter).

**Implementation Strategy:**
- CPU: Cycle-accurate 6502 core with dummy read/write emulation
- PPU: Per-dot rendering at 5.37 MHz (3x CPU clock)
- APU: 1.789773 MHz execution with hardware-accurate mixing
- Mappers: Cycle-based IRQ timing for MMC3/5, VRC scanline counters

#### 2. Code Clarity Over Cleverness

Inspired by Ares's "half the code" philosophy, prioritize readable, maintainable code:
- Trait-based abstractions over macros
- Strong typing (newtype pattern for registers, addresses)
- Clear naming conventions
- Comprehensive inline documentation
- Avoid premature optimization

**Example Pattern:**
```rust
// Strong typing for PPU registers using newtype pattern
#[derive(Copy, Clone, Debug)]
struct VramAddress(u16);

impl VramAddress {
    fn coarse_x(&self) -> u8 { (self.0 & 0x1F) as u8 }
    fn coarse_y(&self) -> u8 { ((self.0 >> 5) & 0x1F) as u8 }
    fn nametable(&self) -> u8 { ((self.0 >> 10) & 0x03) as u8 }
    fn fine_y(&self) -> u8 { ((self.0 >> 12) & 0x07) as u8 }
}
```

#### 3. Safe Rust by Default

Following DaveTCode's zero-unsafe approach:
- Avoid `unsafe` blocks except for FFI (rcheevos, platform APIs)
- No `Rc<RefCell<>>` patterns - prefer owned data and message passing
- Use channels for inter-component communication
- Leverage type system for correctness (state machines as enums)

#### 4. Test-Driven Development

Every component validated before integration:
- Unit tests for individual instructions/operations
- Integration tests for component interactions
- Test ROM validation (nestest.nes, blargg test suite)
- Property-based testing for CPU/PPU timing
- Regression tests for mapper edge cases

#### 5. Modular & Reusable

Crate structure enabling independent use:
- `rustynes-core`: Library with no UI dependencies
- `rustynes-cpu`: Standalone 6502 implementation (reusable for C64, Apple II)
- `rustynes-ppu`: 2C02 PPU implementation
- `rustynes-apu`: 2A03 APU with expansion audio
- Frontend crates: Desktop, Web, headless

---

## Technical Requirements

### Accuracy Goals

| Component | Target Accuracy | Validation Method |
|-----------|----------------|-------------------|
| **CPU (6502)** | 100% instruction-level | nestest.nes golden log |
| **PPU (2C02)** | 100% cycle-accurate | ppu_vbl_nmi, sprite_hit_tests |
| **APU (2A03)** | 99%+ hardware match | apu_test, dmc_tests |
| **Mappers** | 100% for licensed games | Game compatibility matrix |
| **Overall** | 100% TASVideos suite | 156 test pass rate |

**TASVideos Accuracy Test Categories:**
- APU Tests (25): Frame counter, DMC, length counter, sweep, envelope
- CPU Tests (35): Instructions, timing, interrupts, unofficial opcodes
- PPU Tests (45): Rendering, scrolling, sprites, palette, VRAM
- Mapper Tests (51): IRQ timing, banking, mirroring, edge cases

### Mapper Support Goals

**Phase 1 (90% Coverage - First 6 Months):**
- Mapper 0 (NROM) - 9.5% of games
- Mapper 1 (MMC1/SxROM) - 27.9%
- Mapper 2 (UxROM) - 10.6%
- Mapper 3 (CNROM) - 6.3%
- Mapper 4 (MMC3/TxROM) - 23.4%
- Mapper 7 (AxROM) - 3.1%
- Total: 80.8% of licensed library

**Phase 2 (98% Coverage - 12 Months):**
- Add 20 common mappers: 5, 9, 10, 11, 13, 19, 23, 24, 25, 26, 28, 33, 34, 66, 69, 71, 73, 78, 79, 85
- Expansion audio: VRC6, VRC7, MMC5, Namco 163, Sunsoft 5B
- FDS (Famicom Disk System)

**Phase 3 (100%+ Coverage - 24 Months):**
- Target 300+ mappers (Mesen2 level)
- Unlicensed boards (Sachen, Waixing, BMC multicarts)
- UNIF board support (169+ boards like puNES)
- Custom/homebrew mappers

### Platform Support

| Platform | Status | Target Date | Notes |
|----------|--------|-------------|-------|
| **Linux x64** | Primary | Month 1 | Development platform |
| **Windows x64** | Primary | Month 2 | 60% user base |
| **macOS x64** | Primary | Month 2 | Intel Macs |
| **macOS ARM64** | Primary | Month 3 | Apple Silicon |
| **WebAssembly** | Secondary | Month 6 | Browser deployment |
| **Linux ARM64** | Tertiary | Month 12 | Raspberry Pi, Steam Deck |
| **Android** | Future | TBD | Mobile via termux/GUI |
| **iOS** | Future | TBD | Requires code signing |

### ROM Format Support

| Format | Priority | Description |
|--------|----------|-------------|
| **.nes (iNES)** | P0 | Standard format with 16-byte header |
| **.nes (NES 2.0)** | P0 | Extended header with submapper, timing |
| **.fds** | P1 | Famicom Disk System images |
| **.unif** | P2 | Universal NES Image Format (boards) |
| **.nsf** | P2 | NES Sound Format (music playback) |
| **.nsfe** | P3 | Extended NSF with metadata |
| **.zip/.7z** | P1 | Compressed ROM archives |

**Header Parsing:**
- Full NES 2.0 support (mapper high bits, submapper, timing flags)
- iNES 1.0 fallback with heuristics for malformed headers
- Database hash matching for header correction (GoodNES, No-Intro)

### Feature Requirements

#### Essential (MVP - Month 6)
- [x] Cycle-accurate CPU, PPU, APU emulation
- [x] Mappers 0, 1, 2, 3, 4 (80% game coverage)
- [x] Desktop GUI (egui-based, cross-platform)
- [x] Save states (instant save/load)
- [x] Controller input (keyboard + gamepad)
- [x] Audio output (SDL2 backend)
- [x] Video output (wgpu rendering)
- [x] Basic configuration (controls, paths, video settings)

#### High Priority (Month 12)
- [x] RetroAchievements integration (rcheevos)
- [x] Netplay (GGPO-style rollback via backroll-rs)
- [x] Lua scripting (mlua 5.4)
- [x] TAS recording (FM2 format)
- [x] Debugger (CPU disassembly, breakpoints, memory viewer)
- [x] Rewind functionality (ring buffer savestates)
- [x] Fast-forward/slow-motion
- [x] Cheat support (Game Genie, Pro Action Replay)
- [x] Screenshot/video recording

#### Medium Priority (Month 18)
- [x] TAS Editor (greenzone, bookmarks, piano roll)
- [x] Advanced debugger (PPU viewer, trace logger, CDL)
- [x] Expansion audio (VRC6, MMC5, N163, VRC7, S5B)
- [x] Zapper/Power Pad/Four Score support
- [x] WebAssembly build with web frontend
- [x] NTSC filter (blargg, composite video emulation)
- [x] Shader support (CRT effects, scanlines)

#### Low Priority (Month 24+)
- [ ] VS System arcade boards
- [ ] PlayChoice-10 support
- [ ] Barcode readers (Datach)
- [ ] Network protocol for spectating
- [ ] Discord Rich Presence
- [ ] Steam integration (achievements, cloud saves)

---

## Architecture Overview

### High-Level Design

RustyNES follows a **component-based architecture** where major subsystems (CPU, PPU, APU, Cartridge) are independent modules communicating through well-defined interfaces. The **Bus** acts as the central interconnect, routing memory accesses and coordinating timing.

```
┌───────────────────────────────────────────────────────────┐
│                     Frontend Layer                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │  Desktop │  │   Web    │  │ Headless │  │   TUI    │   │
│  │  (egui)  │  │  (wasm)  │  │   (CLI)  │  │(ratatui) │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘   │
└───────┼─────────────┼─────────────┼─────────────┼─────────┘
        │             │             │             │
        └─────────────┴─────────────┴─────────────┘
                      │
        ┌─────────────▼───────────────────────────────────────┐
        │              Core Emulator (rustynes-core)          │
        │  ┌──────────────────────────────────────────────┐   │
        │  │           Console (Master Clock)             │   │
        │  │  ┌────────┐  ┌────────┐  ┌──────┐  ┌──────┐  │   │
        │  │  │  CPU   │◄─┤  Bus   │◄─┤ PPU  │◄─┤ APU  │  │   │
        │  │  │ (6502) │  │(Memory)│  │(2C02)│  │(2A03)│  │   │
        │  │  └───┬────┘  └───┬────┘  └──┬───┘  └──┬───┘  │   │
        │  │      │           │          │         │      │   │
        │  │      └───────────┴──────────┴─────────┘      │   │
        │  │                  │                           │   │
        │  │            ┌─────▼─────┐                     │   │
        │  │            │ Cartridge │                     │   │
        │  │            │  (Mapper) │                     │   │
        │  │            └───────────┘                     │   │
        │  └──────────────────────────────────────────────┘   │
        │                                                     │
        │  ┌──────────────────────────────────────────────┐   │
        │  │         Advanced Features Layer              │   │
        │  │  ┌───────┐ ┌─────────┐ ┌─────┐ ┌──────────┐  │   │
        │  │  │Retro  │ │ Netplay │ │ TAS │ │   Lua    │  │   │
        │  │  │Achieve│ │ (GGPO)  │ │Rec. │ │Scripting │  │   │
        │  │  └───────┘ └─────────┘ └─────┘ └──────────┘  │   │
        │  │  ┌──────┐ ┌─────────┐ ┌──────┐ ┌──────────┐  │   │
        │  │  │Debug │ │ Rewind  │ │Cheats│ │ Shaders  │  │   │
        │  │  └──────┘ └─────────┘ └──────┘ └──────────┘  │   │
        │  └──────────────────────────────────────────────┘   │
        └─────────────────────────────────────────────────────┘
```

### Timing Model

The NES operates on a **master clock** of 21.477272 MHz (NTSC) divided into:
- **CPU Clock**: 21.477272 MHz ÷ 12 = **1.789773 MHz** (~559 ns/cycle)
- **PPU Clock**: 21.477272 MHz ÷ 4 = **5.369318 MHz** (~186 ns/dot)
- **APU Clock**: Same as CPU (1.789773 MHz)

**Ratio:** 3 PPU dots per 1 CPU cycle (exact, no drift)

#### Master Clock Implementation

```rust
pub struct Console {
    cpu: Cpu,
    ppu: Ppu,
    apu: Apu,
    bus: Bus,
    cartridge: Box<dyn Mapper>,

    master_clock: u64,      // Total dots elapsed (PPU resolution)
    cpu_cycles: u64,        // CPU cycles completed
    frame_count: u64,       // Frames rendered
}

impl Console {
    /// Execute one CPU instruction
    pub fn step(&mut self) -> u8 {
        // CPU executes one instruction (2-7 cycles)
        let cpu_cycles = self.cpu.step(&mut self.bus);

        // PPU runs 3x as fast (3 dots per CPU cycle)
        for _ in 0..(cpu_cycles * 3) {
            self.ppu.step(&mut self.bus, &mut self.cartridge);

            // Check for NMI (VBlank start)
            if self.ppu.nmi_triggered() {
                self.cpu.trigger_nmi();
            }
        }

        // APU runs at CPU speed
        self.apu.step(cpu_cycles);

        // Mappers may have IRQ counters
        self.cartridge.clock(cpu_cycles);
        if self.cartridge.irq_pending() {
            self.cpu.trigger_irq();
        }

        self.master_clock += (cpu_cycles * 3) as u64;
        self.cpu_cycles += cpu_cycles as u64;

        cpu_cycles
    }

    /// Run until frame complete (29780 CPU cycles, 89341 PPU dots)
    pub fn step_frame(&mut self) {
        let target = self.frame_count + 1;
        while self.frame_count < target {
            self.step();
            if self.ppu.frame_complete() {
                self.frame_count += 1;
            }
        }
    }
}
```

### Memory Map

```
CPU Address Space (16-bit):
$0000-$07FF   2KB Internal RAM (mirrored 4x to $1FFF)
$2000-$2007   PPU Registers (mirrored to $3FFF)
$4000-$4017   APU & I/O Registers
$4018-$401F   APU/IO test mode (usually disabled)
$4020-$FFFF   Cartridge space (PRG-ROM, PRG-RAM, mapper)
    $6000-$7FFF   Battery-backed SRAM (8KB typical)
    $8000-$FFFF   PRG-ROM banks (32KB typical)

PPU Address Space (14-bit):
$0000-$0FFF   Pattern Table 0 (CHR)
$1000-$1FFF   Pattern Table 1 (CHR)
$2000-$23FF   Nametable 0
$2400-$27FF   Nametable 1
$2800-$2BFF   Nametable 2
$2C00-$2FFF   Nametable 3
$3000-$3EFF   Mirrors of nametables
$3F00-$3F1F   Palette RAM (32 bytes)
$3F20-$3FFF   Mirrors of palette
```

### Data Flow

**Frame Execution Cycle:**

1. **CPU Fetch** → Instruction from bus → Decode → Execute
2. **Memory Access** → Bus routes to RAM/PPU/APU/Cartridge
3. **PPU Rendering** → 3 dots per CPU cycle → Pixel output
4. **APU Synthesis** → Generate audio samples
5. **Mapper Logic** → Banking, IRQ counters, special hardware
6. **NMI/IRQ** → Interrupt CPU at precise cycles
7. **Frame Complete** → Output video + audio buffers

---

## Core Engine Architecture

### CPU Implementation (6502)

#### Design Goals

1. **Cycle Accuracy**: Exact cycle count for every instruction
2. **Dummy Reads/Writes**: Emulate bus activity for timing-sensitive code
3. **Interrupt Timing**: Precise IRQ/NMI handling with polling delays
4. **Unofficial Opcodes**: Full support for illegal/undocumented instructions

#### CPU State

```rust
pub struct Cpu {
    // Registers
    pub a: u8,              // Accumulator
    pub x: u8,              // X index
    pub y: u8,              // Y index
    pub sp: u8,             // Stack pointer (0x0100 + sp)
    pub pc: u16,            // Program counter
    pub p: Status,          // Processor status flags

    // Interrupt state
    nmi_pending: bool,
    irq_pending: bool,
    irq_line: bool,

    // Cycle tracking
    cycles: u64,            // Total cycles executed
    cycle_count: u8,        // Cycles remaining in current instruction

    // DMA state
    dma_active: bool,
    dma_page: u8,
    dma_addr: u8,
    dma_cycles: u16,
}

bitflags! {
    pub struct Status: u8 {
        const CARRY     = 0b0000_0001;  // C
        const ZERO      = 0b0000_0010;  // Z
        const INTERRUPT = 0b0000_0100;  // I
        const DECIMAL   = 0b0000_1000;  // D (not used on NES)
        const BREAK     = 0b0001_0000;  // B
        const UNUSED    = 0b0010_0000;  // Always 1
        const OVERFLOW  = 0b0100_0000;  // V
        const NEGATIVE  = 0b1000_0000;  // N
    }
}
```

#### Instruction Dispatch

**Table-Driven Approach** (Mesen2 style):

```rust
type InstructionFn = fn(&mut Cpu, &mut Bus, AddressingMode) -> u8;

pub struct Cpu {
    // ... fields ...

    // Lookup tables (256 entries each)
    instruction_table: [InstructionFn; 256],
    addressing_mode_table: [AddressingMode; 256],
    cycle_table: [u8; 256],
}

impl Cpu {
    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        // Check for interrupts before instruction fetch
        if self.nmi_pending {
            return self.handle_nmi(bus);
        }
        if self.irq_pending && !self.p.contains(Status::INTERRUPT) {
            return self.handle_irq(bus);
        }

        // Fetch opcode
        let opcode = self.read(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);

        // Dispatch via lookup tables
        let addr_mode = self.addressing_mode_table[opcode as usize];
        let instruction = self.instruction_table[opcode as usize];
        let base_cycles = self.cycle_table[opcode as usize];

        // Execute instruction (returns extra cycles for page crossing)
        let extra_cycles = instruction(self, bus, addr_mode);

        base_cycles + extra_cycles
    }
}
```

#### Addressing Modes

```rust
#[derive(Copy, Clone, Debug)]
pub enum AddressingMode {
    Implied,            // INX
    Accumulator,        // ASL A
    Immediate,          // LDA #$10
    ZeroPage,           // LDA $10
    ZeroPageX,          // LDA $10,X
    ZeroPageY,          // LDA $10,Y
    Absolute,           // LDA $1234
    AbsoluteX,          // LDA $1234,X
    AbsoluteY,          // LDA $1234,Y
    Indirect,           // JMP ($1234)
    IndexedIndirect,    // LDA ($10,X)
    IndirectIndexed,    // LDA ($10),Y
    Relative,           // BEQ label
}

impl Cpu {
    /// Get effective address and handle page crossing
    fn get_address(&mut self, bus: &mut Bus, mode: AddressingMode)
        -> (u16, bool)
    {
        match mode {
            AddressingMode::Immediate => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (addr, false)
            }

            AddressingMode::ZeroPage => {
                let addr = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                (addr, false)
            }

            AddressingMode::ZeroPageX => {
                let base = self.read(bus, self.pc);
                self.pc = self.pc.wrapping_add(1);
                // Dummy read during index calculation
                self.read(bus, base as u16);
                let addr = base.wrapping_add(self.x) as u16;
                (addr, false)
            }

            AddressingMode::Absolute => {
                let lo = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                (hi << 8 | lo, false)
            }

            AddressingMode::AbsoluteX => {
                let lo = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let hi = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);

                let base = hi << 8 | lo;
                let addr = base.wrapping_add(self.x as u16);

                // Page crossing adds 1 cycle for reads
                let page_crossed = (base & 0xFF00) != (addr & 0xFF00);

                if page_crossed {
                    // Dummy read from wrong page
                    let dummy_addr = (base & 0xFF00) | (addr & 0x00FF);
                    self.read(bus, dummy_addr);
                }

                (addr, page_crossed)
            }

            AddressingMode::IndirectIndexed => {
                // LDA ($10),Y - 5-6 cycles
                let ptr = self.read(bus, self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);

                // Read pointer from zero page (wraps at page boundary)
                let lo = self.read(bus, ptr) as u16;
                let hi = self.read(bus, (ptr + 1) & 0xFF) as u16;
                let base = hi << 8 | lo;

                let addr = base.wrapping_add(self.y as u16);
                let page_crossed = (base & 0xFF00) != (addr & 0xFF00);

                if page_crossed {
                    let dummy_addr = (base & 0xFF00) | (addr & 0x00FF);
                    self.read(bus, dummy_addr);
                }

                (addr, page_crossed)
            }

            // ... other modes ...
        }
    }
}
```

#### Example Instructions

```rust
impl Cpu {
    /// LDA - Load Accumulator (2-5 cycles depending on mode)
    fn lda(&mut self, bus: &mut Bus, mode: AddressingMode) -> u8 {
        let (addr, page_crossed) = self.get_address(bus, mode);
        self.a = self.read(bus, addr);

        self.set_zero_negative(self.a);

        // Add extra cycle if page crossed (AbsoluteX/Y, IndirectIndexed)
        if page_crossed { 1 } else { 0 }
    }

    /// STA - Store Accumulator (3-5 cycles, no page cross penalty)
    fn sta(&mut self, bus: &mut Bus, mode: AddressingMode) -> u8 {
        let (addr, _) = self.get_address(bus, mode);
        self.write(bus, addr, self.a);
        0  // Writes always take full cycles
    }

    /// ADC - Add with Carry (2-5 cycles)
    fn adc(&mut self, bus: &mut Bus, mode: AddressingMode) -> u8 {
        let (addr, page_crossed) = self.get_address(bus, mode);
        let value = self.read(bus, addr);

        let carry = if self.p.contains(Status::CARRY) { 1 } else { 0 };
        let sum = (self.a as u16) + (value as u16) + carry;

        // Set flags
        self.p.set(Status::CARRY, sum > 0xFF);
        self.p.set(Status::ZERO, (sum & 0xFF) == 0);
        self.p.set(Status::NEGATIVE, (sum & 0x80) != 0);

        // Overflow: (A^result) & (M^result) & 0x80
        let result = sum as u8;
        let overflow = (self.a ^ result) & (value ^ result) & 0x80 != 0;
        self.p.set(Status::OVERFLOW, overflow);

        self.a = result;

        if page_crossed { 1 } else { 0 }
    }

    /// RMW instructions (Read-Modify-Write) like INC, DEC, ASL, ROL
    /// These take 6 cycles for absolute, 7 for absolute indexed
    fn inc(&mut self, bus: &mut Bus, mode: AddressingMode) -> u8 {
        let (addr, _) = self.get_address(bus, mode);

        // Cycle 1-2: Read address
        let value = self.read(bus, addr);

        // Cycle 3: Write original value back (dummy write)
        self.write(bus, addr, value);

        // Cycle 4: Write modified value
        let result = value.wrapping_add(1);
        self.write(bus, addr, result);

        self.set_zero_negative(result);
        0
    }
}
```

#### Interrupt Handling

```rust
impl Cpu {
    pub fn trigger_nmi(&mut self) {
        self.nmi_pending = true;
    }

    pub fn trigger_irq(&mut self) {
        self.irq_line = true;
    }

    fn handle_nmi(&mut self, bus: &mut Bus) -> u8 {
        self.nmi_pending = false;

        // Dummy read at current PC
        self.read(bus, self.pc);

        // Push PC high byte
        self.push(bus, (self.pc >> 8) as u8);

        // Push PC low byte
        self.push(bus, self.pc as u8);

        // Push status (B flag clear, unused set)
        let status = (self.p.bits() & !0x10) | 0x20;
        self.push(bus, status);

        // Set interrupt disable
        self.p.insert(Status::INTERRUPT);

        // Read NMI vector
        let lo = self.read(bus, 0xFFFA) as u16;
        let hi = self.read(bus, 0xFFFB) as u16;
        self.pc = (hi << 8) | lo;

        7  // NMI takes 7 cycles
    }

    fn handle_irq(&mut self, bus: &mut Bus) -> u8 {
        self.irq_pending = false;

        // Same as NMI but vector at $FFFE/F
        self.read(bus, self.pc);
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, self.pc as u8);

        let status = (self.p.bits() & !0x10) | 0x20;
        self.push(bus, status);
        self.p.insert(Status::INTERRUPT);

        let lo = self.read(bus, 0xFFFE) as u16;
        let hi = self.read(bus, 0xFFFF) as u16;
        self.pc = (hi << 8) | lo;

        7
    }
}
```

#### DMA (Direct Memory Access)

OAM DMA ($4014 write) suspends CPU for 513-514 cycles:

```rust
impl Cpu {
    pub fn trigger_oam_dma(&mut self, page: u8) {
        self.dma_active = true;
        self.dma_page = page;
        self.dma_addr = 0;
        // +1 if on odd cycle, +2 if on even
        self.dma_cycles = if self.cycles % 2 == 1 { 513 } else { 514 };
    }

    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        if self.dma_active {
            if self.dma_cycles > 0 {
                if self.dma_cycles <= 512 && self.dma_cycles % 2 == 0 {
                    // Read from CPU memory
                    let addr = ((self.dma_page as u16) << 8) | (self.dma_addr as u16);
                    let value = bus.read(addr);

                    // Write to PPU OAM
                    bus.write_ppu_oam(self.dma_addr, value);

                    self.dma_addr = self.dma_addr.wrapping_add(1);
                    if self.dma_addr == 0 {
                        self.dma_active = false;
                    }
                }
                self.dma_cycles -= 1;
                return 1;
            }
        }

        // Normal instruction execution
        // ... (as before)
    }
}
```

---

### PPU Implementation (2C02)

#### Design Goals

1. **Dot-Level Accuracy**: Render every pixel at correct cycle
2. **Scrolling Precision**: Accurate coarse/fine scroll behavior
3. **Sprite Rendering**: Sprite 0 hit, overflow flag, 8-sprite limit
4. **Race Conditions**: VBL flag read/write races, OAMADDR corruption

#### PPU State

```rust
pub struct Ppu {
    // Registers (CPU-visible at $2000-$2007)
    ctrl: PpuCtrl,          // $2000 PPUCTRL
    mask: PpuMask,          // $2001 PPUMASK
    status: PpuStatus,      // $2002 PPUSTATUS
    oam_addr: u8,           // $2003 OAMADDR

    // Internal state
    v: VramAddress,         // Current VRAM address (15 bits)
    t: VramAddress,         // Temporary VRAM address (latch)
    fine_x: u8,             // Fine X scroll (3 bits)
    w: bool,                // Write toggle (first/second write)

    // Rendering state
    scanline: u16,          // Current scanline (0-261)
    dot: u16,               // Current dot (0-340)
    frame: u64,             // Frame counter
    odd_frame: bool,        // Odd/even frame flag

    // Buffers
    vram: [u8; 2048],       // Internal VRAM (nametables, 2KB)
    palette: [u8; 32],      // Palette RAM
    oam: [u8; 256],         // Object Attribute Memory (sprites)
    secondary_oam: [u8; 32], // Sprite evaluation buffer

    // Pixel output
    framebuffer: [u8; 256 * 240],  // Final pixel colors (palette indices)

    // NMI state
    nmi_occurred: bool,
    nmi_output: bool,
    nmi_pending: bool,
}
```

#### PPU Registers

```rust
bitflags! {
    struct PpuCtrl: u8 {
        const NAMETABLE_X       = 0b0000_0001;  // Base nametable X
        const NAMETABLE_Y       = 0b0000_0010;  // Base nametable Y
        const INCREMENT_MODE    = 0b0000_0100;  // VRAM increment (1 or 32)
        const SPRITE_TABLE      = 0b0000_1000;  // Sprite pattern table
        const BACKGROUND_TABLE  = 0b0001_0000;  // Background pattern table
        const SPRITE_SIZE       = 0b0010_0000;  // 8x8 or 8x16 sprites
        const MASTER_SLAVE      = 0b0100_0000;  // PPU master/slave
        const NMI_ENABLE        = 0b1000_0000;  // NMI at VBlank start
    }
}

bitflags! {
    struct PpuMask: u8 {
        const GRAYSCALE         = 0b0000_0001;  // Grayscale mode
        const SHOW_BG_LEFT      = 0b0000_0010;  // Show background in leftmost 8px
        const SHOW_SPRITES_LEFT = 0b0000_0100;  // Show sprites in leftmost 8px
        const SHOW_BACKGROUND   = 0b0000_1000;  // Enable background rendering
        const SHOW_SPRITES      = 0b0001_0000;  // Enable sprite rendering
        const EMPHASIZE_RED     = 0b0010_0000;  // Emphasize red
        const EMPHASIZE_GREEN   = 0b0100_0000;  // Emphasize green
        const EMPHASIZE_BLUE    = 0b1000_0000;  // Emphasize blue
    }
}

bitflags! {
    struct PpuStatus: u8 {
        const SPRITE_OVERFLOW   = 0b0010_0000;  // >8 sprites on scanline
        const SPRITE_ZERO_HIT   = 0b0100_0000;  // Sprite 0 collision
        const VBLANK            = 0b1000_0000;  // VBlank flag
    }
}
```

#### VRAM Address Structure

```rust
/// PPU VRAM address (15 bits)
/// yyy NN YYYYY XXXXX
/// ||| || ||||| +++++-- coarse X scroll (0-31)
/// ||| || +++++-------- coarse Y scroll (0-31)
/// ||| ++-------------- nametable select (0-3)
/// +++----------------- fine Y scroll (0-7)
#[derive(Copy, Clone, Debug)]
struct VramAddress(u16);

impl VramAddress {
    fn coarse_x(&self) -> u8 {
        (self.0 & 0x1F) as u8
    }

    fn coarse_y(&self) -> u8 {
        ((self.0 >> 5) & 0x1F) as u8
    }

    fn nametable_x(&self) -> u8 {
        ((self.0 >> 10) & 1) as u8
    }

    fn nametable_y(&self) -> u8 {
        ((self.0 >> 11) & 1) as u8
    }

    fn fine_y(&self) -> u8 {
        ((self.0 >> 12) & 0x7) as u8
    }

    fn set_coarse_x(&mut self, x: u8) {
        self.0 = (self.0 & !0x1F) | (x as u16 & 0x1F);
    }

    fn set_coarse_y(&mut self, y: u8) {
        self.0 = (self.0 & !0x3E0) | ((y as u16 & 0x1F) << 5);
    }

    fn increment_coarse_x(&mut self) {
        if self.coarse_x() == 31 {
            self.set_coarse_x(0);
            // Flip horizontal nametable
            self.0 ^= 0x0400;
        } else {
            self.0 += 1;
        }
    }

    fn increment_y(&mut self) {
        let mut fine_y = self.fine_y();
        if fine_y < 7 {
            fine_y += 1;
            self.0 = (self.0 & !0x7000) | ((fine_y as u16) << 12);
        } else {
            fine_y = 0;
            self.0 &= !0x7000;

            let mut coarse_y = self.coarse_y();
            if coarse_y == 29 {
                coarse_y = 0;
                self.0 ^= 0x0800;  // Flip vertical nametable
            } else if coarse_y == 31 {
                coarse_y = 0;  // Out of bounds, wrap without nametable flip
            } else {
                coarse_y += 1;
            }
            self.set_coarse_y(coarse_y);
        }
    }
}
```

#### PPU Rendering Cycle

**Scanline Breakdown:**
- **Scanlines 0-239**: Visible scanlines (rendering)
- **Scanline 240**: Post-render scanline (idle)
- **Scanlines 241-260**: VBlank (NMI occurs at 241, dot 1)
- **Scanline 261**: Pre-render scanline (setup for next frame)

**Dot Breakdown (per scanline):**
- **Dots 0**: Idle
- **Dots 1-256**: Visible pixels + tile fetches for next scanline
- **Dots 257-320**: Sprite fetches for next scanline
- **Dots 321-336**: First two tiles of next scanline
- **Dots 337-340**: Unused nametable fetches

```rust
impl Ppu {
    pub fn step(&mut self, bus: &mut Bus, cartridge: &mut dyn Mapper) {
        // Rendering enabled check
        let rendering = self.mask.contains(PpuMask::SHOW_BACKGROUND)
                     || self.mask.contains(PpuMask::SHOW_SPRITES);

        match self.scanline {
            0..=239 => {
                // Visible scanlines
                self.render_scanline(bus, cartridge, rendering);
            }

            240 => {
                // Post-render idle scanline
            }

            241 => {
                if self.dot == 1 {
                    // Set VBlank flag
                    self.status.insert(PpuStatus::VBLANK);
                    self.nmi_occurred = true;

                    if self.ctrl.contains(PpuCtrl::NMI_ENABLE) {
                        self.nmi_pending = true;
                    }
                }
            }

            261 => {
                // Pre-render scanline
                if self.dot == 1 {
                    // Clear VBlank, sprite 0 hit, sprite overflow
                    self.status.remove(PpuStatus::VBLANK);
                    self.status.remove(PpuStatus::SPRITE_ZERO_HIT);
                    self.status.remove(PpuStatus::SPRITE_OVERFLOW);
                    self.nmi_occurred = false;
                }

                if rendering {
                    // Same tile fetching as visible scanlines
                    self.render_scanline(bus, cartridge, true);

                    // Odd frame skip (dot 339 -> 0 on scanline 0)
                    if self.dot == 339 && self.odd_frame {
                        self.dot = 340;  // Will wrap to 0 next increment
                    }
                }
            }

            _ => {}
        }

        // Advance to next dot/scanline
        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;

            if self.scanline > 261 {
                self.scanline = 0;
                self.frame += 1;
                self.odd_frame = !self.odd_frame;
            }
        }
    }

    fn render_scanline(&mut self, bus: &mut Bus, cartridge: &mut dyn Mapper, rendering: bool) {
        match self.dot {
            1..=256 => {
                // Background rendering
                if rendering && self.mask.contains(PpuMask::SHOW_BACKGROUND) {
                    self.fetch_background_tile(bus, cartridge);

                    // Output pixel
                    if self.scanline < 240 {
                        let x = self.dot - 1;
                        let pixel = self.get_background_pixel();
                        self.framebuffer[(self.scanline as usize) * 256 + x as usize] = pixel;
                    }

                    // Increment X every 8 dots
                    if self.dot % 8 == 0 {
                        self.v.increment_coarse_x();
                    }
                }

                // Increment Y at end of scanline
                if self.dot == 256 && rendering {
                    self.v.increment_y();
                }
            }

            257 => {
                // Copy horizontal position from T to V
                if rendering {
                    self.v.0 = (self.v.0 & !0x041F) | (self.t.0 & 0x041F);
                }

                // Start sprite evaluation for next scanline
                self.evaluate_sprites();
            }

            280..=304 => {
                // Pre-render scanline: copy vertical position from T to V
                if self.scanline == 261 && rendering {
                    self.v.0 = (self.v.0 & !0x7BE0) | (self.t.0 & 0x7BE0);
                }
            }

            321..=336 => {
                // Fetch first two tiles of next scanline
                if rendering && self.mask.contains(PpuMask::SHOW_BACKGROUND) {
                    self.fetch_background_tile(bus, cartridge);

                    if self.dot % 8 == 0 {
                        self.v.increment_coarse_x();
                    }
                }
            }

            _ => {}
        }
    }
}
```

#### Background Rendering

**Tile Fetch Cycle (8 dots per tile):**
1. **Dot 0**: Fetch nametable byte (tile index)
2. **Dot 2**: Fetch attribute byte (palette)
3. **Dot 4**: Fetch pattern low byte
4. **Dot 6**: Fetch pattern high byte
5. **Dots 7**: Load shift registers

```rust
impl Ppu {
    fn fetch_background_tile(&mut self, bus: &mut Bus, cartridge: &mut dyn Mapper) {
        let cycle = (self.dot - 1) % 8;

        match cycle {
            0 => {
                // Fetch nametable byte
                let addr = 0x2000 | (self.v.0 & 0x0FFF);
                self.tile_index = self.read_vram(bus, cartridge, addr);
            }

            2 => {
                // Fetch attribute byte
                let addr = 0x23C0
                    | (self.v.0 & 0x0C00)  // Nametable select
                    | ((self.v.0 >> 4) & 0x38)  // Coarse Y / 4
                    | ((self.v.0 >> 2) & 0x07); // Coarse X / 4

                let attribute = self.read_vram(bus, cartridge, addr);

                // Select 2-bit palette based on position within 4x4 tile group
                let shift_x = (self.v.coarse_x() & 2) != 0;
                let shift_y = (self.v.coarse_y() & 2) != 0;
                let shift = (shift_x as u8) * 2 + (shift_y as u8) * 4;

                self.tile_palette = (attribute >> shift) & 0x03;
            }

            4 => {
                // Fetch pattern low byte
                let table = if self.ctrl.contains(PpuCtrl::BACKGROUND_TABLE) {
                    0x1000
                } else {
                    0x0000
                };

                let addr = table
                    + (self.tile_index as u16) * 16
                    + self.v.fine_y() as u16;

                self.tile_low = self.read_vram(bus, cartridge, addr);
            }

            6 => {
                // Fetch pattern high byte (8 bytes after low)
                let table = if self.ctrl.contains(PpuCtrl::BACKGROUND_TABLE) {
                    0x1000
                } else {
                    0x0000
                };

                let addr = table
                    + (self.tile_index as u16) * 16
                    + self.v.fine_y() as u16
                    + 8;

                self.tile_high = self.read_vram(bus, cartridge, addr);
            }

            7 => {
                // Load shift registers
                self.bg_shift_low |= self.tile_low as u16;
                self.bg_shift_high |= self.tile_high as u16;
                self.bg_shift_palette |= (self.tile_palette as u16) * 0b01010101;
            }

            _ => {}
        }
    }

    fn get_background_pixel(&mut self) -> u8 {
        if !self.mask.contains(PpuMask::SHOW_BACKGROUND) {
            return 0;
        }

        // Select bit from shift register based on fine_x
        let bit_select = 0x8000 >> self.fine_x;

        let pixel_low = (self.bg_shift_low & bit_select) != 0;
        let pixel_high = (self.bg_shift_high & bit_select) != 0;
        let pixel = (pixel_high as u8) << 1 | (pixel_low as u8);

        if pixel == 0 {
            return 0;  // Transparent (use backdrop color)
        }

        // Get palette from attribute shift register
        let palette = ((self.bg_shift_palette >> (14 - self.fine_x * 2)) & 0x03) as u8;

        // Palette index: palette * 4 + pixel
        self.palette[((palette * 4 + pixel) as usize)]
    }
}
```

#### Sprite Rendering

**Sprite Evaluation (Dot 257-320):**
- Check all 64 OAM entries for sprites on next scanline
- Copy up to 8 sprites to secondary OAM
- Set sprite overflow flag if >8 found (with hardware bug)

**Sprite Fetching (Dot 257-320):**
- Fetch pattern data for 8 sprites (8 dots per sprite)
- 4 memory accesses per sprite: Y, tile, attribute, X

```rust
#[derive(Copy, Clone, Debug)]
struct Sprite {
    y: u8,          // Y position (top of sprite)
    tile: u8,       // Tile index
    attr: u8,       // Attributes (palette, priority, flip)
    x: u8,          // X position (left edge)
}

impl Ppu {
    fn evaluate_sprites(&mut self) {
        self.sprite_count = 0;
        self.secondary_oam.fill(0xFF);

        let scanline = self.scanline as i16;
        let sprite_height = if self.ctrl.contains(PpuCtrl::SPRITE_SIZE) {
            16  // 8x16 sprites
        } else {
            8   // 8x8 sprites
        };

        for i in 0..64 {
            let offset = i * 4;
            let y = self.oam[offset] as i16;

            // Check if sprite is on next scanline
            let diff = scanline - y;
            if diff >= 0 && diff < sprite_height {
                if self.sprite_count < 8 {
                    // Copy to secondary OAM
                    let dest = self.sprite_count * 4;
                    self.secondary_oam[dest..dest+4]
                        .copy_from_slice(&self.oam[offset..offset+4]);

                    if i == 0 {
                        self.sprite_zero_on_line = true;
                    }

                    self.sprite_count += 1;
                } else {
                    // Sprite overflow (with hardware bug emulation)
                    self.status.insert(PpuStatus::SPRITE_OVERFLOW);
                    break;
                }
            }
        }
    }

    fn fetch_sprite_data(&mut self, bus: &mut Bus, cartridge: &mut dyn Mapper) {
        for i in 0..8 {
            if i >= self.sprite_count {
                // Fill with dummy sprite
                self.sprites[i] = Sprite {
                    y: 0xFF, tile: 0xFF, attr: 0xFF, x: 0xFF
                };
                continue;
            }

            let offset = i * 4;
            let y = self.secondary_oam[offset];
            let tile = self.secondary_oam[offset + 1];
            let attr = self.secondary_oam[offset + 2];
            let x = self.secondary_oam[offset + 3];

            self.sprites[i] = Sprite { y, tile, attr, x };

            // Fetch pattern data
            let row = (self.scanline as u8).wrapping_sub(y);
            let flipped_v = (attr & 0x80) != 0;
            let sprite_row = if flipped_v {
                if self.ctrl.contains(PpuCtrl::SPRITE_SIZE) {
                    15 - row
                } else {
                    7 - row
                }
            } else {
                row
            };

            let (table, tile_index) = if self.ctrl.contains(PpuCtrl::SPRITE_SIZE) {
                // 8x16 sprites: bit 0 of tile selects pattern table
                let table = if (tile & 1) != 0 { 0x1000 } else { 0x0000 };
                let tile_idx = tile & 0xFE;
                (table, tile_idx)
            } else {
                // 8x8 sprites: PPUCTRL selects pattern table
                let table = if self.ctrl.contains(PpuCtrl::SPRITE_TABLE) {
                    0x1000
                } else {
                    0x0000
                };
                (table, tile)
            };

            let addr = table + (tile_index as u16) * 16 + sprite_row as u16;
            let low = self.read_vram(bus, cartridge, addr);
            let high = self.read_vram(bus, cartridge, addr + 8);

            self.sprite_patterns[i] = (low, high);
        }
    }

    fn get_sprite_pixel(&self, x: u16) -> (u8, bool) {
        if !self.mask.contains(PpuMask::SHOW_SPRITES) {
            return (0, false);
        }

        for i in 0..self.sprite_count {
            let sprite = self.sprites[i];
            let sprite_x = sprite.x as u16;

            if x < sprite_x || x >= sprite_x + 8 {
                continue;  // Not in range
            }

            let offset = (x - sprite_x) as u8;
            let flipped_h = (sprite.attr & 0x40) != 0;
            let bit = if flipped_h { offset } else { 7 - offset };

            let (low, high) = self.sprite_patterns[i];
            let pixel = ((high >> bit) & 1) << 1 | ((low >> bit) & 1);

            if pixel != 0 {
                let palette = (sprite.attr & 0x03) + 4;  // Sprite palettes 4-7
                let color = self.palette[(palette * 4 + pixel) as usize];

                let priority = (sprite.attr & 0x20) == 0;  // 0 = in front
                let sprite_zero = i == 0 && self.sprite_zero_on_line;

                return (color, sprite_zero);
            }
        }

        (0, false)  // Transparent
    }

    fn composite_pixel(&mut self, x: u16) -> u8 {
        let bg_pixel = self.get_background_pixel();
        let (sprite_pixel, sprite_zero) = self.get_sprite_pixel(x);

        // Sprite 0 hit detection
        if sprite_zero && bg_pixel != 0 && sprite_pixel != 0 {
            if x != 255 && self.mask.contains(PpuMask::SHOW_BACKGROUND | PpuMask::SHOW_SPRITES) {
                self.status.insert(PpuStatus::SPRITE_ZERO_HIT);
            }
        }

        // Priority multiplexer
        if bg_pixel == 0 && sprite_pixel != 0 {
            sprite_pixel
        } else if bg_pixel != 0 && sprite_pixel == 0 {
            bg_pixel
        } else if bg_pixel != 0 && sprite_pixel != 0 {
            // Both opaque: check priority
            let sprite = self.sprites[0];  // First opaque sprite
            if (sprite.attr & 0x20) == 0 {
                sprite_pixel  // Sprite in front
            } else {
                bg_pixel      // Background in front
            }
        } else {
            // Both transparent: use backdrop color
            self.palette[0]
        }
    }
}
```

---

### APU Implementation (2A03)

#### Design Goals

1. **Hardware-Accurate Mixing**: Match NES audio output characteristics
2. **Expansion Audio**: VRC6, VRC7, MMC5, N163, S5B, FDS
3. **Low-Latency Output**: Minimal audio buffering for responsive gameplay
4. **High-Quality Resampling**: Sinc interpolation for smooth 48kHz output

#### APU State

```rust
pub struct Apu {
    // Channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    // Frame counter
    frame_counter: FrameCounter,
    frame_irq: bool,

    // Cycle tracking
    cycles: u64,

    // Audio output
    sample_rate: f32,       // Output sample rate (48000 Hz)
    cpu_clock: f32,         // CPU clock (1789773 Hz)
    sample_accumulator: f32,
    output_buffer: Vec<f32>,
}

struct PulseChannel {
    enabled: bool,
    duty: u8,               // Duty cycle (0-3 = 12.5%, 25%, 50%, 75%)
    length_counter: u8,
    envelope: Envelope,
    sweep: Sweep,
    timer: u16,
    timer_period: u16,
    sequence_pos: u8,
}

struct TriangleChannel {
    enabled: bool,
    length_counter: u8,
    linear_counter: u8,
    linear_counter_reload: u8,
    control_flag: bool,
    timer: u16,
    timer_period: u16,
    sequence_pos: u8,
}

struct NoiseChannel {
    enabled: bool,
    length_counter: u8,
    envelope: Envelope,
    mode: bool,             // Short/long LFSR mode
    timer: u16,
    timer_period: u16,
    lfsr: u16,              // 15-bit shift register
}

struct DmcChannel {
    enabled: bool,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,
    sample_buffer: u8,
    sample_buffer_empty: bool,
    shift_register: u8,
    bits_remaining: u8,
    silence_flag: bool,
    output_level: u8,
    timer: u16,
    timer_period: u16,
    irq_enabled: bool,
    irq_flag: bool,
    loop_flag: bool,
}
```

#### Channel Implementation

```rust
impl Apu {
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000 => {
                // Pulse 1: Duty, loop, const vol, vol/env
                self.pulse1.duty = (value >> 6) & 0x03;
                self.pulse1.envelope.loop_flag = (value & 0x20) != 0;
                self.pulse1.envelope.constant_flag = (value & 0x10) != 0;
                self.pulse1.envelope.volume = value & 0x0F;
            }

            0x4001 => {
                // Pulse 1: Sweep
                self.pulse1.sweep.enabled = (value & 0x80) != 0;
                self.pulse1.sweep.period = (value >> 4) & 0x07;
                self.pulse1.sweep.negate = (value & 0x08) != 0;
                self.pulse1.sweep.shift = value & 0x07;
                self.pulse1.sweep.reload = true;
            }

            0x4002 => {
                // Pulse 1: Timer low
                self.pulse1.timer_period = (self.pulse1.timer_period & 0xFF00) | value as u16;
            }

            0x4003 => {
                // Pulse 1: Length counter, timer high
                self.pulse1.timer_period = (self.pulse1.timer_period & 0x00FF)
                    | ((value as u16 & 0x07) << 8);

                if self.pulse1.enabled {
                    self.pulse1.length_counter = LENGTH_TABLE[(value >> 3) as usize];
                }

                self.pulse1.sequence_pos = 0;
                self.pulse1.envelope.start = true;
            }

            0x4015 => {
                // Status register
                self.pulse1.enabled = (value & 0x01) != 0;
                self.pulse2.enabled = (value & 0x02) != 0;
                self.triangle.enabled = (value & 0x04) != 0;
                self.noise.enabled = (value & 0x08) != 0;
                self.dmc.enabled = (value & 0x10) != 0;

                if !self.pulse1.enabled {
                    self.pulse1.length_counter = 0;
                }
                // ... other channels ...

                self.dmc.irq_flag = false;
            }

            0x4017 => {
                // Frame counter
                self.frame_counter.mode = if (value & 0x80) != 0 {
                    FrameCounterMode::FiveStep
                } else {
                    FrameCounterMode::FourStep
                };

                self.frame_counter.irq_inhibit = (value & 0x40) != 0;

                if self.frame_counter.irq_inhibit {
                    self.frame_irq = false;
                }

                // Writing resets sequencer
                self.frame_counter.reset = true;
            }

            _ => {}
        }
    }

    pub fn step(&mut self, cpu_cycles: u8) {
        for _ in 0..cpu_cycles {
            self.step_cycle();
        }
    }

    fn step_cycle(&mut self) {
        // Frame counter (clocks every other CPU cycle)
        if self.cycles % 2 == 0 {
            self.frame_counter.step(
                &mut self.pulse1,
                &mut self.pulse2,
                &mut self.triangle,
                &mut self.noise,
            );

            if self.frame_counter.irq {
                self.frame_irq = true;
            }
        }

        // Pulse channels (every other cycle)
        if self.cycles % 2 == 0 {
            self.pulse1.step_timer();
            self.pulse2.step_timer();
        }

        // Triangle channel (every cycle)
        self.triangle.step_timer();

        // Noise channel (every other cycle)
        if self.cycles % 2 == 0 {
            self.noise.step_timer();
        }

        // DMC channel (every cycle)
        self.dmc.step_timer();

        // Mix and output sample
        self.mix_sample();

        self.cycles += 1;
    }

    fn mix_sample(&mut self) {
        // Hardware-accurate mixing
        let pulse_out = if self.pulse1.output() == 0 && self.pulse2.output() == 0 {
            0.0
        } else {
            95.88 / ((8128.0 / (self.pulse1.output() as f32 + self.pulse2.output() as f32)) + 100.0)
        };

        let tnd = self.triangle.output() as f32 * 3.0
                + self.noise.output() as f32 * 2.0
                + self.dmc.output() as f32;

        let tnd_out = if tnd == 0.0 {
            0.0
        } else {
            159.79 / ((1.0 / tnd) + 100.0)
        };

        let mixed = pulse_out + tnd_out;

        // Resample to output rate (48kHz from 1.789MHz)
        self.sample_accumulator += 1.0;

        let sample_threshold = self.cpu_clock / self.sample_rate;

        if self.sample_accumulator >= sample_threshold {
            self.output_buffer.push(mixed);
            self.sample_accumulator -= sample_threshold;
        }
    }
}

impl PulseChannel {
    fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if self.length_counter == 0 {
            return 0;
        }

        if self.timer_period < 8 || self.timer_period > 0x7FF {
            return 0;  // Ultrasonic/inaudible
        }

        // Duty cycle lookup
        const DUTY_TABLE: [[u8; 8]; 4] = [
            [0, 1, 0, 0, 0, 0, 0, 0],  // 12.5%
            [0, 1, 1, 0, 0, 0, 0, 0],  // 25%
            [0, 1, 1, 1, 1, 0, 0, 0],  // 50%
            [1, 0, 0, 1, 1, 1, 1, 1],  // 75% (negated 25%)
        ];

        if DUTY_TABLE[self.duty as usize][self.sequence_pos as usize] == 0 {
            return 0;
        }

        // Return envelope volume
        self.envelope.output()
    }

    fn step_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.sequence_pos = (self.sequence_pos + 1) % 8;
        } else {
            self.timer -= 1;
        }
    }
}

impl TriangleChannel {
    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.linear_counter == 0 {
            return 0;
        }

        // Triangle wave sequence (32 steps)
        const SEQUENCE: [u8; 32] = [
            15, 14, 13, 12, 11, 10,  9,  8,  7,  6,  5,  4,  3,  2,  1,  0,
             0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15,
        ];

        SEQUENCE[self.sequence_pos as usize]
    }

    fn step_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;

            if self.length_counter > 0 && self.linear_counter > 0 {
                self.sequence_pos = (self.sequence_pos + 1) % 32;
            }
        } else {
            self.timer -= 1;
        }
    }
}
```

---

(Continued in next part due to length...)

impl NoiseChannel {
    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 {
            return 0;
        }
        if (self.lfsr & 1) == 0 {
            self.envelope.output()
        } else {
            0
        }
    }

    fn step_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            let feedback = if self.mode {
                (self.lfsr & 1) ^ ((self.lfsr >> 6) & 1)
            } else {
                (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1)
            };
            self.lfsr >>= 1;
            self.lfsr |= feedback << 14;
        } else {
            self.timer -= 1;
        }
    }
}

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6,
    160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];
```

#### DMC Channel

```rust
impl DmcChannel {
    fn output(&self) -> u8 {
        self.output_level
    }

    fn step_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            if !self.silence_flag {
                if (self.shift_register & 1) == 1 {
                    if self.output_level <= 125 {
                        self.output_level += 2;
                    }
                } else {
                    if self.output_level >= 2 {
                        self.output_level -= 2;
                    }
                }
            }
            self.shift_register >>= 1;
            self.bits_remaining -= 1;
            if self.bits_remaining == 0 {
                self.bits_remaining = 8;
                if self.sample_buffer_empty {
                    self.silence_flag = true;
                } else {
                    self.silence_flag = false;
                    self.shift_register = self.sample_buffer;
                    self.sample_buffer_empty = true;
                }
            }
        } else {
            self.timer -= 1;
        }
    }
}
```

#### Frame Counter

```rust
struct FrameCounter {
    mode: FrameCounterMode,
    cycles: u16,
    irq_inhibit: bool,
    irq: bool,
}

#[derive(Copy, Clone, PartialEq)]
enum FrameCounterMode {
    FourStep,
    FiveStep,
}

impl FrameCounter {
    fn step(&mut self, pulse1: &mut PulseChannel, pulse2: &mut PulseChannel,
            triangle: &mut TriangleChannel, noise: &mut NoiseChannel) {
        self.cycles += 1;
        match self.mode {
            FrameCounterMode::FourStep => {
                match self.cycles {
                    7457 => self.clock_quarter_frame(pulse1, pulse2, triangle, noise),
                    14913 => {
                        self.clock_quarter_frame(pulse1, pulse2, triangle, noise);
                        self.clock_half_frame(pulse1, pulse2, triangle, noise);
                    }
                    22371 => self.clock_quarter_frame(pulse1, pulse2, triangle, noise),
                    29829 => {
                        if !self.irq_inhibit { self.irq = true; }
                        self.clock_quarter_frame(pulse1, pulse2, triangle, noise);
                        self.clock_half_frame(pulse1, pulse2, triangle, noise);
                    }
                    29830 => {
                        if !self.irq_inhibit { self.irq = true; }
                        self.cycles = 0;
                    }
                    _ => {}
                }
            }
            FrameCounterMode::FiveStep => {
                match self.cycles {
                    7457 => self.clock_quarter_frame(pulse1, pulse2, triangle, noise),
                    14913 => {
                        self.clock_quarter_frame(pulse1, pulse2, triangle, noise);
                        self.clock_half_frame(pulse1, pulse2, triangle, noise);
                    }
                    22371 => self.clock_quarter_frame(pulse1, pulse2, triangle, noise),
                    37281 => {
                        self.clock_quarter_frame(pulse1, pulse2, triangle, noise);
                        self.clock_half_frame(pulse1, pulse2, triangle, noise);
                    }
                    37282 => self.cycles = 0,
                    _ => {}
                }
            }
        }
    }

    fn clock_quarter_frame(&mut self, p1: &mut PulseChannel, p2: &mut PulseChannel,
                           tri: &mut TriangleChannel, noise: &mut NoiseChannel) {
        p1.envelope.step();
        p2.envelope.step();
        noise.envelope.step();
        if tri.control_flag {
            tri.linear_counter = tri.linear_counter_reload;
        } else if tri.linear_counter > 0 {
            tri.linear_counter -= 1;
        }
    }

    fn clock_half_frame(&mut self, p1: &mut PulseChannel, p2: &mut PulseChannel,
                        tri: &mut TriangleChannel, noise: &mut NoiseChannel) {
        if p1.length_counter > 0 && !p1.envelope.loop_flag {
            p1.length_counter -= 1;
        }
        if p2.length_counter > 0 && !p2.envelope.loop_flag {
            p2.length_counter -= 1;
        }
        if tri.length_counter > 0 && !tri.control_flag {
            tri.length_counter -= 1;
        }
        if noise.length_counter > 0 && !noise.envelope.loop_flag {
            noise.length_counter -= 1;
        }
        p1.sweep.step(&mut p1.timer_period);
        p2.sweep.step(&mut p2.timer_period);
    }
}
```

#### Expansion Audio

**VRC6 (Konami):**

```rust
pub struct Vrc6Audio {
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    sawtooth: Vrc6Sawtooth,
}

struct Vrc6Pulse {
    enabled: bool,
    volume: u8,
    duty: u8,
    timer: u16,
    timer_period: u16,
    sequence_pos: u8,
}

struct Vrc6Sawtooth {
    enabled: bool,
    accumulator: u8,
    accumulator_rate: u8,
    timer: u16,
    timer_period: u16,
    step: u8,
}

impl Vrc6Audio {
    fn output(&self) -> f32 {
        let p1 = if self.pulse1.enabled { self.pulse1.volume as f32 } else { 0.0 };
        let p2 = if self.pulse2.enabled { self.pulse2.volume as f32 } else { 0.0 };
        let saw = if self.sawtooth.enabled {
            (self.sawtooth.accumulator >> 3) as f32
        } else {
            0.0
        };
        (p1 + p2 + saw) / 63.0
    }
}
```

**MMC5 (Nintendo):**

```rust
pub struct Mmc5Audio {
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    pcm: u8,
}

impl Mmc5Audio {
    fn output(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let pcm = self.pcm as f32;
        (p1 + p2 + pcm) / 48.0
    }
}
```

**Namco 163:**

```rust
pub struct N163Audio {
    channels: Vec<N163Channel>,
    wave_ram: [u8; 128],
    num_channels: u8,
}

struct N163Channel {
    frequency: u32,
    phase: u32,
    length: u8,
    offset: u8,
    volume: u8,
}

impl N163Audio {
    fn output(&self) -> f32 {
        let mut sum = 0.0;
        for ch in &self.channels[0..self.num_channels as usize] {
            let sample_pos = ((ch.phase >> 16) as u8 + ch.offset) % ch.length;
            let sample = self.wave_ram[sample_pos as usize] & 0x0F;
            sum += (sample as f32) * (ch.volume as f32) / 15.0;
        }
        sum / (self.num_channels as f32)
    }
}
```

---

## Memory Subsystem

### Bus Architecture

```rust
pub struct Bus {
    ram: [u8; 2048],
    cartridge: Box<dyn Mapper>,
    ppu: Rc<RefCell<Ppu>>,
    apu: Rc<RefCell<Apu>>,
    controller1: Controller,
    controller2: Controller,
}

impl Bus {
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.borrow_mut().read_register(0x2000 + (addr & 0x0007)),
            0x4000..=0x4015 => self.apu.borrow_mut().read_register(addr),
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            0x4020..=0xFFFF => self.cartridge.read_prg(addr),
            _ => 0
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.borrow_mut().write_register(0x2000 + (addr & 0x0007), value),
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.borrow_mut().write_register(addr, value),
            0x4014 => self.ppu.borrow_mut().trigger_oam_dma(value),
            0x4016 => {
                self.controller1.write(value);
                self.controller2.write(value);
            }
            0x4020..=0xFFFF => self.cartridge.write_prg(addr, value),
            _ => {}
        }
    }
}
```

### Mapper Trait

```rust
pub trait Mapper: Send {
    fn read_prg(&mut self, addr: u16) -> u8;
    fn write_prg(&mut self, addr: u16, value: u8);
    fn read_chr(&mut self, addr: u16) -> u8;
    fn write_chr(&mut self, addr: u16, value: u8);
    fn clock(&mut self, cycles: u8);
    fn irq_pending(&self) -> bool;
    fn mirroring(&self) -> Mirroring;
    fn save_state(&self) -> Vec<u8>;
    fn load_state(&mut self, data: &[u8]);
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
}
```

### Mapper Implementations

#### Mapper 0 (NROM)

```rust
pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
}

impl Mapper for Mapper0 {
    fn read_prg(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                let index = offset % self.prg_rom.len();
                self.prg_rom[index]
            }
            _ => 0
        }
    }

    fn write_prg(&mut self, _addr: u16, _value: u8) {}

    fn read_chr(&mut self, addr: u16) -> u8 {
        if self.chr_rom.is_empty() {
            self.chr_ram[addr as usize]
        } else {
            self.chr_rom[addr as usize]
        }
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_rom.is_empty() {
            self.chr_ram[addr as usize] = value;
        }
    }

    fn clock(&mut self, _cycles: u8) {}
    fn irq_pending(&self) -> bool { false }
    fn mirroring(&self) -> Mirroring { self.mirroring }
    fn save_state(&self) -> Vec<u8> { Vec::new() }
    fn load_state(&mut self, _data: &[u8]) {}
}
```

#### Mapper 1 (MMC1)

```rust
pub struct Mapper1 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    shift_register: u8,
    shift_count: u8,
    mirroring: Mirroring,
    prg_mode: u8,
    chr_mode: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
}

impl Mapper for Mapper1 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if (value & 0x80) != 0 {
            self.shift_register = 0;
            self.shift_count = 0;
            self.prg_mode = 3;
            return;
        }
        self.shift_register |= (value & 1) << self.shift_count;
        self.shift_count += 1;
        if self.shift_count == 5 {
            match addr {
                0x8000..=0x9FFF => self.write_control(self.shift_register),
                0xA000..=0xBFFF => self.chr_bank0 = self.shift_register,
                0xC000..=0xDFFF => self.chr_bank1 = self.shift_register,
                0xE000..=0xFFFF => self.prg_bank = self.shift_register & 0x0F,
                _ => {}
            }
            self.shift_register = 0;
            self.shift_count = 0;
        }
    }

    fn read_prg(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank_size = 16384;
                let offset = (addr - 0x8000) as usize;
                match self.prg_mode {
                    0 | 1 => {
                        let bank = (self.prg_bank & 0xFE) as usize;
                        self.prg_rom[bank * bank_size + offset]
                    }
                    2 => {
                        if addr < 0xC000 {
                            self.prg_rom[offset]
                        } else {
                            let bank = self.prg_bank as usize;
                            self.prg_rom[bank * bank_size + (offset - bank_size)]
                        }
                    }
                    3 => {
                        if addr < 0xC000 {
                            let bank = self.prg_bank as usize;
                            self.prg_rom[bank * bank_size + offset]
                        } else {
                            let last_bank = (self.prg_rom.len() / bank_size) - 1;
                            self.prg_rom[last_bank * bank_size + (offset - bank_size)]
                        }
                    }
                    _ => 0
                }
            }
            _ => 0
        }
    }

    fn read_chr(&mut self, addr: u16) -> u8 {
        let bank_size = if self.chr_mode == 0 { 8192 } else { 4096 };
        if self.chr_mode == 0 {
            let bank = (self.chr_bank0 & 0xFE) as usize;
            self.chr_rom[bank * bank_size + addr as usize]
        } else {
            if addr < 0x1000 {
                let bank = self.chr_bank0 as usize;
                self.chr_rom[bank * bank_size + addr as usize]
            } else {
                let bank = self.chr_bank1 as usize;
                self.chr_rom[bank * bank_size + (addr - 0x1000) as usize]
            }
        }
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_rom.is_empty() {
            // CHR-RAM
        }
    }

    fn clock(&mut self, _cycles: u8) {}
    fn irq_pending(&self) -> bool { false }
    fn mirroring(&self) -> Mirroring { self.mirroring }
    fn save_state(&self) -> Vec<u8> { Vec::new() }
    fn load_state(&mut self, _data: &[u8]) {}
}
```

#### Mapper 4 (MMC3)

```rust
pub struct Mapper4 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    bank_select: u8,
    prg_mode: bool,
    chr_mode: bool,
    banks: [u8; 8],
    irq_counter: u8,
    irq_reload: u8,
    irq_enabled: bool,
    irq_pending: bool,
    irq_reload_pending: bool,
    last_a12: bool,
    a12_filter: u8,
    mirroring: Mirroring,
}

impl Mapper for Mapper4 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    self.bank_select = value & 0x07;
                    self.prg_mode = (value & 0x40) != 0;
                    self.chr_mode = (value & 0x80) != 0;
                } else {
                    self.banks[self.bank_select as usize] = value;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    self.mirroring = if (value & 1) == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_reload = value;
                } else {
                    self.irq_reload_pending = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn read_prg(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank_size = 8192;
                let bank = match addr {
                    0x8000..=0x9FFF => {
                        if self.prg_mode {
                            (self.prg_rom.len() / bank_size) - 2
                        } else {
                            self.banks[6] as usize
                        }
                    }
                    0xA000..=0xBFFF => self.banks[7] as usize,
                    0xC000..=0xDFFF => {
                        if self.prg_mode {
                            self.banks[6] as usize
                        } else {
                            (self.prg_rom.len() / bank_size) - 2
                        }
                    }
                    0xE000..=0xFFFF => (self.prg_rom.len() / bank_size) - 1,
                    _ => 0
                };
                let offset = (addr as usize % bank_size);
                self.prg_rom[bank * bank_size + offset]
            }
            _ => 0
        }
    }

    fn read_chr(&mut self, addr: u16) -> u8 {
        let a12 = (addr & 0x1000) != 0;
        if a12 && !self.last_a12 {
            self.a12_filter += 1;
            if self.a12_filter >= 2 {
                self.clock_irq();
                self.a12_filter = 0;
            }
        }
        self.last_a12 = a12;

        let bank_size = 1024;
        let bank = match addr {
            0x0000..=0x03FF => self.banks[0] as usize,
            0x0400..=0x07FF => self.banks[0] as usize + 1,
            0x0800..=0x0BFF => self.banks[1] as usize,
            0x0C00..=0x0FFF => self.banks[1] as usize + 1,
            0x1000..=0x13FF => self.banks[2] as usize,
            0x1400..=0x17FF => self.banks[3] as usize,
            0x1800..=0x1BFF => self.banks[4] as usize,
            0x1C00..=0x1FFF => self.banks[5] as usize,
            _ => 0,
        };
        let offset = (addr as usize % bank_size);
        self.chr_rom[bank * bank_size + offset]
    }

    fn write_chr(&mut self, _addr: u16, _value: u8) {}

    fn clock(&mut self, _cycles: u8) {}

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> { Vec::new() }
    fn load_state(&mut self, _data: &[u8]) {}
}

impl Mapper4 {
    fn clock_irq(&mut self) {
        if self.irq_reload_pending {
            self.irq_counter = self.irq_reload;
            self.irq_reload_pending = false;
        } else if self.irq_counter == 0 {
            self.irq_counter = self.irq_reload;
        } else {
            self.irq_counter -= 1;
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}
```

---

## Graphics Pipeline

### wgpu Rendering

```rust
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface,
    swap_chain: wgpu::SwapChain,
    pipeline: wgpu::RenderPipeline,
    nes_texture: wgpu::Texture,
    palette_texture: wgpu::Texture,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

impl WgpuRenderer {
    pub fn render_frame(&mut self, framebuffer: &[u8; 256 * 240]) {
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.nes_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            framebuffer,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: NonZeroU32::new(256),
                rows_per_image: NonZeroU32::new(240),
            },
            wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );

        let frame = self.swap_chain.get_current_frame().unwrap().output;
        let mut encoder = self.device.create_command_encoder(&Default::default());

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("NES Render Pass"),
                color_attachments: &[wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
```

### NES Palette

```rust
pub const NES_PALETTE: [[u8; 3]; 64] = [
    [84, 84, 84], [0, 30, 116], [8, 16, 144], [48, 0, 136],
    [68, 0, 100], [92, 0, 48], [84, 4, 0], [60, 24, 0],
    [32, 42, 0], [8, 58, 0], [0, 64, 0], [0, 60, 0],
    [0, 50, 60], [0, 0, 0], [0, 0, 0], [0, 0, 0],

    [152, 150, 152], [8, 76, 196], [48, 50, 236], [92, 30, 228],
    [136, 20, 176], [160, 20, 100], [152, 34, 32], [120, 60, 0],
    [84, 90, 0], [40, 114, 0], [8, 124, 0], [0, 118, 40],
    [0, 102, 120], [0, 0, 0], [0, 0, 0], [0, 0, 0],

    [236, 238, 236], [76, 154, 236], [120, 124, 236], [176, 98, 236],
    [228, 84, 236], [236, 88, 180], [236, 106, 100], [212, 136, 32],
    [160, 170, 0], [116, 196, 0], [76, 208, 32], [56, 204, 108],
    [56, 180, 204], [60, 60, 60], [0, 0, 0], [0, 0, 0],

    [236, 238, 236], [168, 204, 236], [188, 188, 236], [212, 178, 236],
    [236, 174, 236], [236, 174, 212], [236, 180, 176], [228, 196, 144],
    [204, 210, 120], [180, 222, 120], [168, 226, 144], [152, 226, 180],
    [160, 214, 228], [160, 162, 160], [0, 0, 0], [0, 0, 0],
];
```

---

## Audio Pipeline

### SDL2 Backend

```rust
pub struct AudioBackend {
    device: sdl2::audio::AudioDevice<AudioCallback>,
    sender: Sender<Vec<f32>>,
}

struct AudioCallback {
    receiver: Receiver<Vec<f32>>,
    buffer: VecDeque<f32>,
}

impl sdl2::audio::AudioCallback for AudioCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        while let Ok(samples) = self.receiver.try_recv() {
            self.buffer.extend(samples);
        }
        for sample in out.iter_mut() {
            *sample = self.buffer.pop_front().unwrap_or(0.0);
        }
    }
}
```

---

## Input System

```rust
pub struct Controller {
    state: u8,
    shift_register: u8,
    strobe: bool,
}

impl Controller {
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let mask = match button {
            Button::A => 0x01,
            Button::B => 0x02,
            Button::Select => 0x04,
            Button::Start => 0x08,
            Button::Up => 0x10,
            Button::Down => 0x20,
            Button::Left => 0x40,
            Button::Right => 0x80,
        };
        if pressed {
            self.state |= mask;
        } else {
            self.state &= !mask;
        }
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            (self.state & 1) | 0x40
        } else {
            let result = (self.shift_register & 1) | 0x40;
            self.shift_register = (self.shift_register >> 1) | 0x80;
            result
        }
    }
}
```

---

## Advanced Features

### RetroAchievements

```rust
pub struct RetroAchievements {
    client: *mut rc_client_t,
    user: Option<String>,
    game_hash: Option<String>,
}

impl RetroAchievements {
    pub fn new() -> Result<Self> {
        unsafe {
            let client = rc_client_create(
                Some(read_memory_callback),
                Some(event_callback)
            );
            Ok(Self {
                client,
                user: None,
                game_hash: None,
            })
        }
    }

    pub fn login(&mut self, user: &str, token: &str) -> Result<()> {
        unsafe {
            rc_client_begin_login_with_token(
                self.client,
                user.as_ptr() as *const i8,
                token.as_ptr() as *const i8,
                Some(login_callback),
                std::ptr::null_mut(),
            );
            self.user = Some(user.to_string());
            Ok(())
        }
    }

    pub fn do_frame(&mut self) {
        unsafe {
            rc_client_do_frame(self.client);
        }
    }
}
```

### Netplay (GGPO)

```rust
pub struct NetplaySession {
    session: P2PSession<GgpoConfig>,
    local_player: PlayerHandle,
    remote_player: PlayerHandle,
}

impl NetplaySession {
    pub fn advance_frame(&mut self, emulator: &mut Console, local_input: u8) -> Result<()> {
        self.session.add_local_input(
            self.local_player,
            &NetplayInput {
                controller_state: local_input,
                frame: emulator.frame_count as u32,
            },
        )?;

        let mut requests = Vec::new();
        self.session.advance_frame(&mut requests)?;

        for request in requests {
            match request {
                GGPORequest::SaveGameState { cell, frame } => {
                    let state = emulator.save_state();
                    cell.save(frame, Some(state));
                }
                GGPORequest::LoadGameState { cell, frame } => {
                    if let Some(state) = cell.load() {
                        emulator.load_state(&state);
                    }
                }
                GGPORequest::AdvanceFrame { inputs } => {
                    emulator.step_frame();
                }
            }
        }
        Ok(())
    }
}
```

### TAS Recording

```rust
pub struct Fm2Movie {
    version: u8,
    emulator_version: String,
    rom_filename: String,
    rom_checksum: String,
    rerecord_count: u32,
    input_log: Vec<InputFrame>,
}

impl Fm2Movie {
    pub fn record_frame(&mut self, controller1: u8) {
        self.input_log.push(InputFrame {
            commands: 0,
            controller1,
            controller2: 0,
            controller3: 0,
            controller4: 0,
        });
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let mut file = File::create(path)?;
        writeln!(file, "version {}", self.version)?;
        writeln!(file, "emuVersion {}", self.emulator_version)?;
        writeln!(file, "romFilename {}", self.rom_filename)?;
        writeln!(file, "PowerOn")?;

        for frame in &self.input_log {
            let line = format!(
                "|{}|{}{}{}{}{}{}{}{}||",
                frame.commands,
                if frame.controller1 & 0x80 != 0 { 'R' } else { '.' },
                if frame.controller1 & 0x40 != 0 { 'L' } else { '.' },
                if frame.controller1 & 0x20 != 0 { 'D' } else { '.' },
                if frame.controller1 & 0x10 != 0 { 'U' } else { '.' },
                if frame.controller1 & 0x08 != 0 { 'T' } else { '.' },
                if frame.controller1 & 0x04 != 0 { 'S' } else { '.' },
                if frame.controller1 & 0x02 != 0 { 'B' } else { '.' },
                if frame.controller1 & 0x01 != 0 { 'A' } else { '.' },
            );
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }
}
```

### Lua Scripting

```rust
pub struct LuaEngine {
    lua: Lua,
    emulator: Arc<Mutex<Console>>,
}

impl LuaEngine {
    pub fn new(emulator: Arc<Mutex<Console>>) -> Result<Self> {
        let lua = Lua::new();
        Self::register_memory_api(&lua, emulator.clone())?;
        Self::register_emulator_api(&lua, emulator.clone())?;
        Self::register_input_api(&lua, emulator.clone())?;
        Ok(Self { lua, emulator })
    }

    fn register_memory_api(lua: &Lua, emulator: Arc<Mutex<Console>>) -> LuaResult<()> {
        let memory = lua.create_table()?;
        let emu = emulator.clone();
        memory.set("readbyte", lua.create_function(move |_, addr: u16| {
            Ok(emu.lock().unwrap().read_memory(addr))
        })?)?;
        lua.globals().set("memory", memory)?;
        Ok(())
    }

    pub fn run_script(&self, script: &str) -> LuaResult<()> {
        self.lua.load(script).exec()
    }
}
```

---

## Performance Optimizations

### 1. Lazy Rendering

```rust
pub struct LazyPpu {
    ppu: Ppu,
    fast_forward: bool,
}

impl LazyPpu {
    pub fn step(&mut self) {
        if self.fast_forward {
            self.ppu.step_no_render();
        } else {
            self.ppu.step();
        }
    }
}
```

### 2. Audio Ring Buffer

```rust
pub struct AudioRingBuffer {
    buffer: Vec<f32>,
    write_pos: usize,
    read_pos: usize,
    size: usize,
}

impl AudioRingBuffer {
    pub fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            write_pos: 0,
            read_pos: 0,
            size,
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.size;
    }

    pub fn pop(&mut self) -> Option<f32> {
        if self.read_pos == self.write_pos {
            return None;
        }
        let sample = self.buffer[self.read_pos];
        self.read_pos = (self.read_pos + 1) % self.size;
        Some(sample)
    }
}
```

### 3. SIMD Optimizations

```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

pub fn mix_audio_simd(
    pulse1: &[f32],
    pulse2: &[f32],
    output: &mut [f32]
) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        for i in (0..output.len()).step_by(4) {
            let p1 = _mm_loadu_ps(&pulse1[i]);
            let p2 = _mm_loadu_ps(&pulse2[i]);
            let sum = _mm_add_ps(p1, p2);
            _mm_storeu_ps(&mut output[i], sum);
        }
    }
}
```

### 4. Savestate Compression

```rust
use flate2::Compression;
use flate2::write::GzEncoder;

pub fn compress_savestate(state: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(state).unwrap();
    encoder.finish().unwrap()
}
```

---

## Crate Structure

```
rustynes/
├── Cargo.toml
├── crates/
│   ├── rustynes-core/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── console.rs
│   │   │   ├── bus.rs
│   │   │   └── savestate.rs
│   │   └── Cargo.toml
│   ├── rustynes-cpu/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cpu.rs
│   │   │   ├── instructions.rs
│   │   │   └── addressing.rs
│   │   └── Cargo.toml
│   ├── rustynes-ppu/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── ppu.rs
│   │   │   └── rendering.rs
│   │   └── Cargo.toml
│   ├── rustynes-apu/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── apu.rs
│   │   │   └── channels/
│   │   └── Cargo.toml
│   ├── rustynes-mappers/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── mapper000.rs
│   │   │   ├── mapper001.rs
│   │   │   └── mapper004.rs
│   │   └── Cargo.toml
│   ├── rustynes-desktop/
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   └── gui.rs
│   │   └── Cargo.toml
│   ├── rustynes-web/
│   │   └── Cargo.toml
│   ├── rustynes-tas/
│   │   └── Cargo.toml
│   ├── rustynes-netplay/
│   │   └── Cargo.toml
│   └── rustynes-achievements/
│       └── Cargo.toml
└── tests/
    └── integration_tests.rs
```

### Cargo.toml (Workspace)

```toml
[workspace]
members = [
    "crates/rustynes-core",
    "crates/rustynes-cpu",
    "crates/rustynes-ppu",
    "crates/rustynes-apu",
    "crates/rustynes-mappers",
    "crates/rustynes-desktop",
    "crates/rustynes-web",
    "crates/rustynes-tas",
    "crates/rustynes-netplay",
    "crates/rustynes-achievements",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
authors = ["RustyNES Contributors"]

[workspace.dependencies]
# Core dependencies
bitflags = "2.4"
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
log = "0.4"

# Graphics
wgpu = "0.18"
egui = "0.24"
egui-wgpu = "0.24"

# Audio
sdl2 = "0.36"

# Netplay
backroll = "0.2"

# Lua
mlua = { version = "0.9", features = ["lua54", "vendored"] }

# Achievements (FFI)
rcheevos-sys = "0.1"

# Utils
md5 = "0.7"
flate2 = "1.0"
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_lda_immediate() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new_test();

        // LDA #$42
        bus.write(0x8000, 0xA9);
        bus.write(0x8001, 0x42);
        cpu.pc = 0x8000;

        let cycles = cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cycles, 2);
        assert!(!cpu.p.contains(Status::ZERO));
        assert!(!cpu.p.contains(Status::NEGATIVE));
    }

    #[test]
    fn test_ppu_vblank_nmi() {
        let mut ppu = Ppu::new();

        // Run to VBlank (scanline 241, dot 1)
        ppu.ctrl.insert(PpuCtrl::NMI_ENABLE);

        while ppu.scanline != 241 || ppu.dot != 1 {
            ppu.step(&mut bus, &mut mapper);
        }

        assert!(ppu.status.contains(PpuStatus::VBLANK));
        assert!(ppu.nmi_pending);
    }

    #[test]
    fn test_apu_pulse_output() {
        let mut pulse = PulseChannel::new();
        pulse.enabled = true;
        pulse.length_counter = 10;
        pulse.envelope.volume = 15;
        pulse.timer_period = 100;
        pulse.duty = 2;  // 50% duty

        // Test output varies with sequence position
        assert_eq!(pulse.output(), 0);  // Low part of duty
        pulse.sequence_pos = 2;
        assert_eq!(pulse.output(), 15);  // High part of duty
    }
}
```

### Integration Tests

```rust
#[test]
fn test_nestest_rom() {
    let rom = load_rom("tests/test_roms/nestest.nes");
    let mut console = Console::new(rom);

    // Run nestest automated mode
    console.cpu.pc = 0xC000;

    for _ in 0..26554 {
        console.step();
    }

    // Check result code
    let result = console.bus.read(0x0002);
    assert_eq!(result, 0x00, "nestest failed with code {}", result);
}

#[test]
fn test_ppu_vbl_nmi() {
    let rom = load_rom("tests/test_roms/ppu_vbl_nmi/rom_singles/01-vbl_basics.nes");
    let mut console = Console::new(rom);

    // Run for 10 frames
    for _ in 0..10 {
        console.step_frame();
    }

    // Check pass/fail in RAM
    let result = console.bus.read(0x00F0);
    assert_eq!(result, 0x01, "VBL NMI test failed");
}
```

### Property-Based Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_cpu_adc_commutative(a in 0u8..255, b in 0u8..255) {
        let mut cpu1 = Cpu::new();
        let mut cpu2 = Cpu::new();
        let mut bus = Bus::new_test();

        cpu1.a = a;
        cpu1.adc(&mut bus, b);
        let result1 = cpu1.a;

        cpu2.a = b;
        cpu2.adc(&mut bus, a);
        let result2 = cpu2.a;

        prop_assert_eq!(result1, result2);
    }

    #[test]
    fn test_ppu_coarse_x_increment(x in 0u8..32) {
        let mut addr = VramAddress(0);
        addr.set_coarse_x(x);

        let original = addr.coarse_x();
        addr.increment_coarse_x();

        if x == 31 {
            prop_assert_eq!(addr.coarse_x(), 0);
        } else {
            prop_assert_eq!(addr.coarse_x(), x + 1);
        }
    }
}
```

### Benchmark Tests

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_cpu_instruction(c: &mut Criterion) {
    let mut cpu = Cpu::new();
    let mut bus = Bus::new_test();

    c.bench_function("lda_immediate", |b| {
        b.iter(|| {
            cpu.pc = 0x8000;
            black_box(cpu.step(&mut bus))
        })
    });
}

fn benchmark_ppu_scanline(c: &mut Criterion) {
    let mut ppu = Ppu::new();
    let mut bus = Bus::new_test();
    let mut mapper = Box::new(Mapper0::new());

    c.bench_function("render_scanline", |b| {
        b.iter(|| {
            for _ in 0..341 {
                black_box(ppu.step(&mut bus, &mut *mapper));
            }
        })
    });
}

criterion_group!(benches, benchmark_cpu_instruction, benchmark_ppu_scanline);
criterion_main!(benches);
```

---

## Implementation Roadmap

### Phase 1: MVP (Months 1-6)

**Milestone 1.1: Core Engine (Month 1-2)**
- CPU implementation (6502 core, all official opcodes)
- PPU rendering (background + sprites, no scrolling)
- Basic bus and memory system
- Mapper 0 (NROM)
- Simple SDL2 window output

**Deliverable:** Run Super Mario Bros title screen

**Milestone 1.2: Accuracy & Testing (Month 3)**
- Pass nestest.nes golden log
- Implement PPU scrolling (coarse X/Y, fine X/Y)
- APU implementation (pulse, triangle, noise, DMC)
- Audio output via SDL2

**Deliverable:** Pass 50% of blargg test ROMs

**Milestone 1.3: Common Mappers (Month 4-5)**
- Mapper 1 (MMC1) - 27.9% coverage
- Mapper 2 (UxROM) - 10.6%
- Mapper 3 (CNROM) - 6.3%
- Mapper 4 (MMC3) - 23.4%
- Save/load states

**Deliverable:** 80% game compatibility

**Milestone 1.4: Desktop Frontend (Month 6)**
- egui GUI (file picker, settings, controls)
- wgpu rendering pipeline
- Controller input (keyboard + gamepad)
- Configuration persistence

**Deliverable:** Playable desktop emulator

### Phase 2: Advanced Features (Months 7-12)

**Milestone 2.1: RetroAchievements (Month 7)**
- rcheevos FFI bindings
- User login and game identification
- Achievement unlock notifications
- Hardcore mode enforcement

**Deliverable:** Full RetroAchievements integration

**Milestone 2.2: Netplay (Month 8-9)**
- backroll-rs GGPO integration
- P2P session management
- Savestate serialization
- Input delay configuration

**Deliverable:** Low-latency online multiplayer

**Milestone 2.3: TAS Tools (Month 10)**
- FM2 format parser and writer
- Frame-by-frame recording
- Input playback with desync detection
- Rewind functionality (ring buffer)

**Deliverable:** FCEUX-compatible TAS recording

**Milestone 2.4: Lua Scripting (Month 11)**
- mlua integration (Lua 5.4)
- Memory API (read/write byte/word)
- Emulator control API (frameadvance, pause)
- Input API (joypad read/write)
- GUI overlay API (text, shapes)

**Deliverable:** Bot scripting support

**Milestone 2.5: Debugger (Month 12)**
- CPU disassembly view
- Breakpoints (execute, read, write)
- Memory viewer/editor
- PPU viewer (nametables, palettes, sprites)
- Trace logger

**Deliverable:** Developer-grade debugging tools

### Phase 3: Expansion (Months 13-18)

**Milestone 3.1: Expansion Audio (Month 13-14)**
- VRC6 (Konami pulse + sawtooth)
- VRC7 (FM synthesis)
- MMC5 (pulse + PCM)
- Namco 163 (wavetable)
- Sunsoft 5B (AY-3-8910 PSG)
- FDS (Famicom Disk System)

**Deliverable:** Expansion audio support for all major chips

**Milestone 3.2: Additional Mappers (Month 15-16)**
- 20 common mappers (5, 9, 10, 11, 13, 19, 23, 24, etc.)
- UNIF format support
- Mapper database integration

**Deliverable:** 98% game compatibility

**Milestone 3.3: WebAssembly (Month 17-18)**
- wasm32-unknown-unknown target
- Web frontend (HTML5 canvas)
- Browser audio via Web Audio API
- ROM file loading
- Touch controls for mobile

**Deliverable:** Browser-playable emulator

### Phase 4: Polish (Months 19-24)

**Milestone 4.1: Video Filters (Month 19)**
- NTSC filter (blargg algorithm)
- CRT shader (scanlines, curvature)
- Upscaling filters (hq2x, xBR)
- Custom shader support

**Deliverable:** Visual enhancement options

**Milestone 4.2: Advanced Input (Month 20)**
- Zapper light gun
- Power Pad
- Four Score (4 players)
- Arkanoid paddle
- Input recording/playback

**Deliverable:** Peripheral support

**Milestone 4.3: TAS Editor (Month 21-22)**
- Greenzone (savestate cache)
- Bookmarks with notes
- Timeline branching
- Piano roll input editor
- Pattern recording

**Deliverable:** Full-featured TAS editor

**Milestone 4.4: Final Polish (Month 23-24)**
- Performance profiling and optimization
- Memory usage reduction
- 100% test ROM pass rate
- Documentation completion
- Release packaging

**Deliverable:** Production-ready v1.0 release

---

## Reference Matrix

### Emulator Feature Comparison

| Feature | RustyNES | Mesen2 | FCEUX | puNES |
|---------|----------|--------|-------|-------|
| **Language** | Rust | C++ | C++ | C++ |
| **CPU Accuracy** | Cycle-accurate | Cycle-accurate | ~95% | ~98% |
| **PPU Accuracy** | Dot-level | Dot-level | Scanline | Dot-level |
| **Mapper Count** | 300+ (target) | 290+ | 177+ | 461+ |
| **Expansion Audio** | All chips | All chips | VRC6/7, MMC5 | All chips |
| **RetroAchievements** | Native | Via RALibretro | Via RALibretro | Via RALibretro |
| **Netplay** | GGPO rollback | None | None | None |
| **TAS Tools** | FM2 recording | Basic | Full (TAS Editor) | None |
| **Lua Scripting** | mlua 5.4 | Yes | Lua 5.1 | No |
| **Debugger** | Full | Excellent | Excellent | Basic |
| **WebAssembly** | Yes | No | No | No |
| **Cross-Platform** | All | Windows/Linux/macOS | Windows/Linux/macOS | Windows/Linux |

### Architecture Influence Matrix

| Component | Primary Source | Secondary Sources |
|-----------|---------------|-------------------|
| **CPU Core** | Mesen2 | TetaNES, DaveTCode |
| **PPU Rendering** | Mesen2, Pinky | rib/nes-emulator |
| **APU Synthesis** | TetaNES | Rustico, Mesen2 |
| **Mapper System** | puNES, Mesen2 | All Rust emulators |
| **Expansion Audio** | Rustico | puNES, Mesen2 |
| **Safe Rust Patterns** | DaveTCode | TetaNES |
| **TAS Tools** | FCEUX | Mesen2 |
| **Debugger** | Mesen2, FCEUX | rib/nes-emulator |
| **Web Deployment** | takahirox | kamiyaowl |
| **Code Clarity** | Ares | starrhorne |

### Test ROM Coverage Goals

| Test Suite | Target Pass Rate | Validation |
|------------|------------------|------------|
| **nestest.nes** | 100% | CPU instruction accuracy |
| **blargg CPU** | 100% (all 11 tests) | CPU edge cases, timing |
| **blargg PPU** | 100% (all 10 tests) | PPU rendering, VBL, NMI |
| **blargg APU** | 95%+ (DMC edge cases hard) | APU timing, channels |
| **mmc3_test** | 100% | MMC3 IRQ timing |
| **sprite_hit** | 100% | Sprite 0 hit detection |
| **ppu_vbl_nmi** | 100% | VBlank NMI timing |
| **oam_stress** | 100% | OAM evaluation bugs |
| **TASVideos suite** | 100% (156 tests) | Overall accuracy |

---

## Dependencies

### Core Dependencies

```toml
[dependencies]
# Emulation core
bitflags = "2.4"           # CPU/PPU register flags
serde = "1.0"              # Serialization (savestates)
thiserror = "1.0"          # Error handling
log = "0.4"                # Logging
env_logger = "0.11"        # Log backend

# Math & utilities
md5 = "0.7"                # ROM checksums
crc32fast = "1.3"          # Fast CRC32
flate2 = "1.0"             # Savestate compression
base64 = "0.21"            # FM2 format encoding

# Graphics (desktop)
wgpu = "0.18"              # GPU rendering
egui = "0.24"              # Immediate-mode GUI
egui-wgpu = "0.24"         # egui + wgpu integration
winit = "0.29"             # Window creation

# Audio
sdl2 = "0.36"              # Audio backend
cpal = "0.15"              # Cross-platform audio (alternative)

# Input
gilrs = "0.10"             # Gamepad support

# Netplay
backroll = "0.2"           # GGPO rollback netcode
ggrs = "0.9"               # Alternative netcode

# Achievements
rcheevos-sys = "0.1"       # RetroAchievements FFI bindings

# Lua scripting
mlua = { version = "0.9", features = ["lua54", "vendored"] }

# WebAssembly
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
web-sys = "0.3"
js-sys = "0.3"
wasm-bindgen-futures = "0.4"

# Development
[dev-dependencies]
criterion = "0.5"          # Benchmarking
proptest = "1.4"           # Property-based testing
pretty_assertions = "1.4"  # Better assertion output
```

### Build Dependencies

```toml
[build-dependencies]
bindgen = "0.69"           # Generate rcheevos FFI bindings
cc = "1.0"                 # Compile C code
```

### Platform-Specific

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = "0.52"

[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"

[target.'cfg(target_os = "macos")'.dependencies]
cocoa = "0.25"
objc = "0.2"
```

---

## Appendices

### Appendix A: NES Hardware Specifications

**CPU:**
- Processor: Ricoh 2A03 (modified MOS 6502)
- Clock: 1.789773 MHz (NTSC), 1.662607 MHz (PAL)
- Address space: 16-bit (64KB)
- RAM: 2KB internal
- Stack: $0100-$01FF (256 bytes)

**PPU:**
- Chip: Ricoh 2C02 (NTSC), 2C07 (PAL)
- Clock: 5.369318 MHz (NTSC), 3x CPU
- Resolution: 256x240 pixels
- Colors: 64-color palette, 25 on-screen
- Sprites: 64 sprites, 8 per scanline
- VRAM: 2KB internal (nametables)
- Palette RAM: 32 bytes

**APU:**
- Channels: 2 pulse, 1 triangle, 1 noise, 1 DMC
- Sample rate: 1.789773 MHz
- DMC samples: 7-bit, 1-127
- Frame counter: 4-step or 5-step

**Cartridge:**
- PRG-ROM: 16KB minimum, up to 8MB
- CHR-ROM/RAM: 8KB typical
- Battery-backed SRAM: 8KB typical
- Mappers: 300+ variants

**Timing:**
- NTSC: 60.0988 Hz, 29780.5 CPU cycles/frame
- PAL: 50.0070 Hz, 33247.5 CPU cycles/frame
- Dots per scanline: 341 (NTSC), 341 (PAL)
- Scanlines per frame: 262 (NTSC), 312 (PAL)

### Appendix B: Mapper Database

**Top 20 Mappers by Usage:**
1. Mapper 0 (NROM) - 9.5%
2. Mapper 1 (MMC1) - 27.9%
3. Mapper 2 (UxROM) - 10.6%
4. Mapper 3 (CNROM) - 6.3%
5. Mapper 4 (MMC3) - 23.4%
6. Mapper 5 (MMC5) - 1.2%
7. Mapper 7 (AxROM) - 3.1%
8. Mapper 9 (MMC2) - 0.8%
9. Mapper 10 (MMC4) - 0.3%
10. Mapper 11 (Color Dreams) - 1.4%
11. Mapper 13 (CPROM) - 0.2%
12. Mapper 19 (Namco 163) - 0.5%
13. Mapper 23 (VRC2/4) - 0.6%
14. Mapper 24 (VRC6) - 0.3%
15. Mapper 25 (VRC4) - 0.4%
16. Mapper 26 (VRC6) - 0.2%
17. Mapper 33 (Taito) - 0.3%
18. Mapper 34 (BNROM) - 0.2%
19. Mapper 66 (GxROM) - 0.5%
20. Mapper 69 (Sunsoft FME-7) - 0.3%

**Cumulative Coverage:** 87.6%

### Appendix C: Test ROM Sources

**CPU Tests:**
- nestest.nes - Comprehensive CPU test
- blargg cpu_exec_space - Execution from various spaces
- blargg cpu_interrupts - IRQ/NMI timing
- blargg cpu_timing_test6 - Instruction timing

**PPU Tests:**
- blargg ppu_vbl_nmi - VBlank NMI timing
- blargg ppu_sprite_hit - Sprite 0 hit detection
- blargg ppu_sprite_overflow - Sprite overflow flag
- Acid2 PPU - Visual regression test

**APU Tests:**
- blargg apu_test - All APU channels
- blargg dmc_tests - DMC edge cases
- blargg test_apu_env - Envelope tests
- blargg test_apu_sweep - Sweep tests

**Mapper Tests:**
- mmc3_test_2 - MMC3 IRQ timing
- holy_diver_batman - MMC5 edge cases
- action_53 - Action 53 mapper

### Appendix D: Glossary

**6502:** The CPU used in the NES (Ricoh 2A03 variant)

**APU:** Audio Processing Unit - NES sound chip

**CHR:** Character ROM/RAM - graphics data storage

**DMC:** Delta Modulation Channel - APU sample playback

**FM2:** FCEUX movie format for TAS recordings

**GGPO:** Good Game Peace Out - rollback netcode protocol

**Mapper:** Memory Management Controller - extends NES addressing

**MMC:** Memory Management Controller (Nintendo)

**NMI:** Non-Maskable Interrupt - triggered at VBlank

**PPU:** Picture Processing Unit - NES graphics chip

**PRG:** Program ROM/RAM - game code storage

**Scanline:** One horizontal line of pixels (341 dots)

**TAS:** Tool-Assisted Speedrun - frame-perfect gameplay

**VRAM:** Video RAM - PPU nametable storage

**VRC:** Konami's Video RAM Controller series

**wgpu:** Cross-platform GPU abstraction library

---

## Conclusion

RustyNES represents a comprehensive, modern approach to NES emulation, combining the best architectural patterns from existing emulators with Rust's safety guarantees and modern features. The design prioritizes:

1. **Accuracy** - Cycle-accurate emulation matching Mesen2's 100% test pass rate
2. **Features** - RetroAchievements, GGPO netplay, TAS tools, Lua scripting
3. **Performance** - wgpu GPU acceleration, SIMD optimizations, lazy rendering
4. **Safety** - Zero unsafe code where possible, leveraging Rust's type system
5. **Modularity** - Clean crate separation enabling library reuse
6. **Cross-Platform** - Desktop, Web, potential mobile/embedded

**Target Timeline:** 24 months from inception to production v1.0

**Success Metrics:**
- 100% TASVideos accuracy test pass rate
- 300+ mapper implementations
- Sub-16ms frame time (60 FPS) on mid-range hardware
- Active RetroAchievements community adoption
- FCEUX-compatible TAS recording/playback
- Browser deployment via WebAssembly

This architecture document serves as the technical blueprint for RustyNES development, incorporating lessons learned from 13 reference emulators and synthesizing best practices into a cohesive, implementable design.

---

**Document Version:** 1.0.0
**Generated:** 2025-12-18
**Total Sections:** 17
**Total Lines:** 3457
**Status:** Complete Specification
