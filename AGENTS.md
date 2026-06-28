# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2024 workspace pinned to Rust 1.96 in `rust-toolchain.toml`.
Workspace crates live under `crates/`: `rustynes-core` contains the emulator facade,
scheduler, save states, and shared integration logic; `rustynes-cpu`, `rustynes-ppu`,
`rustynes-apu`, and `rustynes-mappers` model the hardware subsystems; `rustynes-frontend`
contains the `rustynes` desktop binary; `rustynes-test-harness` runs ROM-based
regression tests. Project documentation is in `docs/`, planning tickets are in
`to-dos/`, benchmark targets sit beside crate code in `benches/`, and licensed
test ROM fixtures are under `tests/roms/`. `ref-docs/` contains immutable hardware and emulation reference; updates go in dated supplemental files. ADRs go in `docs/adr/` (Michael Nygard format).

## Build, Test, and Development Commands

- `cargo build --workspace`: build all crates.
- `cargo run --release -p rustynes-frontend -- path/to/rom.nes`: run the emulator frontend against a local ROM.
- `cargo test --workspace`: run unit and integration tests.
- `cargo test --workspace --features test-roms`: include ROM harness tests.
- `cargo test --workspace --features test-roms,commercial-roms`: include 60-ROM commercial oracle (needs local dumps).
- `cargo clippy --workspace --all-targets -- -D warnings`: run the CI lint gate.
- `cargo fmt --all --check`: verify Rust formatting; use `cargo fmt --all` to apply it.
- `cargo bench --workspace`: run Criterion benchmarks.
- `cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features`: cross-compile to no_std.

## Coding Style & Naming Conventions

Use rustfmt defaults with crate-level import grouping from `rustfmt.toml`.
`.editorconfig` requires UTF-8, LF endings, final newlines, and spaces: four for Rust, two for Markdown/TOML/YAML.
Public APIs should have rustdoc comments. Workspace lints warn on `missing_docs`, with Clippy `pedantic` and `nursery` enabled; justify local `#[allow]` uses.

**CRITICAL DOCUMENTATION RULE:** Always craft extensive, robust, and technically detailed Rust preambles (`//!`) at the top of crates/modules and inline source code comments (`///` and `//`). Documentation must match the quantity, quality, length, and technical depth seen in existing `rustynes-*` crates, comprehensively explaining the *why* alongside architectural details, memory safety guarantees, and lockstep timing considerations.

`unsafe` blocks require a `// SAFETY:` comment explaining the invariant. The chip stack is `#![no_std]` + `extern crate alloc;`.
No emojis in code, comments, or commits (project policy).
Hot paths (`Cpu::tick`, `Ppu::tick`, mapper register access) MUST avoid allocations, prefer fixed arrays, and be highly optimized. Target <= 2 ms/frame headless.

## Testing Guidelines

Place crate-specific integration tests in `crates/<crate>/tests/`. ROM-driven
coverage belongs in `crates/rustynes-test-harness/tests/` with fixtures documented in
`tests/roms/README.md` and `tests/roms/LICENSES.md`. Never commit commercial
ROMs; use `tests/roms/external/` for local-only material.
For accuracy work: pin the failing test ROM expectation first, then implement until it passes. The blargg / kevtris / mmc3_test_2 / AccuracyCoin suites are the closed-form definition of "cycle-accurate."

## Commit & Pull Request Guidelines

Use Conventional Commits, matching the existing history:
`fix(ppu): handle OAMDATA writes during rendering` or `docs(changelog): record release notes`.
Keep subjects imperative, under 72 characters, and without trailing periods.
Branch names should follow `<type>/<short-desc>`.

**CRITICAL COMMIT RULE:** Commit message bodies should ALWAYS be robust, comprehensive, and technically detailed. Go beyond a summary to explain architectural impacts, mathematical implementations, memory constraints, and deep technical specifics.

Before opening a PR, run fmt, clippy, and tests; link the relevant ticket from `to-dos/`; update `CHANGELOG.md` under `[Unreleased]` for user-visible behavior; and update `docs/` when architecture, mapper support, or frontend behavior changes. A chip-behavior change MUST touch both the chip code and the chip's `docs/<subsystem>.md` to prevent drift.

## Documentation & Markdown Guidelines

- `docs/STATUS.md` is the authoritative single source of truth for per-suite pass counts, the mapper matrix, and version policy.
- Markdownlint is a CI gate (pre-commit, pinned `markdownlint-cli v0.39.0`). Verify with `pre-commit run markdownlint --all-files`.
- `.markdownlint.json` keeps `MD013`/`MD033`/`MD041` disabled by design.
- `rustynes-core` re-exports public types from the chip crates; downstream consumers should depend on `rustynes-core` rather than the chip crates directly.
