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

A: Yes. RustyNES is well past its first stable release — the current release is **v2.6.12 "Groundwork"** (the bitstream was an NROM-only console. Rung 7 landed five mapper families and 142 co-simulation gates verify them, and the layer that turns that RTL into a bitstream was never told: `rtl/emu.sv` left `cart_mapper`, `cart_prg_16k_banks` and `cart_chr_8k_banks` unconnected, so Quartus tied all three to GND -- mapper 0 for EVERY cartridge, `prg_8k_count = 0` collapsing PRG to an 8 KiB window, and CHR forced to RAM. The declared 256 KiB PRG and 128 KiB CHR were implemented as 8 KiB each; connecting three wires takes block memory from 666,061 to 3,680,717 bits and timing still closes at all four corners. NOTHING COULD HAVE CAUGHT IT: simulation cannot, because `emu.sv` is not in the testbench file list and the harness drives those ports itself, so all 142 gates exercised a correctly-configured cartridge; and Quartus DID say so three times, in messages that cite an INSTANCE path rather than a file and are absent from the "0 errors, N warnings" tally, so the existing checker read 0 of 125. Two gates close that -- one fails on an unconnected pin of any module this repository declares, the other pins the warning SET rather than its count -- and both are demonstrated to fail by mutation. The `hps_io` tie-off audit that followed raised nine `T-MISTER-*` tickets, and annotating each with a blocker turned "none of these landed" into a measurement: not one is blocked on EFFORT, so the list is the rung-6 agenda rather than a backlog. `T-MISTER-SAVE` was attempted and refuted -- every save route terminates in `hps_io`, which no gate here instantiates. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction, on **v2.6.11 "Exposure"** — a picture is a gate the ladder did not have. All 141 co-simulation gates THEN IN THE SUITE were green (it ends this release at 142, the one it added) and TWO OF SIX commercial games rendered wrong -- a CHR-RAM write was taking the shared-pin composite address built for FETCHES instead of `v`, so the layout was right and the tiles were scrambled. The split is exactly CHR-ROM against CHR-RAM, which named the mechanism before any tracing, and a CONTROL says it is not v2.6.10's regression: the pre-M10K-fix RTL differs by the IDENTICAL 16,565 pixels, so the defect dates from the cartridge landing in v2.6.9. It was not UNREACHED -- the DUT asserts `chr_wr` 9,600 times in the Battletoads run -- it was UNCOMPARED: only THREE of the 141 gates compare a framebuffer, all three ship CHR-ROM, every other gate is CPU-side, and AccuracyCoin, the widest gate in the suite, is CHR-ROM too. The rung-7 gates' own comment says what they are for -- "these gates are about BANKING and nothing else" -- and it was accurate, and it was the whole coverage. Six commercial titles now render byte-identically to the oracle over all 61,440 pixels, published as a montage built by a script that REFUSES to publish a tile that differs from the oracle. The v2.6.10 bitstream carries the defect and a published version is immutable, so the corrected `.rbf` ships here. The same release finds EIGHT release leads describing v2.6.10 with v2.6.9's summary, and v2.6.9 gone from the lineage entirely, with `docs/STATUS.md` naming v2.6.10 under the codename "Abeyance" -- every existing check passing CORRECTLY, because they pin the version TOKEN and the token was right. Prose cannot be audited; an ORDERING can, so two gates are added and both are demonstrated to fail by mutation. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed, on **v2.6.10 "Inference"** — the cartridge meets the synthesiser -- five boards verified across 141 co-simulation gates had never been through Quartus, and `chr` written from TWO `always_ff` blocks cannot infer as one M10K, so 128 KB of CHR stayed in flip-flops at 1,048,576 registers against roughly 166,000; simulation cannot ask this question. The fitter was also throttling itself under Auto Fit, and at full effort all six seeds close where two had failed. Built on **v2.6.9 "Abeyance"**, an exclusion hides improvement as well as regression, and both denied co-simulation streams close. The larger one was never the console: `apuconflict039` had been carried for seven releases as a declared diagnostic whose bus surface "carries nine divergences BY DESIGN", and the nine were a defect in the HARNESS -- on a cycle the CPU is held, the testbench built its record's bus data from a stale local rather than from the RTL's own latch. Taking it from the latch makes the stream IDENTICAL on all 357,361 overlapping cycles and all 88 checkpoints, and the local is now dead and deleted. The phrase "by design" is what stopped anyone re-checking it, because it reads as a property of the thing under test when it was a property of the instrument reading it. The other stream differs on EXACTLY ONE cycle, a documented and attributed OAM-corruption asymmetry -- and carrying that needed an instrument the suite did not have, because the PLANNED mechanism was refuted by its own mutation pass: an allowance by checkpoint index cannot work on a rolling hash, since one divergent cycle poisons every checkpoint after it, so allowing the first differing window simply moved the failure to the next one and allowing the rest is the all-or-nothing deny it was meant to replace. A per-cycle nine-field comparator with a scoped allowance costs ONE cycle of coverage instead of seventy-one checkpoints -- 357,360 of 357,361, against nothing at all before -- and it fails BOTH ways, so a DUT that improves cannot leave a stale allowance quietly hiding coverage; six mutations confirm it, including a cycle outside the compared window being REFUSED rather than allowed to match nothing. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction and were re-run anyway. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed. Built on **v2.6.8 "Arrears"** — a deny list is an assertion about the thing under test and nobody re-measured it -- four of six denied co-simulation streams were already passing, three of them never run by the suite at all, and the nestest gate widened 19x to all 5,062,680 cycles, closing caveat C4 by demonstration. Built on **v2.6.7 "Detent"** — the bitstream becomes a published release artifact and a one-cycle disagreement is pinned to the cycle it happens on. Every release from here ships a `.rbf` -- committed to the sibling's `releases/` and attached to the GitHub release on BOTH repositories -- reversing v2.6.6, which produced one and withheld it because no hardware had run it: the MiSTer distribution mechanism reads that path out of the REPOSITORY, so an empty `releases/` describes an undistributable core rather than a cautious one, and the caution moves from an absence into a disclosure naming what the ladder cannot reach by construction (the PPU gate compares the pre-palette index and the APU gate per-channel integer levels, so the palette, the video timing constants, the audio absolute level and its band-limiting all sit downstream of every gate). The build is REPRODUCIBLE and that is now measured rather than argued -- a from-scratch compile and an incremental one produce a byte-identical bitstream -- which is also how v2.6.6's published slack figures came to be WITHDRAWN: no corner of a clean rebuild reproduces them, the innocent explanation (a different timing corner) was checked first and refuted, and the correct pair is +0.108 ns setup and +0.042 ns hold at the binding corner. THE RELEASE GATE WAS READING THE WRONG CORNER -- Slow 100C is not the binding one on this design, so a bitstream failing at Slow -40C would have passed while the gate reported three times the real margin -- and the checker that reads it was wrong twice before mutation found both: it first extracted ZERO rows from both summary tables and reported that as "no negative slack", then, once fixed, reported FOURTEEN clocks from a report emptied of its data, having run past the closing rule into the next tables. Caveat C2 splits in two. The first residual was a TRACE OBSERVATION POINT -- the harness built its record after eight of a CPU cycle's twelve master clocks while the oracle reads at end-of-cycle, and the frame-counter interrupt asserts on the final edge -- and closing it took checkpoint comparisons from 3 to 11 and failures at one checkpoint from 30 to 3. The second is REAL, and the FIRST fix for it was REFUTED in a way that found the right one: moving all four effects of the write one cycle later to match the oracle drops blargg from 11/11 to 4 of 11, one of the seven being the ROM written to probe exactly that timing. Read as a measurement that PROVES the sequencer's maturation is correctly placed, which leaves only the other effects the same write schedules -- so separating ONLY the interrupt clear lands it at write+3, the documented cycle, while the frame counter's zeroing stays put. Checkpoint streams go from 11 identical to 52 of 58 and the suite from 87 to 122, with blargg still 11/11 and the bus still matching on all 2,680,239 overlapping cycles. The checkpoint gate is registered over 51 comparisons and STATES ITS BLIND SPOT: not one gated golden ever raises an NMI, so it cannot catch an nmi_line defect, and the attempt to close that hole found a FOURTH divergence cluster that three goldens had been hiding inside a "26 skipped" tally line. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed. Built on **v2.6.6 "Chassis"** — the console becomes a MiSTer core -- `sys/` vendored byte-identical against `Template_MiSTer@3ea1134c` (57 files, 0 content differences), a top level, a clock, a palette, video sync and an audio mixer, compiled by Quartus 17.0.2 into a Cyclone V bitstream with 0 errors, timing CLOSED, and a warning count taken from 111 to THREE -- all three inside the vendored framework or Quartus's own megafunction, none of them citing this project's RTL (worst setup +0.086 ns and worst hold +0.096 ns at the binding corners, seed 3 -- v2.6.7 also withdraws the +0.363/+0.245 v2.6.6 published, which a clean rebuild of that configuration reproduces at no corner, TNS 0.000 on every clock; the console own clock +13.514 ns at an Fmax of 30.26 MHz against the 21.477272 MHz it needs). The emulation core is unchanged, so the co-simulation suite is an ACCEPTANCE CRITERION rather than a formality -- 87 passed, 0 failed -- and it earned that immediately, because the cartridge memories had to be rewritten: an M10K read is REGISTERED, so 40 KiB of asynchronously-read cartridge was 393,216 registers against roughly 166,000 available, under a comment claiming it inferred block RAM from the source style alone, and the README had stated the correct rule since v2.4.3. Two defects were found only by asking whether the outputs would work on real hardware: the audio would have been a full-scale DC rail, because the mixer output is unipolar with silence at zero and the framework maps unsigned silence to -32768, and two OSD scanline options did nothing because VGA_SL was tied to zero. And a convention enforced by a glob has no error message: sys_top.sdc groups the core clock by matching the hierarchical name pattern *|pll|pll_inst|altera_pll_i|*, so a differently-named PLL matched no group at all and every crossing to the framework audio, HDMI and HPS domains was analysed as synchronous -- -13.901 ns of slack and -422,601 ns of TNS on a design whose Fmax was already above requirement, with the compile succeeding and the Assembler reporting 0 errors and 0 warnings throughout. Built on **v2.6.5 "Muster"** — rung 5 closes — the AccuracyCoin status vector is identical entry for entry across all 146 entries, with 146 of 146 executed on both sides and none NotRun, where the same gate read 5 of 146 at the version's start. A muster is a roll call where every name is called AND answered, which is the two-clause acceptance exactly. Five PPU defects close the last six differing entries and four were invisible to every gate that existed when the version opened: the background shift registers' RELOAD and their shift clock need SEPARATE gates (with one shared gate the serial-in test was not merely failing but ARITHMETICALLY UNREACHABLE, since reload dots are absolute and the reload discards the low seven bits, so a serial-in one can never reach bit 7 on any alignment — and modelling both structures reproduces BOTH measured shifter values); the sprite X counters are NOT gated on rendering, which AccuracyCoin states outright and the ROM that states it passes either way, because it expects no hit at X=254 and a sprite shoved 18 dots right is also off the line; the PPUADDR second-write v-copy is DELAYED, as the wiki says inside the write sequence itself, swept 1 to 4 dots against a control at 8 and 12 that fails; and the pre-render line CLEARS secondary OAM, without which scanline 0 draws what scanline 239 left — no sprite can ever render on scanline 0, because OAM Y is one less than the display row, and a sprite-0 probe over the full 134 M-cycle battery found 24 hits with four of them there; and the octal latch holding across the read dot, which is verified by exactly ONE gate and was unverifiable until the v-copy delay landed, the two composing the hybrid address together and neither producing it alone. A DIAGNOSIS IS RETRACTED: the residual was read as a two-dot CPU/PPU alignment error from comparing dot spans across two instruments, and at the committed alignment the two consoles execute identical pc, bus_addr and bus_access for 1,695,131 cycles while a two-dot shift moves the first fork back to 593,228 and takes the differing share from 5.13% to 66.80%. The oracle changes on the default path, so AccuracyCoin 141/141 (RAM decoder) and nestest 0-diff are VERIFIED, not asserted, on **v2.6.4 "Rubric"** — OAM DMA lands and all nine AccuracyCoin disagreements close, every rule that closed the last three stated by the test ROM and by neither nesdev page — and then the gate that certified them is measured to cover 88 of 146 entries. The emulation core is unchanged, on **v2.6.3 "Mainspring"** — the DUT runs on one master clock, and four enables that were never enabling — plus AccuracyCoin end to end and a status vector that names its disagreements by test. The emulation core is unchanged, on **v2.6.2 "Witness"** — rung 4 closes: blargg APU battery 11/11 on the co-simulation DUT, six defects no self-written gate could see, and a suite that had been asserting nothing for five minor releases. The emulation core is unchanged, on **v2.6.1 "Interleave"** — the DMC and its DMA cycle steal in the MiSTer co-simulation DUT, cycle-exact on the bus. The emulation core is unchanged, on **v2.6.0 "Assay"** — the triangle, the noise channel and the sweep unit **in the MiSTer co-simulation DUT** — and an audit of how much of the APU was fitted to the oracle rather than derived from documentation. The emulation core is unchanged, on **v2.5.9 "Overture"** — rung 4 opens: the two pulse channels, the frame counter, and four ROM defects the stimulus measurement found first, on **v2.5.8 "Blanking"** — VBlank, NMI and the PPUSTATUS race close rung 3 — and both fixes were deletions, on **v2.5.7 "Collimation"** — sprite rendering closes exact — the phase was wrong by two dots, and every window was compensating, on **v2.5.6 "Vestige"** — Sprite evaluation closes: all 59,993 overlapping cycles match, nine of nine behavioural mutants caught and two proved inert (announced as seven of eight at the cut), and the fix is a byte index that outlives the walk that set it, on **v2.5.5 "Raster"** — the first full frame, and three blind spots in the stimulus that fed it, on **v2.5.4 "Escapement"** — the background fetch pipeline, and an access two dots early that five gates could not see, on **v2.5.3 "Hysteresis"** — toggling rendering takes effect three dots after the write, and four instruments to prove it, on **v2.5.2 "Dormant"** — the 2C02 register file, and a gate that passed while testing nothing, on **v2.5.1 "Retrace"** — the interrupt sweep closes rung 2, and a gate reported a pass it could not have earned, on **v2.5.0 "Rungwork"** — the 6502 rung, and the two gates it cannot reach, on **v2.4.9 "Plumbline II"** — the bus half of rung 2, and what it found the day it existed, on **v2.4.8 "Palimpsest"** — read-modify-write, and a gate that cannot see its own subject, on **v2.4.7 "Keystone"** — the stack closes, and a dead line proves itself dead, on **v2.4.6 "Abacus"** — the core learns arithmetic, on **v2.4.5 "Compass"** — the core reaches memory, and chooses, on **v2.4.4 "Ignition"** — the first real RTL of the co-simulation programme, on v2.4.3 "Touchstone", the two Fabric risks settled before any RTL, on v2.4.2 "Cairn", the rung-0 compare surface of the v2.4.1 → v2.5.0 "Fabric" line, on v2.4.1 "Fabric" and the never-tagged v2.4.0 "Concordance", atop the v2.0.0 "Timebase" one-clock scheduler base), a complete, playable desktop application plus native Android / iOS / Libretro builds and a browser build. See [`to-dos/ROADMAP.md`](to-dos/ROADMAP.md) for what shipped and the forward directions.

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
