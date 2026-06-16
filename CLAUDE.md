# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **New here since v0.8.x?** The emulation core was replaced with the cycle-accurate engine and the repo was re-cut as v1.0.0. Read `docs/v1.0.0-synthesis-handoff-2026-06-13.md` first — it explains what changed, the `rustynes-*` architecture, where everything moved, and the hard constraints. Then update this file + your memory as you work.

## What this is

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in pure Rust. The accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU events visible to subsequent CPU code, and a lookup-table non-linear audio mixer with band-limited synthesis. The frontend is pure Rust (`winit` + `wgpu` + `cpal` + `egui`).

**Current release: v1.4.1** (a patch on **v1.4.0 "Fidelity"**) — built on the cycle-accurate v1.0.0 production cut (v1.3.0 "Bedrock" + v1.2.0 "Curator" + v1.1.0 "Scriptable" preceded it). **v1.4.1** adds four more BestEffort mapper boot/decode fixes from the boot-smoke-vs-real-dumps pass — m92 (Jaleco JF-19 fixed-first/switchable-high PRG window, reset vector lives in the fixed half), m94 (UN1ROM bus-conflict window mapping + 3-bit bank decode, *Senjou no Ookami*), m145 (Sachen SA-72007 accept 16 KiB NROM-128 PRG), m147 (Sachen 3018 / TXC JV001 protection-handshake chip read) — plus reorganizes the boot-smoke screenshot corpus to mirror the per-mapper `tests/roms/` tier layout (+ `scripts/screenshots/categorize_screenshots.py`). BestEffort-only (outside the oracle), so AccuracyCoin holds 100% (139/139) and the shipped/native/`no_std`/wasm builds stay byte-identical to v1.4.0. The underlying **v1.4.0 "Fidelity"** release polishes accuracy (triangle ultrasonic silence; the DMC-DMA ↔ controller-read conflict verified-and-documented), adds per-channel audio mixing, finishes the devtools (symbol-file `.sym`/`.mlb`/`.nl` loading + event breakpoints), adds browser QoL (wasm `.rnm` movie I/O + IndexedDB save-states), runs a measure-first perf pass (−8% on the rendering-heavy bench), ships a colorful `rustynes help` TUI + styled `--help` (clap 4 + ratatui, native-only/wasm-gated-out), and takes mapper coverage 101 → **113 families** (boot-smoke verified, with reset-vector/decode fixes to m132/m143/m225/m226/m233/m242/m246) — all additive/off-by-default, AccuracyCoin 100% (139/139) held, determinism intact. The accuracy residuals all converge on the future v2.0 fractional-master-clock refactor (ADR 0002); casual-mode browser RetroAchievements (ADR 0015) + the v1.2.0-era F1/F3 manual-verify items remain deferred. The preceding **v1.3.0 "Bedrock"** modernized the toolchain (edition 2024 / Rust 1.96 / egui 0.34.3 + wgpu 29.0.3 + rfd 0.17.2), fixed frame pacing, adds the Memory Compare panel + a menu/Settings reorg + per-setting auto-save, takes mapper coverage to **101 families** + Vs. DualSystem header detection, adds HD-pack `<condition>`/`<background>` rules (ADR 0014), netplay desync diagnostics + niche peripheral aliases (Family Trainer / Subor keyboard / Konami+Bandai Hyper Shot), and exercises the PGO/BOLT CI gate — all additive/off-by-default, AccuracyCoin 100% (139/139) held, determinism intact. **Casual-mode browser RetroAchievements is a documented carryover (ADR 0015)** — needs an Emscripten/pure-Rust rcheevos→wasm build + live-browser verification; native RA unaffected. The preceding **v1.2.0 "Curator"** was a broad library / compatibility / reach release: mapper coverage 51 → **87 families** behind a CI accuracy-tiering honesty gate (ADR 0011), `.zip` loading + `.ips`/`.ups`/`.bps` soft-patching, a per-game DB + in-app ROM-Database editor, live NTSC knobs + a composable shader stack + CRT preset bank (ADR 0013) + a default-off HD-pack loader, Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game Genie code DB, Lua `onNmi`/`onIrq`/`setInput`, menu-bar contextual enable/disable + remappable shortcuts + Font Awesome icons, web touch controls + Power Pad + an experimental piccolo wasm-Lua backend (ADR 0012), a turn-key netplay `deploy/` bundle, and a manual/release-only PGO CI gate — all additive/off-by-default, with AccuracyCoin 100% (139/139) and the determinism contract intact. v1.1.0 added: visual filters (full NES_NTSC + CRT/scanline shader + `.pal` loading), input & peripherals (Power Pad, turbo/autofire, input-display overlay, per-game mirroring-override database), debugger devtools (breakpoints, trace logger, event viewer), audio (NSF/NSFe player + 5-band EQ), and the flagship **Lua scripting** engine. Headline facts:

