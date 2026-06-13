# RustyNES Development TODO Tracker

**RustyNES version:** v1.0.0 (production cut)
**Project Status:** Released — the v1.0.0 development phases are delivered.

---

## Overview

This directory holds the phase-and-sprint development history that produced
RustyNES v1.0.0. The phases below are **largely delivered** — RustyNES ships
at v1.0.0 with a cycle-accurate core (AccuracyCoin 100%), 51 mapper families,
FDS, Vs./PC10, rollback netplay, RetroAchievements, TAS movie tooling, and the
performance + desktop-UX shell.

**Release line (this project's own versions):** `v0.1.0…v0.8.6` (the parent
emulator) → `v0.9.0…v0.9.7` (engine-lineage integration stages) → **`v1.0.0`**
(this synthesis: the cycle-accurate engine + the ported desktop-UX shell +
production polish). The single shipped tag is **v1.0.0**. Version markers in the
phase bodies that read `v1.x`/`v2.x` are the inbound **engine's** prior lineage
(developed across its v2.0–v2.8 line, shipped here at v1.0.0), not RustyNES
releases of their own.

The authoritative, living status lives in [`docs/STATUS.md`](../docs/STATUS.md)
(per-suite test pass counts, mapper coverage, feature flags, version policy) and
the root [`CHANGELOG.md`](../CHANGELOG.md) + [`ROADMAP.md`](../ROADMAP.md). This
tracker is kept for the historical phase/sprint breakdown and forward roadmap
notes; when it cites pass/fail numbers they should be read against
`docs/STATUS.md`.

> The detailed phase/sprint roadmap is in [`ROADMAP.md`](./ROADMAP.md) in this
> directory. The version markers inside the phase history (engine v0.9.x →
> v2.x line) are engine-lineage anchors documenting how the v1.0.0 technology
> was built, not RustyNES releases of their own.

---

## Delivered for v1.0.0

| Area | Status |
|------|--------|
| **CPU 6502** | Delivered — nestest 0-diff, all official + unofficial opcodes, cycle-accurate bus interleaving |
| **PPU 2C02** | Delivered — dot-resolution lockstep, BG + sprite pipelines, sprite-0/overflow accuracy |
| **APU 2A03** | Delivered — band-limited polyphase-BLEP synthesis, non-linear mixer, DMC DMA |
| **Mappers** | Delivered — 51 families incl. MMC1-5, full VRC line, FME-7, Vs./PC10 RGB boards |
| **Expansion audio** | Delivered — VRC6, VRC7 OPLL FM, Sunsoft 5B, Namco 163, MMC5 |
| **FDS** | Delivered — real-BIOS boot, read/write drive, multi-side, 2C33 audio |
| **Frontend** | Delivered — winit + wgpu + cpal + egui desktop shell, debugger, NTSC filter |
| **Desktop UX shell** | Delivered — always-on egui menu bar (File/Emulation/Tools/View/Debug/Help) + status bar (ROM name / run state / fading messages / FPS), first-run Welcome modal, tabbed Settings window (Display/Audio/Input/Advanced), light/dark/system themes, 8:7 pixel-aspect toggle, fullscreen + 1x-4x window-size scaling, recent-ROMs MRU (max 10, persisted) + Clear Recent, save-state slots (0-9), Keyboard Shortcuts + About windows, opt-in Pause-When-Unfocused, surfaced Cheats/Movies/Netplay/RA/Performance tool panels |
| **Playback controls** | Delivered — pause (Space), reset (F2), power-cycle (F3), fast-forward (Tab, audio muted), frame-advance while paused (Backslash), toggle menu bar (M) |
| **Performance / UX** | Delivered — display-sync pacing matrix + late input latch, lock-free audio ring + DRC, run-ahead (default 1), dedicated emulation thread (best-effort Linux priority elevation) |
| **Save states / rewind / TAS** | Delivered — `.rns` save states, rewind ring, `.rnm` movie record/replay |
| **Netplay** | Delivered — 2-4 player rollback over UDP (native) + WebRTC (browser) |
| **RetroAchievements** | Delivered — achievements, leaderboards, rich presence, hardcore (opt-in/native) |
| **WebAssembly** | Delivered — `wasm-winit` / `wasm-canvas` browser builds, AudioWorklet, rAF display-sync |

