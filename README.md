# RustyNES

[![Build Status](https://github.com/yourusername/rustynes/workflows/CI/badge.svg)](https://github.com/yourusername/rustynes/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#platform-support)

**A next-generation NES emulator written in pure Rust**

---

> **Status:** Architecture complete, implementation in progress. See [ROADMAP.md](ROADMAP.md) for development timeline.

---

## Highlights

| Feature | Description |
|---------|-------------|
| **Cycle-Accurate** | Sub-cycle precision for CPU, PPU, and APU - targeting 100% TASVideos test suite pass rate |
| **300+ Mappers** | Comprehensive cartridge support covering all licensed games plus homebrew |
| **RetroAchievements** | Native rcheevos integration for achievement hunting |
| **GGPO Netplay** | Frame-perfect rollback netcode via backroll-rs |
| **TAS Tools** | FM2 format support with rewind, frame advance, and movie recording |
| **Lua Scripting** | Modern Lua 5.4 scripting via mlua for automation and bots |
| **GPU Accelerated** | Cross-platform wgpu rendering with shader support |
| **Pure Rust** | Zero unsafe code where possible, leveraging Rust's safety guarantees |

---

## Quick Start

```bash
# Clone the repository
git clone https://github.com/yourusername/rustynes.git
cd rustynes

# Build and run
cargo build --release
cargo run --release -p rustynes-desktop

# Run with a ROM
cargo run --release -p rustynes-desktop -- path/to/rom.nes
```

### Prerequisites

- **Rust 1.75+** ([rustup.rs](https://rustup.rs))
- **SDL2** development libraries (see [Build Documentation](docs/dev/BUILD.md))

---

## Default Controls

| NES | Keyboard | Gamepad |
|-----|----------|---------|
| D-Pad | WASD / Arrows | D-Pad |
| A | K / Z | A Button |
| B | J / X | B Button |
| Start | Enter | Start |
| Select | Right Shift | Select |

---

## Features

### MVP (Phase 1)
- [ ] Cycle-accurate 6502/2A03 CPU emulation
- [ ] Dot-level 2C02 PPU rendering
- [ ] Hardware-accurate 2A03 APU synthesis
- [ ] Mappers 0, 1, 2, 3, 4 (80% game coverage)
- [ ] Cross-platform GUI (egui + wgpu)
- [ ] Save states and battery saves
- [ ] Gamepad support (SDL2)

### Planned (Phases 2-4)
- [ ] RetroAchievements integration
- [ ] GGPO-style rollback netplay
- [ ] Lua 5.4 scripting
- [ ] TAS recording/playback
- [ ] Integrated debugger
- [ ] Rewind and fast-forward
- [ ] WebAssembly build
- [ ] CRT/NTSC shaders

See [ROADMAP.md](ROADMAP.md) for the complete development plan.

---

## Platform Support

| Platform | Status |
|----------|--------|
| **Windows x64** | Primary |
| **Linux x64** | Primary |
| **macOS x64/ARM64** | Primary |
| **WebAssembly** | Planned |
| **Linux ARM64** | Planned |

### System Requirements

**Minimum:** 1.5 GHz dual-core, 512 MB RAM, OpenGL 3.3
**Recommended:** 2.0 GHz quad-core, 2 GB RAM, Vulkan/Metal/DX12 GPU

---

## Documentation

| Document | Description |
|----------|-------------|
| [OVERVIEW.md](OVERVIEW.md) | Philosophy, accuracy goals, emulation approach |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design, component relationships, Rust patterns |
| [ROADMAP.md](ROADMAP.md) | Development phases and milestones |

### Hardware Documentation

| Component | Location |
|-----------|----------|
| **CPU (6502)** | [docs/cpu/](docs/cpu/) - Instruction set, timing, unofficial opcodes |
| **PPU (2C02)** | [docs/ppu/](docs/ppu/) - Rendering, scrolling, sprite evaluation |
| **APU (2A03)** | [docs/apu/](docs/apu/) - Audio channels, mixing, frame counter |
| **Memory Bus** | [docs/bus/](docs/bus/) - Address space, bus conflicts |
| **Mappers** | [docs/mappers/](docs/mappers/) - Cartridge banking and variants |

### Development Guides

| Guide | Purpose |
|-------|---------|
| [CONTRIBUTING](docs/dev/CONTRIBUTING.md) | Code style and PR process |
| [BUILD](docs/dev/BUILD.md) | Toolchain and cross-compilation |
| [TESTING](docs/dev/TESTING.md) | Test ROM suites and CI |
| [DEBUGGING](docs/dev/DEBUGGING.md) | Built-in debugger usage |
| [GLOSSARY](docs/dev/GLOSSARY.md) | NES terminology reference |

### API Reference

| API | Description |
|-----|-------------|
| [CORE_API](docs/api/CORE_API.md) | Embedding the emulator as a library |
| [SAVE_STATES](docs/api/SAVE_STATES.md) | State serialization format |
| [CONFIGURATION](docs/api/CONFIGURATION.md) | Runtime options and settings |

---

## Building from Source

```bash
# Debug build (faster compilation)
cargo build --workspace

# Release build (optimized)
cargo build --workspace --release

# Run tests
cargo test --workspace

# Run lints
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all
```

### Platform Dependencies

**Ubuntu/Debian:**
```bash
sudo apt install build-essential cmake libsdl2-dev
```

**Fedora:**
```bash
sudo dnf install gcc cmake SDL2-devel
```

**macOS:**
```bash
brew install cmake sdl2
```

**Windows:** Install Visual Studio 2019+ with C++ tools, download SDL2 from [libsdl.org](https://libsdl.org)

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](docs/dev/CONTRIBUTING.md) for guidelines.

```bash
# Quick contribution workflow
git checkout -b feature/my-feature
# Make changes...
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
git commit -m "feat: add my feature"
git push origin feature/my-feature
# Open Pull Request
```

---

## Acknowledgments

RustyNES draws inspiration from these excellent projects:

- **Mesen2** - Gold standard accuracy and debugging
- **FCEUX** - TAS tools and FM2 format
- **puNES** - Extensive mapper implementations
- **TetaNES** - Rust patterns and wgpu rendering
- **Pinky** - PPU rendering techniques
- **NesDev Community** - Comprehensive hardware documentation

---

## License

Dual-licensed under your choice of:

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

---

## Community

- [GitHub Issues](https://github.com/yourusername/rustynes/issues) - Bug reports and feature requests
- [GitHub Discussions](https://github.com/yourusername/rustynes/discussions) - Questions and ideas

---

<p align="center">
  <strong>Built with Rust. Powered by passion for retro gaming.</strong>
</p>
