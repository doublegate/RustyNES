# RustyNES Overview

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Project Vision](#project-vision)
- [Design Philosophy](#design-philosophy)
- [Accuracy Goals](#accuracy-goals)
- [Emulation Approach](#emulation-approach)
- [Target Audience](#target-audience)
- [Feature Comparison](#feature-comparison)
- [Technical Highlights](#technical-highlights)

---

## Project Vision

RustyNES aims to be the **definitive NES emulator for the modern era** - combining cycle-perfect accuracy with contemporary features and the safety guarantees of Rust. We're building more than just an emulator; we're creating a comprehensive platform for NES preservation, competitive play, tool-assisted speedrunning, and homebrew development.

### Core Objectives

1. **Accuracy Without Compromise**
   - 100% pass rate on TASVideos Accuracy Test suite
   - Cycle-accurate CPU, dot-level PPU, hardware-precise APU
   - Perfect timing for edge cases and undocumented behavior

2. **Modern Feature Set**
   - RetroAchievements for in-game achievements
   - GGPO rollback netplay for online multiplayer
   - TAS tools matching FCEUX capabilities
   - Lua 5.4 scripting for automation

3. **Developer-Centric Design**
   - Advanced debugging tools (CPU, PPU, APU viewers)
   - Trace logger and code-data logger
   - Clean, modular architecture for library reuse
   - Comprehensive API documentation

4. **Universal Accessibility**
   - Cross-platform: Windows, Linux, macOS, Web
   - Multiple frontend options: GUI, TUI, headless
   - Low system requirements, high performance
   - Open-source with permissive licensing

---

## Design Philosophy

### 1. Accuracy First, Speed Second

Following Mesen2's philosophy, **accuracy is non-negotiable**. Every component (CPU, PPU, APU) must pass all relevant test ROMs before any optimization work begins. We target cycle-accurate emulation with sub-cycle precision where required.

**Implementation Strategy:**
- **CPU**: Cycle-accurate 6502 core with dummy read/write emulation
- **PPU**: Per-dot rendering at 5.37 MHz (3x CPU clock)
- **APU**: 1.789773 MHz execution with hardware-accurate mixing
- **Mappers**: Cycle-based IRQ timing for MMC3/MMC5, VRC scanline counters

**Why Accuracy Matters:**
- Games rely on precise timing for sprite multiplexing
- Many use mapper IRQs for split-screen effects
- TAS movies require deterministic execution
- Homebrew developers need a reliable testing platform

### 2. Code Clarity Over Cleverness

Inspired by Ares's "half the code" philosophy, we prioritize **readable, maintainable code**:

- Trait-based abstractions over macro magic
- Strong typing using newtype pattern for registers/addresses
- Clear naming conventions aligned with hardware documentation
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

### 3. Safe Rust by Default

Following DaveTCode's zero-unsafe approach:

- Avoid `unsafe` blocks except for FFI (rcheevos, platform APIs)
- No `Rc<RefCell<>>` anti-patterns - prefer owned data and message passing
- Use channels for inter-component communication
- Leverage type system for correctness (state machines as enums)

**Benefits:**
- Memory safety guaranteed at compile time
- No data races in concurrent code
- Easier reasoning about program behavior
- Better error messages from the compiler

### 4. Test-Driven Development

Every component is validated before integration:

- **Unit tests** for individual instructions/operations
- **Integration tests** for component interactions
- **Test ROM validation** (nestest.nes, blargg suite, TASVideos)
- **Property-based testing** for CPU/PPU timing invariants
- **Regression tests** for mapper edge cases

### 5. Modular & Reusable

Crate structure enabling independent use:

```
rustynes-core/         # Core emulation engine (no_std compatible)
rustynes-cpu/          # Standalone 6502 (reusable for C64, Apple II)
rustynes-ppu/          # 2C02 PPU implementation
rustynes-apu/          # 2A03 APU with expansion audio
rustynes-mappers/      # All mapper implementations
```

**Use Cases:**
- Embedding in other emulators
- Academic research on NES hardware
- Homebrew development tools
- Benchmarking and optimization studies

---

## Accuracy Goals

### Target Metrics

| Component | Target Accuracy | Validation Method |
|-----------|----------------|-------------------|
| **CPU (6502)** | 100% instruction-level | nestest.nes golden log match |
| **PPU (2C02)** | 100% cycle-accurate | ppu_vbl_nmi, sprite_hit_tests, scrolltest |
| **APU (2A03)** | 99%+ hardware match | apu_test, dmc_tests, blargg suite |
| **Mappers** | 100% for licensed | Game compatibility matrix (700+ titles) |
| **Overall** | 100% TASVideos suite | 156 test ROM pass rate |

### TASVideos Accuracy Test Categories

The comprehensive test suite covers:

1. **APU Tests (25 ROMs)**
   - Frame counter modes (4-step, 5-step)
   - DMC DMA behavior and timing
   - Length counter and sweep units
   - Envelope and linear counter
   - Channel mixing characteristics

2. **CPU Tests (35 ROMs)**
   - All official instructions (56 opcodes)
   - Unofficial/illegal opcodes (100+ variants)
   - Interrupt timing (NMI, IRQ, BRK)
   - Page-crossing penalties
   - Dummy reads and writes

3. **PPU Tests (45 ROMs)**
   - Rendering timing (341 dots × 262 scanlines)
   - Scrolling behavior (Loopy's model)
   - Sprite evaluation and overflow
   - Sprite 0 hit detection
   - VBlank flag timing
   - Palette RAM quirks

4. **Mapper Tests (51 ROMs)**
   - MMC3 IRQ scanline counter
   - MMC5 advanced features
   - VRC IRQ timing
   - Banking edge cases
   - Mirroring modes

---

## Emulation Approach

### Cycle-Accurate vs. Scanline-Based

RustyNES uses **hybrid emulation** for optimal accuracy-performance balance:

#### CPU: Instruction-Level Cycle Accuracy

Execute one instruction at a time, tracking exact cycle counts:

```rust
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    let opcode = self.read(bus, self.pc);
    let instruction = self.instruction_table[opcode as usize];
    let base_cycles = self.cycle_table[opcode as usize];

    // Execute and return total cycles (base + page crossing penalties)
    let extra_cycles = instruction(self, bus, mode);
    base_cycles + extra_cycles
}
```

**Why this approach:**
- Simplifies instruction implementation
- Natural handling of page-crossing penalties
- Easy integration with PPU clock (3 PPU dots per CPU cycle)

#### PPU: Dot-Level Rendering

Render every pixel at the correct cycle:

```
Frame: 262 scanlines × 341 dots = 89,342 PPU cycles
Each CPU cycle triggers 3 PPU dots
Odd frames skip 1 dot (89,341 total)
```

**Rendering pipeline per dot:**
1. Fetch background tile data (every 8 dots)
2. Evaluate sprites (dots 257-320)
3. Output pixel to framebuffer
4. Update scroll registers

**Critical timing points:**
- **Dot 1, Scanline 241**: VBlank flag set, NMI triggered
- **Dot 257**: Copy horizontal scroll from T to V
- **Dots 280-304, Scanline 261**: Copy vertical scroll from T to V
- **Dot 340, Odd Frames**: Skip to dot 0 of next scanline

#### APU: Sample-Based Synthesis

Generate audio samples at output rate (48 kHz):

```rust
pub fn step(&mut self, cpu_cycles: u8) {
    self.cycles += cpu_cycles as u64;

    // Advance frame counter
    self.frame_counter.step(cpu_cycles);

    // Generate samples when accumulator exceeds threshold
    let samples_to_generate = /* calculate based on cycles */;
    for _ in 0..samples_to_generate {
        let sample = self.mix_channels();
        self.output_buffer.push(sample);
    }
}
```

**Why this approach:**
- Matches hardware behavior (continuous analog output)
- High-quality resampling (sinc interpolation)
- Low latency (minimal buffering)

### Master Clock Synchronization

The NES operates on a **master clock** that drives all components:

```
Master Clock (NTSC): 21.477272 MHz
├─ CPU Clock: ÷12 = 1.789773 MHz (~559 ns/cycle)
├─ PPU Clock: ÷4  = 5.369318 MHz (~186 ns/dot)
└─ APU Clock: Same as CPU (1.789773 MHz)

Ratio: 3 PPU dots per 1 CPU cycle (exact, no drift)
```

**Implementation:**
```rust
pub fn step(&mut self) -> u8 {
    let cpu_cycles = self.cpu.step(&mut self.bus);

    // PPU runs 3x as fast
    for _ in 0..(cpu_cycles * 3) {
        self.ppu.step(&mut self.bus, &mut self.cartridge);
        if self.ppu.nmi_triggered() {
            self.cpu.trigger_nmi();
        }
    }

    // APU runs at CPU speed
    self.apu.step(cpu_cycles);

    cpu_cycles
}
```

---

## Target Audience

RustyNES is designed for multiple user groups with diverse needs:

### 1. Emulation Enthusiasts

**Needs:**
- High accuracy for authentic gameplay
- RetroAchievements integration
- Save states and rewind
- Customizable video/audio settings

**Features:**
- 100% game compatibility goal
- CRT shaders and NTSC filters
- Multiple controller support
- Cloud save synchronization

### 2. TAS Community

**Needs:**
- Frame-perfect execution
- FM2 movie recording/playback
- TAS editor with greenzone
- Lua scripting for automation

**Features:**
- Deterministic emulation
- Input recording/editing
- RAM search and watch
- Breakpoints and memory viewer

### 3. Netplay Users

**Needs:**
- Low-latency online play
- Rollback netcode
- Spectator mode
- Tournament features

**Features:**
- GGPO rollback (1-2 frame lag)
- Lobby system
- Replay saving
- Ranked matchmaking (future)

### 4. Homebrew Developers

**Needs:**
- Accurate hardware emulation
- Advanced debugging tools
- Test ROM automation
- Performance profiling

**Features:**
- CPU/PPU/APU state viewers
- Trace logger (instruction/scanline)
- Code-data logger (CDL)
- Cycle-level breakpoints

### 5. Rust Developers

**Needs:**
- Clean reference implementation
- Reusable components
- Comprehensive documentation
- Test coverage examples

**Features:**
- Well-documented crate APIs
- Property-based testing examples
- Benchmark suite
- Architecture diagrams

---

## Feature Comparison

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

---

## Technical Highlights

### Rust-Specific Advantages

1. **Memory Safety**
   - No buffer overflows or use-after-free
   - Data race prevention at compile time
   - Automatic resource management (RAII)

2. **Zero-Cost Abstractions**
   - Trait dispatch with monomorphization
   - Inline optimizations across crate boundaries
   - SIMD auto-vectorization where applicable

3. **Concurrency**
   - Lock-free channels for component communication
   - Thread-safe by default (Send/Sync traits)
   - Async/await for network features

4. **Tooling**
   - Cargo for dependency management
   - Built-in testing framework
   - Integrated benchmarking (Criterion)
   - Documentation generation (rustdoc)

### Performance Optimizations

Planned optimizations (after accuracy validation):

- **CPU**: Jump table dispatch, inline hot paths
- **PPU**: SIMD pixel compositing, batch rendering
- **APU**: Fast sinc resampling, SSE/NEON mixing
- **Mappers**: Precomputed banking tables

**Target Performance:**
- 1000+ FPS on modern CPUs (16x real-time)
- <5ms frame time for 60 FPS gameplay
- <100 MB memory footprint

### Cross-Platform Strategy

**Graphics**: wgpu (Vulkan/Metal/DX12/WebGPU)
- Single codebase for all platforms
- GPU-accelerated scanline rendering
- Shader support for post-processing

**Audio**: SDL2 or cpal
- Low-latency output (<20ms)
- Resampling to native rates (44.1/48 kHz)
- Volume normalization

**Input**: gilrs + winit
- Gamepad auto-detection
- Hotplug support
- Customizable key bindings

---

## Conclusion

RustyNES represents the intersection of **accuracy**, **performance**, and **modern software engineering**. By leveraging Rust's strengths and learning from the best existing emulators, we aim to create a platform that serves casual players, competitive speedrunners, TAS creators, and homebrew developers equally well.

The journey to 100% accuracy is long, but with a strong foundation and community support, RustyNES will become the go-to NES emulator for the next generation of retro gaming enthusiasts.

---

## Next Steps

- Read [ARCHITECTURE.md](ARCHITECTURE.md) for system design details
- Check [ROADMAP.md](ROADMAP.md) for development timeline
- Explore [cpu/](cpu/), [ppu/](ppu/), [apu/](apu/) for hardware specifications

**Ready to contribute?** See [dev/CONTRIBUTING.md](dev/CONTRIBUTING.md)