- **AccuracyCoin 100.00% (139/139)**, `nestest` 0-diff against the Nintendulator golden log, blargg / kevtris suites green. The PPU-dot lockstep scheduler is the only path (no legacy integer-lockstep fallback).
- **51 mapper families**, Famicom Disk System (FDS), Vs. System / PlayChoice-10 RGB PPU, region timing (NTSC / PAL / Dendy) as data, not a build fork.
- **Rollback netplay** (GGPO-style, UDP native + WebRTC browser, 2–4 players), **RetroAchievements** (opt-in, native-only, vendored rcheevos FFI), **TAS movies** (`.rnm`), save-states, rewind, run-ahead, Game Genie + raw-RAM cheats, Four Score.
- **Frontend polish:** display-sync pacing matrix, a dedicated emulation thread, late-latched input, a lock-free audio ring with dynamic rate control, an egui debugger overlay, and a desktop UX shell (native menu bar, recent-ROMs list, tabbed Settings window, light/dark/system themes, 8:7 pixel-aspect correction, status bar).
- **WebAssembly / GitHub Pages** build (winit+wgpu and a lightweight canvas embed), live at <https://doublegate.github.io/RustyNES/>.
- Workspace: edition 2024, toolchain pinned **1.96** (bumped from 2021/1.86 in v1.3.0 "Bedrock" Workstream A), license **MIT OR Apache-2.0**, author **DoubleGate**.

