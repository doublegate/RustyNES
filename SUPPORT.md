# Support

Thank you for using RustyNES! This document provides guidance on how to get help and support.

## Getting Help

### Before Asking for Help

1. **Check the Documentation**
   - [README.md](README.md) - Project overview and quick start
   - [docs/](docs/) - Comprehensive documentation
   - [`to-dos/ROADMAP.md`](to-dos/ROADMAP.md) - Current development status (the root `ROADMAP.md` is a pre-1.0 historical snapshot)
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

A: Yes. RustyNES is well past its first stable release — the current release is **v2.6.6 "Chassis"** (the console becomes a MiSTer core -- `sys/` vendored byte-identical against `Template_MiSTer@3ea1134c` (57 files, 0 content differences), a top level, a clock, a palette, video sync and an audio mixer, compiled by Quartus 17.0.2 into a Cyclone V bitstream with 0 errors, timing CLOSED, and a warning count taken from 111 to THREE -- all three inside the vendored framework or Quartus's own megafunction, none of them citing this project's RTL (worst setup +0.363 ns, worst hold +0.245 ns, TNS 0.000 on every clock; the console own clock +13.514 ns at an Fmax of 30.26 MHz against the 21.477272 MHz it needs). The emulation core is unchanged, so the co-simulation suite is an ACCEPTANCE CRITERION rather than a formality -- 87 passed, 0 failed -- and it earned that immediately, because the cartridge memories had to be rewritten: an M10K read is REGISTERED, so 40 KiB of asynchronously-read cartridge was 393,216 registers against roughly 166,000 available, under a comment claiming it inferred block RAM from the source style alone, and the README had stated the correct rule since v2.4.3. Two defects were found only by asking whether the outputs would work on real hardware: the audio would have been a full-scale DC rail, because the mixer output is unipolar with silence at zero and the framework maps unsigned silence to -32768, and two OSD scanline options did nothing because VGA_SL was tied to zero. And a convention enforced by a glob has no error message: sys_top.sdc groups the core clock by matching the hierarchical name pattern *|pll|pll_inst|altera_pll_i|*, so a differently-named PLL matched no group at all and every crossing to the framework audio, HDMI and HPS domains was analysed as synchronous -- -13.901 ns of slack and -422,601 ns of TNS on a design whose Fmax was already above requirement, with the compile succeeding and the Assembler reporting 0 errors and 0 warnings throughout. Built on **v2.6.5 "Muster"** — rung 5 closes — the AccuracyCoin status vector is identical entry for entry across all 146 entries, with 146 of 146 executed on both sides and none NotRun, where the same gate read 5 of 146 at the version's start. A muster is a roll call where every name is called AND answered, which is the two-clause acceptance exactly. Five PPU defects close the last six differing entries and four were invisible to every gate that existed when the version opened: the background shift registers' RELOAD and their shift clock need SEPARATE gates (with one shared gate the serial-in test was not merely failing but ARITHMETICALLY UNREACHABLE, since reload dots are absolute and the reload discards the low seven bits, so a serial-in one can never reach bit 7 on any alignment — and modelling both structures reproduces BOTH measured shifter values); the sprite X counters are NOT gated on rendering, which AccuracyCoin states outright and the ROM that states it passes either way, because it expects no hit at X=254 and a sprite shoved 18 dots right is also off the line; the PPUADDR second-write v-copy is DELAYED, as the wiki says inside the write sequence itself, swept 1 to 4 dots against a control at 8 and 12 that fails; and the pre-render line CLEARS secondary OAM, without which scanline 0 draws what scanline 239 left — no sprite can ever render on scanline 0, because OAM Y is one less than the display row, and a sprite-0 probe over the full 134 M-cycle battery found 24 hits with four of them there; and the octal latch holding across the read dot, which is verified by exactly ONE gate and was unverifiable until the v-copy delay landed, the two composing the hybrid address together and neither producing it alone. A DIAGNOSIS IS RETRACTED: the residual was read as a two-dot CPU/PPU alignment error from comparing dot spans across two instruments, and at the committed alignment the two consoles execute identical pc, bus_addr and bus_access for 1,695,131 cycles while a two-dot shift moves the first fork back to 593,228 and takes the differing share from 5.13% to 66.80%. The oracle changes on the default path, so AccuracyCoin 141/141 (RAM decoder) and nestest 0-diff are VERIFIED, not asserted, on **v2.6.4 "Rubric"** — OAM DMA lands and all nine AccuracyCoin disagreements close, every rule that closed the last three stated by the test ROM and by neither nesdev page — and then the gate that certified them is measured to cover 88 of 146 entries. The emulation core is unchanged, on **v2.6.3 "Mainspring"** — the DUT runs on one master clock, and four enables that were never enabling — plus AccuracyCoin end to end and a status vector that names its disagreements by test. The emulation core is unchanged, on **v2.6.2 "Witness"** — rung 4 closes: blargg APU battery 11/11 on the co-simulation DUT, six defects no self-written gate could see, and a suite that had been asserting nothing for five minor releases. The emulation core is unchanged, on **v2.6.1 "Interleave"** — the DMC and its DMA cycle steal in the MiSTer co-simulation DUT, cycle-exact on the bus. The emulation core is unchanged, on **v2.6.0 "Assay"** — the triangle, the noise channel and the sweep unit **in the MiSTer co-simulation DUT** — and an audit of how much of the APU was fitted to the oracle rather than derived from documentation. The emulation core is unchanged, on **v2.5.9 "Overture"** — rung 4 opens: the two pulse channels, the frame counter, and four ROM defects the stimulus measurement found first, on **v2.5.8 "Blanking"** — VBlank, NMI and the PPUSTATUS race close rung 3 — and both fixes were deletions, on **v2.5.7 "Collimation"** — sprite rendering closes exact — the phase was wrong by two dots, and every window was compensating, on **v2.5.6 "Vestige"** — Sprite evaluation closes: all 59,993 overlapping cycles match, nine of nine behavioural mutants caught and two proved inert (announced as seven of eight at the cut), and the fix is a byte index that outlives the walk that set it, on **v2.5.5 "Raster"** — the first full frame, and three blind spots in the stimulus that fed it, on **v2.5.4 "Escapement"** — the background fetch pipeline, and an access two dots early that five gates could not see, on **v2.5.3 "Hysteresis"** — toggling rendering takes effect three dots after the write, and four instruments to prove it, on **v2.5.2 "Dormant"** — the 2C02 register file, and a gate that passed while testing nothing, on **v2.5.1 "Retrace"** — the interrupt sweep closes rung 2, and a gate reported a pass it could not have earned, on **v2.5.0 "Rungwork"** — the 6502 rung, and the two gates it cannot reach, on **v2.4.9 "Plumbline II"** — the bus half of rung 2, and what it found the day it existed, on **v2.4.8 "Palimpsest"** — read-modify-write, and a gate that cannot see its own subject, on **v2.4.7 "Keystone"** — the stack closes, and a dead line proves itself dead, on **v2.4.6 "Abacus"** — the core learns arithmetic, on **v2.4.5 "Compass"** — the core reaches memory, and chooses, on **v2.4.4 "Ignition"** — the first real RTL of the co-simulation programme, on v2.4.3 "Touchstone", the two Fabric risks settled before any RTL, on v2.4.2 "Cairn", the rung-0 compare surface of the v2.4.1 → v2.5.0 "Fabric" line, on v2.4.1 "Fabric" and the never-tagged v2.4.0 "Concordance", atop the v2.0.0 "Timebase" one-clock scheduler base), a complete, playable desktop application plus native Android / iOS / Libretro builds and a browser build. See [`to-dos/ROADMAP.md`](to-dos/ROADMAP.md) for what shipped and the forward directions.

