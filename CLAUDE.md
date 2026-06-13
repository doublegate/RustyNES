# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **New here since v0.8.x?** The emulation core was replaced with the cycle-accurate engine and the repo was re-cut as v1.0.0. Read `docs/v1.0.0-synthesis-handoff-2026-06-13.md` first — it explains what changed, the `rustynes-*` architecture, where everything moved, and the hard constraints. Then update this file + your memory as you work.

## What this is

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in pure Rust. The accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU events visible to subsequent CPU code, and a lookup-table non-linear audio mixer with band-limited synthesis. The frontend is pure Rust (`winit` + `wgpu` + `cpal` + `egui`).

**Current release: v1.0.0** — the first stable, production cut. It integrates the cycle-accurate emulation engine (the `rustynes-*` crates) with a complete desktop UX shell and a full documentation synthesis. Headline facts:

- **AccuracyCoin 100.00% (139/139)**, `nestest` 0-diff against the Nintendulator golden log, blargg / kevtris suites green. The PPU-dot lockstep scheduler is the only path (no legacy integer-lockstep fallback).
- **51 mapper families**, Famicom Disk System (FDS), Vs. System / PlayChoice-10 RGB PPU, region timing (NTSC / PAL / Dendy) as data, not a build fork.
- **Rollback netplay** (GGPO-style, UDP native + WebRTC browser, 2–4 players), **RetroAchievements** (opt-in, native-only, vendored rcheevos FFI), **TAS movies** (`.rnm`), save-states, rewind, run-ahead, Game Genie + raw-RAM cheats, Four Score.
- **Frontend polish:** display-sync pacing matrix, a dedicated emulation thread, late-latched input, a lock-free audio ring with dynamic rate control, an egui debugger overlay, and a desktop UX shell (native menu bar, recent-ROMs list, tabbed Settings window, light/dark/system themes, 8:7 pixel-aspect correction, status bar).
- **WebAssembly / GitHub Pages** build (winit+wgpu and a lightweight canvas embed), live at <https://doublegate.github.io/RustyNES/>.
- Workspace: edition 2021, toolchain pinned **1.86**, license **MIT OR Apache-2.0**, author **DoubleGate**.

**Engine lineage (history, not release numbers).** The emulation core descends from an extensively-documented accuracy program (the "RustyNES" engine line) whose internal milestones — the cycle-accurate core, the master-clock scheduler reaching 100%, FDS, netplay, the RetroAchievements + arcade-platform pass, and the performance pass — are folded into RustyNES documentary stages v0.9.0–v0.9.7 and culminate in this v1.0.0 production cut. Where deep technical narrative references old "v1.x"/"v2.x" anchors (e.g. the `v2.0` master-clock refactor, ADRs, audit logs under `docs/`), read those as **upstream engine lineage** — historical engineering context, not RustyNES release versions. `docs/STATUS.md` is the authoritative per-suite pass-count + mapper-coverage matrix.

## Build / test / lint

```bash
# Build
cargo check --workspace
cargo build --workspace
cargo build --release --workspace

# Tests
cargo test --workspace                              # unit + integration
cargo test --workspace --features test-roms         # + AccuracyCoin / blargg / kevtris ROM suites
cargo test --workspace --features test-roms,commercial-roms  # + 60-ROM commercial oracle (needs local dumps)
cargo test -p rustynes-cpu                          # single crate
cargo test -p rustynes-cpu nestest                  # single test by name substring
cargo test --workspace -- --test-threads=1          # serial (for flake debugging)

# Run only the #[ignore]'d expected-fail probes
cargo test --workspace --features test-roms --no-fail-fast -- --ignored

# Quality gates (all run in CI; all must be green)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# no_std cross-compile (the chip stack must compile against core + alloc only)
cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features

# Frontend (winit + wgpu + cpal + egui). Binary name is `rustynes`.
cargo run --release -p rustynes-frontend -- path/to/rom.nes
cargo run --release -p rustynes-frontend                 # opens with no ROM; menu / F12 to load
# Default keys P1: arrows = D-pad, Z = A, X = B, Enter = Start, RShift = Select.
# Default keys P2: WASD = D-pad, Q = A, E = B, P = Start, L = Select.
# System: Esc = quit, F1 = save state, F4 = load state, F5 (held) = rewind,
# F2 = reset, F3 = power-cycle, F12 = open ROM, F9 = FDS disk-swap, ~ = toggle debugger.
# F6/F7/F8 = TAS movie record/play/branch. Drag-and-drop a .nes/.fds to load.
# USB gamepads auto-bind to P1 (Xbox-style: South=A, West=B, Start, Back/Select, DPad).

# WebAssembly frontend — needs `trunk` + the wasm32 target (auto-installed from
# rust-toolchain.toml). Run from crates/rustynes-frontend/web:
#   trunk serve                                          # dev server
#   trunk build --release                                # wasm-winit (default)
#   trunk build --release --no-default-features --features wasm-canvas  # lightweight embed
# wasm clippy gates (CI): cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown
#   --lib --bins -- -D warnings   (and again with --no-default-features --features wasm-canvas)
# GOTCHA: web/Trunk.toml pins the wasm-bindgen CLI version, which MUST exactly match the
#   wasm-bindgen LIBRARY in Cargo.lock (grep -A1 'name = "wasm-bindgen"' Cargo.lock). A
#   mismatch fails `trunk build` (and the Pages deploy) at the wasm-bindgen step, but wasm
#   clippy still passes — so bump the pin whenever a resolve moves the library version.
# CI deploy: .github/workflows/web.yml ("Deploy Pages (demo + docs)") publishes BOTH the
#   playable demo (root) and the workspace rustdoc (/api/) to GitHub Pages from the
#   "GitHub Actions" source: https://doublegate.github.io/RustyNES/ + /api/.

# Benchmarks (criterion)
cargo bench -p rustynes-cpu
cargo bench -p rustynes-ppu
cargo bench -p rustynes-mappers
cargo bench -p rustynes-core
```

