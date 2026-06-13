# Contributing to RustyNES

Thank you for your interest in contributing to RustyNES! We welcome contributions from developers of all skill levels.

## Quick Links

- [Code of Conduct](#code-of-conduct)
- [How Can I Contribute?](#how-can-i-contribute)
- [Development Setup](#development-setup)
- [Coding Standards](#coding-standards)
- [The Quality Gate](#the-quality-gate)
- [Pull Request Process](#pull-request-process)
- [Test ROM Legalities](#test-rom-legalities)
- [Getting Help](#getting-help)

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior to <parobek@gmail.com>.

## How Can I Contribute?

### Reporting Bugs

- Use the [Bug Report template](.github/ISSUE_TEMPLATE/bug_report.md).
- Search existing issues first to avoid duplicates.
- Include the ROM name + iNES mapper number, expected vs. actual behavior, reproduction steps (frame number if possible), a save state (`.rns`) if applicable, the `rustynes --version` output, and your OS.
- Provide logs when possible (`RUST_LOG=debug`).

### Suggesting Features

- Use the [Feature Request template](.github/ISSUE_TEMPLATE/feature_request.md).
- Clearly describe the problem and proposed solution, and explain use cases.
- Check the [ROADMAP](ROADMAP.md) for alignment with project goals.

### Requesting Mappers

- Use the [Mapper Request template](.github/ISSUE_TEMPLATE/mapper_request.md).
- Provide the mapper number, games that use it, NESdev wiki links, and test ROM information.

### Contributing Code

Areas where help is especially valued:

- **Accuracy** — closing residual test-ROM cases, region (PAL/Dendy) edge cases, new mapper families.
- **Testing** — test ROM integration, property-based tests, game compatibility, benchmarking.
- **Documentation** — rustdoc comments, examples, subsystem-spec clarifications.
- **Tooling** — CI/CD, build scripts, debugging utilities.

## Development Setup

### Prerequisites

- **Rust 1.86** (pinned in `rust-toolchain.toml`; `rustup` auto-installs it, including the `wasm32-unknown-unknown` and `thumbv7em-none-eabihf` targets).
- **Git**.
- **System libraries** for the `winit` + `wgpu` + `cpal` frontend.

### Platform-Specific Setup

**Ubuntu/Debian:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential git \
  libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev libasound2-dev libudev-dev
```

**CachyOS / Arch:**

```bash
sudo pacman -S --needed base-devel git libxkbcommon wayland alsa-lib systemd-libs
```

**Fedora:**

```bash
sudo dnf install gcc git wayland-devel libxkbcommon-devel alsa-lib-devel systemd-devel
```

**macOS:**

```bash
brew install git   # the wgpu/Metal + CoreAudio stack ships with the OS
```

**Windows:** install Visual Studio 2019+ with the C++ build tools. The frontend uses DX12/Vulkan via `wgpu`; no extra audio/windowing libraries are required.

### Fork and Clone

```bash
# Fork the repository on GitHub first, then:
git clone https://github.com/YOUR_USERNAME/RustyNES.git
cd RustyNES
git remote add upstream https://github.com/doublegate/RustyNES.git
```

### Build and Test

```bash
cargo build --workspace                              # build everything
cargo test --workspace                               # unit + integration tests
cargo test --workspace --features test-roms          # + AccuracyCoin / blargg / kevtris ROM suites
cargo test -p rustynes-cpu                           # a single crate
cargo build --release --workspace                    # optimized build
cargo run --release -p rustynes-frontend -- rom.nes  # run the emulator (binary: rustynes)
```

## Coding Standards

### Rust Style

- **Format:** `cargo fmt` (rustfmt defaults).
- **Lint:** pass `cargo clippy --workspace --all-targets -- -D warnings` with no warnings.
- **Edition:** Rust 2021. **MSRV:** 1.86.
- The chip stack (`rustynes-{cpu,ppu,apu,mappers,core}`) is `#![no_std]` + `extern crate alloc;`. `unsafe` is only permitted at FFI boundaries (`rustynes-cheevos`) and the one native priority hook in `rustynes-frontend`, and **must** carry a `// SAFETY:` comment explaining the invariant.
- No emojis in code, comments, or commits (project policy).

### Documentation

All public APIs must have rustdoc comments. Modules get `//!`, items get `///`. Document the *why*, not just the *what*. The `doc` job runs with `RUSTDOCFLAGS="-D warnings"`, so broken intra-doc links fail CI.

### Determinism (hard contract)

Same seed + ROM + input sequence ⇒ bit-identical framebuffer and audio. Do not introduce hidden non-determinism (system time, thread scheduling, OS RNG) into the core. This is what makes save-states, TAS movies, regression oracles, and netplay rollback correct. Frontend-only concerns (dynamic rate control, run-ahead, pacing) must stay in the frontend, never in core synthesis.

### Testing Requirements

- **Unit tests** for new functions and modules.
- **Integration / test-ROM tests** for emulation-accuracy changes. For accuracy work, pin the failing test-ROM expectation first, then implement until it passes.
- Cover new public behavior; favor concrete asserted values over smoke checks.

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

<body — optional, explains *why*>

<footer — optional, e.g. "Closes #42">
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `perf`, `build`, `ci`. Subject is imperative, ≤ 72 chars, no trailing period.

**Example:**

```
fix(ppu): correct sprite 0 hit timing

Sprite 0 hit fired one dot too late; detection now occurs at the
correct dot. Validated against the kevtris sprite-hit timing ROM.

Fixes #85
```

## The Quality Gate

Before opening a PR, verify all of the following pass:

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] If you touched the ROM harness: `cargo test --workspace --features test-roms` still green
- [ ] If you touched the chip stack: `cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features` still compiles
- [ ] New public items have rustdoc comments
- [ ] **`CHANGELOG.md` `[Unreleased]` updated for any user-visible change**
- [ ] Affected `docs/` files updated if architecture or chip behavior changed
- [ ] Ticket marked complete in the relevant `to-dos/` sprint file (if you picked one)

## Pull Request Process

1. **Sync with upstream:**

   ```bash
   git fetch upstream && git rebase upstream/main
   ```

2. **Run the quality gate** (above).

3. **Branch + push:** branch names are `<type>/<short-desc>` (e.g. `feat/cpu-immediate-addressing`).

   ```bash
   git push origin feat/my-feature
   ```

4. **Open the PR**, fill out the template, reference the ticket(s) and any relevant `docs/` files, and ensure CI passes.

5. **Respond to review feedback** promptly and keep the PR scope focused.

Maintainers aim to review within 3–14 days; at least one approval and green CI are required to merge. After merge your contribution is credited in `CHANGELOG.md` and the release notes.

## Test ROM Legalities

The repository ships test ROMs (`tests/roms/`) that are individually CC0 or public-domain; do not add ROMs unless their license is explicitly documented in `tests/roms/LICENSES.md`. **Never commit commercial Nintendo ROMs.** Place them in `tests/roms/external/` (gitignored) for local oracle testing.

## Getting Help

- **Documentation:** the [docs/](docs/) folder (subsystem specs + `docs/STATUS.md` + `docs/adr/`).
- **GitHub Discussions:** [general questions and design discussion](https://github.com/doublegate/RustyNES/discussions). Use Discussions (not Issues) for design questions.
- **GitHub Issues:** [bug reports and concrete feature requests](https://github.com/doublegate/RustyNES/issues).
- **NESdev Forums:** [NES hardware questions](https://forums.nesdev.org/).
- Check issues labeled [`good first issue`](https://github.com/doublegate/RustyNES/labels/good%20first%20issue) and [`help wanted`](https://github.com/doublegate/RustyNES/labels/help%20wanted), or the [ROADMAP](ROADMAP.md) for upcoming work.

## License

By contributing to RustyNES, you agree that your contributions will be dual-licensed under both the [MIT License](LICENSE-MIT) and the [Apache License 2.0](LICENSE-APACHE).

---

**Thank you for contributing to RustyNES!** Your efforts help preserve video game history and grow an accurate, modern emulation platform.
