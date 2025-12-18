# RustyNES Documentation Index

**Last Updated:** 2025-12-18
**Total Documents:** 37

---

## Quick Navigation

- [CPU Documentation](#cpu-documentation)
- [PPU Documentation](#ppu-documentation)
- [APU Documentation](#apu-documentation)
- [Bus & Memory](#bus--memory)
- [Mappers](#mappers)
- [Input](#input)
- [Testing](#testing)
- [API Reference](#api-reference)
- [Development](#development)

---

## CPU Documentation

Complete 6502 CPU implementation reference.

### Core Specifications

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [CPU_6502.md](cpu/CPU_6502.md) | High-level CPU overview | 666 | Complete |
| [**CPU_6502_SPECIFICATION.md**](cpu/CPU_6502_SPECIFICATION.md) | **All 256 opcodes with cycle-accurate timing** | **610** | **New** |
| [**CPU_TIMING_REFERENCE.md**](cpu/CPU_TIMING_REFERENCE.md) | **Per-instruction cycle counts** | **450+** | **New** |
| [CPU_TIMING.md](cpu/CPU_TIMING.md) | Timing overview | 566 | Complete |
| [CPU_UNOFFICIAL_OPCODES.md](cpu/CPU_UNOFFICIAL_OPCODES.md) | Illegal opcode reference | 617 | Complete |

### Key Topics

- **All 256 Opcodes:** Official (151) + Unofficial (105)
- **Addressing Modes:** 13 modes with page crossing behavior
- **Interrupt Handling:** NMI, IRQ, BRK, RESET timing
- **Cycle Accuracy:** Exact cycle counts including penalties
- **DMA Timing:** OAM DMA and DMC DMA interaction

---

## PPU Documentation

Complete 2C02 PPU rendering pipeline.

### Core Specifications

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [PPU_OVERVIEW.md](ppu/PPU_OVERVIEW.md) | High-level PPU architecture | 695 | Complete |
| [**PPU_2C02_SPECIFICATION.md**](ppu/PPU_2C02_SPECIFICATION.md) | **Complete register behavior** | **500+** | **New** |
| [**PPU_TIMING_DIAGRAM.md**](ppu/PPU_TIMING_DIAGRAM.md) | **262 scanlines × 341 dots** | **475+** | **New** |
| [PPU_TIMING.md](ppu/PPU_TIMING.md) | Timing overview | 485 | Complete |
| [PPU_RENDERING.md](ppu/PPU_RENDERING.md) | Rendering pipeline | 578 | Complete |

### Scrolling & Sprites

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [PPU_SCROLLING.md](ppu/PPU_SCROLLING.md) | Scrolling overview | 500 | Complete |
| [**PPU_SCROLLING_INTERNALS.md**](ppu/PPU_SCROLLING_INTERNALS.md) | **Loopy's implementation** | **425+** | **New** |
| [**PPU_SPRITE_EVALUATION.md**](ppu/PPU_SPRITE_EVALUATION.md) | **Sprite eval, overflow bug, sprite 0** | **475+** | **New** |

### Key Topics

- **Registers:** $2000-$2007 complete specification
- **Internal Registers:** v, t, x, w (Loopy's model)
- **Rendering:** Dot-by-dot pipeline (341 dots/scanline)
- **Scrolling:** Mid-frame changes, split-screen
- **Sprites:** 8 sprites/scanline, overflow bug, sprite 0 hit

---

## APU Documentation

Complete 2A03 audio processing unit.

### Core Specifications

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [APU_OVERVIEW.md](apu/APU_OVERVIEW.md) | High-level APU architecture | 530 | Complete |
| [**APU_2A03_SPECIFICATION.md**](apu/APU_2A03_SPECIFICATION.md) | **Complete channel specs** | **425+** | **New** |
| [APU_CHANNELS.md](apu/APU_CHANNELS.md) | Channel details | 595 | Complete |
| [APU_TIMING.md](apu/APU_TIMING.md) | Frame sequencer | 574 | Complete |

### Key Topics

- **5 Channels:** Pulse 1/2, Triangle, Noise, DMC
- **Frame Sequencer:** 4-step (60 Hz) and 5-step (48 Hz)
- **Envelope Generator:** ADSR for pulse/noise
- **Sweep Unit:** Automatic frequency adjustment
- **Non-Linear Mixer:** Accurate channel mixing formulas

---

## Bus & Memory

Memory mapping and bus architecture.

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [MEMORY_MAP.md](bus/MEMORY_MAP.md) | CPU address space | 560 | Complete |
| [BUS_CONFLICTS.md](bus/BUS_CONFLICTS.md) | Bus conflict handling | 558 | Complete |

### Key Topics

- **CPU Address Space:** $0000-$FFFF mapping
- **PPU Address Space:** $0000-$3FFF mapping
- **Open Bus Behavior:** Read from unmapped addresses
- **Bus Conflicts:** NROM, CNROM, UXROM handling

---

## Mappers

NES cartridge mapper implementations.

### Mapper System

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [MAPPER_OVERVIEW.md](mappers/MAPPER_OVERVIEW.md) | Mapper architecture | 532 | Complete |
| [**MAPPER_IMPLEMENTATION_GUIDE.md**](mappers/MAPPER_IMPLEMENTATION_GUIDE.md) | **How to implement mappers** | **450+** | **New** |

### Individual Mappers

| Document | Mapper # | Name | Lines | Status |
|----------|----------|------|-------|--------|
| [MAPPER_NROM.md](mappers/MAPPER_NROM.md) | 000 | NROM | 343 | Complete |
| [MAPPER_MMC1.md](mappers/MAPPER_MMC1.md) | 001 | MMC1 | 246 | Complete |
| [MAPPER_UXROM.md](mappers/MAPPER_UXROM.md) | 002 | UxROM | 459 | Complete |
| [MAPPER_CNROM.md](mappers/MAPPER_CNROM.md) | 003 | CNROM | 238 | Complete |
| [MAPPER_MMC3.md](mappers/MAPPER_MMC3.md) | 004 | MMC3 | 275 | Complete |

### Key Topics

- **Mapper Trait:** Rust trait for all mappers
- **Banking Systems:** PRG/CHR banking patterns
- **IRQ Generation:** Scanline counters (MMC3, VRC)
- **Bus Conflicts:** Hardware quirks
- **Testing:** Per-mapper test ROMs

---

## Input

Controller and input device handling.

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [INPUT_HANDLING.md](input/INPUT_HANDLING.md) | Controller implementation | 342 | Complete |

### Key Topics

- **Standard Controller:** D-pad, A, B, Select, Start
- **Shift Register:** Serial reading protocol
- **Multiple Controllers:** Up to 4 controllers
- **Special Devices:** Zapper, Power Pad

---

## Testing

Test ROMs and validation methodology.

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [**TEST_ROM_GUIDE.md**](testing/TEST_ROM_GUIDE.md) | **Complete test ROM catalog** | **425+** | **New** |
| [**NESTEST_GOLDEN_LOG.md**](testing/NESTEST_GOLDEN_LOG.md) | **nestest.nes methodology** | **400+** | **New** |

### Key Topics

- **nestest.nes:** CPU golden log comparison
- **Blargg Suite:** instr_test-v5, cpu_timing, cpu_interrupts
- **PPU Tests:** sprite_hit, sprite_overflow, ppu_vbl_nmi
- **APU Tests:** apu_test, dmc_tests
- **Mapper Tests:** mmc3_test, mapper-specific ROMs

---

## API Reference

Public API for RustyNES crates.

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [CORE_API.md](api/CORE_API.md) | Core crate API | 297 | Complete |
| [SAVE_STATES.md](api/SAVE_STATES.md) | Save state format | 327 | Complete |
| [CONFIGURATION.md](api/CONFIGURATION.md) | Configuration API | 303 | Complete |

### Key Topics

- **Embedding:** Using RustyNES as a library
- **Save States:** State serialization/deserialization
- **Configuration:** Runtime settings
- **Callbacks:** Frame rendering, audio output

---

## Development

Developer guides and contribution information.

| Document | Description | Lines | Status |
|----------|-------------|-------|--------|
| [BUILD.md](dev/BUILD.md) | Build instructions | 172 | Complete |
| [CONTRIBUTING.md](dev/CONTRIBUTING.md) | Contribution guide | 203 | Complete |
| [TESTING.md](dev/TESTING.md) | Testing strategy | 188 | Complete |
| [DEBUGGING.md](dev/DEBUGGING.md) | Debugging guide | 187 | Complete |
| [GLOSSARY.md](dev/GLOSSARY.md) | NES terminology | 253 | Complete |

### Key Topics

- **Building:** Prerequisites, feature flags
- **Contributing:** Code style, PR process
- **Testing:** Test ROM automation, CI/CD
- **Debugging:** Common issues, tools
- **Glossary:** NES hardware terms

---

## Document Legend

- **Bold** = New document created 2025-12-18
- Lines = Approximate line count
- Status:
  - **Complete** = Existing documentation
  - **New** = Created in this session
  - **Planned** = Future documentation

---

## Recently Created (2025-12-18)

### High-Priority Technical Specifications (10 documents, 5,000+ lines)

1. **CPU_6502_SPECIFICATION.md** (610 lines) - All 256 opcodes
2. **CPU_TIMING_REFERENCE.md** (450+ lines) - Cycle-accurate timing
3. **PPU_2C02_SPECIFICATION.md** (500+ lines) - Complete register behavior
4. **PPU_TIMING_DIAGRAM.md** (475+ lines) - Dot-by-dot timing
5. **PPU_SCROLLING_INTERNALS.md** (425+ lines) - Loopy's implementation
6. **PPU_SPRITE_EVALUATION.md** (475+ lines) - Sprite evaluation + overflow bug
7. **APU_2A03_SPECIFICATION.md** (425+ lines) - Complete APU specification
8. **MAPPER_IMPLEMENTATION_GUIDE.md** (450+ lines) - Mapper development guide
9. **TEST_ROM_GUIDE.md** (425+ lines) - Test ROM catalog
10. **NESTEST_GOLDEN_LOG.md** (400+ lines) - nestest methodology

See [DOCUMENTATION_GENERATION_SUMMARY.md](../DOCUMENTATION_GENERATION_SUMMARY.md) for complete details.

---

## Future Documentation

### High Priority (P1) - 11 documents

- APU channel deep-dives (Pulse, Triangle, Noise, DMC)
- Additional mapper specs (MMC1, MMC3, NROM, UxROM, CNROM)
- Bus architecture documentation
- Save state format specification

### Medium Priority (P2-P3) - 40 documents

- ROM formats (iNES, NES 2.0, NSF, FM2, FDS)
- API documentation (per-crate APIs, Lua scripting)
- Advanced features (RetroAchievements, netplay, TAS tools)
- Platform guides (Windows, macOS, Linux, WASM builds)
- Testing documentation (Blargg matrix, accuracy validation)
- Development process (coding standards, architecture decisions)

---

## External Resources

### Primary References

- [NESdev Wiki](https://www.nesdev.org/wiki/) - Hardware specifications
- [NESdev Forums](https://forums.nesdev.org/) - Technical discussions
- [TASVideos](https://tasvideos.org/EmulatorResources/NESAccuracyTests) - Accuracy tests
- [Visual 6502](http://visual6502.org/) - Transistor-level CPU simulation
- [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02) - PPU simulation

### Test ROMs

- [nes-test-roms (GitHub)](https://github.com/christopherpow/nes-test-roms)
- [Blargg's test ROMs](https://www.nesdev.org/wiki/Emulator_tests)
- [bisqwit/nes_tests](https://bisqwit.iki.fi/src/nes_tests)

---

**Total Documentation:** 37 documents
**New This Session:** 10 comprehensive specifications (5,000+ lines)
**Last Updated:** 2025-12-18
