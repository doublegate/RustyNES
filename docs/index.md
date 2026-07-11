# RustyNES Documentation

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in
pure Rust. Its accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling
at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU
events visible to subsequent CPU code, and a lookup-table non-linear audio mixer
with band-limited synthesis. The desktop frontend is pure Rust
(`winit` + `wgpu` + `cpal` + `egui`), and the same deterministic `#![no_std]`
chip stack drives the Android, iOS, WebAssembly, and Libretro builds.

This handbook is the reader-friendly view of the specs that live in the
repository's `docs/` tree. Those documents are the **spec, not history**: they
are updated in the same change as the code they describe.

## Try it and read the code

<div class="grid cards" markdown>

- :material-play-circle: **[Play the demo](../)**

    The full winit + wgpu + egui emulator, compiled to WebAssembly and running
    in your browser at the site root.

- :material-book-open-variant: **[API documentation](../api/)**

    The generated `rustdoc` for every `rustynes-*` workspace crate — the CPU,
    PPU, APU, mappers, core, and frontend public interfaces.

- :material-github: **[Source on GitHub](https://github.com/doublegate/RustyNES)**

    Issues, releases, the full `CHANGELOG.md`, and the build instructions.

</div>

## Start here

New to the codebase or the emulator? These pages are the fastest way in:

- **[Architecture](architecture.md)** — the load-bearing decisions: the PPU as
  master clock, the Bus that owns all mutable state, the one-directional
  workspace dependency graph, and the determinism contract.
- **[Scheduler](scheduler.md)** — how the single canonical cycle counter drives
  the CPU, PPU, and APU in lockstep so mid-instruction events land at the right
  dot.
- **[Project Status](STATUS.md)** — the single source of truth for per-suite
  pass counts, the mapper matrix, and version policy.
- **[Getting Started](user-guide/getting-started.md)** — install, load a ROM,
  and play, from the end-user's side.

## What's in here

- **Emulation Core** — per-chip specs for the
  [CPU](cpu-6502.md), [PPU](ppu-2c02.md), and [APU](apu-2a03.md), plus the
  [scheduler](scheduler.md), the [mapper](mappers.md) families, and the
  [cartridge format](cartridge-format.md).
- **Frontend & Features** — the [egui shell](frontend.md), [Lua
  scripting](scripting.md), [WebRTC netplay](netplay-webrtc.md),
  [RetroAchievements](cheevos-browser.md), [compatibility](compatibility.md),
  and [performance](performance.md).
- **Testing & Accuracy** — the [testing strategy](testing-strategy.md), the
  [accuracy ledger](accuracy-ledger.md), and the [hardware emulation
  checklist](nesdev-hardware-emulation-checklist.md) that defines what
  "cycle-accurate" means here.
- **Platforms** — the native [Android](android.md) and [iOS / iPadOS](ios.md)
  apps and the [Libretro core](libretro/architecture.md).
- **User Guide** — a task-oriented [manual](user-guide/README.md) covering
  controls, menus, save states, configuration, and troubleshooting.