**Released: v1.4.0 "Fidelity".** Cut from the merged Fidelity train — PR #91 (A/B: triangle-ultrasonic + DMC-DMA-conflict accuracy + 240p/test-ROM hardening) · #92 (H: clap-4 styled `--help` + `rustynes help` ratatui TUI, native-only) · #93 (C: per-channel audio mixing UI, core `channel_gain` g==1.0 fast-path byte-identical) · #94 (D: devtools symbol-file `.sym`/`.mlb`/`.nl` loading + event breakpoints) · #95 (F: measure-first perf pass, PPU scanline-flag cache + MMC5 hot-path, −8% rendering-heavy bench, byte-identical) · #96 (E: browser wasm `.rnm` movie I/O + IndexedDB save-states, native byte-identical) · #97 (G: mapper sweep 101→113 BestEffort + boot-smoke decode/reset-vector fixes to m132/m143/m225/m226/m233/m242/m246) — all additive + off-by-default so shipped/native/`no_std`/wasm stay byte-identical and AccuracyCoin holds **100% (139/139)**. The accuracy residuals (the 3 hard-tier C1 cases) + casual-mode browser RetroAchievements (ADR 0015) + the v1.2.0-era F1 on-device-touch / F3 live-netplay manual-verify items remain deferred; the accuracy residuals all converge on the future v2.0 fractional-master-clock refactor (ADR 0002). The preceding **v1.3.0 "Bedrock"** was cut from the merged Bedrock train — PRs #79/#80 (toolchain: edition 2024 / MSRV 1.96 / egui 0.34.3 + wgpu 29.0.3 + rfd 0.17.2) · #82 (frame-pacing B1/B2) · #84 (devtools: Memory Compare + menu/Settings reorg + per-setting auto-save) · #85 (mapper sweep 87→101, BestEffort) · #86 (Vs. DualSystem header detection) · #87 (m218 16K-PRG fix + BestEffort boot-smoke screenshots) · #88 (HD-pack `<condition>`/`<background>`, ADR 0014) · #89 (netplay desync diagnostics + niche peripheral aliases) — all additive + off-by-default so shipped/native/`no_std`/wasm stay byte-identical and AccuracyCoin holds **100% (139/139)**. The PGO/BOLT gate (`pgo.yml`) was exercised (>3%-Criterion + byte-identical promotion). The C1 hard-tier residuals were re-baselined: `cpu_interrupts_v2` closed (5/5 strict); 3 (`mmc3_test_2/4` #3 + 2 `apu_reset`) stay deferred to a future v2.0 fractional-master-clock refactor (ADR 0002). **Carryover (maintainer-manual, can't be CI-self-certified):** casual-mode browser RetroAchievements (ADR 0015 — needs an Emscripten/pure-Rust rcheevos→wasm build + live-browser verify; native RA unaffected); plus the v1.2.0-era **F1 on-device touch UX** + **F3 live-netplay host/TURN + `deploy/README.md` matrix** (then flip `docs/netplay-webrtc.md` §4 to Verified). The preceding **v1.2.0 "Curator"** (tagged + published) contents: **mapper tiering** (Core/Curated/BestEffort, ADR 0011, 87 families + a CI honesty gate `crates/rustynes-test-harness/tests/mapper_tier_honesty.rs`) + the **m89 bus-conflict fix**; the **SMB3 World 1-1 flicker fix** (it was the PPU OAM-row-corruption model keyed off the raw dot — replaced with a faithful TriCNES eval-pointer port; see `docs/ppu-2c02.md`); **ZIP + IPS/UPS/BPS soft-patching** + a **per-game DB with an in-app ROM-Database editor**; **NTSC per-knob live tuning**, a **composable `ShaderStack` + CRT preset bank** (ADR 0013), and an **HD-pack loader** behind the default-off **`hd-pack`** feature (output-only PPU tile-source export, proven byte-identical on/off); **Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game Genie code DB**; **Lua `onNmi`/`onIrq`/`setInput`** (the last gated like `emu.write`); **menu-bar UX** (contextual enable/disable + remappable `[input.system]` shortcut accelerators + full **FontAwesome** icons via a vendored OFL-1.1 font); **web touch controls + Power Pad wasm feed**; an **experimental wasm Lua piccolo backend** behind the default-off **`script-wasm`** feature (ADR 0012, explicitly NOT byte-parity with native mlua); a **turn-key netplay `deploy/` bundle**; and a **manual/release-only PGO CI job** (`pgo.yml`, >3%-Criterion + byte-identical promotion gate). **Outstanding for the maintainer (carried into v1.3.0):** the two manual-verification items that can't be CI-self-certified — **F1 on-device touch UX** and **F3 live-netplay host/TURN + the `deploy/README.md` connectivity matrix** (then flip `docs/netplay-webrtc.md` §4 to Verified).

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
cargo clippy -p rustynes-frontend --all-targets --features scripting -- -D warnings  # v1.1.0 Lua engine
cargo clippy -p rustynes-frontend --all-targets --features scripting,hd-pack -- -D warnings  # v1.2.0 HD-pack
cargo clippy -p rustynes-frontend --all-targets --features retroachievements -- -D warnings  # RA FFI — DON'T skip this one
# After any `cargo clippy --fix`, re-run clippy for EVERY feature combo (incl. retroachievements):
# --fix compiles only the active feature set, so it can strip cfg-gated code that another feature
# needs (bit PR #80: an `elidable_lifetime_names` autofix removed a `<'a>` the `retroachievements`
# `ra` param uses, breaking that build). Wasm clippy commands are in the WebAssembly section below.
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
# NEVER `--all-features`: v1.2.0's `script-wasm` (piccolo/wasm) and `scripting` (mlua/native) are
# mutually-exclusive rustynes-script backends, so `--all-features` can't resolve. CI (and the
# pre-commit hook) use EXPLICIT features. Also: a rustdoc intra-doc link to a feature-only dep
# (e.g. [`piccolo`] / [`mlua`]) FAILS the default `cargo doc --workspace --no-deps` — use plain
# `code` spans for mutually-exclusive-feature crate names (bit PR #76).

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