Toolchain is **Rust 1.86** pinned in `rust-toolchain.toml` (required by `edition2024` transitive deps in the frontend stack). CI runs the test job on stable across Linux/macOS/Windows plus an MSRV pin at 1.86 on Linux.

On Linux, anything that pulls in `rustynes-frontend` (which `cargo test --workspace` does) needs the wgpu/winit/cpal system deps:

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev libasound2-dev libudev-dev
# CachyOS / Arch:
sudo pacman -S --needed libxkbcommon wayland alsa-lib systemd-libs
```

## Architecture — load-bearing facts

These cross-cutting decisions span multiple files. Reading individual chip docs without them in mind will mislead.

**The PPU is the master clock.** The scheduler advances one PPU dot per `tick_one_dot()`; the CPU advances on every third dot (NTSC / Dendy; 3.2nd dot PAL); the APU advances every other CPU cycle. This is **lockstep**, not catch-up. It is the central architectural choice and the reason mid-instruction PPU events (sprite-zero hit at a precise dot, MMC3 IRQ at PPU dot 260, mid-scanline scroll writes) work without per-quirk patches. See `docs/scheduler.md`.

**The Bus owns everything mutable.** `rustynes-core::Bus` holds the PPU, APU, mapper-via-cart, WRAM, controllers, and open-bus latch. The CPU borrows `&mut Bus` during `tick()`. Per the TetaNES postmortem, this single choice avoids the borrow-checker fight that the alternative ("CPU holds PPU, but PPU also needs CPU bus") creates. The PPU and APU each see a smaller trait (`PpuBus`, `ApuBus`) for what they actually need — mapper-mediated CHR/nametable reads and DMC sample fetches respectively.

**Workspace dependency graph is one-directional.** `rustynes-cpu` has no PPU or APU dep. `rustynes-ppu` depends on `rustynes-mappers` only (CHR/nametable bus). `rustynes-apu` is independent. `rustynes-core` ties them together. Result: each chip is fuzzable and benchmarkable in isolation. Adding a cross-chip dependency breaks this invariant — don't.

**Mapper IRQ logic lives in the mapper, not the PPU.** The PPU calls `Mapper::notify_a12(level)` on every A12 transition; the mapper internally filters (MMC3's "3 falling edges of M2" lives in the MMC3 impl). MMC5 uses different scanline detection via `notify_scanline_start` / `notify_vblank`. VRC2/4/6, Sunsoft FME-7, and Namco 163 tick on `notify_cpu_cycle()`. All such hooks are default-no-op on `Mapper`. See `docs/mappers.md` for the per-mapper IRQ family table.

**Determinism is a hard contract.** Same seed + ROM + input sequence ⇒ bit-identical framebuffer and audio. CPU/PPU initial phase alignment is randomized at power-on from a seeded PRNG; reset preserves alignment. This is required for save-state round-trip, regression tests, TAS replay, and netplay rollback. Don't introduce hidden non-determinism (system time, thread scheduling, OS RNG) into the core. Netplay's dynamic rate control and run-ahead live in the **frontend** (a resampler stage / snapshot-restore orchestration), never in the core's synthesis — that is what keeps the contract intact.

**The frontend is an always-on egui shell, not a bare window.** `rustynes-frontend` is winit + wgpu + cpal + egui, and egui runs **every frame**: `DebuggerOverlay::render_shell` draws a persistent menu bar (File / Emulation / Tools / View / Debug / Help) + status bar + tabbed Settings window, with the toggleable (`` ` ``) CPU/PPU/APU/memory debugger panels layered on top. The shell never holds the emu lock inside the egui closure — menu interactions return a `MenuAction` that `App::dispatch_menu_action` runs *after* the egui pass, and the hidden render branch copies the framebuffer under a brief lock, drops it, and renders/presents with `nes = None` (the locked branch is taken only when the overlay is visible or a `nes`-reading tool panel like Cheats is open). On native the emulator runs on a dedicated thread (`emu-thread`, default-on) communicating via the `Arc<Mutex<EmuCore>>` handle + lock-free `SharedInput`; the winit thread only does UI + present. Full spec in `docs/frontend.md` (this is just the primer).

