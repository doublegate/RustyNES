# RustyNES Overview

**Document Version:** 2.0.0
**Last Updated:** 2026-06-13
**Applies to:** RustyNES v1.0.0

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

As of **v1.0.0**, that vision is realized: RustyNES clears the Mesen2 / higan / ares accuracy bar, ships a polished desktop application and a browser build, and supports the full platform surface — netplay, achievements, TAS movies, a debugger, FDS, and arcade (Vs. / PlayChoice-10) hardware.

> RustyNES's emulation core descends from an extensively-documented accuracy program. Where this and related docs reference deep "v1.x"/"v2.x" engine narrative, read it as upstream engine lineage (engineering history), not as RustyNES release versions — the current release is v1.0.0.

---

## Design Philosophy

### 1. Accuracy first, speed second

The PPU is the master clock and CPU/PPU/APU/mappers run in tight lockstep at PPU-dot resolution on a master-clock-precise timebase. This makes sub-instruction edge cases (sprite-zero hit at a precise dot, mid-scanline scroll writes, mapper IRQ timing) correct by construction rather than patched per-quirk. Performance work is byte-identical by construction and gated by a commercial-ROM regression oracle.

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
| **AccuracyCoin** | **98.58% (139/141)** (RAM-direct decoder) — the two newest upstream PPU tests ("ALE + Read", "Hybrid Addresses") are known gaps |
| **`nestest`** | **0-diff** against the Nintendulator golden log |
| **blargg / kevtris / `mmc3_test_2`** | Green |
| **Commercial-ROM oracle** | 60-ROM byte-identical regression gate + extended visual survey |
| **Region** | NTSC / PAL / Dendy with exact CPU:PPU ratios (3:1, 3.2:1) |

`docs/STATUS.md` is the authoritative, always-current pass-count and mapper-coverage matrix.

---

## Emulation Approach

RustyNES uses **cycle-accurate, dot-level** emulation rather than scanline-based shortcuts. The scheduler advances one PPU dot at a time; the CPU advances on the appropriate dot for the region; the APU advances every other CPU cycle. The Bus owns all mutable device state, and the CPU borrows it during `tick()` — the architectural choice (per the TetaNES postmortem) that avoids the borrow-checker fight a split bus creates. See [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`docs/scheduler.md`](docs/scheduler.md).

---

## Target Audience

1. **Emulation enthusiasts** — reference-grade accuracy with a modern, themeable desktop UX and an in-app debugger.
2. **The TAS community** — frame-perfect deterministic `.rnm` movie record / playback / branching built directly on the determinism contract.
3. **Netplay users** — GGPO-style rollback netplay (2–4 players), native (UDP) and in the browser (WebRTC).
4. **Homebrew developers** — broad mapper coverage (51 families), FDS, an instruction/PPU/memory debugger, and an embeddable `no_std` core.
5. **Rust developers** — a clean, modular workspace and a reusable 6502 CPU crate.

---

## Feature Summary

| Area | What ships in v1.0.0 |
|------|----------------------|
| **Accuracy** | PPU-dot lockstep, master-clock timebase, AccuracyCoin 100%, `nestest` 0-diff |
| **Cartridges** | 51 mapper families incl. expansion audio (VRC6/VRC7-OPLL/Sunsoft 5B/N163/MMC5) |
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
