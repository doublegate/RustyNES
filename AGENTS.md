# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2021 workspace pinned to Rust 1.86 in `rust-toolchain.toml`.
Workspace crates live under `crates/`: `nes-core` contains the emulator facade,
scheduler, save states, and shared integration logic; `nes-cpu`, `nes-ppu`,
`nes-apu`, and `nes-mappers` model the hardware subsystems; `nes-frontend`
contains the `rustynes-v2` desktop binary; `nes-test-harness` runs ROM-based
regression tests. Project documentation is in `docs/`, planning tickets are in
`to-dos/`, benchmark targets sit beside crate code in `benches/`, and licensed
test ROM fixtures are under `tests/roms/`.

## Build, Test, and Development Commands

- `cargo build --workspace`: build all crates.
- `cargo run --release -p nes-frontend -- path/to/rom.nes`: run the emulator
  frontend against a local ROM.
- `cargo test --workspace`: run unit and integration tests.
- `cargo test --workspace --features test-roms`: include ROM harness tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: run the CI lint gate.
- `cargo fmt --all --check`: verify Rust formatting; use `cargo fmt --all` to
  apply it.
- `cargo bench --workspace`: run Criterion benchmarks.

## Coding Style & Naming Conventions

Use rustfmt defaults with crate-level import grouping from `rustfmt.toml`.
`.editorconfig` requires UTF-8, LF endings, final newlines, and spaces: four for
Rust, two for Markdown/TOML/YAML. Public APIs should have rustdoc comments.
Workspace lints warn on `unsafe_code` and `missing_docs`, with Clippy
`pedantic` and `nursery` enabled; justify local `#[allow]` uses. Prefer
allocation-light code in CPU, PPU, APU, mapper, and scheduler hot paths.

## Testing Guidelines

Place crate-specific integration tests in `crates/<crate>/tests/`. ROM-driven
coverage belongs in `crates/nes-test-harness/tests/` with fixtures documented in
`tests/roms/README.md` and `tests/roms/LICENSES.md`. Never commit commercial
ROMs; use `tests/roms/external/` for local-only material. Add focused tests for
mapper, timing, save-state, and audio changes, and run the `test-roms` feature
when touching harness code or fixtures.

## Commit & Pull Request Guidelines

Use Conventional Commits, matching the existing history:
`fix(ppu): handle OAMDATA writes during rendering` or
`docs(changelog): record release notes`. Keep subjects imperative, under 72
characters, and without trailing periods. Before opening a PR, run fmt, clippy,
and tests; link the relevant ticket from `to-dos/`; update `CHANGELOG.md` for
user-visible behavior; and update `docs/` when architecture, mapper support, or
frontend behavior changes.