---

## Forward roadmap (post-v1.0.0)

Remaining/optional follow-ups (see `ROADMAP.md` for detail):

- Lua scripting (the one advertised-but-unbuilt v1.0.0 feature; deferred to a
  post-1.0 release).
- Mobile (iOS / Android) frontends.
- The long-tail mapper coverage (toward the ~300-mapper full set) and 100%
  TASVideos compatibility.
- A live RetroAchievements account-allowlisting pass with the RA team.
- Vs. DualSystem (two-CPU/two-PPU) games — currently detection-flagged, not yet
  emulated.
- A handful of game-specific compatibility items (FDS side-B / Kid Icarus
  post-registration path; Mito Koumon m89; the GxROM-66 / SMB3 reports).
- Browser RetroAchievements (needs an emscripten or pure-Rust `rcheevos` path).

---

## This directory

- [`ROADMAP.md`](./ROADMAP.md) — the phase/sprint roadmap entry point (status +
  links to every phase overview).
- Phase overviews + sprint files (engine-lineage development history; phases 1-5
  delivered, phases 6-8 marked COMPLETE/SUPERSEDED in-file):
  - `phase-1-foundation/` — workspace, cartridge parser, CPU core (nestest)
  - `phase-2-graphics-timing/` — PPU, lockstep scheduler, simple mappers
  - `phase-3-audio-polish/` — APU channels, DMC, mixer
  - `phase-4-mapper-coverage/` — MMC3, misc + VRC + MMC5 mappers
  - `phase-5-frontend-tooling/` — frontend, save/rewind, debugger + release
  - `phase-6-v1.0.0-final/` + `phase-6-v1-closeout/` — v1.0.0 closeout (SUPERSEDED)
  - `phase-7-nesdev-accuracy-hardening/` — accuracy hardening (COMPLETE)
  - `phase-8-v1.2.0-accuracy-residuals/` — DMC get/put scheduler (COMPLETE)
  - `archive/legacy-v0.8-todos/` — the parent emulator's pre-synthesis milestone
    TODOs, retained verbatim as history.
- Historical session reports (dated, point-in-time; superseded by the synthesis —
  see the status note at the top of each):
  [`TEST-ROM-ACQUISITION-REPORT.md`](./TEST-ROM-ACQUISITION-REPORT.md),
  [`TEST-ROM-WORKFLOW-SUMMARY.md`](./TEST-ROM-WORKFLOW-SUMMARY.md),
  [`TODO_AUDIT_SUMMARY_REPORT.md`](./TODO_AUDIT_SUMMARY_REPORT.md),
  [`TODO-GENERATION-STATUS.md`](./TODO-GENERATION-STATUS.md).

---

## Status Markers

- Done — delivered in v1.0.0
- In Progress — active development
- Pending — not started
- Optional — nice-to-have / out of core scope

---

## Key Documentation

### Status & history

- [STATUS.md](../docs/STATUS.md) — single source of truth (pass counts, mapper
  matrix, feature flags, version policy)
- [CHANGELOG.md](../CHANGELOG.md) — release history
- [ROADMAP.md](../ROADMAP.md) — project roadmap (root)
- [ROADMAP.md](./ROADMAP.md) — phase/sprint roadmap (this directory)

### Architecture & design

- [ARCHITECTURE.md](../ARCHITECTURE.md) — system design
- [OVERVIEW.md](../OVERVIEW.md) — project philosophy
- [docs/architecture.md](../docs/architecture.md) — cross-cutting decisions

### Component specifications

- [CPU 6502](../docs/cpu-6502.md)
- [PPU 2C02](../docs/ppu-2c02.md)
- [APU 2A03](../docs/apu-2a03.md)
- [Scheduler](../docs/scheduler.md)
- [Mappers](../docs/mappers.md)
- [Cartridge format](../docs/cartridge-format.md)

### Testing

- [Testing strategy](../docs/testing-strategy.md)
- [Build and tooling](../docs/build-and-tooling.md)

---

**Repository:** <https://github.com/doublegate/RustyNES>
