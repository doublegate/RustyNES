# Rustico Technical Report

**Repository:** [github.com/zeta0134/rustico](https://github.com/zeta0134/rustico)
**Author:** Nicholas Flynt (zeta0134)
**Language:** Rust
**License:** MIT
**Stars:** 100+ | **Status:** Active Development

---

## Executive Summary

Rustico is an audio-focused NES/Famicom emulator that prioritizes expansion audio accuracy. It stands out as one of the few Rust emulators to implement VRC6, VRC7, MMC5, N163, S5B (Sunsoft 5B), and FDS expansion audio. The project targets "modern retro" software, homebrew, and chiptune creation, making it particularly valuable for music-focused applications.

---

## Architecture Overview

### Crate Organization

```
rustico/
├── core/       # Emulation library (minimal dependencies)
├── sdl/        # SDL2 frontend (most complete)
├── egui/       # Egui frontend (in development)
├── cli/        # Command-line interface
├── wasm/       # WebAssembly build
└── ui-common/  # Shared UI utilities
```

**Key Design Decision:** The `/core` crate maintains minimal dependencies (only Rust's standard FileIO) for maximum portability. This is the only crate needed for custom shells or game wrappers.

### Dependency Philosophy

- **Core:** Standard library only (FileIO for ROM loading)
- **SDL Frontend:** rust-sdl2 for cross-platform compatibility
- **Egui Frontend:** Modern immediate-mode GUI (in development)

---

## Emulation Accuracy

### CPU (6502)

- All official instructions implemented
- **Unofficial instructions supported** (including NOPs and STPs)
- Cycle-stepped execution (can pause between instruction ticks)
- Implements dummy access patterns
- DMA cycle delays (with known timing flaws)
- DPCM corruption glitches reproduced (not hardware-accurate timing)

### PPU (2C02)

- Memory mapping with full mapper support
- Nametable mirroring (all modes, mapper-controlled)
- Cycle timing handles tricky games (Battletoads works)
- Advanced raster tricks and homebrew stable
- Sprite overflow implemented (bug behavior not emulated)

### APU (2A03)

- **Feature-complete audio emulation**
- Pulse, Triangle, Noise, DMC all working
- DMC wait delay (not perfectly accurate)
- **1.7 MHz audio emulation** with downsampling
- Hardware-accurate 2A03 channel mixing (+/- few dB)

### Expansion Audio (Unique Strength)

| Expansion | Status | Notes |
|-----------|--------|-------|
| MMC5 | Working | Square + PCM channels |
| VRC6 | Working | 2 Pulse + Sawtooth |
| S5B (Sunsoft 5B) | Working | AY-3-8910 variant |
| N163 | Working | Wavetable synthesis |
| FDS | Working | Frequency modulation |
| VRC7 | Implemented | ADSR research needed |

**Expansion mixing:** Under active research for accuracy.

### Mappers

| Category | Examples | Status |
|----------|----------|--------|
| Common | NROM, MMC1, MMC3 | Supported |
| Advanced | MMC5, Rainbow | Implemented (untested features) |
| FDS | Famicom Disk System | **Fully supported** |
| Timing Tests | blargg's mapper tests | Some failures |

**Notes:**
- FDS requires separate BIOS (homebrew replacement should work)
- PAL and Vs. System entirely unimplemented

---

## Features

### Core Emulation
- [x] iNES ROM format
- [x] FDS disk images (with BIOS)
- [x] Expansion audio (6 systems)
- [x] Snapshot/Restore (save states)
- [ ] PAL support (planned)
- [ ] Vs. System support

### User Interface
- [x] SDL2 frontend (full-featured)
- [x] Egui frontend (barebones, WIP)
- [x] WebAssembly build
- [x] Command-line interface
- [x] Standard controller support
- [ ] Additional peripherals (planned)

### Developer Features
- [x] Multiple frontend architecture
- [x] Portable core library
- [x] Accuracy test tracking

---

## Technical Highlights

### 1. High-Fidelity Audio Emulation

```
Audio is emulated at 1.7 MHz then downsampled, so high noise
and other unusual timbres are reproduced faithfully.
```

This approach captures audio nuances lost by lower-resolution emulation.

### 2. Expansion Audio Focus

Rustico is one of the most complete Rust implementations for expansion audio:

- **VRC6:** Konami's enhanced audio (Castlevania III Japan)
- **VRC7:** FM synthesis (Lagrange Point)
- **MMC5:** Extra channels (Castlevania III US)
- **N163:** Namco wavetable (Rolling Thunder)
- **S5B:** Sunsoft's AY variant (Gimmick!)
- **FDS:** Disk system modulation

### 3. FDS Support

Full Famicom Disk System emulation including:
- Disk image loading
- BIOS requirement (or homebrew replacement)
- FDS expansion audio

### 4. Advanced Mapper Implementation

MMC5 and Rainbow mappers implemented, though not fully tested due to limited software availability.

---

## Code Metrics & Structure

### Overview

| Metric | Value |
|--------|-------|
| **Total Lines of Code** | 24,430 |
| **Source Files** | 74 Rust files |
| **Crates** | 5 (core, sdl, egui, cli, ui-common) |
| **Mapper Implementations** | 21 mappers |
| **Test Functions** | 0 (no unit tests found) |

### Lines of Code by Component

| Component | LOC | Purpose |
|-----------|-----|---------|
| **Mappers Total** | 10,395 | All mapper implementations |
| **APU Total** | 1,924 | Complete audio subsystem |
| **Piano Roll** | 1,696 | Music visualization UI |
| **PPU** | 936 | Picture processing unit |
| **Addressing** | 807 | CPU addressing modes |
| **Opcodes** | 606 | 6502 instruction set |
| **Palettes** | 515 | NES color palettes |
| **iNES Format** | 454 | ROM loading |
| **Cycle CPU** | 429 | CPU emulation core |

### Largest Mapper Implementations

| Mapper | LOC | Expansion Audio |
|--------|-----|-----------------|
| **Rainbow** | 1,573 | None |
| **NSF** | 1,405 | Music player format |
| **VRC7** | 1,205 | FM synthesis |
| **VRC6** | 1,118 | 2 Pulse + Sawtooth |
| **FDS** | 1,098 | Frequency modulation |
| **MMC5** | 895 | Square + PCM |
| **FME7** | 676 | AY-3-8910 (S5B) |
| **N163** | 665 | Wavetable synthesis |
| **MMC3** | 367 | Most common mapper |
| **MMC1** | 295 | Second most common |

### APU Submodule Breakdown

| Module | Purpose |
|--------|---------|
| mod.rs (769 LOC) | Main APU coordinator, mixer tables |
| dmc.rs | Delta Modulation Channel |
| pulse.rs | Pulse wave channels (1 & 2) |
| filters.rs | Audio filtering chain |
| triangle.rs | Triangle wave channel |
| noise.rs | Noise channel |
| audio_channel.rs | Channel state abstraction |
| volume_envelope.rs | ADSR envelope |
| length_counter.rs | Duration control |
| ring_buffer.rs | Audio buffering |

### Testing Strategy

**Current Status:** No traditional unit tests (`#[test]` functions not found)

**Alternative Validation:**
- Accuracy tracked against [TASVideos NES accuracy tests](http://tasvideos.org/EmulatorResources/NESAccuracyTests.html)
- Manual testing with homebrew and commercial ROMs
- Known limitations documented in README
- Community feedback from chiptune creators

---

## CPU Implementation Details

### 6502 Core (429 LOC)

**Architecture:**
```rust
pub struct Registers {
    pub a: u8,      // Accumulator
    pub x: u8,      // X index
    pub y: u8,      // Y index
    pub pc: u16,    // Program counter
    pub s: u8,      // Stack pointer
    pub flags: Flags,
}

pub struct Flags {
    pub carry: bool,
    pub zero: bool,
    pub decimal: bool,
    pub interrupts_disabled: bool,
    pub overflow: bool,
    pub negative: bool,
    pub last_nmi: bool,  // Internal tracking
}
```

**Features:**
- **Cycle-stepped execution:** Can pause between instruction ticks
- **Service routine handling:** NMI/IRQ interrupt support
- **OAM DMA:** Sprite memory transfer with cycle delays
- **Dummy access patterns:** Emulates 6502 dummy reads
- **Status register encoding:** Bit-accurate P register manipulation

**Addressing Modes (807 LOC):**
- All 13 6502 addressing modes implemented
- Separate module for clean code organization
- Cycle-accurate address calculation

**Opcodes (606 LOC):**
- All 151 official opcodes
- Unofficial instructions supported (NOPs, STPs)
- Documentation sourced from llx.com and nesdev.com

### DMA Implementation

**OAM DMA Cycle Tracking:**
```rust
pub struct CpuState {
    pub oam_dma_active: bool,
    pub oam_dma_cycle: u16,
    pub oam_dma_address: u16,
    // ... interrupt tracking
}
```

**Known Timing Limitations:**
- DMA cycle delays present but not hardware-perfect
- DPCM corruption glitches reproduced (timing approximated)
- DMC wait delay implemented (accuracy pending research)

---

## PPU Implementation Details

### Core PPU (936 LOC)

**Sprite System:**
```rust
pub struct SpriteLatch {
    tile_index: u8,
    bitmap_high: u8,
    bitmap_low: u8,
    attributes: u8,
    x_counter: u8,
    y_pos: u8,
    active: bool,
}
```

**Features:**
- **Sprite evaluation:** 8 sprites per scanline
- **Sprite overflow:** Implemented correctly
- **Sprite overflow bug:** NOT emulated (accuracy limitation)
- **X/Y flip:** Attribute-based sprite transforms
- **Priority:** Background/foreground handling

**Memory Architecture:**
- Internal VRAM (nametables)
- OAM (Object Attribute Memory)
- Secondary OAM (evaluated sprites)
- Palette RAM (32 bytes)
- Open bus behavior for unmapped reads

**Timing:**
- Cycle-accurate enough for tricky games (Battletoads)
- Raster tricks and homebrew stable
- Not pixel-perfect (scanline-accurate in places)

**Mapper Integration:**
- Nametable mirroring fully controlled by mappers
- CHR ROM/RAM switching support
- Split-screen scrolling (MMC3, etc.)

---

## APU & Expansion Audio Implementation

### Standard APU (1,924 LOC)

**Channel Architecture:**
```rust
pub struct ApuState {
    pub pulse_1: PulseChannelState,
    pub pulse_2: PulseChannelState,
    pub triangle: TriangleChannelState,
    pub noise: NoiseChannelState,
    pub dmc: DmcState,

    // Mixer tables (hardware-accurate)
    pub pulse_table: Vec<f32>,  // 31 entries
    pub tnd_table: Vec<f32>,    // 16*16*128 entries
}
```

**Mixer Formulas (Hardware-Accurate):**
- **Pulse:** `95.52 / (8128.0 / n + 100.0)`
- **TND:** `159.79 / (1.0 / (t/8227.0 + n/12241.0 + d/22638.0) + 100.0)`
- Achieves accuracy within "a few dB" of real hardware

**Audio Pipeline:**
1. **Emulation:** 1.7 MHz sample rate (CPU clock rate)
2. **Staging Buffer:** Collects high-res samples
3. **Filter Chain:** Configurable NES/Famicom filtering
4. **Downsampling:** To target sample rate (typically 44.1 kHz)
5. **Output Buffer:** Final i16 PCM samples

**Frame Sequencer:**
- 4-step and 5-step modes
- Quarter-frame and half-frame events
- Frame interrupt generation
- Length counters and envelopes

### Expansion Audio Implementations

| System | LOC | Implementation Notes |
|--------|-----|---------------------|
| **VRC6** | 1,118 | 2 pulse + sawtooth, accurate waveform generation |
| **VRC7** | 1,205 | FM synthesis (YM2413), ADSR research needed |
| **MMC5** | 895 | 2 pulse + PCM channel, complex mapper integration |
| **N163** | 665 | Wavetable synthesis, 1-8 channels |
| **FME7 (S5B)** | 676 | AY-3-8910 PSG, square + noise + envelope |
| **FDS** | 1,098 | Frequency modulation, wavetable + volume envelope |

**Expansion Mixing:**
- Individual channel outputs
- Mixer ratios under active research
- Channel muting support in UI

---

## Piano Roll & Visualization

### Piano Roll Window (1,696 LOC)

**Unique Feature:** One of the most sophisticated chiptune visualization systems in any NES emulator.

**Capabilities:**
- **Real-time note display:** Musical note visualization during playback
- **Channel muting:** Click waveforms to toggle channels
- **Multiple resolutions:** 270p, 480p, 720p, 1080p configs
- **Custom coloring:** Configurable via TOML files
- **Expansion audio:** Visualizes VRC6, VRC7, MMC5, N163, S5B, FDS

**Performance:**
- Decent on most hardware
- May struggle on weak GPUs (Raspberry Pi)
- Some monitors have display issues with many channels

**Influence:**
- Inspired SPCPresenter (SNES music visualizer)
- [Mesen piano roll Lua script](https://github.com/zeta0134/mesen-piano-roll) by same author
- Used by chiptune community

**Configuration Files:**
```
cli/configs/
├── piano_roll_270p.toml
├── piano_roll_480p.toml
├── piano_roll_720p.toml
├── piano_roll_1080p.toml
└── piano_roll_colors.toml
```

---

## Build System & Dependencies

### Crate Dependencies

**Core (rustico-core):**
```toml
[dependencies]
# INTENTIONALLY MINIMAL - only Rust std for FileIO
# No external dependencies for maximum portability
```

**SDL Frontend:**
```toml
[dependencies]
dirs = "5.0.1"          # User directories
image = "0.24.6"        # Image loading
sdl2 = "0.36.0"         # Cross-platform multimedia
nfd2 = "0.3.1"          # Native file dialogs
rustico-core = { path = "../core" }
rustico-ui-common = { path = "../ui-common" }
```

**Egui Frontend:**
```toml
[dependencies]
cpal = "0.15.3"         # Cross-platform audio
eframe = "0.26.2"       # Egui framework
env_logger = "0.10"     # Logging
```

**UI Common:**
```toml
[dependencies]
csscolorparser = "0.6.1"  # Color parsing
image = "0.19"            # Image processing
toml = "0.5"              # Config files
regex = "1.6"             # Pattern matching
```

### CI/CD

**Found:** `.github/workflows/package.yml` in SDL crate

**Build Targets:**
- Linux (native)
- macOS (native)
- Windows (native)
- WebAssembly (browser)

---

## Accuracy Analysis

### Known Limitations

1. **API Instability:** "presently in constant flux and lacks a stable API"
2. **Timing Accuracy:** DMA and some mapper timing not perfect
3. **No PAL/Dendy:** NTSC only
4. **Sprite Overflow Bug:** Not emulated (games relying on this will have issues)
5. **VRC7 ADSR:** FM synthesis needs more research
6. **Frame Stepping:** Piano roll misses APU frame events when stepping

### What Works Well

**Difficult Games:**
- **Battletoads:** Complex raster effects work correctly
- **Homebrew:** Advanced tricks stable
- **Split-screen scrolling:** MMC3 and others accurate enough

**Audio Quality:**
- 1.7 MHz emulation captures subtle timbres
- Hardware-accurate mixer formulas (within few dB)
- Expansion audio working for major systems

**Mapper Coverage:**
- Common mappers (NROM, MMC1, MMC3) solid
- Advanced mappers (MMC5, Rainbow) implemented but undertested
- FDS fully functional (with BIOS)

### Test ROM Performance

**Tracking Method:**
- Manually validated against [TASVideos accuracy test suite](https://tasvideos.org/EmulatorResources/NESAccuracyTests)
- [NESdev emulator tests](https://www.nesdev.org/wiki/Emulator_tests)
- Known failures documented in README

**Notable Tests:**
- blargg's mapper tests: Some failures
- Sprite overflow: Passes (bug not emulated)
- Timing tests: Good but not perfect

---

## Community & Ecosystem

### Project Status

- **Repository:** [github.com/zeta0134/rustico](https://github.com/zeta0134/rustico)
- **Stars:** 100+
- **Development:** Active
- **History:** Renamed from RusticNES, moved to monorepo architecture

### Target Audience

1. **Chiptune Creators:** Primary focus
2. **Homebrew Developers:** Modern retro software
3. **Music Visualizers:** Piano roll feature
4. **Expansion Audio Enthusiasts:** Comprehensive support

### Related Projects

- **[rusticnes-core](https://github.com/zeta0134/rusticnes-core):** Original project (archived)
- **[rusticnes-sdl](https://github.com/zeta0134/rusticnes-sdl):** Old SDL frontend (archived)
- **[mesen-piano-roll](https://github.com/zeta0134/mesen-piano-roll):** Lua script for Mesen

### Influence

**SPCPresenter:** SNES music visualizer that ported Rustico's piano roll design ([GitHub](https://github.com/nununoisy/spc-presenter-rs))

---

## Comparison with Other Audio-Focused Emulators

| Feature | Rustico | TetaNES | Mesen2 |
|---------|---------|---------|--------|
| **Expansion Audio** | 6 systems | 0 systems | 7 systems |
| **Piano Roll** | Built-in, 1,696 LOC | None | Lua script |
| **Audio Resolution** | 1.7 MHz | Standard | Configurable |
| **Music Visualization** | Excellent | None | Good (via script) |
| **Chiptune Focus** | Primary | Not a focus | Secondary |
| **Audio Accuracy** | Very good | Good | Excellent |

---

## Recommendations for Reference

1. **Study expansion audio implementation** for VRC6, VRC7, MMC5, N163, S5B
2. **Reference FDS emulation** for disk system support
3. **Use the 1.7 MHz audio sampling approach** for high-fidelity audio
4. **Adopt the core/UI separation pattern** with minimal core dependencies
5. **Study piano roll visualization** for music-focused applications
6. **Reference mixer formulas** for hardware-accurate channel mixing
7. **Review audio filter chain** for authentic NES/Famicom sound

---

## Use Cases

| Use Case | Suitability |
|----------|-------------|
| Playing expansion audio games | Excellent |
| Chiptune creation/playback | Excellent |
| FDS game emulation | Excellent |
| Homebrew development | Excellent |
| General game playing | Good |
| Cycle-perfect accuracy | Limited |

---

## Community & Documentation

- **GitHub Issues:** Active bug tracking and mapper requests
- **Homebrew Focus:** Designed for modern retro development
- **Music Community:** Targeting chiptune creators

---

## Sources

- [GitHub - zeta0134/rustico](https://github.com/zeta0134/rustico)
- [TASVideos NES Accuracy Tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)
- [NESdev Wiki - Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [GitHub - zeta0134/mesen-piano-roll](https://github.com/zeta0134/mesen-piano-roll)
- [GitHub - nununoisy/spc-presenter-rs](https://github.com/nununoisy/spc-presenter-rs)

---

*Report Generated: December 2024*
*Enhanced: December 2024 with comprehensive code analysis and community research*
