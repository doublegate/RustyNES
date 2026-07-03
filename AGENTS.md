<!-- Managed by Master-Claude. Universal rules come from the imported/inlined core.
     Edit only inside the MC-PROJECT block; mc-sync overwrites everything else. -->
<!-- mc-core: 0.1.0 | mode=import | lang=rust -->
# AGENTS.md — RustyNES

@/home/parobek/.claude/master-core/AGENTS.base.md
@/home/parobek/.claude/master-core/lang/rust.md
@/home/parobek/.claude/master-core/modules/10-commits-and-versioning.md
@/home/parobek/.claude/master-core/modules/20-testing-and-accuracy.md
@/home/parobek/.claude/master-core/modules/30-quality-gates.md
@/home/parobek/.claude/master-core/modules/40-docs-and-adrs.md
@/home/parobek/.claude/master-core/modules/50-architecture-patterns.md
@/home/parobek/.claude/master-core/modules/60-security.md
@/home/parobek/.claude/master-core/modules/70-release-ceremony.md
@/home/parobek/.claude/master-core/modules/80-phase-sprint-workflow.md
@/home/parobek/.claude/master-core/modules/90-multi-language-integration.md
@/home/parobek/.claude/master-core/modules/95-named-pattern-library.md

<<< MC-PROJECT-START >>>

## Project: RustyNES

> **New here since v0.8.x?** The emulation core was replaced with the cycle-accurate engine and the repo was re-cut as v1.0.0. Read `docs/v1.0.0-synthesis-handoff-2026-06-13.md` first — it explains what changed, the `rustynes-*` architecture, where everything moved, and the hard constraints. Then update this file + your memory as you work.

## What this is

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in pure Rust. The accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU events visible to subsequent CPU code, and a lookup-table non-linear audio mixer with band-limited synthesis. The frontend is pure Rust (`winit` + `wgpu` + `cpal` + `egui`).

**Current release: v2.0.0 "Timebase"** (2026-07-03, the one-clock / every-cycle-bus-access scheduler rewrite + Vs. `DualSystem` dual-console support; preceded by v1.10.0 "Arcade", 2026-07-01). This is RustyNES's designated MAJOR-boundary release — see "Timebase (v2.0.0)" below. RustyNES is now a multi-platform emulation suite, all on the one byte-identical cycle-accurate core — **`docs/STATUS.md` is the authoritative current-state record.** What ships beyond the desktop app:

- **Timebase (v2.0.0)** — the scheduler substrate is rewritten from a five-counter dot-lockstep model to a single canonical cycle counter, every CPU cycle a real bus access, and a split-around-the-access `start_cycle`/`end_cycle` PPU catch-up (ADR 0002 / ADR 0029), now the *only* scheduler path. This is a MAJOR-boundary breaking change (ADR 0003): `.rns` save-state and `.rnm` movie format epochs bump (ADR 0028) — a pre-v2.0.0 `.rns` slot now fails to load with a clear error instead of silently misinterpreting stale bytes. Landed across five betas + rc.1 (PRs #217–223). Also new: core-level **Vs. `DualSystem`** dual-console support (`Emu::Dual`, `crates/rustynes-core`) for the four Vs. arcade cabinet boards — core-and-test-harness-only, frontend wiring deferred. The R1/R2 MMC3 IRQ-timing residual is by-design-deferred beyond this release with a mechanism-level finding recorded in ADR 0002 (not closed, not silently dropped). **AccuracyCoin holds 100% (139/139)** throughout.

- **Native Android app** — the **v1.8.0 → v1.8.9 "Android"** train (`crates/rustynes-mobile` UniFFI bridge + `crates/rustynes-android` JNI/NDK host + a Jetpack Compose app, ADR 0024): full on-device emulation, multi-touch + P1–P4 hardware controllers, wgpu `SurfaceView` rendering + the shared WGSL shader stack, save-states / battery SRAM, Lua, RetroAchievements, direct-IP + CGNAT/TURN room-code netplay, a box-art ROM library, and platform polish (adaptive / foldable / TV, Material You, capture / PiP / home-screen widget). Distributed as **GitHub-Releases sideload** now; Google Play deferred to v2.1.0 (ADR 0025 `foss`/`play` split).
- **Native iOS / iPadOS app** — the **v1.9.0 → v1.9.9 "iOS" TestFlight train** (`crates/rustynes-ios` Metal + CoreAudio shim reusing `rustynes-mobile` verbatim → UniFFI-generated Swift, ADR 0026): a native SwiftUI shell over wgpu→Metal, multi-touch + GameController, the shader stack, TAS / HD-pack / palettes / per-game DB, Lua + RetroAchievements, LAN + room-code netplay, CloudKit save-state sync, accessibility + EN/ES i18n + ReplayKit + Game Center, and the v1.9.9 creator tools (Cheats, a FOSS-gated read-only debugger, a touch TAStudio piano-roll, foreign movie import, a host audio-depth DSP). Ships to **interim TestFlight** now; App Store + AltStore PAL deferred to v2.1.0 (ADR 0027). Mobile ROM loading is iNES / NES 2.0 only (FDS / NSF a post-v2.0.0 carryover). Readiness record: `docs/ios-v1.9.9-readiness.md`.
- **Native Libretro core** — `crates/rustynes-libretro` (builds the `rustynes_libretro` shared library — `.so` / `.dylib` / `.dll` by platform, per the crate `Makefile`) integrates RustyNES into RetroArch (RetroAchievements, dynamic audio sync, deterministic save-state / rollback). Docs in `docs/libretro/`, plan in `to-dos/libretro/`, reference in `ref-docs/RustyNES-Libretro_Core.md`; the crate `Makefile` cross-compiles natively and `docs/libretro/UPSTREAM_SYNC.md` covers the re-fork / upstream-info-file (libretro-super + libretro-docs) workflow.
- **Mapper breadth → 172 families** (up from 168 at the v1.7.x tag), Core / Curated / BestEffort behind the CI accuracy-honesty gate.
- **Release automation** — `.github/workflows/release-auto.yml`: when a new version goes final-green on `main`, it auto-tags + publishes the GitHub Release (body from a maintainer-authored `.github/release-notes/vX.Y.Z.md` override, else the CHANGELOG `[X.Y.Z]` section; title codename parsed from the CHANGELOG header) and builds + attaches the desktop binaries by invoking `release.yml` via `workflow_call` (a tag pushed by `GITHUB_TOKEN` can't trigger `on: push: tags`, hence the direct call). The v1.8.0–v1.9.9 GitHub Releases are all published with comprehensive notes + Linux / macOS-aarch64 / Windows binaries.

Platform additions through v1.10.0 were **host-only and additive**: the deterministic `#![no_std]` chip stack was untouched and byte-identical on ARM. **v2.0.0 "Timebase" is different by design** — it rewrites the scheduler substrate itself (still `#![no_std]`-clean, still AccuracyCoin 100% (139/139), but the save-state / movie format epochs deliberately bump per ADR 0028, so cross-version `.rns`/`.rnm` round-trip is a v1.x-only guarantee, not a v1.x⇄v2.x one). Forward path: the **v2.0.1 → v2.0.9** mobile-finalization re-port train onto the v2.0.0 core → **v2.1.0** the joint Google Play + Apple App Store + AltStore PAL + F-Droid launch.

---

**Release history → `CHANGELOG.md`.** The full per-release detail — features, the mapper-count growth (51 → **172 families**), ADRs, and PR trains for **v1.0.0 → v2.0.0** (plus the documentary engine-lineage stages v0.9.0–v0.9.7) — lives in `CHANGELOG.md` (the single source of truth for user-visible change), the per-release GitHub Releases, and `to-dos/plans/`. Every release through v1.10.0 was **additive / off-by-default**, so with new features off those builds stayed byte-identical; **v2.0.0 is RustyNES's one designated breaking release** (ADR 0003) — the one-clock, every-cycle-bus-access scheduler (ADR 0002 / ADR 0029) is now the *only* path, and the old PPU-dot lockstep model is retired. **AccuracyCoin holds 100% (139/139)** on every release including v2.0.0. Workspace baseline: edition 2024, Rust **1.96**, license **MIT OR Apache-2.0**, author **DoubleGate**; the WebAssembly / GitHub Pages build is live at <https://doublegate.github.io/RustyNES/>.

**Engine-lineage versioning (read carefully).** The core descends from an accuracy program whose internal "v1.x / v2.x" milestones are folded into RustyNES stages v0.9.0–v0.9.7 → the v1.0.0 production cut. Read deep-narrative "v2.0" anchors from before 2026-07-03 (the master-clock refactor, old ADRs / audit logs under `docs/`) as **upstream engine lineage**, never as RustyNES release versions — that engine-lineage v2.0 work shipped as the v1.0.0 production core (2026-06-13) and is a *different* thing from RustyNES's own **v2.0.0 "Timebase"** release (2026-07-03, the current release), which replaces that same dot-lockstep scheduler with the one-clock model. `docs/STATUS.md` is the authoritative per-suite pass-count + mapper matrix.

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
# Maximal NATIVE build — the "cargo --full equivalent" (#54): the `full` feature
# aggregates every native feature (retroachievements + scripting + script-ipc +
# hd-pack + debug-hooks + av-record, additive on top of the default set). It is
# purely opt-in (shipped/default build + core unchanged). Aliases in .cargo/config.toml:
cargo full-run path/to/rom.nes                           # run the maximal native binary (alias ends in `--`, so flags forward, e.g. `cargo full-run --fullscreen rom.nes`)
cargo full-build                                         # build it (= --release -p rustynes-frontend --features full)
# WASM-only features (script-wasm, browser-cheevos, wasm-canvas) are excluded by design.
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

- `crates/rustynes-{cpu,ppu,apu,mappers,core,netplay,cheevos,frontend,test-harness}/` — the core emulation stack; crate name = dir name. The binary is `rustynes` (in `rustynes-frontend`). Plus the supporting crates: `rustynes-script` (Lua), `rustynes-ra` (RetroAchievements session state), `rustynes-gfx-shaders` (shared WGSL), `rustynes-hdpack` (HD-pack loader/compositor + HD audio), `rustynes-monetization` (dormant until v2.1.0), and the **platform crates** `rustynes-mobile` (the UniFFI bridge — generates Kotlin *and* Swift), `rustynes-android` (JNI/NDK host), `rustynes-ios` (Metal + CoreAudio shim; only the `#[cfg(target_os="ios")]` glue is iOS-specific), and `rustynes-libretro` (the RetroArch core; builds the platform-appropriate `rustynes_libretro` shared library — `.so` / `.dylib` / `.dll`). The `android/` and `ios/` dirs hold the Compose / SwiftUI apps.
- `docs/` — implementation specs. These are the **spec**, not history: update them in the same PR as the code change. Per-subsystem files (`cpu-6502.md`, `ppu-2c02.md`, `apu-2a03.md`, `mappers.md`, `cartridge-format.md`, `scheduler.md`) + cross-cutting (`architecture.md`, `testing-strategy.md`, `performance.md`, `frontend.md`, `compatibility.md`). `docs/STATUS.md` is the **single source of truth** for per-suite pass counts, the mapper matrix, and version policy. `docs/adr/` holds Michael-Nygard-format ADRs.
- `ref-docs/` — immutable hardware + emulation reference (60+ source research report). Updates go in dated supplemental files.
- `to-dos/ROADMAP.md` → phase/sprint files — tickets with stable IDs `T-PS-NNN`. Reference in commits.
- `tests/roms/` — CC0 / public-domain test ROMs (committed). `tests/roms/external/` — your own commercial dumps (gitignored).
- `tests/golden/` / `screenshots/` — reference framebuffers, audio, and the visual baseline corpus (committed). `tests/captures/` — current-run output (gitignored).

**Never commit commercial Nintendo ROMs.**

## Workflow conventions

- Branch names: `<type>/<short-desc>`.
- A chip-behavior change touches both the chip code and the chip's `docs/<subsystem>.md`. They drift apart easily; don't let them.
- For accuracy work: pin the failing test ROM expectation first, then implement until it passes.
- Hot paths (`Cpu::tick`, `Ppu::tick`, mapper register access): no allocations, prefer fixed arrays, profile (`cargo bench` + `perf record`) before adding abstractions. Target ≤ 2 ms/frame headless.
- `unsafe` requires a `// SAFETY:` comment explaining the invariant. The chip stack is `#![no_std]` + `extern crate alloc;`; only `rustynes-frontend` and `rustynes-cheevos` (FFI) carry `unsafe`.
- **Comprehensive rustdoc + comments (project rule).** Craft extensive `//!` crate/module preambles and `///` / `//` inline comments matching the quantity, quality, and technical depth of the existing `rustynes-*` crates — explain the *why* alongside the architectural detail, the memory-safety guarantees, and the lockstep-timing considerations.
- **Comprehensive commit bodies (project rule).** Commit message bodies are robust, comprehensive, and technically detailed: go beyond a summary to explain architectural impact, the mathematical implementation, memory constraints, and the deep technical specifics (the maintainer's house style; see `docs/guidelines`).
- Code style: rustfmt defaults + the crate-level import grouping in `rustfmt.toml`; `.editorconfig` mandates UTF-8 / LF / a final newline and indentation of four spaces for Rust, two for Markdown / TOML / YAML. Justify any local `#[allow]`.

## Operating notes for Claude Code

- `docs/STATUS.md` and the "Current release" summary above are the current-state source of truth; `CHANGELOG.md` and the `docs/audit/` logs carry the deep engine-lineage history.
- `ref-docs/` is immutable. Research updates go in dated supplemental files.
- ADRs go in `docs/adr/` (Michael Nygard format).
- `rustynes-core` re-exports the public types from the chip crates; downstream consumers (`rustynes-frontend`, `rustynes-test-harness`) should depend on `rustynes-core` rather than the chip crates directly.
- When relabeling old engine "v2.x" narrative for users, present it as upstream lineage/history — **never as a current RustyNES release version.** The current release is **v2.0.0 "Timebase"** (2026-07-03, the one-clock / every-cycle-bus-access scheduler rewrite + Vs. `DualSystem` dual-console support; preceded by v1.10.0 "Arcade" the native Libretro / RetroArch core, the v1.9.0→v1.9.9 iOS TestFlight train, the v1.8.0→v1.8.9 "Android" train, and the desktop-feature lineage v1.1.0→v1.7.1, all on the v1.0.0 production core; see the top "Current release" block + `docs/STATUS.md`). **Never claim any version *later* than v2.0.0 is released** (in particular the mobile app-store launch is the future **v2.1.0**, reached via the **v2.0.1 → v2.0.9** mobile-finalization re-port train — see `to-dos/ROADMAP.md`). Two distinct "v2.0"s exist and must not be conflated, **both now shipped, at different times, for different reasons**: the **engine-lineage v2.0** master-clock work shipped as the **v1.0.0** production core (2026-06-13) — it was the *only* scheduler through v1.10.0. RustyNES's own **v2.0.0 "Timebase"** release (2026-07-03) is a *different* milestone that *replaces* that same dot-lockstep scheduler outright: the **one-clock + every-cycle-bus-access collapse** (a single canonical cycle counter + a split-around-the-access `start_cycle`/`end_cycle` PPU catch-up, mirroring Mesen2's structure), full Vs. `DualSystem` dual-console emulation (core-and-harness-only; frontend wiring deferred), and the breaking save-state / cross-version changes it entailed (ADR 0002 / ADR 0028 / ADR 0029) — the one release that broke byte-identity / save-state compatibility, by design. The R1/R2 hard-tier MMC3 IRQ-timing residual was investigated under a bounded-effort campaign and is by-design-deferred beyond v2.0.0, not closed — see ADR 0002's decision-update section for the mechanism-level finding.
- **Forward plans + roadmap live in `to-dos/`.** `to-dos/ROADMAP.md` (updated in #129) is the planning entry point and frames the release line + "the path to v2.0.0 and beyond"; `to-dos/plans/` holds the per-release plan docs (through `v1.7.0-forge-plan.md` on `main`, plus the staged-forward `v1.8.0-android-plan.md` / `v1.9.0-ios-plan.md` / `v2.0.0-master-clock-plan.md`) + the `to-dos/plans/engine-lineage/` history archive + a `to-dos/plans/research/` reference-mining archive.
- The v1.0.0 release + GitHub Pages/CI + post-release record is in `docs/v1.0.0-synthesis-handoff-2026-06-13.md` — read it before touching CI, Pages, or release tooling. Full per-release history is in `CHANGELOG.md`.
- **Markdownlint is a CI gate** (pre-commit, pinned `markdownlint-cli v0.39.0`). The local `markdownlint` binary is a newer version that reports rules v0.39.0 lacks (e.g. MD060) — those are NOT gated; verify with `pre-commit run markdownlint --all-files`, not the bare binary. `.markdownlint.json` keeps `MD013`/`MD033`/`MD041` disabled by design (long technical tables, the README HTML banner/`<img>`, the HTML-led README). `.markdownlintignore` exempts `ref-docs/`, `ref-proj/`, the vendored `tricnes/` + upstream READMEs, and the frozen `docs/archive/` + `to-dos/archive/` trees — don't lint or reformat those.
- **RetroAchievements client identity:** the RA HTTP User-Agent (how RA authenticates/identifies/allowlists the client) is `RustyNES/<crate version> rcheevos/<rcheevos version>` — the `RA_USER_AGENT` const in `crates/rustynes-cheevos/src/http.rs`; the rcheevos version auto-syncs from the vendored `rc_version.h` via `build.rs` (`RCHEEVOS_VERSION`). Keep the leading `RustyNES/` token (a regression test guards it).
- **Exhaustive Documentation Sweeps:** When tasked with generating comprehensive project documentation or wikis, always recursively list and read the contents of `docs/`, `ref-docs/`, and `to-dos/` to ensure no deep technical knowledge is missed.
- **GitHub Wiki Initialization:** When assisting with GitHub Wiki deployments for the first time, instruct the user to click "Create the first page" in the GitHub UI to provision the `.wiki.git` repository. If the Wiki is cloned locally inside the main repository, ensure its folder (e.g., `RustyNES.wiki/`) is added to `.gitignore`.
- **Symlinked Agent Configs:** Ensure symlinked agent files (like `GEMINI.md` -> `AGENTS.md`) are explicitly removed from `.gitignore` so they are correctly tracked by version control.

<<< MC-PROJECT-END >>>