Toolchain is **Rust 1.96** pinned in `rust-toolchain.toml` (bumped from 1.86 in v1.3.0 to unblock the edition-2024 + egui 0.34 / wgpu 29 / rfd 0.17 dependency tier). CI runs the test job on stable across Linux/macOS/Windows plus an MSRV pin at 1.96 on Linux.

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
- When relabeling old engine "v2.x" narrative for users, present it as upstream lineage/history — **never as a current RustyNES release version.** The current *tagged* release is **v1.4.1** (a BestEffort-mapper-fix patch on **v1.4.0 "Fidelity"**; built on the v1.0.0 production core; v1.3.0 "Bedrock" + v1.2.0 "Curator" + v1.1.0 "Scriptable" preceded it; see the top summary).
- **v1.1.0 "Scriptable" is shipped + live** (tag `v1.1.0` on commit `75e42cd`, GitHub release published 2026-06-15; the `release.yml` matrix builds + attaches the Linux/macOS-aarch64/Windows binaries and `web.yml` redeploys Pages on the tag push). A single v1.1.0 tag superseded the never-cut v1.0.1 / beta train. It is purely additive on the v1.0.0 core — AccuracyCoin 100% held; the `scripting` + `debug-hooks` features are off by default so the shipped, wasm, and `no_std` builds are byte-identical to v1.0.0. Shipped: video filters (NES_NTSC/CRT/`.pal`), Power Pad + turbo/autofire + input-display + per-game mirroring DB, debugger devtools (breakpoints/trace/event behind `debug-hooks`), NSF/NSFe + 5-band EQ, and the flagship Lua 5.4 engine (`rustynes-script`, `docs/scripting.md`). NOT shipped at v1.1.0 (follow-ups — **all since landed in v1.2.0 on `main`**): Family BASIC keyboard, Game Genie code DB, Lua `onNmi`/`onIrq`/`setInput`, wasm Lua. **v1.0.0 remains tagged + live**; its full release + post-release record is in `docs/v1.0.0-synthesis-handoff-2026-06-13.md` — read it before touching CI, Pages, or release tooling.
- **v1.2.0 "Curator" beta content is on `main` but NOT yet tagged** (full per-workstream summary in the "In development" paragraph near the top). Do NOT describe v1.2.0 as released until the maintainer cuts it (version bump 1.1.0→1.2.0, `CHANGELOG [Unreleased]`→`[1.2.0]`, the `v1.2.0` tag → `release.yml` + Pages). Two manual-verification items remain the maintainer's (can't be CI-self-certified): **F1 on-device touch UX** and **F3 live-netplay host/TURN + the `deploy/README.md` connectivity matrix** (then flip `docs/netplay-webrtc.md` §4 to Verified). New default-off feature flags: **`hd-pack`** (frontend; forwards to core's PPU tile-source export) and **`script-wasm`** (frontend wasm-only piccolo Lua, mutually exclusive with `scripting` — see the build-gates note + `docs/adr/0012`). ADRs added: **0011** (mapper tiering), **0012** (wasm-Lua piccolo), **0013** (composable shader stack).
- **Markdownlint is a CI gate** (pre-commit, pinned `markdownlint-cli v0.39.0`). The local `markdownlint` binary is a newer version that reports rules v0.39.0 lacks (e.g. MD060) — those are NOT gated; verify with `pre-commit run markdownlint --all-files`, not the bare binary. `.markdownlint.json` keeps `MD013`/`MD033`/`MD041` disabled by design (long technical tables, the README HTML banner/`<img>`, the HTML-led README). `.markdownlintignore` exempts `ref-docs/`, `ref-proj/`, the vendored `tricnes/` + upstream READMEs, and the frozen `docs/archive/` + `to-dos/archive/` trees — don't lint or reformat those.
- **RetroAchievements client identity:** the RA HTTP User-Agent (how RA authenticates/identifies/allowlists the client) is `RustyNES/<crate version> rcheevos/<rcheevos version>` — the `RA_USER_AGENT` const in `crates/rustynes-cheevos/src/http.rs`; the rcheevos version auto-syncs from the vendored `rc_version.h` via `build.rs` (`RCHEEVOS_VERSION`). Keep the leading `RustyNES/` token (a regression test guards it).
