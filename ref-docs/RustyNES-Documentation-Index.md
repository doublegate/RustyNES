# RustyNES - NES Emulator Project Documentation

**Version:** 1.0.0  
**Project:** RustyNES - Next-Generation NES Emulator in Rust  
**Document Type:** Documentation Index & Planning Guide  
**Status:** Planning Phase

---

## Table of Contents

1. [Core Project Documents](#core-project-documents)
2. [Hardware Component Specifications](#hardware-component-specifications)
3. [System Integration Documents](#system-integration-documents)
4. [Development & Testing Documents](#development--testing-documents)
5. [API & Integration Documents](#api--integration-documents)
6. [Recommended Starter Set](#recommended-starter-set-priority-order)
7. [Next Set of Documentation](#next-set-of-documentation-for-rustynes)
   - [Tier 1: Component Deep-Dive Specifications](#tier-1-component-deep-dive-specifications)
   - [Tier 2: Memory & Mapper Documentation](#tier-2-memory--mapper-documentation)
   - [Tier 3: ROM & File Format Documentation](#tier-3-rom--file-format-documentation)
   - [Tier 4: Crate API Documentation](#tier-4-crate-api-documentation)
   - [Tier 5: Advanced Feature Implementation Guides](#tier-5-advanced-feature-implementation-guides)
   - [Tier 6: Platform & Build Documentation](#tier-6-platform--build-documentation)
   - [Tier 7: Testing & Validation Documentation](#tier-7-testing--validation-documentation)
   - [Tier 8: Development Process Documentation](#tier-8-development-process-documentation)
8. [Recommended Document Priority Order](#recommended-document-priority-order)

---

## Core Project Documents

| Document | Purpose |
|----------|---------|
| `docs/README.md` | Project introduction, quick start, build instructions, license |
| `docs/OVERVIEW.md` | Philosophy, accuracy goals, emulation approach (cycle-accurate vs scanline) |
| `docs/ARCHITECTURE.md` | High-level system design, component relationships, data flow diagrams |
| `docs/ROADMAP.md` | Development phases, milestones, feature priorities |

---

## Hardware Component Specifications

| Document | Purpose |
|----------|---------|
| `docs/cpu/CPU_6502.md` | Ricoh 2A03 core, instruction set, addressing modes, cycle timing |
| `docs/cpu/CPU_TIMING.md` | Per-instruction cycle counts, page-crossing penalties, interrupt latency |
| `docs/cpu/CPU_UNOFFICIAL_OPCODES.md` | Undocumented opcodes for compatibility |
| `docs/ppu/PPU_OVERVIEW.md` | 2C02 architecture, rendering pipeline, VRAM layout |
| `docs/ppu/PPU_TIMING.md` | Scanline timing, sprite evaluation, NMI generation |
| `docs/ppu/PPU_RENDERING.md` | Background/sprite rendering, pixel priority, palette handling |
| `docs/ppu/PPU_SCROLLING.md` | Loopy's scrolling model, fine X/Y, coarse scroll |
| `docs/apu/APU_OVERVIEW.md` | Audio channels, frame counter, mixer |
| `docs/apu/APU_CHANNELS.md` | Pulse, triangle, noise, DMC specifications |
| `docs/apu/APU_TIMING.md` | Frame sequencer, length counters, sweep units |

---

## System Integration Documents

| Document | Purpose |
|----------|---------|
| `docs/bus/MEMORY_MAP.md` | CPU/PPU address spaces, mirroring, open bus behavior |
| `docs/bus/BUS_CONFLICTS.md` | Bus conflict handling, simultaneous access |
| `docs/mappers/MAPPER_OVERVIEW.md` | Cartridge architecture, PRG/CHR banking concepts |
| `docs/mappers/MAPPER_NROM.md` | Mapper 000 (baseline, no banking) |
| `docs/mappers/MAPPER_MMC1.md` | Mapper 001 (SxROM) |
| `docs/mappers/MAPPER_MMC3.md` | Mapper 004 (TxROM, scanline counter) |
| `docs/mappers/MAPPER_*.md` | Additional mappers as needed |
| `docs/input/INPUT_HANDLING.md` | Controller polling, strobe behavior, expansion port |

---

## Development & Testing Documents

| Document | Purpose |
|----------|---------|
| `docs/dev/CONTRIBUTING.md` | Code style, PR process, commit conventions |
| `docs/dev/BUILD.md` | Toolchain requirements, feature flags, cross-compilation |
| `docs/dev/TESTING.md` | Test ROM suites, accuracy benchmarks, CI pipeline |
| `docs/dev/DEBUGGING.md` | Built-in debugger, memory viewer, PPU state inspection |
| `docs/dev/GLOSSARY.md` | NES-specific terminology reference |

---

## API & Integration Documents

| Document | Purpose |
|----------|---------|
| `docs/api/CORE_API.md` | Emulator core interface, embedding guide |
| `docs/api/SAVE_STATES.md` | State serialization format, versioning |
| `docs/api/CONFIGURATION.md` | Runtime options, accuracy/performance tradeoffs |

---

## Recommended Starter Set (Priority Order)

For initial development, begin with these eight documents:

1. **`README.md`** — First impression, build instructions
2. **`OVERVIEW.md`** — Project philosophy and accuracy targets
3. **`ARCHITECTURE.md`** — Component diagram and interaction model
4. **`docs/cpu/CPU_6502.md`** — The heart of the system
5. **`docs/ppu/PPU_OVERVIEW.md`** — Most complex component
6. **`docs/bus/MEMORY_MAP.md`** — Glue between components
7. **`docs/mappers/MAPPER_OVERVIEW.md`** — Cartridge abstraction layer
8. **`docs/dev/TESTING.md`** — Validation strategy from day one

---

## Next Set of Documentation for RustyNES

### Tier 1: Component Deep-Dive Specifications

These documents expand on the architecture with **implementation-ready detail**:

| Document | Purpose |
|----------|---------|
| `docs/cpu/CPU_6502_SPECIFICATION.md` | Complete instruction reference with cycle-by-cycle breakdown, all 256 opcodes (including 105 unofficial), addressing mode implementations, interrupt edge cases |
| `docs/cpu/CPU_TIMING_REFERENCE.md` | Per-instruction cycle counts, page-crossing penalties, interrupt polling windows, DMA timing interactions |
| `docs/cpu/CPU_UNOFFICIAL_OPCODES.md` | All 105 undocumented instructions (LAX, SAX, DCP, ISC, SLO, RLA, SRE, RRA, ANC, ALR, ARR, XAA, AHX, TAS, SHY, SHX, LAS, KIL) |
| `docs/ppu/PPU_2C02_SPECIFICATION.md` | Complete register behavior, dot-by-dot rendering state machine, internal latches, decay behavior |
| `docs/ppu/PPU_SCROLLING_INTERNALS.md` | Loopy's scrolling document implementation (v/t registers, fine X, coarse X/Y increments, mid-frame changes) |
| `docs/ppu/PPU_SPRITE_EVALUATION.md` | Secondary OAM population, sprite overflow bug emulation, sprite 0 hit timing window |
| `docs/ppu/PPU_TIMING_DIAGRAM.md` | Visual timing diagrams for all 262 scanlines × 341 dots, memory access patterns |
| `docs/apu/APU_2A03_SPECIFICATION.md` | Complete channel specifications, frame sequencer modes (4-step/5-step), mixer equations |
| `docs/apu/APU_CHANNEL_PULSE.md` | Pulse channel deep-dive: duty cycles, sweep unit, envelope, muting conditions |
| `docs/apu/APU_CHANNEL_TRIANGLE.md` | Triangle channel: linear counter, sequencer, ultrasonic handling |
| `docs/apu/APU_CHANNEL_NOISE.md` | Noise channel: LFSR modes, period table, mode flag behavior |
| `docs/apu/APU_CHANNEL_DMC.md` | DMC channel: sample fetching, DMA conflicts, IRQ generation, memory reader state machine |
| `docs/apu/APU_EXPANSION_AUDIO.md` | VRC6, VRC7, MMC5, N163, Sunsoft 5B, FDS expansion chip specifications |

---

### Tier 2: Memory & Mapper Documentation

| Document | Purpose |
|----------|---------|
| `docs/bus/BUS_ARCHITECTURE.md` | CPU/PPU bus separation, open bus behavior, address decoding, bus conflict emulation |
| `docs/bus/MEMORY_ACCESS_PATTERNS.md` | Read-modify-write timing, dummy reads/writes, DMA bus hijacking |
| `docs/mappers/MAPPER_IMPLEMENTATION_GUIDE.md` | How to implement a new mapper: trait requirements, testing checklist, common patterns |
| `docs/mappers/MAPPER_000_NROM.md` | NROM variants (NROM-128, NROM-256), CHR-ROM vs CHR-RAM |
| `docs/mappers/MAPPER_001_MMC1.md` | MMC1 shift register, banking modes, PRG/CHR switching, WRAM control |
| `docs/mappers/MAPPER_002_UXROM.md` | UxROM variants, bus conflicts, oversize ROM handling |
| `docs/mappers/MAPPER_003_CNROM.md` | CNROM variants, bus conflicts, security/copy protection |
| `docs/mappers/MAPPER_004_MMC3.md` | MMC3 IRQ counter (A12 edge detection), banking modes, mirroring control |
| `docs/mappers/MAPPER_005_MMC5.md` | MMC5 complexity: ExRAM modes, split screen, expansion audio, multiplier |
| `docs/mappers/MAPPER_SUBMAPPER_GUIDE.md` | NES 2.0 submapper handling, per-mapper submapper behavior |

---

### Tier 3: ROM & File Format Documentation

| Document | Purpose |
|----------|---------|
| `docs/formats/INES_FORMAT.md` | iNES 1.0 header parsing, trainer handling, common header errors |
| `docs/formats/NES20_FORMAT.md` | NES 2.0 extended header: submappers, timing, EEPROM, misc ROMs |
| `docs/formats/UNIF_FORMAT.md` | Universal NES Image Format: chunk parsing, board name mapping |
| `docs/formats/FDS_FORMAT.md` | Famicom Disk System: disk format, BIOS requirements, disk write handling |
| `docs/formats/NSF_FORMAT.md` | NES Sound Format: player driver, bank switching, expansion audio detection |
| `docs/formats/FM2_FORMAT.md` | FCEUX movie format: header fields, input encoding, subtitle support, sync verification |
| `docs/formats/SAVESTATE_FORMAT.md` | RustyNES savestate format: versioning, compression, component serialization order |

---

### Tier 4: Crate API Documentation

| Document | Purpose |
|----------|---------|
| `docs/api/RUSTYNES_CORE_API.md` | `rustynes-core` public API: Console, configuration, embedding guide |
| `docs/api/RUSTYNES_CPU_API.md` | `rustynes-cpu` API: standalone 6502 usage, bus trait requirements |
| `docs/api/RUSTYNES_PPU_API.md` | `rustynes-ppu` API: framebuffer access, debug inspection methods |
| `docs/api/RUSTYNES_APU_API.md` | `rustynes-apu` API: sample output, expansion audio registration |
| `docs/api/RUSTYNES_MAPPERS_API.md` | `rustynes-mappers` API: Mapper trait, dynamic mapper loading |
| `docs/api/LUA_SCRIPTING_API.md` | Complete Lua API reference: memory, emu, input, gui, joypad tables |
| `docs/api/NETPLAY_PROTOCOL.md` | GGPO session management, state serialization, rollback requirements |

---

### Tier 5: Advanced Feature Implementation Guides

| Document | Purpose |
|----------|---------|
| `docs/features/RETROACHIEVEMENTS_INTEGRATION.md` | rcheevos integration: login flow, memory descriptors, hardcore mode |
| `docs/features/NETPLAY_IMPLEMENTATION.md` | backroll-rs setup, input prediction, savestate requirements for rollback |
| `docs/features/TAS_RECORDING.md` | Deterministic execution requirements, FM2 recording, desync detection |
| `docs/features/TAS_EDITOR_DESIGN.md` | Greenzone architecture, piano roll implementation, branching |
| `docs/features/REWIND_IMPLEMENTATION.md` | Ring buffer savestate strategy, memory budget, compression tradeoffs |
| `docs/features/DEBUGGER_ARCHITECTURE.md` | Breakpoint system, trace logging, PPU/APU state inspection |
| `docs/features/CHEAT_SYSTEM.md` | Game Genie decoding, Pro Action Replay format, runtime patching |

---

### Tier 6: Platform & Build Documentation

| Document | Purpose |
|----------|---------|
| `docs/platform/BUILD_GUIDE.md` | Prerequisites, feature flags, cross-compilation targets |
| `docs/platform/WINDOWS_BUILD.md` | MSVC vs GNU toolchain, SDL2 setup, code signing |
| `docs/platform/MACOS_BUILD.md` | Intel/ARM universal binary, app bundle creation, notarization |
| `docs/platform/LINUX_BUILD.md` | Package dependencies, AppImage creation, Steam Deck optimization |
| `docs/platform/WASM_BUILD.md` | wasm-pack workflow, web frontend, browser limitations |
| `docs/platform/EMBEDDED_CONSIDERATIONS.md` | Memory constraints, no-std possibilities, fixed-point APU |

---

### Tier 7: Testing & Validation Documentation

| Document | Purpose |
|----------|---------|
| `docs/testing/TEST_ROM_GUIDE.md` | Test ROM inventory, expected results, failure interpretation |
| `docs/testing/NESTEST_GOLDEN_LOG.md` | nestest.nes automation, golden log comparison methodology |
| `docs/testing/BLARGG_TEST_MATRIX.md` | All blargg tests with pass/fail criteria and failure root causes |
| `docs/testing/ACCURACY_VALIDATION.md` | TASVideos suite methodology, game compatibility testing |
| `docs/testing/REGRESSION_TESTING.md` | CI pipeline design, screenshot comparison, audio fingerprinting |
| `docs/testing/PERFORMANCE_BENCHMARKS.md` | Criterion benchmarks, profiling methodology, optimization targets |

---

### Tier 8: Development Process Documentation

| Document | Purpose |
|----------|---------|
| `CONTRIBUTING.md` | Code style, PR process, commit conventions, review checklist |
| `docs/dev/CODING_STANDARDS.md` | Rust idioms, naming conventions, documentation requirements |
| `docs/dev/ARCHITECTURE_DECISIONS.md` | ADRs (Architecture Decision Records) for major design choices |
| `docs/dev/DEBUGGING_TIPS.md` | Common emulation bugs, debugging strategies, useful test ROMs |
| `docs/dev/PERFORMANCE_GUIDE.md` | Profiling workflow, hot path identification, optimization patterns |

---

## Recommended Document Priority Order

For immediate development focus, create these documents first:

| Priority | Documents | Rationale |
|----------|-----------|-----------|
| **P0** | `CPU_6502_SPECIFICATION.md`, `CPU_TIMING_REFERENCE.md` | CPU is foundation; must be 100% correct first |
| **P0** | `PPU_2C02_SPECIFICATION.md`, `PPU_TIMING_DIAGRAM.md` | PPU timing is critical for game compatibility |
| **P0** | `NESTEST_GOLDEN_LOG.md`, `TEST_ROM_GUIDE.md` | Validation methodology from day one |
| **P1** | `MAPPER_IMPLEMENTATION_GUIDE.md`, `MAPPER_004_MMC3.md` | MMC3 covers 23% of games and has tricky IRQ |
| **P1** | `APU_2A03_SPECIFICATION.md`, `APU_CHANNEL_DMC.md` | DMC is most complex audio component |
| **P1** | `SAVESTATE_FORMAT.md` | Required for netplay, rewind, TAS |
| **P2** | `RUSTYNES_CORE_API.md`, `LUA_SCRIPTING_API.md` | Public API stability for integrators |
| **P2** | `RETROACHIEVEMENTS_INTEGRATION.md` | Key differentiating feature |
| **P3** | Platform-specific build guides | Cross-platform is a stated goal |

---

**Document Version:** 1.0.0  
**Last Updated:** 2025-12-18  
**Status:** Documentation Planning Complete
