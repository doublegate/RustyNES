# Building RustyNES

> **Authoritative reference:** [`../build-and-tooling.md`](../build-and-tooling.md)
> is the single source of truth for the toolchain, workspace layout, feature
> flags, profiles, and CI. This page is a quick developer-oriented summary.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Toolchain Setup](#toolchain-setup)
- [Building](#building)
- [Feature Flags](#feature-flags)
- [Platform-Specific Dependencies](#platform-specific-dependencies)
- [Cross-Compilation](#cross-compilation)
- [WebAssembly Build](#webassembly-build)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

### Required

- **Rust** 1.96.0 (pinned in `rust-toolchain.toml`; the channel auto-installs).
  Edition 2024. MSRV 1.96 unblocks the edition-2024 + egui 0.34.3 / wgpu 29 /
  rfd 0.17.2 dependency tier (bumped from 1.86 in v1.3.0 "Bedrock").
- **Cargo** (included with Rust).

### System libraries

The frontend uses **winit + wgpu + cpal + egui** (not SDL2). The chip crates
build with no system deps; the frontend needs windowing / GPU / audio dev
libraries on Linux.

---

## Toolchain Setup

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Or visit**: <https://rustup.rs>

The pinned toolchain (1.96.0) and the cross-compile targets
(`thumbv7em-none-eabihf` and `wasm32-unknown-unknown`) are all declared in
`rust-toolchain.toml`, so `rustup` installs them automatically on first build.

### Verify Installation

```bash
rustc --version  # Should report 1.96.0 (the pinned channel)
cargo --version
```

---

## Building

### Debug Build

```bash
cargo build --workspace
```

**Output**: `target/debug/rustynes` (the frontend binary is `rustynes`).

### Release Build

```bash
cargo build --release --workspace
```

**Output**: `target/release/rustynes`.

### Run Directly

```bash
cargo run --release -p rustynes-frontend -- path/to/rom.nes
cargo run --release -p rustynes-frontend            # open with no ROM; F12 to load
```

---

## Feature Flags

The frontend (`rustynes-frontend`) and core (`rustynes-core`) gate optional
functionality with Cargo features. See
[`../build-and-tooling.md`](../build-and-tooling.md) and
[`../STATUS.md`](../STATUS.md) for the authoritative list. The
developer-relevant ones:

| Feature | Crate(s) | Default | Description |
|---------|----------|---------|-------------|
| `emu-thread` | `rustynes-frontend` | **Yes** (native) | Dedicated emulation thread. `--no-default-features` keeps the synchronous (winit-thread) path for A/B. |
| `mapper-audio` | `rustynes-mappers` | **Yes** | On-cart expansion audio (VRC6/VRC7-OPLL/Sunsoft-5B/Namco-163/MMC5). |
| `std` | `rustynes-core` | **Yes** | Host build; off enables the `no_std + alloc` chip stack. |
| `test-roms` | `rustynes-test-harness` | No | Gates the vendored test-ROM integration suite. |
| `commercial-roms` | `rustynes-test-harness` | No | 60-ROM oracle against user-supplied dumps (not committed). |
| `retroachievements` | `rustynes-cheevos` + `rustynes-ra` + frontend | No | RetroAchievements (native-only; vendored `rcheevos` C lib). |
| `scripting` | `rustynes-script` + frontend | No | Lua 5.4 engine (mlua, vendored; native). Mutually exclusive with `script-wasm`. |
| `script-wasm` | `rustynes-script` + frontend | No | Pure-Rust piccolo Lua VM for the wasm backend (ADR 0012). Mutually exclusive with `scripting`. |
| `script-ipc` | `rustynes-frontend` | No | Host-mediated IPC/automation (`comm.*`/`client.*`/`userdata.*`; ADR 0016). |
| `hd-pack` | `rustynes-frontend` + `rustynes-hdpack` | No | HD-pack tile-substitution loader/compositor + HD audio. |
| `debug-hooks` | `rustynes-frontend` | No | Debugger visualization hooks (event viewer, trace, inspector). |
| `av-record` | `rustynes-frontend` | No | A/V recording. |
| `browser-cheevos` | `rustynes-frontend` | No | Casual-mode browser RetroAchievements scaffolding (ADR 0015; maintainer-manual carryover). |
| `full` | `rustynes-frontend` | No | Aggregates every native feature (`retroachievements` + `scripting` + `script-ipc` + `hd-pack` + `debug-hooks` + `av-record`). Use `cargo full-run` / `cargo full-build` (aliases in `.cargo/config.toml`). |
| `wasm-winit` / `wasm-canvas` | `rustynes-frontend` | (wasm only) | The two browser build flavours (mutually exclusive). |

> **Note:** netplay and TAS movie (`.rnm`) support ship in the default frontend
> build — they are not behind opt-in feature flags. The Lua scripting engine is
> shipped (since v1.1.0 "Scriptable") behind the off-by-default `scripting`
> feature. **Never** `--all-features`: the `scripting` (mlua) and `script-wasm`
> (piccolo) backends are mutually exclusive and cannot co-resolve; CI and the
> pre-commit hook use explicit feature sets.

### Build Examples

**Synchronous path (no dedicated emu thread)**:

```bash
cargo build --release -p rustynes-frontend --no-default-features
```

**With the test-ROM suite**:

```bash
cargo test --workspace --features test-roms
```

**Headless chip core only (no_std cross-compile gate)**:

```bash
rustup target add thumbv7em-none-eabihf
cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features
```

**Maximal native build** (every native feature; aliases in `.cargo/config.toml`):

```bash
cargo full-build                 # = --release -p rustynes-frontend --features full
cargo full-run path/to/rom.nes   # build + run the maximal native binary
```

---

## Platform-Specific Dependencies

### Linux

Building the frontend (and `cargo test --workspace`, which compiles it) needs
the wgpu/winit/cpal system libraries:

**Debian/Ubuntu**:

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev \
  libasound2-dev libudev-dev pkg-config
```

**Fedora**:

```bash
sudo dnf install libxkbcommon-devel wayland-devel alsa-lib-devel systemd-devel
```

**Arch / CachyOS**:

```bash
sudo pacman -S --needed libxkbcommon wayland alsa-lib systemd-libs
```

`wgpu` finds Vulkan via `libvulkan` (any vendor); no Vulkan SDK required.

### macOS

Xcode command-line tools (`xcode-select --install`). `wgpu` uses Metal; no
extra dependencies.

### Windows

MSVC build tools (Visual Studio 2019+ with the C++ workload) and the
Windows 10+ SDK. `wgpu` uses D3D12 by default; no Vulkan SDK required.

---

## Cross-Compilation

### Linux to Windows

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt-get install mingw-w64
cargo build --release --target x86_64-pc-windows-gnu
```

Release artifacts ship for `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`,
and `x86_64-pc-windows-msvc` (the `x86_64-apple-darwin` target was retired —
see [`../adr/0009-drop-x86_64-darwin-release-target.md`](../adr/0009-drop-x86_64-darwin-release-target.md)).

---

## WebAssembly Build

The browser frontend builds for `wasm32-unknown-unknown` via
[`trunk`](https://trunkrs.dev) in two mutually-exclusive flavours. Run from
`crates/rustynes-frontend/web`:

```bash
trunk serve                                                          # dev server
trunk build --release                                                # wasm-winit (default)
trunk build --release --no-default-features --features wasm-canvas   # lightweight embed
```

CI deploys the `wasm-winit` build to GitHub Pages
(<https://doublegate.github.io/RustyNES/>, with the workspace rustdoc at
`/api/`). The compressed size budget gate is
`scripts/wasm_size_budget.sh crates/rustynes-frontend/web/dist 5242880`.

> **Gotcha:** `web/Trunk.toml` pins the wasm-bindgen CLI version, which must
> exactly match the wasm-bindgen library in `Cargo.lock`
> (`grep -A1 'name = "wasm-bindgen"' Cargo.lock`). A mismatch fails `trunk build`
> (and the Pages deploy) at the wasm-bindgen step even though wasm clippy still
> passes — bump the pin whenever a resolve moves the library version.

## Android Build

The Android app cross-compiles the `rustynes-mobile` UniFFI bridge +
`rustynes-android` JNI glue + `rustynes-monetization` ad-policy crate via
`cargo ndk`, then builds the Jetpack Compose shell with Gradle (AGP 9.2.1 /
Gradle 9.4.1 / compileSdk 37 / targetSdk 36 / minSdk 26). See
[`../android.md`](../android.md) for the full setup.

---

## Troubleshooting

### Missing Linux windowing/audio libraries

Install the platform dependencies above (`libxkbcommon`, `wayland`,
`alsa-lib` / `libasound2-dev`, `libudev-dev`).

### "linker 'cc' not found"

```bash
# Debian/Ubuntu
sudo apt-get install build-essential
# macOS
xcode-select --install
```

### Slow Debug Builds

Use `--release` for emulator testing (the `[profile.dev]` `opt-level = 1` keeps
debug builds usable, but release is much faster for play-testing).

### Out of Memory During Compilation

```bash
cargo build --release -j 2
```

---

## References

- [build-and-tooling.md](../build-and-tooling.md) — authoritative build/toolchain/CI reference
- [STATUS.md](../STATUS.md) — feature-flag state, test counts, version policy
- [CONTRIBUTING.md](CONTRIBUTING.md) — development guidelines
- [TESTING.md](TESTING.md) — running tests
- [DEBUGGING.md](DEBUGGING.md) — debugging tools
