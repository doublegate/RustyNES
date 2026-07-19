# Support

Thank you for using RustyNES! This document provides guidance on how to get help and support.

## Getting Help

### Before Asking for Help

1. **Check the Documentation**
   - [README.md](README.md) - Project overview and quick start
   - [docs/](docs/) - Comprehensive documentation
   - [ROADMAP.md](ROADMAP.md) - Current development status
   - [FAQ](#frequently-asked-questions) - Common questions (below)

2. **Search Existing Resources**
   - [GitHub Issues](https://github.com/doublegate/RustyNES/issues) - Known bugs and feature requests
   - [GitHub Discussions](https://github.com/doublegate/RustyNES/discussions) - Community Q&A
   - [Closed Issues](https://github.com/doublegate/RustyNES/issues?q=is%3Aissue+is%3Aclosed) - Previously resolved issues

3. **Verify You Have the Latest Version**

   ```bash
   git pull origin main
   cargo build --release --workspace
   ```

---

## Support Channels

### GitHub Discussions (Recommended)

For questions, ideas, and general discussion:

[Start a Discussion](https://github.com/doublegate/RustyNES/discussions)

**Use discussions for:**

- General questions about RustyNES
- Usage help ("How do I...?")
- Feature ideas and brainstorming
- Showing off your projects using RustyNES
- Community chat

**Categories:**

- **Q&A**: Ask questions and get answers
- **Ideas**: Share feature ideas and enhancements
- **Show and Tell**: Share your projects and screenshots
- **General**: Everything else

### GitHub Issues

For bug reports and concrete feature requests:

[Create an Issue](https://github.com/doublegate/RustyNES/issues/new/choose)

**Use issues for:**

- Bug reports (crashes, incorrect behavior)
- Specific feature requests
- Documentation errors
- Build problems

**Do NOT use issues for:**

- General questions (use Discussions)
- Support requests (use Discussions)
- Vague ideas (use Discussions first)

### Community Resources

**NESdev Community** (for general NES emulation questions):

- [NESdev Wiki](https://www.nesdev.org/wiki/)
- [NESdev Forums](https://forums.nesdev.org/)
- [NESdev Discord](https://discord.gg/nesdev)

**Rust Community** (for Rust language questions):

- [Rust Users Forum](https://users.rust-lang.org/)
- [Rust Discord](https://discord.gg/rust-lang)
- [r/rust on Reddit](https://www.reddit.com/r/rust/)

---

## Frequently Asked Questions

### General Questions

**Q: What is RustyNES?**

A: RustyNES is a cycle-accurate NES emulator written in pure Rust, clearing the Mesen2 / higan / ares accuracy bar, with advanced features like netplay, TAS tools, and RetroAchievements.

**Q: Can I use RustyNES now?**

A: Yes. RustyNES is well past its first stable release — the current release is **v2.0.4 "Harbor"** (the head of the v2.0.x mobile-finalization train atop the v2.0.0 "Timebase" one-clock scheduler rewrite), a complete, playable desktop application plus native Android / iOS / Libretro builds and a browser build. See [ROADMAP.md](ROADMAP.md) for what shipped and the forward directions.

**Q: How accurate is RustyNES?**

A: AccuracyCoin 100% (141/141) — every assigned test passes, including the two newest upstream PPU tests ("ALE + Read", "Hybrid Addresses"), which the v2.0.3 2-cycle-ALE PPU-fetch promotion closed — `nestest` 0-diff, and the blargg / kevtris suites green, validated by a byte-identical commercial-ROM regression oracle. See [docs/STATUS.md](docs/STATUS.md) for the authoritative pass-count matrix.

**Q: How can I contribute?**

A: See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines. We welcome code, documentation, testing, and design contributions.

**Q: Is RustyNES open source?**

A: Yes! RustyNES is dual-licensed under MIT/Apache-2.0. You're free to use, modify, and distribute it according to those licenses.

### Technical Questions

**Q: What platforms are supported?**

A: Native Windows, Linux, and macOS, plus a WebAssembly / GitHub Pages browser build — all from one `winit` + `wgpu` + `cpal` + `egui` frontend.

**Q: What ROMs are supported?**

A: iNES and NES 2.0 ROM formats across **51 mapper families** (including expansion audio), the Famicom Disk System (real-BIOS boot), and Vs. System / PlayChoice-10 arcade hardware. Additional mapper families are added demand-driven; see [ROADMAP.md](ROADMAP.md).

**Q: Does RustyNES support [feature]?**

A: The feature set includes rollback netplay (2–4 players), RetroAchievements (opt-in), TAS movie record/playback, save-states, rewind, run-ahead, Game Genie + raw-RAM cheats, an egui debugger, Lua scripting, a TAS editor, HD packs, and shader/NTSC filters — plus native Android / iOS / Libretro builds. Check the [ROADMAP.md](ROADMAP.md) for delivered milestones and forward directions.

**Q: Can I embed RustyNES in my project?**

A: Yes! The `rustynes-core` crate is designed to be embeddable. See the `rustynes-core` rustdoc (`cargo doc -p rustynes-core --open`) for the library API.

### Build and Installation

**Q: How do I build RustyNES?**

A: See [docs/dev/BUILD.md](docs/dev/BUILD.md) for detailed build instructions. Quick start:

```bash
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES
cargo build --release --workspace
```

**Q: What are the prerequisites?**

A: Rust 1.96 (pinned in `rust-toolchain.toml`; `rustup` auto-installs it) and the `winit` + `wgpu` + `cpal` system libraries (libxkbcommon / wayland / alsa / udev on Linux; nothing extra on macOS/Windows). See [docs/dev/BUILD.md](docs/dev/BUILD.md) for platform-specific instructions.

**Q: Build is failing, what do I do?**

A:

1. Ensure you have Rust 1.96 or newer: `rustc --version`
2. Install the frontend system libraries (see [BUILD.md](docs/dev/BUILD.md))
3. Try a clean build: `cargo clean && cargo build --workspace`
4. Check [GitHub Issues](https://github.com/doublegate/RustyNES/issues) for known build problems
5. Ask for help in [Discussions](https://github.com/doublegate/RustyNES/discussions)

**Q: Can I use RustyNES on [my platform]?**

A: Check the [Platform Support](README.md#platform-support) section in the README. If your platform isn't listed, ask in Discussions about porting feasibility.

### Usage Questions

**Q: How do I load a ROM?**

A: `cargo run --release -p rustynes-frontend -- path/to/rom.nes` (binary: `rustynes`), or launch with no ROM and use the File menu / F12 / drag-and-drop.

**Q: What are the default controls?**

A: See the [Controls Table](README.md#default-controls) in the README. Controls will be configurable in the settings.

**Q: Where are save files stored?**

A: Save files are stored in platform-specific directories following OS conventions. See [the save-states guide](docs/user-guide/save-states-and-rewind.md) for details.

**Q: Can I use a gamepad?**

A: Yes. USB gamepads auto-bind to player 1 (Xbox-style: South = A, West = B, Start, Back = Select, D-Pad) and are rebindable. Most standard controllers (Xbox, PlayStation, Switch Pro, etc.) work.

### Development Questions

**Q: How is the codebase structured?**

A: RustyNES is a Cargo workspace of `rustynes-*` crates (cpu / ppu / apu / mappers / core / frontend, plus netplay / cheevos / test-harness). See [ARCHITECTURE.md](ARCHITECTURE.md) for the complete architecture overview.

**Q: Where do I start if I want to contribute?**

A:

1. Read [CONTRIBUTING.md](CONTRIBUTING.md)
2. Check [good first issue](https://github.com/doublegate/RustyNES/labels/good%20first%20issue) labels
3. Ask in [Discussions](https://github.com/doublegate/RustyNES/discussions) what needs help

**Q: What coding standards does RustyNES follow?**

A: See [docs/dev/STYLE_GUIDE.md](docs/dev/STYLE_GUIDE.md) for detailed style guidelines. TL;DR: `cargo fmt` and `cargo clippy -- -D warnings`.

**Q: How do I run tests?**

A: See [docs/dev/TESTING.md](docs/dev/TESTING.md) for the complete testing guide. Quick start: `cargo test --workspace`

**Q: Where can I find reference documentation?**

A: The `docs/` folder contains comprehensive documentation covering CPU, PPU, APU, mappers, testing, and more. Start with [docs/DOCUMENTATION_INDEX.md](docs/DOCUMENTATION_INDEX.md).

---

## Reporting Issues

### Bug Reports

If you've found a bug, please [create an issue](https://github.com/doublegate/RustyNES/issues/new?template=bug_report.md) with:

- Clear description of the bug
- Steps to reproduce
- Expected vs. actual behavior
- System information
- ROM information (if applicable)
- Logs/screenshots

See the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md) for the complete format.

### Feature Requests

For feature requests, please [create an issue](https://github.com/doublegate/RustyNES/issues/new?template=feature_request.md) with:

- Clear description of the feature
- Problem it solves
- Proposed solution
- Use cases
- Impact analysis

See the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md) for the complete format.

---

## Response Times

This is a volunteer-driven project. Please be patient while waiting for responses:

- **Critical bugs**: 1-3 days
- **Bug reports**: 3-7 days
- **Feature requests**: 1-2 weeks
- **Questions in Discussions**: 1-7 days (community may respond faster)
- **Pull requests**: 3-14 days

---

## Code of Conduct

All community interactions must follow our [Code of Conduct](CODE_OF_CONDUCT.md). Please be respectful, constructive, and welcoming.

### Reporting Code of Conduct Violations

Report violations privately to: <parobek@gmail.com>

---

## Additional Resources

### Documentation

| Document | Description |
|----------|-------------|
| [README.md](README.md) | Project overview |
| [OVERVIEW.md](OVERVIEW.md) | Philosophy and goals |
| [ARCHITECTURE.md](ARCHITECTURE.md) | System design |
| [ROADMAP.md](ROADMAP.md) | Development plan |
| [docs/](docs/) | Complete documentation |

### External Resources

| Resource | Link |
|----------|------|
| **NESdev Wiki** | <https://www.nesdev.org/wiki/> |
| **NESdev Forums** | <https://forums.nesdev.org/> |
| **6502 Reference** | <https://www.nesdev.org/obelisk-6502-guide/> |
| **TASVideos** | <https://tasvideos.org/> |
| **RetroAchievements** | <https://retroachievements.org/> |

### Related Projects

RustyNES draws inspiration from:

- [Mesen2](https://github.com/SourMesen/Mesen2) - Accuracy and debugging
- [FCEUX](https://github.com/TASEmulators/fceux) - TAS tools
- [puNES](https://github.com/punesemu/puNES) - Mapper coverage
- [TetaNES](https://github.com/lukexor/tetanes) - Rust implementation
- [Pinky](https://github.com/koute/pinky) - PPU rendering

---

## Contact

- **GitHub Issues**: [Bug reports and feature requests](https://github.com/doublegate/RustyNES/issues)
- **GitHub Discussions**: [Questions and community chat](https://github.com/doublegate/RustyNES/discussions)
- **Email**: <parobek@gmail.com> (for security issues and private matters only)

---

**Thank you for using RustyNES! We're excited to have you in the community.**
