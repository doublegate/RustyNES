# RustyNES Documentation Index

**RustyNES version:** v1.0.0

This index maps the `docs/` tree for RustyNES v1.0.0 — the cycle-accurate
NES/Famicom emulator. The single source of truth for test pass counts, mapper
coverage, feature flags, and version policy is [`STATUS.md`](STATUS.md).

---

## Subsystem specifications

The core "spec" docs — kept in sync with the code in the same PR as a change.

| Document | Subsystem |
|----------|-----------|
| [cpu-6502.md](cpu-6502.md) | 6502 CPU — opcodes, addressing modes, cycle-accurate bus interleaving, interrupts |
| [ppu-2c02.md](ppu-2c02.md) | 2C02 PPU — rendering pipeline, scrolling (Loopy), sprite evaluation, registers |
| [apu-2a03.md](apu-2a03.md) | 2A03 APU — 5 channels, frame sequencer, non-linear mixer, DMC DMA |
| [scheduler.md](scheduler.md) | PPU-master-clock lockstep scheduler (dot-resolution timing) |
| [mappers.md](mappers.md) | Mapper system — 51 families, banking, per-mapper IRQ, expansion audio |
| [cartridge-format.md](cartridge-format.md) | iNES / NES 2.0 / FDS parsing |
| [architecture.md](architecture.md) | Cross-cutting design (Bus owns mutable state, one-directional crate graph, determinism contract) |
| [frontend.md](frontend.md) | The `rustynes` desktop app (winit + wgpu + cpal + egui), audio engine, pacing, run-ahead |

## Cross-cutting references

| Document | Topic |
|----------|-------|
| [STATUS.md](STATUS.md) | **Single source of truth** — per-suite test pass counts, mapper matrix, feature flags, version policy |
| [testing-strategy.md](testing-strategy.md) | The six testing layers; test ROMs as the spec |
| [performance.md](performance.md) | Performance targets, measured baselines, optimization landings, PGO recipe |
| [benchmarks.md](benchmarks.md) | Full reproducible benchmark record (R1 master clock A/B, performance-pass baselines) |
| [compatibility.md](compatibility.md) | ROM-format + mapper + per-game compatibility status |
| [glossary.md](glossary.md) | NES hardware + emulation terminology |
| [build-and-tooling.md](build-and-tooling.md) | Build, feature flags, toolchain, CI |
| [nesdev-hardware-emulation-checklist.md](nesdev-hardware-emulation-checklist.md) | Hardware-behavior coverage checklist |
| [netplay-webrtc.md](netplay-webrtc.md) | Rollback netplay (UDP + browser WebRTC) design |
| [ppu-trace-tooling.md](ppu-trace-tooling.md) | PPU state-trace diagnostic tooling |
| [ra-integration-request.md](ra-integration-request.md) | RetroAchievements allowlisting request template |

---

## Subdirectories

| Directory | Contents |
|-----------|----------|
| [adr/](adr/) | Architecture Decision Records (Michael Nygard format) — mapper dispatch, IRQ-timing coordination, save-state migration, VRC7 audio, DMC scheduler, TAS movie format, etc. |
| [audit/](audit/) | Dated investigation + decision-rationale notes (the accuracy-program "why" history). Retains the engine-lineage version markers and dates. |
| [release-notes/](release-notes/) | Per-version release notes for the engine-lineage line that produced v1.0.0 (`v2.0.0` … `v2.8.0`). Kept as lineage history; the current RustyNES release is **v1.0.0** (see the root `CHANGELOG.md`). |
| [tooling/](tooling/) | Oracle / cross-emulator tooling setup (Mesen2 trace, AccuracyCoin extraction). |
| [user-guide/](user-guide/) | End-user docs — getting started, controls, configuration, debugger, save states/rewind, display/audio, file locations, compatibility, troubleshooting. |
| [features/](features/) | Deep design docs per feature — debugger, expansion audio, RetroAchievements, rewind, TAS movies, video filters, WASM integration. |
| [dev/](dev/) | Developer guides — build, contributing, debugging, testing, style guide, architecture decisions, glossary. |
| [testing/](testing/) | Test-ROM catalogs, nestest golden-log methodology, PPU/game-ROM testing strategy, baselines audit. |
| [archive/legacy-v0.8-docs/](archive/legacy-v0.8-docs/) | The pre-synthesis documentation set (historical; superseded by the docs above). |

---

## External references

- [NESdev Wiki](https://www.nesdev.org/wiki/) — hardware specifications
- [NESdev Forums](https://forums.nesdev.org/) — technical discussions
- [TASVideos accuracy tests](https://tasvideos.org/EmulatorResources/NESAccuracyTests)
- [Visual 6502](http://visual6502.org/) / [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02)
- [nes-test-roms (GitHub)](https://github.com/christopherpow/nes-test-roms) · [Blargg's test ROMs](https://www.nesdev.org/wiki/Emulator_tests)

---

**Source of truth:** [STATUS.md](STATUS.md) · **Release history:** [`../CHANGELOG.md`](../CHANGELOG.md)