**Q: How accurate is RustyNES?**

A: AccuracyCoin 100% (141/141) — every assigned test passes, including the two newest upstream PPU tests ("ALE + Read", "Hybrid Addresses"), which the v2.0.3 2-cycle-ALE PPU-fetch promotion closed — `nestest` 0-diff, and the blargg / kevtris suites green, validated by a byte-identical commercial-ROM regression oracle. See [docs/STATUS.md](docs/STATUS.md) for the authoritative pass-count matrix.

**Q: How can I contribute?**

A: See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines. We welcome code, documentation, testing, and design contributions.

**Q: Is RustyNES open source?**

A: Yes! RustyNES is licensed under GPL-3.0-or-later. You're free to use, modify, and distribute it under the terms of that license (including making source available for derivatives).

### Technical Questions

**Q: What platforms are supported?**

A: Native Windows, Linux, and macOS, plus a WebAssembly / GitHub Pages browser build — all from one `winit` + `wgpu` + `cpal` + `egui` frontend.

**Q: What ROMs are supported?**

A: iNES and NES 2.0 ROM formats across **174 mapper families** (including expansion audio) plus the UNIF (`.unf`) container, the Famicom Disk System (real-BIOS boot), and Vs. System / PlayChoice-10 arcade hardware. Additional mapper families are added demand-driven; see [`to-dos/ROADMAP.md`](to-dos/ROADMAP.md).

**Q: Does RustyNES support [feature]?**

A: The feature set includes rollback netplay (2–4 players), RetroAchievements (opt-in), TAS movie record/playback, save-states, rewind, run-ahead, Game Genie + raw-RAM cheats, an egui debugger, Lua scripting, a TAS editor, HD packs, and shader/NTSC filters — plus native Android / iOS / Libretro builds. Check [`to-dos/ROADMAP.md`](to-dos/ROADMAP.md) for delivered milestones and forward directions.

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
