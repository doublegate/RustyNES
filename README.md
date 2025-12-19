# RustyNES

[![Build Status](https://github.com/doublegate/RustyNES/workflows/CI/badge.svg)](https://github.com/doublegate/RustyNES/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#platform-support)
[![Documentation](https://img.shields.io/badge/docs-latest-blue.svg)](https://doublegate.github.io/RustyNES/)
[![codecov](https://codecov.io/gh/doublegate/RustyNES/branch/main/graph/badge.svg)](https://codecov.io/gh/doublegate/RustyNES)

## Overview

A next-generation NES emulator written in pure Rust — targeting 100% accuracy, modern features, and safe code

---

> **Status:** Pre-Implementation (Architecture & Documentation Complete). Ready to begin Phase 1 CPU implementation. See [ROADMAP.md](ROADMAP.md) for development timeline.

---

## Why RustyNES?

RustyNES combines **accuracy-first emulation** with **modern features** and the **safety guarantees of Rust**. Whether you're a casual player, TAS creator, speedrunner, or homebrew developer, RustyNES provides a comprehensive platform for NES emulation.

**Key Differentiators:**

- Cycle-accurate emulation targeting 100% TASVideos test suite
- Modern features: RetroAchievements, GGPO netplay, TAS tools, Lua scripting
- Safe Rust with minimal unsafe code (only in FFI boundaries)
- Modular architecture allowing independent use of CPU/PPU/APU crates
- Cross-platform with WebAssembly support

---

## Highlights

| Feature               | Description                                                                               |
| --------------------- | ----------------------------------------------------------------------------------------- |
| **Cycle-Accurate**    | Sub-cycle precision for CPU, PPU, and APU - targeting 100% TASVideos test suite pass rate |
| **300+ Mappers**      | Comprehensive cartridge support covering all licensed games plus homebrew                 |
| **RetroAchievements** | Native rcheevos integration for achievement hunting                                       |
| **GGPO Netplay**      | Frame-perfect rollback netcode via backroll-rs                                            |
| **TAS Tools**         | FM2 format support with rewind, frame advance, and movie recording                        |
| **Lua Scripting**     | Modern Lua 5.4 scripting via mlua for automation and bots                                 |
| **GPU Accelerated**   | Cross-platform wgpu rendering with shader support                                         |
| **Pure Rust**         | Zero unsafe code where possible, leveraging Rust's safety guarantees                      |

---

## Quick Start

### Download Binaries (Coming Soon)

Pre-built binaries will be available for Windows, Linux, and macOS when the MVP is released (target: June 2026).

### Build from Source

**Prerequisites:**

- **Rust 1.75 or newer** — Install via [rustup.rs](https://rustup.rs)
- **SDL2 development libraries** — Platform-specific installation below
- **Git**

**Clone and Build:**

```bash
# Clone the repository
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES

# Build and run (once implemented)
cargo build --release
cargo run --release -p rustynes-desktop

# Run with a ROM
cargo run --release -p rustynes-desktop -- path/to/rom.nes
```

### Platform-Specific Dependencies

**Ubuntu/Debian:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential libsdl2-dev
```

**Fedora:**

```bash
sudo dnf install gcc SDL2-devel
```

**macOS:**

```bash
brew install sdl2
```

**Windows:**

- Install [Visual Studio 2019+](https://visualstudio.microsoft.com/) with C++ tools
- Download SDL2 development libraries from [libsdl.org](https://libsdl.org)
- Set `SDL2_PATH` environment variable to SDL2 location

### Basic Usage Examples

```bash
# Run with a specific ROM
./rustynes-desktop game.nes

# Enable debug logging
RUST_LOG=debug ./rustynes-desktop game.nes

# Run test ROMs (for accuracy validation)
cargo test nestest_rom
cargo test blargg_cpu_tests
cargo test blargg_ppu_tests
```

---

## Configuration

RustyNES supports extensive configuration through:

- Configuration files (TOML format)
- Command-line arguments
- GUI settings panel

See [docs/api/CONFIGURATION.md](docs/api/CONFIGURATION.md) for complete options.

### Feature Flags

Build with specific features enabled:

```bash
# Build with all features
cargo build --release --all-features

# Build desktop GUI only
cargo build --release -p rustynes-desktop

# Build WebAssembly
wasm-pack build crates/rustynes-web --target web

# Build with specific features
cargo build --release --features "netplay,tas,lua"
```

---

## Default Controls

| NES    | Keyboard      | Gamepad  |
| ------ | ------------- | -------- |
| D-Pad  | WASD / Arrows | D-Pad    |
| A      | K / Z         | A Button |
| B      | J / X         | B Button |
| Start  | Enter         | Start    |
| Select | Right Shift   | Select   |

---

## Features

### Current Status

- [x] **Architecture Design** - Complete modular crate structure with 10 component crates
- [x] **Documentation** - 39 comprehensive specification and implementation guides covering CPU, PPU, APU, mappers, testing, and development
- [x] **Project Setup** - Workspace structure created, ready for implementation
- [ ] **Implementation** - Beginning Phase 1 (CPU, PPU, APU, mappers, GUI)

### MVP (Phase 1) - Target: June 2026

- [ ] Cycle-accurate 6502/2A03 CPU emulation (all 256 opcodes)
- [ ] Dot-level 2C02 PPU rendering (341x262 scanlines)
- [ ] Hardware-accurate 2A03 APU synthesis (all 5 channels)
- [ ] Mappers 0, 1, 2, 3, 4 (80% game coverage, 500+ games)
- [ ] Cross-platform GUI (egui + wgpu)
- [ ] Save states and battery saves
- [ ] Gamepad support (SDL2)
- [ ] 85% TASVideos test suite pass rate

### Planned (Phases 2-4) - Target: December 2027

- [ ] RetroAchievements integration (rcheevos FFI)
- [ ] GGPO-style rollback netplay (backroll-rs)
- [ ] Lua 5.4 scripting with memory/GUI APIs
- [ ] TAS recording/playback (FM2 format)
- [ ] Integrated debugger (CPU, PPU, APU viewers)
- [ ] Rewind, fast-forward, slow-motion
- [ ] WebAssembly build with PWA support
- [ ] CRT/NTSC shaders and video filters
- [ ] Expansion audio (VRC6, VRC7, MMC5, FDS, N163, 5B)
- [ ] 300+ mapper implementations (100%+ game coverage)
- [ ] 100% TASVideos accuracy test pass rate

See [ROADMAP.md](ROADMAP.md) for the complete development plan.

### Comparison with Other Emulators

| Feature | RustyNES | Mesen2 | FCEUX | puNES | TetaNES |
|---------|----------|--------|-------|-------|---------|
| **CPU Accuracy** | Cycle | Cycle | Cycle | Instruction | Cycle |
| **PPU Accuracy** | Dot | Dot | Scanline | Dot | Dot |
| **Mapper Count** | 300+ (goal) | 300+ | 200+ | 461+ | 10 |
| **RetroAchievements** | ✓ | ✓ | ✗ | ✗ | ✗ |
| **GGPO Netplay** | ✓ | ✗ | ✗ | ✗ | ✗ |
| **TAS Editor** | ✓ | ✓ | ✓ | ✗ | ✗ |
| **Lua Scripting** | ✓ (5.4) | ✓ (5.4) | ✓ (5.1) | ✗ | ✗ |
| **Debugger** | Advanced | Advanced | Advanced | Basic | Basic |
| **WebAssembly** | ✓ | ✗ | ✓ | ✗ | ✓ |
| **Language** | Rust | C++ | C++ | C++ | Rust |
| **License** | MIT/Apache | GPL-3.0 | GPL-2.0 | GPL-2.0 | GPL-3.0 |

**RustyNES Advantages:**

- Modern Rust codebase with memory safety guarantees
- Unique combination of accuracy + netplay + RetroAchievements
- Modular design allowing component reuse
- Permissive dual licensing (MIT/Apache-2.0)

---

## Technical Details

### NES Timing Model

RustyNES implements cycle-accurate timing based on the NES master clock:

```text
Master Clock (NTSC): 21.477272 MHz
├─ CPU Clock: ÷12 = 1.789773 MHz (~559 ns/cycle)
├─ PPU Clock: ÷4  = 5.369318 MHz (~186 ns/dot)
└─ APU Clock: Same as CPU (1.789773 MHz)

Frame Timing:
- Scanlines: 262 (NTSC), 312 (PAL)
- Dots per scanline: 341 (340 on odd frames)
- Total PPU cycles: 89,342 (NTSC), 106,392 (PAL)
- CPU cycles per frame: 29,781 (NTSC), 35,464 (PAL)
- Target framerate: 60.0988 Hz (NTSC), 50.0070 Hz (PAL)
```

### Accuracy Targets

| Component | Target | Validation |
|-----------|--------|------------|
| **CPU (6502)** | 100% instruction-level | nestest.nes golden log match |
| **PPU (2C02)** | 100% cycle-accurate | ppu_vbl_nmi, sprite_hit_tests, scrolltest |
| **APU (2A03)** | 99%+ hardware match | apu_test, dmc_tests, blargg suite |
| **Mappers** | 100% for licensed | 700+ game compatibility matrix |
| **Overall** | 100% TASVideos suite | 156 test ROM pass rate |

### Architecture Overview

```text
┌──────────────────────────────────────────────────────────┐
│                      RustyNES Core                       │
├─────────────┬─────────────┬─────────────┬────────────────┤
│   CPU       │    PPU      │    APU      │    Mappers     │
│  (6502)     │  (2C02)     │  (2A03)     │  (0-300+)      │
│             │             │             │                │
│ • Cycle     │ • Dot-level │ • 5 Channels│ • Banking      │
│   accurate  │   rendering │ • Expansion │ • IRQ timing   │
│ • All 256   │ • Scrolling │   audio     │ • Mirroring    │
│   opcodes   │ • Sprites   │ • Mixing    │                │
└─────────────┴─────────────┴─────────────┴────────────────┘
       │              │              │             │
       └──────────────┴──────────────┴─────────────┘
                         │
              ┌──────────┴──────────┐
              │     Memory Bus      │
              │  (Address Space)    │
              └──────────┬──────────┘
                         │
       ┌─────────────────┼────────────────┐
       │                 │                │
┌──────┴──────┐   ┌──────┴──────┐   ┌─────┴─────┐
│   Desktop   │   │     Web     │   │  Headless │
│  (egui/wgpu)│   │   (WASM)    │   │   (API)   │
└─────────────┘   └─────────────┘   └───────────┘
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for comprehensive system design.

---

## Platform Support

| Platform            | Status  |
| ------------------- | ------- |
| **Windows x64**     | Primary |
| **Linux x64**       | Primary |
| **macOS x64/ARM64** | Primary |
| **WebAssembly**     | Planned |
| **Linux ARM64**     | Planned |

### System Requirements

**Minimum:** 1.5 GHz dual-core, 512 MB RAM, OpenGL 3.3
**Recommended:** 2.0 GHz quad-core, 2 GB RAM, Vulkan/Metal/DX12 GPU

---

## Documentation

| Document                           | Description                                           |
| ---------------------------------- | ----------------------------------------------------- |
| [OVERVIEW.md](OVERVIEW.md)         | Philosophy, accuracy goals, emulation approach        |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design, component relationships, Rust patterns |
| [ROADMAP.md](ROADMAP.md)           | Development phases and milestones                     |

### Hardware Documentation

| Component      | Location                                                             |
| -------------- | -------------------------------------------------------------------- |
| **CPU (6502)** | [docs/cpu/](docs/cpu/) - Instruction set, timing, unofficial opcodes |
| **PPU (2C02)** | [docs/ppu/](docs/ppu/) - Rendering, scrolling, sprite evaluation     |
| **APU (2A03)** | [docs/apu/](docs/apu/) - Audio channels, mixing, frame counter       |
| **Memory Bus** | [docs/bus/](docs/bus/) - Address space, bus conflicts                |
| **Mappers**    | [docs/mappers/](docs/mappers/) - Cartridge banking and variants      |

### Development Guides

| Guide                                    | Purpose                         |
| ---------------------------------------- | ------------------------------- |
| [CONTRIBUTING](docs/dev/CONTRIBUTING.md) | Code style and PR process       |
| [BUILD](docs/dev/BUILD.md)               | Toolchain and cross-compilation |
| [TESTING](docs/dev/TESTING.md)           | Test ROM suites and CI          |
| [DEBUGGING](docs/dev/DEBUGGING.md)       | Built-in debugger usage         |
| [GLOSSARY](docs/dev/GLOSSARY.md)         | NES terminology reference       |

### API Reference

| API                                        | Description                         |
| ------------------------------------------ | ----------------------------------- |
| [CORE_API](docs/api/CORE_API.md)           | Embedding the emulator as a library |
| [SAVE_STATES](docs/api/SAVE_STATES.md)     | State serialization format          |
| [CONFIGURATION](docs/api/CONFIGURATION.md) | Runtime options and settings        |

---

## Building from Source

### Standard Build

```bash
# Debug build (faster compilation, includes debug symbols)
cargo build --workspace

# Release build (optimized, ~10x faster runtime)
cargo build --workspace --release

# Build specific crate
cargo build -p rustynes-desktop --release

# Run tests
cargo test --workspace

# Run lints
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all

# Generate documentation
cargo doc --workspace --no-deps --open
```

### Development Build

```bash
# Build with all warnings
cargo build --workspace -- -D warnings

# Build and run in watch mode (requires cargo-watch)
cargo install cargo-watch
cargo watch -x 'build --workspace'

# Run specific tests
cargo test -p rustynes-cpu test_lda_immediate

# Run with debug logging
RUST_LOG=debug cargo run -p rustynes-desktop

# Benchmark critical paths (requires nightly)
cargo +nightly bench -p rustynes-core
```

### Cross-Compilation

```bash
# Install cross-compilation tool
cargo install cross

# Build for Linux ARM64
cross build --target aarch64-unknown-linux-gnu --release

# Build for Windows from Linux
cross build --target x86_64-pc-windows-gnu --release

# Build for Raspberry Pi
cross build --target armv7-unknown-linux-gnueabihf --release
```

### WebAssembly Build

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for web
wasm-pack build crates/rustynes-web --target web --release

# Build with specific features
wasm-pack build crates/rustynes-web --target web --features "netplay,lua"

# Test in browser
cd crates/rustynes-web/www
npm install
npm start
```

### Feature Flags Reference

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Basic emulation | ✓ |
| `netplay` | GGPO rollback netcode | ✗ |
| `achievements` | RetroAchievements integration | ✗ |
| `tas` | TAS recording/playback | ✗ |
| `lua` | Lua 5.4 scripting | ✗ |
| `debugger` | Advanced debugging tools | ✗ |
| `expansion-audio` | VRC6/VRC7/N163/etc. audio | ✗ |

---

## Contributing

Contributions of all kinds are welcome! Whether you're fixing bugs, adding features, improving documentation, or testing, we'd love your help.

### Ways to Contribute

- **Code**: Implement CPU/PPU/APU features, mappers, or GUI improvements
- **Testing**: Run test ROMs, report bugs, verify accuracy
- **Documentation**: Improve guides, add examples, clarify specifications
- **Design**: UI/UX improvements, icons, artwork

### Getting Started

1. **Read the contribution guide**: [CONTRIBUTING.md](CONTRIBUTING.md)
2. **Find an issue**: Check [`good first issue`](https://github.com/doublegate/RustyNES/labels/good%20first%20issue) or [`help wanted`](https://github.com/doublegate/RustyNES/labels/help%20wanted) labels
3. **Ask questions**: Use [GitHub Discussions](https://github.com/doublegate/RustyNES/discussions) if you need guidance

### Quick Contribution Workflow

```bash
# 1. Fork and clone
git clone https://github.com/YOUR_USERNAME/RustyNES.git
cd RustyNES

# 2. Create a feature branch
git checkout -b feature/my-feature

# 3. Make changes and test
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all

# 4. Commit using conventional commits
git commit -m "feat(cpu): implement ADC instruction"

# 5. Push and create PR
git push origin feature/my-feature
```

### Development Resources

| Resource | Purpose |
|----------|---------|
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contribution guidelines |
| [docs/dev/BUILD.md](docs/dev/BUILD.md) | Build instructions |
| [docs/dev/TESTING.md](docs/dev/TESTING.md) | Testing guide |
| [docs/dev/STYLE_GUIDE.md](docs/dev/STYLE_GUIDE.md) | Code style standards |
| [docs/dev/DEBUGGING.md](docs/dev/DEBUGGING.md) | Debugging techniques |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System architecture |

---

## Acknowledgments

RustyNES stands on the shoulders of giants. We're grateful to:

### Reference Emulators

These projects provided invaluable reference implementations and inspiration:

- **[Mesen2](https://github.com/SourMesen/Mesen2)** - Gold standard for accuracy and debugging tools
- **[FCEUX](https://github.com/TASEmulators/fceux)** - TAS tools, FM2 format, and mapper reference
- **[puNES](https://github.com/punesemu/puNES)** - Comprehensive mapper catalog (461+ implementations)
- **[TetaNES](https://github.com/lukexor/tetanes)** - Rust architecture patterns and wgpu rendering
- **[Pinky](https://github.com/koute/pinky)** - PPU rendering techniques and Visual2C02 test validation
- **[Rustico](https://github.com/zeta0134/rustico)** - Expansion audio implementation patterns
- **[DaveTCode/nes-emulator-rust](https://github.com/DaveTCode/nes-emulator-rust)** - Zero-unsafe Rust patterns

### Community and Resources

- **[NESdev Community](https://www.nesdev.org/)** - Comprehensive hardware documentation and forums
- **[TASVideos](https://tasvideos.org/)** - Accuracy test suite and validation framework
- **[RetroAchievements](https://retroachievements.org/)** - Achievement system integration
- **[blargg](http://blargg.8bitalley.com/)** - Test ROM suites for CPU, PPU, and APU validation
- **[Visual 6502](http://visual6502.org/)** - Die-level CPU simulation and verification
- **[kevtris](https://forums.nesdev.org/memberlist.php?mode=viewprofile&u=5)** - PPU research and Visual2C02

### Contributors

Thank you to all contributors who help make RustyNES better! See the [Contributors page](https://github.com/doublegate/RustyNES/graphs/contributors) for the complete list.

### Funding

Development is currently unfunded and volunteer-driven. If you'd like to support the project:

- Star the repository
- Report bugs and test PRs
- Contribute code or documentation
- Spread the word about RustyNES

---

## License

RustyNES is dual-licensed under your choice of:

- **[MIT License](LICENSE-MIT)** - Permissive, allows commercial use
- **[Apache License 2.0](LICENSE-APACHE)** - Permissive with patent grant

**Why dual license?**

- Maximum compatibility with other projects
- Choose the license that best fits your use case
- Both licenses allow commercial and private use

### Third-Party Licenses

RustyNES uses several open-source libraries with compatible licenses:

- **wgpu** - MIT/Apache-2.0 (graphics)
- **egui** - MIT/Apache-2.0 (GUI)
- **SDL2** - zlib (audio/input)
- **mlua** - MIT (Lua scripting)
- **backroll** - Apache-2.0 (netplay)
- **rcheevos** - MIT (RetroAchievements)

See individual crate `Cargo.toml` files for complete dependency licenses.

---

## Community

### Get Help

- **[GitHub Discussions](https://github.com/doublegate/RustyNES/discussions)** - Ask questions, share ideas
- **[GitHub Issues](https://github.com/doublegate/RustyNES/issues)** - Report bugs, request features
- **[SUPPORT.md](SUPPORT.md)** - Detailed support information

### Stay Updated

- **[GitHub Releases](https://github.com/doublegate/RustyNES/releases)** - New versions and changelogs
- **[CHANGELOG.md](CHANGELOG.md)** - Detailed version history
- **[ROADMAP.md](ROADMAP.md)** - Development plans and milestones

### Related Projects

Explore other NES emulation projects:

- [NESdev Wiki](https://www.nesdev.org/wiki/) - Hardware documentation
- [NESdev Forums](https://forums.nesdev.org/) - Community discussions
- [TASVideos](https://tasvideos.org/) - Tool-assisted speedrun community

---

## Citation

If you use RustyNES in academic research, please cite:

```bibtex
@software{rustynes2025,
  author = {RustyNES Contributors},
  title = {RustyNES: A Next-Generation NES Emulator in Rust},
  year = {2025},
  url = {https://github.com/doublegate/RustyNES},
  note = {Cycle-accurate Nintendo Entertainment System emulator}
}
```

---

<p align="center">
  <strong>Built with Rust. Powered by passion for retro gaming.</strong><br>
  <sub>Preserving video game history, one frame at a time.</sub>
</p>

<p align="center">
  <a href="#quick-start">Get Started</a> •
  <a href="CONTRIBUTING.md">Contribute</a> •
  <a href="docs/">Documentation</a> •
  <a href="https://github.com/doublegate/RustyNES/discussions">Discuss</a>
</p>
