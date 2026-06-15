# RustyNES Development TODO Tracker

**RustyNES version:** v1.1.0 (first feature release on the v1.0.0 production core)
**Project Status:** Released — v1.1.0 shipped on top of the v1.0.0 cycle-accurate core.

---

## Overview

This directory holds the phase-and-sprint development history that produced
RustyNES v1.0.0 (the cycle-accurate production core) and the v1.1.0 feature
release built on top of it. The phases below are **delivered** — RustyNES ships
at v1.1.0 with a cycle-accurate core (AccuracyCoin 100%), 51 mapper families,
FDS, Vs./PC10, rollback netplay, RetroAchievements, TAS movie tooling, the
performance + desktop-UX shell, and the v1.1.0 feature set (visual filters,
peripherals/QoL, debugger devtools, NSF player + EQ, and the Lua scripting
engine).

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
| ------ | ------ |
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

## Forward roadmap (post-v1.1.0)

**v1.1.0 is shipped** — the first feature release on the v1.0.0 production core.
The two release-named folders that staged it (`v1.0.1-compat-hygiene/` and
`v1.1.0-features/`, along with the engine-lineage `phase-N` plans) are now
archived under [`archive/`](./archive/README.md). What landed in v1.1.0:

- **Visual filters** — full NES_NTSC composite emulation + CRT / scanline
  shaders + `.pal` palette-file loading.
- **Peripherals & QoL** — NES Power Pad support, turbo / autofire, an
  input-display overlay, and a per-game nametable-mirroring override database.
- **Debugger devtools** — breakpoints, a cycle-accurate trace logger, and an
  event viewer.
- **Audio** — an NSF / NSFe music-file player + a 5-band graphic equalizer.
- **Lua scripting engine** — the flagship feature: a scriptable hook API over
  the emulator core.

See the archived [`v1.1.0-features/`](./archive/v1.1.0-features/overview.md) for
the shipped-feature detail.

Genuine remaining post-v1.1.0 items (see `ROADMAP.md` for detail):

- **Mobile (iOS / Android)** frontends.
- The externally-blocked **RetroAchievements account-allowlisting** pass (a
  request to the RA team, not a code change).
- **Vs. DualSystem** (two-CPU / two-PPU) games.
- **Browser / wasm Lua** (the v1.1.0 Lua engine is native-only).
- **Long-tail mapper coverage** toward the ~300-mapper full set + 100%
  TASVideos compatibility.
- The documented **hard-tier accuracy residuals** — document, don't grind.

---

## This directory

- [`ROADMAP.md`](./ROADMAP.md) — the phase/sprint roadmap entry point (status +
  links to every phase overview).
- [`archive/`](./archive/README.md) — all delivered development history (the
  engine-lineage `phase-N` plans, the shipped `v1.0.1`/`v1.1.0` plans, the
  legacy v0.8 tree, and the historical session reports). Phase overviews +
  sprint files (engine-lineage development history; phases 1-5 delivered, phases
  6-8 marked COMPLETE/SUPERSEDED in-file):
  - `archive/phase-1-foundation/` — workspace, cartridge parser, CPU core (nestest)
  - `archive/phase-2-graphics-timing/` — PPU, lockstep scheduler, simple mappers
  - `archive/phase-3-audio-polish/` — APU channels, DMC, mixer
  - `archive/phase-4-mapper-coverage/` — MMC3, misc + VRC + MMC5 mappers
  - `archive/phase-5-frontend-tooling/` — frontend, save/rewind, debugger + release
  - `archive/phase-6-v1.0.0-final/` + `archive/phase-6-v1-closeout/` — v1.0.0 closeout (SUPERSEDED)
  - `archive/phase-7-nesdev-accuracy-hardening/` — accuracy hardening (COMPLETE)
  - `archive/phase-8-v1.2.0-accuracy-residuals/` — DMC get/put scheduler (COMPLETE)
  - `archive/v1.0.1-compat-hygiene/` — the shipped v1.0.1 compat/hygiene plan
  - `archive/v1.1.0-features/` — the shipped v1.1.0 feature plan
  - `archive/legacy-v0.8-todos/` — the parent emulator's pre-synthesis milestone
    TODOs, retained verbatim as history.
- Historical session reports (dated, point-in-time; superseded by the synthesis —
  see the status note at the top of each):
  [`TEST-ROM-ACQUISITION-REPORT.md`](./archive/TEST-ROM-ACQUISITION-REPORT.md),
  [`TEST-ROM-WORKFLOW-SUMMARY.md`](./archive/TEST-ROM-WORKFLOW-SUMMARY.md),
  [`TODO_AUDIT_SUMMARY_REPORT.md`](./archive/TODO_AUDIT_SUMMARY_REPORT.md),
  [`TODO-GENERATION-STATUS.md`](./archive/TODO-GENERATION-STATUS.md).

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
