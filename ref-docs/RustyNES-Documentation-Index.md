# RustyNES — Reference-Documentation Index (`ref-docs/`)

**Project:** RustyNES — Cycle-Accurate NES Emulator in Rust
**Shipped release:** v1.0.0
**Document type:** Index of the `ref-docs/` reference tree
**Status:** Current

---

## What `ref-docs/` is

`ref-docs/` is RustyNES's **reference and design-history** tier. It holds the
external hardware/emulation research that informed the project, the
state-of-the-art emulator survey it was benchmarked against, and the original
aspirational design specifications (architecture, GUI-framework evaluation,
UI/UX). It is intentionally distinct from `docs/` at the repo root, which is the
**living implementation spec** kept in sync with the code.

Most files here are immutable reference: the research reports, the nesdev-wiki
technical report, the emulator-comparison survey, and the per-emulator technical
reports are external knowledge captured at a point in time and are not updated as
the code evolves. The design-spec files (architecture, GUI-framework, UI/UX v1/v2)
are retained as **design history** — each now carries a "Status (v1.0.0)" note at
the top reconciling its forward-looking design against what actually shipped.

> **Re-versioning note.** RustyNES's own release line runs v0.1.0…v0.8.6 (the
> parent emulator) → v0.9.0…v0.9.7 (engine-lineage integration stages) → **v1.0.0**
> (the synthesis: the cycle-accurate engine developed across the upstream engine's
> v2.0–v2.8 lineage, with the parent's polished UX shell ported onto it). Some
> reference files below were generated under the engine's prior "v2" lineage and
> name it in their titles or dates; those are upstream-engine history, not a
> RustyNES "v2" release. The single shipped tag is **v1.0.0**.

---

## Contents of `ref-docs/`

### Design-history specifications (retained, status-reconciled)

| File | Purpose | Status |
|------|---------|--------|
| `RustyNES-Architecture-Design.md` | Original comprehensive architecture & technical design spec (vision, crate structure, graphics/audio/input pipelines, roadmap, reference matrix). | Design history. Shipped stack is winit + wgpu + egui + cpal across the `rustynes-*` crates; see its top-of-file Status note. |
| `RustyNES-GUI_Framework-Change.md` | Framework-evaluation memo (Iced + WGPU vs egui/pixels/SDL2/Slint/Dioxus). | Design history; decision superseded. Shipped: winit + wgpu (direct) + egui + cpal — not SDL2, not eframe/glow, not the `pixels` crate. See its Status note. |
| `RustyNES-UI_UX-Design-v1.md` | Original UI/UX design spec (Phase 1 MVP + advanced features; "Nostalgic Futurism"). | Design history. Shipped UX = egui menu bar + status bar, Settings window, themes, 8:7 aspect, Welcome/About modals. See its Status note. |
| `RustyNES-UI_UX-Design-v2.md` | Enhanced UI/UX spec (run-ahead, CRT pipeline, HTPC/10-foot mode, HD packs). "Version 2.0.0" is the document revision, not a RustyNES release. | Design history; aspirational. Run-ahead + display-sync pacing + NTSC pass shipped; the elaborate CRT/HTPC/HD-pack vision did not. See its Status note. |

### Research & reference (immutable external knowledge)

| File | Purpose |
|------|---------|
| `research-report.md` | Deep research report (generated 2026-05-10) on cycle-accurate NES emulation in Rust: 60+ surveyed sources on CPU/PPU/APU hardware, mappers, the master-clock/lockstep model, and the emulation state-of-the-art (Mesen2, higan/ares, Visual 2C02/6502). Titled "RustyNES v2" by its generation context — engine lineage, not a release. The hardware reference backing the whole project. |
| `nesdev-wiki-technical-report.md` | Nesdev Wiki technical synthesis (generated 2026-05-20): NES/Famicom hardware, programming, emulation, cartridge formats, mappers, test ROMs, and accuracy risks distilled from the canonical community wiki. |
| `Claude-NES_Emulator_Compare-Opus4.5.md` | Cross-emulator comparison survey: accuracy benchmarks (TASVideos suite), architectural patterns, and feature matrices across the leading NES emulators, with Rust-implementation guidance. |
| `2026-05-19-dmc-dma-cascade-b-handoff.md` | Dated accuracy-investigation handoff (2026-05-19): DMC DMA halt-cycle precision work in `rustynes-core/src/bus.rs::service_dmc_dma` against the AccuracyCoin "APU Registers and DMA" suite. Historical engineering record, kept as-is. |

### `Emulator_TechReports/` — per-emulator deep dives (immutable)

Per-project technical reports surveying notable NES emulators for architecture,
accuracy approach, mapper coverage, and reusable patterns. One file per emulator:

| File | Subject |
|------|---------|
| `Mesen2-Technical-Report.md` | Mesen2 (C++/C#) — the 100%-accuracy gold-standard reference. |
| `Ares-Technical-Report.md` | ares / higan NES core — lockstep PPU-dot scheduling, code clarity. |
| `puNES-Technical-Report.md` | puNES — broad mapper coverage, second-reference accuracy. |
| `FCEUX-Technical-Report.md` | FCEUX — the TAS-tooling and debugging model. |
| `TetaNES-Technical-Report.md` | TetaNES — pure-Rust wgpu + egui + winit + cpal architecture (the closest stack analog). |
| `Rustico-Technical-Report.md` | Rustico (Rust). |
| `Pinky-Technical-Report.md` | Pinky (Rust, libretro approach). |
| `rib-nes-emulator-Technical-Report.md` | rib NES emulator (Rust). |
| `DaveTCode-nes-emulator-rust-Technical-Report.md` | DaveTCode's Rust NES emulator. |
| `kamiyaowl-rust-nes-emulator-Technical-Report.md` | kamiyaowl's Rust NES emulator. |
| `starrhorne-nes-rust-Technical-Report.md` | starrhorne's nes-rust (Rust). |
| `takahirox-nes-rust-Technical-Report.md` | takahirox's nes-rust (Rust). |

---

## Where the living spec lives (the `docs/` tree)

`ref-docs/` is reference; the authoritative, code-synced documentation is under
`docs/` at the repo root. Start there for current implementation detail:

| `docs/` file | Purpose |
|--------------|---------|
| `docs/DOCUMENTATION_INDEX.md` | Index of the living `docs/` tree. |
| `docs/STATUS.md` | Single source of truth for per-suite test counts, the mapper coverage matrix, feature-flag state, and version policy. |
| `docs/architecture.md` | High-level system design (the Bus-owns-everything, PPU-master-clock model). |
| `docs/scheduler.md` | The lockstep PPU-dot scheduler. |
| `docs/cpu-6502.md` · `docs/ppu-2c02.md` · `docs/apu-2a03.md` | Per-chip implementation specs. |
| `docs/mappers.md` | Mapper families + per-mapper IRQ behavior. |
| `docs/cartridge-format.md` | iNES / NES 2.0 / FDS parsing. |
| `docs/frontend.md` | The shipped winit + wgpu + egui + cpal frontend, the UX shell, keybindings, config. |
| `docs/compatibility.md` | Game/feature compatibility, deferred items, scope decisions. |
| `docs/netplay-webrtc.md` | Rollback netplay + browser WebRTC. |
| `docs/testing-strategy.md` · `docs/testing/` | The six testing layers, test-ROM methodology. |
| `docs/performance.md` · `docs/benchmarks.md` | Performance targets and benchmark records. |
| `docs/build-and-tooling.md` | Toolchain, feature flags, build/cross-compile. |
| `docs/glossary.md` | NES terminology. |
| `docs/adr/` | Architecture Decision Records (Michael Nygard format). |
| `docs/release-notes/` | Per-version release notes. |
| `docs/user-guide/` | End-user controls and configuration. |

---

**Last updated:** 2026-06-13
**Status:** Current — reflects the `ref-docs/` tree as shipped at RustyNES v1.0.0
