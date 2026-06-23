# RustyNES Development TODO Tracker

**RustyNES version:** v1.8.8 "Atlas" (latest in the Android platform train on the v1.0.0 production core)
**Project Status:** Released — v1.8.8 "Atlas" shipped; v1.8.9 in development.

---

## Overview

This directory holds the phase-and-sprint development history that produced
RustyNES v1.0.0 (the cycle-accurate production core) and the long line of
feature/platform releases built on top of it. The phases below are
**delivered** — RustyNES ships at v1.8.8 with a cycle-accurate core
(AccuracyCoin 100%), **168 mapper families**, FDS, Vs./PC10, rollback netplay,
RetroAchievements, TAS movie tooling, the performance + desktop-UX shell, the
full v1.1.0 → v1.7.x feature set (Lua scripting, visual filters, the studio /
TAS-tooling / debugger-depth suite, the writable/programmable "Forge" tools,
audio depth, web/wasm parity, an i18n framework), and the v1.8.x **Android app**.

**Release line (this project's own versions):** `v0.1.0…v0.8.6` (the parent
emulator) → `v0.9.0…v0.9.7` (engine-lineage integration stages) → **`v1.0.0`**
(this synthesis: the cycle-accurate engine + the ported desktop-UX shell +
production polish) → **`v1.1.0` "Scriptable" → `v1.2.0` "Curator" → `v1.3.0`
"Bedrock" → `v1.4.0` "Fidelity"** (+ `v1.4.1`) **→ `v1.5.0` "Lens" → `v1.6.0`
"Studio" → `v1.7.0` "Forge"** (+ `v1.7.1`) **→ `v1.8.0` … `v1.8.8` "Atlas"** (the
Android platform train). The current shipped tag is **v1.8.8 "Atlas"**, with
**v1.8.9** in development. The forward path then targets **`v2.0.0` "Timebase"**
(the master-clock rewrite, ADR 0002) and the **v2.0.1 → v2.1.0** mobile-finalization
train (the joint Android + iOS + F-Droid launch at v2.1.0). Version markers in the
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

## Forward roadmap (post-v1.8.8)

**v1.8.8 "Atlas" is shipped** — the latest in the Android platform train on the
v1.0.0 production core. The release-named folders that staged the early feature
releases (`v1.0.1-compat-hygiene/`, `v1.1.0-features/`, the engine-lineage
`phase-N` plans) are archived under [`archive/`](./archive/README.md); the
per-version plans for v1.5.0 → the mobile train live under
[`plans/`](./plans/). The shipped line, in brief: the **Lua scripting
engine** + visual filters/peripherals/devtools/NSF (v1.1.0); the
library/compatibility/reach
pass (v1.2.0); toolchain modernization + Memory-Compare (v1.3.0); accuracy +
finish (v1.4.0/v1.4.1); the insight/scriptability/creator-tooling/polish pass
(v1.5.0); the studio / TAS-tooling / debugger-depth pass + mapper breadth → 150
families (v1.6.0); the writable/programmable "Forge" tools + audio depth +
web/wasm parity + i18n + mapper breadth → **168 families** (v1.7.0/v1.7.1); and
the **Android app** (v1.8.0 … v1.8.8 "Atlas").

**In development — v1.8.9:** the 13-PR Dependabot consolidation (#180), the
dormant `rustynes-monetization` crate build-out (activates the v2.1.0 freemium
model), and a held UX fix.

Genuine remaining post-v1.8.8 items (see `ROADMAP.md` for detail):

- **v2.0.0 "Timebase"** — the master-clock rewrite (ADR 0002): the one breaking
  (save-state / byte-identity) release that closes the hard-tier accuracy
  residuals + adds full Vs. DualSystem emulation. The next architectural milestone.

- **Mobile (iOS / Android)** frontends — a concrete release train: Android
  (v1.8.x GitHub-sideload) + iOS (v1.9.0 TestFlight) interim, then the app-store
  launches deferred to **after v2.0.0** and shipped jointly at **v2.1.0** —
  **Google Play + Apple App Store + F-Droid** (Android final v2.0.1–v2.0.4, iOS
  final v2.0.5–v2.0.8, both-apps readiness v2.0.9). v2.1.0 also lands the
  ad-supported freemium monetization (AppLovin MAX + RevenueCat, a $3.99
  remove-ads unlock) and the `foss`/`play` Android flavor split (ADR 0025). See
  [`plans/v2.0.x-mobile-finalization-plan.md`](plans/v2.0.x-mobile-finalization-plan.md).
- The externally-blocked **RetroAchievements account-allowlisting** pass (a
  request to the RA team, not a code change).
- **Browser / wasm Lua** maturity (the native mlua engine is feature-complete;
  the wasm piccolo backend, ADR 0012, is not byte-parity with native mlua).
- **Long-tail mapper coverage** toward the ~300-mapper full set + 100%
  TASVideos compatibility.
- The documented **hard-tier accuracy residuals** — document, don't grind.

---

## This directory

- [`ROADMAP.md`](./ROADMAP.md) — the phase/sprint roadmap entry point (status +
  links to every phase overview).
- [`DEFERRED-AND-CARRYOVER-FEATURES.md`](./DEFERRED-AND-CARRYOVER-FEATURES.md) —
  the consolidated catalogue of every deferred, carried-over, manual-verify, and
  not-yet-implemented feature (reconciled against `main`), grouped by theme with
  target releases and source plans/ADRs.
- [`plans/`](./plans/README.md) — the per-version plan docs (v1.0.0 synthesis
  through v1.7.0 "Forge", plus the staged-forward v1.8.0 Android / v1.9.0 iOS /
  v2.0.0 master-clock / v2.0.x mobile-finalization plans), the
  `plans/engine-lineage/` history archive, and the `plans/research/`
  reference-mining archive.
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