**Test ROMs are the spec.** When the docs and a passing test ROM disagree, the ROM wins — the docs get updated. The blargg / kevtris / mmc3_test_2 / AccuracyCoin suites in `tests/roms/` are the closed-form definition of "cycle-accurate." See `docs/testing-strategy.md` for the testing layers.

## Where things live

- `crates/rustynes-{cpu,ppu,apu,mappers,core,netplay,cheevos,frontend,test-harness}/` — crate name = dir name. The binary is `rustynes` (in `rustynes-frontend`).
- `docs/` — implementation specs. These are the **spec**, not history: update them in the same PR as the code change. Per-subsystem files (`cpu-6502.md`, `ppu-2c02.md`, `apu-2a03.md`, `mappers.md`, `cartridge-format.md`, `scheduler.md`) + cross-cutting (`architecture.md`, `testing-strategy.md`, `performance.md`, `frontend.md`, `compatibility.md`). `docs/STATUS.md` is the **single source of truth** for per-suite pass counts, the mapper matrix, and version policy. `docs/adr/` holds Michael-Nygard-format ADRs.
- `ref-docs/` — immutable hardware + emulation reference (60+ source research report). Updates go in dated supplemental files.
- `to-dos/ROADMAP.md` → phase/sprint files — tickets with stable IDs `T-PS-NNN`. Reference in commits.
- `tests/roms/` — CC0 / public-domain test ROMs (committed). `tests/roms/external/` — your own commercial dumps (gitignored).
- `tests/golden/` / `screenshots/` — reference framebuffers, audio, and the visual baseline corpus (committed). `tests/captures/` — current-run output (gitignored).

**Never commit commercial Nintendo ROMs.**

## Workflow conventions

- Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`, `build:`, `ci:`); imperative subject ≤ 72 chars.
- Branch names: `<type>/<short-desc>`.
- A chip-behavior change touches both the chip code and the chip's `docs/<subsystem>.md`. They drift apart easily; don't let them.
- For accuracy work: pin the failing test ROM expectation first, then implement until it passes.
- User-visible changes go in `CHANGELOG.md` under `[Unreleased]` in the same PR — `CONTRIBUTING.md`'s quality gate enforces it.
- Hot paths (`Cpu::tick`, `Ppu::tick`, mapper register access): no allocations, prefer fixed arrays, profile (`cargo bench` + `perf record`) before adding abstractions. Target ≤ 2 ms/frame headless.
- `unsafe` requires a `// SAFETY:` comment explaining the invariant. The chip stack is `#![no_std]` + `extern crate alloc;`; only `rustynes-frontend` and `rustynes-cheevos` (FFI) carry `unsafe`.
- No emojis in code, comments, or commits (project policy).

## Operating notes for Claude Code

- `docs/STATUS.md` and the "Current release" summary above are the current-state source of truth; `CHANGELOG.md` and the `docs/audit/` logs carry the deep engine-lineage history.
- `ref-docs/` is immutable. Research updates go in dated supplemental files.
- ADRs go in `docs/adr/` (Michael Nygard format).
- `rustynes-core` re-exports the public types from the chip crates; downstream consumers (`rustynes-frontend`, `rustynes-test-harness`) should depend on `rustynes-core` rather than the chip crates directly.
- When relabeling old engine "v2.x" narrative for users, present it as upstream lineage/history — **never as a current RustyNES release version.** The current release is **v1.0.0**.
- **v1.0.0 is shipped + live** (tag `v1.0.0`, GitHub release + Linux/macOS-aarch64/Windows binaries, the Pages demo + `/api/` docs). The full release + post-release record (the Dependabot `-s ours` integration, the "RustyNES v2" leftover scrub, the combined-Pages + wasm-bindgen fixes, the Actions cleanup) is in `docs/v1.0.0-synthesis-handoff-2026-06-13.md` — read it before touching CI, Pages, or release tooling.
- **RetroAchievements client identity:** the RA HTTP User-Agent (how RA authenticates/identifies/allowlists the client) is `RustyNES/<crate version> rcheevos/<rcheevos version>` — the `RA_USER_AGENT` const in `crates/rustynes-cheevos/src/http.rs`; the rcheevos version auto-syncs from the vendored `rc_version.h` via `build.rs` (`RCHEEVOS_VERSION`). Keep the leading `RustyNES/` token (a regression test guards it).
