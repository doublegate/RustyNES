# Build gotchas, frontend aliases, keys and WebAssembly

> Moved out of `AGENTS.md` (2026-11-14) to keep that file under agy's 24,000-byte rule-file limit. The content is verbatim; relative links in it were written relative to the repository root.
>
> **Read this file when the task runs `cargo clippy --fix`, the maximal native build, the default key map, or the WebAssembly / Pages build.**

## After `cargo clippy --fix`

```bash
# After any `cargo clippy --fix`, re-run clippy for EVERY feature combo (incl. retroachievements):
# --fix compiles only the active feature set, so it can strip cfg-gated code that another feature
# needs (bit PR #80: an `elidable_lifetime_names` autofix removed a `<'a>` the `retroachievements`
# `ra` param uses, breaking that build). Wasm clippy commands are in the WebAssembly section below.
```

## Toolchain history

Toolchain is **Rust 1.100** pinned in `rust-toolchain.toml` (1.86 until v1.3.0, 1.96 until v3.0.1, 1.99 until v3.7.1). **The libretro buildbot follows the same pin**: `.gitlab-ci.yml`'s `RUSTUP_TOOLCHAIN` must equal `rust-toolchain.toml`'s `channel`, and CI's `libretro-cross` job fails if they differ, so move both in one change. v3.0.1 first held the buildbot on 1.96.0 (its image passed `-C ar`, a hard error from Rust 1.97), then lifted the hold once a test branch proved the image had dropped the flag (pipeline 119614, all 15 jobs on 1.99.0); every crate now inherits the workspace `rust-version`. A pushed branch gets its own buildbot pipeline, so test a toolchain move there before merging (`docs/agents/libretro.md`). CI runs the tests on the pinned toolchain across Linux/macOS/Windows.

## Frontend aliases, keys and WebAssembly

```bash
# Maximal NATIVE build — the "cargo --full equivalent" (#54): the `full` feature
# aggregates every native feature (retroachievements + scripting + script-ipc +
# hd-pack + debug-hooks + av-record, additive on top of the default set). It is
# purely opt-in (shipped/default build + core unchanged). Aliases in .cargo/config.toml:
cargo full-run path/to/rom.nes                           # run the maximal native binary (alias ends in `--`, so flags forward, e.g. `cargo full-run --fullscreen rom.nes`)
cargo full-build                                         # build it (= --release -p rustynes-frontend --features full)
cargo full-native                                        # same, tuned for THIS CPU (release-native + target-cpu=native; not portable; target/release-native/rustynes)
cargo full-v3                                            # same, for an x86-64-v3 PC (the whole level, not just AVX2; release-v3; target/release-v3/rustynes)
cargo full-baseline                                      # same, pinned to target-cpu=x86-64 (what releases ship; the A/B baseline -- a user cargo config can make plain full-build native)
# WASM-only features (script-wasm, browser-cheevos, wasm-canvas) are excluded by design.
# Default keys P1: arrows = D-pad, Z = A, X = B, Enter = Start, RShift = Select.
# Default keys P2: WASD = D-pad, Q = A, E = B, P = Start, R = Select (L before v2.7.3).
# Famicom microphone: hold N (M before v2.7.3). M toggles the menu bar.
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
```
