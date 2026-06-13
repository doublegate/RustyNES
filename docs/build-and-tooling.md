# Build and tooling

**References:** Phase 1 toolchain decisions; `docs/architecture.md` §Workspace shape.

## Toolchain

- **Rust edition**: 2021.
- **MSRV (minimum supported Rust version)**: 1.86.0. Pinned via `rust-toolchain.toml`. (Required by transitive deps that use edition2024 features — `icu_*` family pulled in via `directories`/`url`/`idna`.)
- **Channel**: stable. Nightly only for fuzz tests (`cargo +nightly fuzz`).
- **Targets supported**: `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Tier 2: `aarch64-unknown-linux-gnu`. Tier 3 (no CI): `wasm32-unknown-unknown` for browser.

## Workspace layout

```
Cargo.toml                  # workspace manifest
rust-toolchain.toml         # pin 1.75 stable
crates/
├── rustynes-core/               # public re-exports + Nes facade + scheduler + save state
├── rustynes-cpu/                # 2A03 CPU
├── rustynes-ppu/                # 2C02 PPU
├── rustynes-apu/                # 2A03 APU
├── rustynes-mappers/            # cartridge parsing + mapper trait + implementations
├── rustynes-frontend/           # rustynes binary (winit + wgpu + cpal + egui)
└── rustynes-test-harness/       # test ROM runner, golden log compare
tests/                      # workspace-level integration tests
benches/                    # criterion benches
```

## Common commands

| Action | Command |
|--------|---------|
| Build everything | `cargo build --workspace` |
| Build release frontend | `cargo build --release -p rustynes-frontend` |
| Run frontend | `cargo run --release -p rustynes-frontend -- path/to/rom.nes` |
| Unit tests | `cargo test --workspace` |
| Test ROM suite | `cargo test --workspace --features test-roms` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format check | `cargo fmt --all --check` |
| Format apply | `cargo fmt --all` |
| Generate docs | `cargo doc --workspace --no-deps --open` |
| Bench | `cargo bench --workspace` |
| Fuzz (one harness) | `cargo +nightly fuzz run cartridge_parse` |

## Linting policy

`Cargo.toml` workspace-level lints:

```toml
[workspace.lints.rust]
unsafe_code = "warn"           # only in CPU/PPU hot paths after benchmarking
missing_docs = "warn"          # public API documented

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery  = { level = "warn", priority = -1 }
# Allow these (signal-to-noise):
module_name_repetitions = "allow"
similar_names = "allow"
must_use_candidate = "allow"
```

CI enforces `clippy --all-targets -- -D warnings`. Local development: developers can `#[allow]` per-call but should justify in a code comment.

## Formatting

`rustfmt` with default settings + `imports_granularity = "Crate"`. Configured in `rustfmt.toml`.

## Profiles

```toml
[profile.dev]
opt-level = 1                  # debug builds usable for emulator dev (1 fps else)
overflow-checks = true

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
overflow-checks = false        # disabled for speed; CI runs a separate matrix with checks on

[profile.bench]
inherits = "release"
debug = true                   # for perf/flamegraph
```

## Dependencies

See `Cargo.toml` files per crate. Key choices:

- **`bitflags`** for CPU status flags + PPU control bits.
- **`thiserror`** 2.x for typed error enums. Derives `core::error::Error` (Rust 1.81+) — works in `no_std`. Workspace pin: `default-features = false` so the chip stack stays no_std-clean; `rustynes-frontend` and `rustynes-core`'s `std` feature opt back in to `thiserror/std`.
- **`libm`** 0.2 (no_std soft-float math) — pulled by `rustynes-apu` for `expf` when the `rustynes-apu/std` feature is off.
- **`serde` + `bincode`** (optional, behind `serde` feature) for save states.
- **`winit`** + **`wgpu`** + **`cpal`** + **`egui`** + **`gilrs`** for the frontend.
- **`criterion`** + **`proptest`** + **`insta`** as dev-deps.

No runtime async (`tokio`/`async-std`) — emulator core is synchronous; cpal callback runs on its own thread without async.

### `no_std + alloc` migration *(complete)*

The chip stack — `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers`, `rustynes-core` —
is `#![no_std]` + `extern crate alloc;` (Track C5, post-v0.9.0). `rustynes-frontend`
stays `std` because it depends on `wgpu` / `winit` / `cpal` / `egui`.

CI gates this via a dedicated `no_std` job that cross-compiles
`rustynes-core` to a bare-metal embedded target:

```bash
rustup target add thumbv7em-none-eabihf
cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features
```

`rustynes-core`'s `default = ["std"]` cargo feature propagates `std` to its
host-only deps (`lz4_flex`, `sha2`, `thiserror`) and to `rustynes-apu/std`
(which gates `f32::exp` vs `libm::expf` in the mixer's filter-coefficient
init). Desktop builds are unchanged.

To check your changes don't regress the migration, grep for `use std::`
in the chip crates — only the test / bench / example harnesses should hit:

```bash
grep -rn '^use std::' crates/nes-{core,cpu,ppu,apu,mappers}/
```

Run the CI gate locally before committing chip-crate changes:

```bash
cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features
```

## Files in `.github/`

- `actions/rust-setup/action.yml` — shared composite action (toolchain +
  Linux wgpu/winit/cpal deps + cargo cache) used by all three workflows, so
  the setup steps + the apt package list live in exactly one place.
- `workflows/ci.yml` — lint (fmt + clippy + rustdoc on the pinned 1.86
  toolchain, so the gate matches local) + the cross-platform test matrix +
  test-roms + no_std + wasm32 clippy + the frame-time bench gate. Skips
  entirely on documentation-only pushes (`paths-ignore`), and cancels
  superseded PR runs (`concurrency`).
- `workflows/release.yml` — tag-triggered (`v*`), builds the per-platform
  release binaries and attaches them to the GitHub Release (it never writes
  the release body — see the anti-clobber note in the workflow).
- `workflows/web.yml` — deploys the wasm32 frontend to GitHub Pages; build +
  size-budget gate run on PRs (paths-filtered to build inputs), deploy only
  on `main`.
- `ISSUE_TEMPLATE/bug_report.md` and `feature_request.md` — issue templates.
- `PULL_REQUEST_TEMPLATE.md` — PR checklist.

## Local dev environment

- Linux (Wayland or X11): no system deps beyond a working Rust toolchain. `wgpu` finds Vulkan via `libvulkan` (any vendor).
- macOS: Xcode command-line tools.
- Windows: MSVC build tools + Windows 10+ SDK. Vulkan SDK *not* required (wgpu uses D3D12 by default).

## Open questions

- **MSRV bumps.** Plan: bump only when a dependency requires it; document in `CHANGELOG.md`.
- **`-Cpanic=abort` vs unwind.** Abort gives smaller binaries and slightly faster code. We pay it; emulator core does not need to recover from panics.
- **`unsafe` audit.** Only the band-limited audio buffer might need `unsafe` (ring-buffer atomics). Audit at every PR; require justification comment.
