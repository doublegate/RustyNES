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
   cargo build --release
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

A: RustyNES is a next-generation NES emulator written in Rust, targeting 100% accuracy with advanced features like netplay, TAS tools, and RetroAchievements.

**Q: Can I use RustyNES now?**

A: RustyNES is currently in pre-implementation status. Architecture and documentation are complete, but the core emulation is not yet implemented. See [ROADMAP.md](ROADMAP.md) for the development timeline.

**Q: When will RustyNES be ready?**

A: The MVP (basic playable emulator) is targeted for June 2026. Full feature completion is planned for December 2027. See the [development roadmap](ROADMAP.md) for detailed milestones.

**Q: How can I contribute?**

A: See [CONTRIBUTING.md](docs/dev/CONTRIBUTING.md) for contribution guidelines. We welcome code, documentation, testing, and design contributions.

**Q: Is RustyNES open source?**

A: Yes! RustyNES is dual-licensed under MIT/Apache-2.0. You're free to use, modify, and distribute it according to those licenses.

### Technical Questions

**Q: What platforms are supported?**

A: Primary support for Windows (x64), Linux (x64), and macOS (x64/ARM64). WebAssembly and Linux ARM64 support planned for later phases.

**Q: What ROMs are supported?**

A: RustyNES will support iNES and NES 2.0 ROM formats. Mappers 0-4 in Phase 1 (80% of games), expanding to 300+ mappers by Phase 4.

**Q: How accurate is RustyNES?**

A: The goal is 100% accuracy against the TASVideos test suite. Implementation targets cycle-accurate CPU, dot-level PPU, and hardware-accurate APU.

**Q: Does RustyNES support [feature]?**

A: Check the [ROADMAP.md](ROADMAP.md) to see planned features and their implementation phases:

- **Phase 1** (MVP): Basic emulation, mappers 0-4, save states
- **Phase 2** (Features): RetroAchievements, netplay, TAS tools, Lua scripting
- **Phase 3** (Expansion): WebAssembly, expansion audio, more mappers
- **Phase 4** (Polish): CRT filters, enhanced debugger

**Q: Can I embed RustyNES in my project?**

A: Yes! The `rustynes-core` crate is designed to be embeddable. See [docs/api/CORE_API.md](docs/api/CORE_API.md) for the library API (once implemented).

### Build and Installation

**Q: How do I build RustyNES?**

A: See [docs/dev/BUILD.md](docs/dev/BUILD.md) for detailed build instructions. Quick start:

```bash
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES
cargo build --release
```

**Q: What are the prerequisites?**

A: Rust 1.86+ and SDL2 development libraries. See [docs/dev/BUILD.md](docs/dev/BUILD.md) for platform-specific instructions.

**Q: Build is failing, what do I do?**

A:

1. Ensure you have Rust 1.86 or newer: `rustc --version`
2. Install SDL2 development libraries (see [BUILD.md](docs/dev/BUILD.md))
3. Try a clean build: `cargo clean && cargo build`
4. Check [GitHub Issues](https://github.com/doublegate/RustyNES/issues) for known build problems
5. Ask for help in [Discussions](https://github.com/doublegate/RustyNES/discussions)

**Q: Can I use RustyNES on [my platform]?**

A: Check the [Platform Support](README.md#platform-support) section in the README. If your platform isn't listed, ask in Discussions about porting feasibility.

### Usage Questions

**Q: How do I load a ROM?**

A: (Once implemented) `cargo run --release -p rustynes-desktop -- path/to/rom.nes`

**Q: What are the default controls?**

A: See the [Controls Table](README.md#default-controls) in the README. Controls will be configurable in the settings.

**Q: Where are save files stored?**

A: (Once implemented) Save files are stored in platform-specific directories following OS conventions. See [docs/api/SAVE_STATES.md](docs/api/SAVE_STATES.md) for details.

**Q: Can I use a gamepad?**

A: Yes, SDL2 gamepad support is planned for Phase 1. Most standard controllers (Xbox, PlayStation, Switch Pro, etc.) will work.

### Development Questions

**Q: How is the codebase structured?**

A: RustyNES uses a workspace with 10 crates. See [ARCHITECTURE.md](ARCHITECTURE.md) for the complete architecture overview.

**Q: Where do I start if I want to contribute?**

A:

1. Read [CONTRIBUTING.md](docs/dev/CONTRIBUTING.md)
2. Check [good first issue](https://github.com/doublegate/RustyNES/labels/good%20first%20issue) labels
3. Ask in [Discussions](https://github.com/doublegate/RustyNES/discussions) what needs help

**Q: What coding standards does RustyNES follow?**

A: See [docs/dev/STYLE_GUIDE.md](docs/dev/STYLE_GUIDE.md) for detailed style guidelines. TL;DR: `cargo fmt` and `cargo clippy -- -D warnings`.

**Q: How do I run tests?**

A: See [docs/dev/TESTING.md](docs/dev/TESTING.md) for the complete testing guide. Quick start: `cargo test --workspace`

**Q: Where can I find reference documentation?**

A: The `docs/` folder contains 73 comprehensive documentation files covering CPU, PPU, APU, mappers, testing, and more. Start with [docs/DOCUMENTATION_INDEX.md](docs/DOCUMENTATION_INDEX.md).

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
