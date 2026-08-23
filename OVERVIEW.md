# RustyNES Overview

**Document Version:** 2.1.0
**Last Updated:** 2026-08-23
**Applies to:** RustyNES v2.4.7

---

## Table of Contents

- [Project Vision](#project-vision)
- [Design Philosophy](#design-philosophy)
- [Accuracy](#accuracy)
- [Emulation Approach](#emulation-approach)
- [Target Audience](#target-audience)
- [Feature Summary](#feature-summary)
- [Technical Highlights](#technical-highlights)

---

## Project Vision

RustyNES is the **definitive NES emulator for the modern era** — combining cycle-perfect accuracy with a complete contemporary feature set and the safety guarantees of Rust. It is more than an emulator: it is a platform for NES preservation, competitive online play, tool-assisted speedrunning, and homebrew development.

As of **v1.0.0**, that vision was realized: RustyNES clears the Mesen2 / higan / ares accuracy bar, ships a polished desktop application and a browser build, and supports the full platform surface — netplay, achievements, TAS movies, a debugger, FDS, and arcade (Vs. / PlayChoice-10) hardware. Since then the additive v1.x line added three more platforms (native Android, iOS / iPadOS, and a Libretro / RetroArch core), **v2.0.0 "Timebase"** replaced the scheduler substrate with the one-clock / every-cycle-bus-access model (ADR 0029 — the one deliberate breaking release), and the v2.1.x → v2.3.x lines deepened accuracy, presentation, and analysis tooling. The current release is **v2.4.7 "Keystone"**. The never-tagged v2.4.0 "Concordance" shipped inside **v2.4.1 "Fabric"** — this sentence had attached that fact to whichever release was current, carried forward by three mechanical version bumps, and said it of v2.4.2, v2.4.3 and v2.4.4 in turn.

> RustyNES's emulation core descends from an extensively-documented accuracy program. Where this and related docs reference deep "v1.x"/"v2.x" engine narrative, read it as upstream engine lineage (engineering history), not as RustyNES release versions. Two distinct "v2.0"s exist and must not be conflated: the engine-lineage v2.0 master-clock work shipped as RustyNES **v1.0.0**, while RustyNES's own **v2.0.0 "Timebase"** (2026-07-03) is the later release that *replaced* that same scheduler. The current release is **v2.4.7**.

---

## Design Philosophy

### 1. Accuracy first, speed second

Every CPU cycle is a real bus access, driven from a single canonical cycle counter with a split-around-the-access PPU catch-up, on a master-clock-precise timebase -- the v2.0.0 "Timebase" model, which replaced the earlier five-counter PPU-dot lockstep outright. The accuracy it buys is the same dot-resolution accuracy; the mechanism is not. This makes sub-instruction edge cases (sprite-zero hit at a precise dot, mid-scanline scroll writes, mapper IRQ timing) correct by construction rather than patched per-quirk. Performance work is byte-identical by construction and gated by a commercial-ROM regression oracle.

### 2. Determinism as a contract

Same seed + ROM + input ⇒ bit-identical framebuffer and audio. No system time, thread scheduling, or OS RNG touches the core. This single contract is what makes save-states, rewind, frame-perfect TAS replay, and rollback netplay all correct.

### 3. Safe Rust by default

The chip stack is `#![no_std]` + `alloc` and free of `unsafe` except at FFI boundaries (RetroAchievements via the vendored rcheevos C library) and one native priority hook — each guarded by a `// SAFETY:` comment. The whole stack cross-compiles to `thumbv7em-none-eabihf` in CI.

### 4. Test ROMs are the spec

The blargg / kevtris / `mmc3_test_2` / AccuracyCoin suites are the closed-form definition of "cycle-accurate." When the docs and a passing ROM disagree, the ROM wins.

### 5. Modular and reusable

A one-directional crate graph keeps each chip (`rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers`) independently usable, fuzzable, and benchmarkable; adding a mapper touches no chip code.

---

## Accuracy

| Test | Result |
|------|--------|
| **AccuracyCoin** | **100.00% (141/141)** (RAM-direct decoder) — every assigned test passes; the two newest upstream PPU tests ("ALE + Read", "Hybrid Addresses") were closed by the v2.0.3 2-cycle-ALE promotion |
| **`nestest`** | **0-diff** against the Nintendulator golden log |
| **blargg / kevtris / `mmc3_test_2`** | Green |
| **Commercial-ROM oracle** | 60-ROM byte-identical regression gate + extended visual survey |
| **Region** | NTSC / PAL / Dendy with exact CPU:PPU ratios (3:1, 3.2:1) |

`docs/STATUS.md` is the authoritative, always-current pass-count and mapper-coverage matrix.

---

## Emulation Approach

RustyNES uses **cycle-accurate** emulation rather than scanline-based shortcuts. Since **v2.0.0 "Timebase"** the scheduler is a single canonical cycle counter in which every CPU cycle is a real bus access, with a split-around-the-access `start_cycle`/`end_cycle` PPU catch-up (ADR 0002 / ADR 0029). The APU advances every other CPU cycle.

> This paragraph described the **retired** five-counter dot-lockstep model — "the scheduler advances one PPU dot at a time" — which v2.0.0 replaced outright and which is no longer a path in the code. Corrected in v2.4.5, found by review rather than by a gate: release anchors are pinned by `release_anchor_audit`, and ordinary architecture prose is not. The Bus owns all mutable device state, and the CPU borrows it during `tick()` — the architectural choice (per the TetaNES postmortem) that avoids the borrow-checker fight a split bus creates. See [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`docs/scheduler.md`](docs/scheduler.md).

---

## Target Audience

1. **Emulation enthusiasts** — reference-grade accuracy with a modern, themeable desktop UX and an in-app debugger.
2. **The TAS community** — frame-perfect deterministic `.rnm` movie record / playback / branching built directly on the determinism contract.
3. **Netplay users** — GGPO-style rollback netplay (2–4 players), native (UDP) and in the browser (WebRTC).
4. **Homebrew developers** — broad mapper coverage (174 families), FDS, an instruction/PPU/memory debugger, and an embeddable `no_std` core.
5. **Rust developers** — a clean, modular workspace and a reusable 6502 CPU crate.

---

## Feature Summary

| Area | What ships today |
|------|----------------------|
| **Accuracy** | One-clock scheduler (v2.0.0 "Timebase"), master-clock timebase, AccuracyCoin **141/141 (100.00%)**, `nestest` 0-diff |
| **Cartridges** | **174** mapper families incl. expansion audio (VRC6/VRC7-OPLL/Sunsoft 5B/N163/MMC5) |
| **Platforms** | iNES / NES 2.0, Famicom Disk System (real-BIOS boot, read/write, multi-side), Vs. System / PlayChoice-10 RGB |
| **Online** | Rollback netplay, UDP (native) + WebRTC (browser), 2–4 players |
| **Achievements** | RetroAchievements (opt-in, native-only) — login, hardcore, toasts, badges |
| **Tooling** | TAS movies, save-states, rewind, run-ahead, Game Genie + raw-RAM cheats, egui debugger |
| **Input** | Standard pad, Four Score (4-player), Arkanoid Vaus, Zapper; keyboard + USB gamepad |
| **Frontend** | winit + wgpu + cpal + egui; display-sync pacing, dedicated emu thread, low-latency audio; desktop UX shell (menu bar, recent ROMs, tabbed settings, themes, 8:7 pixel-aspect, status bar); optional NTSC filter |
| **Web** | WebAssembly / GitHub Pages build (winit+wgpu and a lightweight canvas embed) |

---

## Technical Highlights

### Rust-specific advantages

- A `#![no_std]` + `alloc` chip stack proven against `core + alloc` only in CI — embeddable beyond the desktop.
- Memory and thread safety enforced by the compiler; `unsafe` confined to FFI and one priority hook, each documented.
- Strong typing (newtype addresses, bitflag status registers) catches whole classes of bug at compile time.

### Performance

- A dedicated native emulation thread isolates emulation cadence from UI/GPU/file-I-O stalls.
- A lock-free SPSC audio ring with dynamic rate control, late-latched input, and a display-sync pacing matrix deliver the smoothest, lowest-latency play.
- A `MapperCaps` capability cache, a pixel-emit LUT, fat LTO, and auto-vectorization keep the rendering-heavy path well under the NTSC frame deadline — all byte-identical by construction.

### Cross-platform

Native Linux / macOS / Windows plus a browser build, all from one `winit` + `wgpu` + `cpal` + `egui` frontend, with multi-platform CI (incl. wasm32) gating every change.

---

## Conclusion

RustyNES v1.0.0 delivers reference-grade NES accuracy in safe, modular Rust, wrapped in a complete modern application and online/TAS/achievement platform. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the system design, [`ROADMAP.md`](ROADMAP.md) for delivered milestones and post-1.0 directions, and [`docs/STATUS.md`](docs/STATUS.md) for the live status matrix.

## Related documentation

- [`README.md`](README.md) — user-facing introduction and quick start.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) / [`docs/architecture.md`](docs/architecture.md) — system design.
- [`CLAUDE.md`](CLAUDE.md) — guidance for working in the codebase.
- [`VERSION-PLAN.md`](VERSION-PLAN.md) — versioning strategy and history.
