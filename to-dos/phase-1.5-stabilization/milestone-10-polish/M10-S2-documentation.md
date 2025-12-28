# M10 Sprint 2: Documentation

**Sprint:** M10-S2 (Documentation)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~2-3 days
**Status:** Pending
**Prerequisites:** M10-S0 Complete, M10-S1 Complete
**Updated:** 2025-12-28

---

## Overview

Create comprehensive documentation including user guide, API documentation, developer guide, and FAQ to ensure users and contributors have all necessary information. Update all documentation to reflect the current eframe 0.33 + egui 0.33 stack and Rust 2024 edition.

## Current Implementation (Post M10-S0)

The following documentation infrastructure exists:

**Completed:**
- [x] `docs/` directory with 40+ specification files
- [x] `CLAUDE.md` project memory file
- [x] `ARCHITECTURE.md` comprehensive design document
- [x] `ROADMAP.md` development timeline
- [x] `README.md` project landing page
- [x] `crates/rustynes-desktop/README.md` GUI architecture documentation

**Needs Update for Current Stack:**
- [ ] Update all docs to reference eframe/egui 0.33 (was 0.29)
- [ ] Update MSRV documentation: 1.75 -> 1.88
- [ ] Update Rust edition: 2021 -> 2024
- [ ] Update cpal version: 0.15 -> 0.16
- [ ] Update ron version: 0.8 -> 0.12
- [ ] Update thiserror version: 1.0 -> 2.0

**Locations:**
- Main docs: `/docs/`
- Project root: `/README.md`, `/ARCHITECTURE.md`, `/ROADMAP.md`
- Desktop crate: `/crates/rustynes-desktop/README.md`

## Objectives

- [ ] Write user guide (installation, usage, troubleshooting)
- [ ] Generate API documentation (rustdoc)
- [ ] Update developer guide (architecture, testing, contributing)
- [ ] Create FAQ (common questions, known issues)
- [ ] Document known limitations (defer to Phase 2)
- [ ] Update README.md and ROADMAP.md

## Tasks

### Task 1: User Guide
- [ ] Write installation guide (binaries, building from source)
- [ ] Write quick start guide (loading ROMs, controls)
- [ ] Document features (save states, screenshots, settings)
- [ ] Write troubleshooting section (common issues, solutions)
- [ ] Add screenshots and GIFs (UI walkthrough)
- [ ] Generate mdBook or similar (HTML user guide)

### Task 2: API Documentation
- [ ] Generate rustdoc for all public APIs
- [ ] Add doc comments to undocumented public items
- [ ] Add code examples for common use cases
- [ ] Document emulator core interfaces (CPU, PPU, APU, Bus)
- [ ] Publish rustdoc to docs.rs or GitHub Pages

### Task 3: Developer Guide
- [ ] Update architecture overview (CPU, PPU, APU, mappers)
- [ ] Document testing strategy (unit, integration, test ROMs)
- [ ] Update contributing guidelines (code style, pull requests)
- [ ] Write build instructions (dependencies, platforms)
- [ ] Document performance profiling (flamegraph, criterion)
- [ ] Add debugging guide (trace logging, test ROM debugging)

### Task 4: FAQ
- [ ] Collect common questions (community, GitHub issues)
- [ ] Write answers with clear explanations
- [ ] Add troubleshooting for known issues
- [ ] Document workarounds for edge cases
- [ ] Publish FAQ on GitHub Wiki or docs/FAQ.md

### Task 5: Known Limitations
- [ ] Document accuracy limitations (remaining test failures)
- [ ] Document mapper limitations (unsupported mappers)
- [ ] Document performance considerations
- [ ] Document platform-specific issues
- [ ] Reference Phase 2 roadmap for future improvements

### Task 6: Update Existing Docs
- [ ] Update README.md (features, status, screenshots)
- [ ] Update ROADMAP.md (Phase 1.5 complete)
- [ ] Update CHANGELOG.md (v0.9.0/v1.0.0-alpha.1 entry)
- [ ] Update VERSION-PLAN.md (version milestones)
- [ ] Update CONTRIBUTING.md (current guidelines)

## User Guide Structure

### 1. Installation

**Binary Installation (Recommended):**
```markdown
## Installation

### Windows
1. Download `rustynes-windows-x64.zip` from the [latest release](https://github.com/doublegate/RustyNES/releases)
2. Extract to a folder (e.g., `C:\RustyNES`)
3. Run `rustynes-desktop.exe`

### macOS
1. Download `rustynes-macos-x64.dmg` from the [latest release](https://github.com/doublegate/RustyNES/releases)
2. Open the DMG and drag RustyNES to Applications
3. Run RustyNES (may need to allow in Security & Privacy settings)

### Linux
1. Download `rustynes-linux-x64.tar.gz` from the [latest release](https://github.com/doublegate/RustyNES/releases)
2. Extract: `tar -xzf rustynes-linux-x64.tar.gz`
3. Run: `./rustynes-desktop`
```

**Building from Source:**
```markdown
## Building from Source

### Prerequisites
- Rust 1.88+ (install via [rustup](https://rustup.rs/))
- Linux: `libasound2-dev`, `libudev-dev`, `libxkbcommon-dev`, `libwayland-dev`
- macOS: Xcode Command Line Tools
- Windows: Visual Studio 2019+ with MSVC toolchain

### Toolchain Requirements
- **Rust Edition:** 2024
- **MSRV:** 1.88 (required by egui 0.33)

### Build Steps
1. Clone repository: `git clone https://github.com/doublegate/RustyNES.git`
2. Change directory: `cd RustyNES`
3. Build: `cargo build --release -p rustynes-desktop`
4. Run: `./target/release/rustynes-desktop`

### Verify Rust Version
```bash
rustc --version  # Should be 1.88.0 or higher
rustup update    # Update to latest if needed
```
```

### 2. Quick Start

```markdown
## Quick Start

### Loading a ROM
1. Launch RustyNES
2. Click **File** → **Open ROM** (or press Ctrl+O)
3. Select your NES ROM file (`.nes` or `.NES`)
4. The game will start automatically

### Controls
**Default Keyboard Mapping:**
| NES Button | Keyboard Key |
|------------|--------------|
| D-Pad Up | Arrow Up |
| D-Pad Down | Arrow Down |
| D-Pad Left | Arrow Left |
| D-Pad Right | Arrow Right |
| A | Z |
| B | X |
| Select | Right Shift |
| Start | Enter |

**Emulator Controls:**
| Action | Shortcut |
|--------|----------|
| Pause/Resume | Ctrl+P |
| Reset | Ctrl+R |
| Save State | Ctrl+S |
| Load State | Ctrl+L |
| Screenshot | F12 |
| Fullscreen | F11 |

### Save States
RustyNES automatically creates save states in the `saves/` directory. You can manually save/load states using **Emulation** → **Save State** / **Load State**.
```

### 3. Features

```markdown
## Features

### Save States
- Quick save/load (Ctrl+S / Ctrl+L)
- Multiple save slots (9 slots, Ctrl+1-9 to save, Alt+1-9 to load)
- Auto-save on close (optional)

### Screenshots
- Press F12 to take a screenshot
- Saved to `screenshots/` directory
- PNG format, timestamped filenames

### Settings
- Video: Scale (1x-4x), filters (none, scanline, CRT)
- Audio: Volume, sample rate, buffer size
- Input: Keyboard mapping, controller support
- Advanced: Debug options, logging
```

### 4. Troubleshooting

```markdown
## Troubleshooting

### ROM won't load
**Error:** "Invalid ROM header"
- **Cause:** ROM file is corrupted or not a valid iNES/NES 2.0 format
- **Solution:** Re-download ROM from a trusted source, verify checksum

### Audio pops/glitches
**Symptom:** Crackling or popping sounds during gameplay
- **Cause:** Audio buffer underrun/overflow
- **Solution:** Increase audio buffer size in settings (Audio → Buffer Size: 2048 or 4096)

### Low performance / FPS drops
**Symptom:** Emulation runs slower than 60 FPS
- **Cause:** Hardware limitations, background processes
- **Solution:**
  - Close other applications
  - Disable VSync (Settings → Video → VSync: Off)
  - Lower scale factor (Settings → Video → Scale: 1x or 2x)

### Controller not detected
**Symptom:** Gamepad not working
- **Solution:**
  - Check controller is connected and recognized by OS
  - Try reconnecting controller
  - Check input settings (Settings → Input → Controller)
```

## API Documentation (rustdoc)

### Example Doc Comments

```rust
/// NES Central Processing Unit (6502)
///
/// The CPU is the heart of the NES, responsible for executing instructions
/// and coordinating all other subsystems (PPU, APU, mappers).
///
/// # Examples
///
/// ```
/// use rustynes_cpu::Cpu;
/// use rustynes_core::Bus;
///
/// let mut cpu = Cpu::new();
/// let mut bus = Bus::new(cartridge);
///
/// // Execute one instruction
/// cpu.step(&mut bus);
/// ```
///
/// # Accuracy
///
/// This CPU implementation is cycle-accurate and passes the nestest.nes
/// golden log test (100% match).
pub struct Cpu {
    // ...
}

/// Execute one CPU instruction
///
/// This method reads the next opcode from the program counter, decodes it,
/// executes the corresponding instruction, and returns the number of cycles
/// consumed.
///
/// # Arguments
///
/// * `bus` - Mutable reference to the system bus for memory access
///
/// # Returns
///
/// Number of cycles consumed by the instruction
///
/// # Examples
///
/// ```
/// let cycles = cpu.step(&mut bus);
/// println!("Executed instruction in {} cycles", cycles);
/// ```
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    // ...
}
```

### Generate and Publish

```bash
# Generate rustdoc locally
cargo doc --workspace --no-deps --open

# Publish to docs.rs (via cargo publish)
# OR
# Publish to GitHub Pages
cargo doc --workspace --no-deps
echo "<meta http-equiv=\"refresh\" content=\"0; url=rustynes_core\">" > target/doc/index.html
# Deploy target/doc to GitHub Pages
```

## Developer Guide Structure

### 1. Architecture Overview

```markdown
## Architecture

RustyNES uses a modular crate structure:

- **rustynes-core**: Main emulation loop, system integration
- **rustynes-cpu**: 6502 CPU (cycle-accurate)
- **rustynes-ppu**: 2C02 PPU (dot-accurate rendering)
- **rustynes-apu**: 2A03 APU (5 audio channels)
- **rustynes-mappers**: Mapper implementations (NROM, MMC1, UxROM, CNROM, MMC3)
- **rustynes-desktop**: Desktop GUI (eframe + egui)

### Desktop Frontend (Post M10-S0)
- **eframe 0.33**: Window management, OpenGL context (glow backend)
- **egui 0.33**: Immediate mode GUI with Modal dialogs, Atoms, Plugin trait
- **cpal 0.16**: Cross-platform audio I/O with buffer underrun reporting
- **gilrs 0.11**: Gamepad support with hotplug detection
- **rfd 0.15**: Native file dialogs
- **ron 0.12**: Configuration persistence (improved parsing)
- **thiserror 2.0**: Error handling (no_std support)

### Toolchain
- **Rust Edition**: 2024
- **MSRV**: 1.88

### Emulation Loop
1. CPU executes one instruction (N cycles)
2. PPU steps N*3 dots (3 PPU dots per CPU cycle)
3. APU steps N cycles
4. Mappers step N cycles (IRQ timing)
5. Repeat until frame complete (262 scanlines)
```

### 2. Testing Strategy

```markdown
## Testing

### Unit Tests
Run unit tests for a specific crate:
```bash
cargo test -p rustynes-cpu
cargo test -p rustynes-ppu
cargo test -p rustynes-apu
```

### Integration Tests
Run test ROM validation:
```bash
cargo test --test test_roms -- nestest
cargo test --test test_roms -- blargg_cpu
cargo test --test test_roms -- blargg_ppu
```

### Test Coverage
Generate coverage report:
```bash
cargo tarpaulin --workspace --exclude-files benches/ --out Html
open tarpaulin-report.html
```
```

### 3. Contributing Guidelines

```markdown
## Contributing

### Code Style
- Follow Rust conventions (rustfmt, clippy)
- Run before committing:
  ```bash
  cargo fmt --check
  cargo clippy --workspace -- -D warnings
  cargo test --workspace
  ```

### Pull Requests
1. Fork repository
2. Create feature branch (`git checkout -b feature/my-feature`)
3. Commit changes (`git commit -m 'Add my feature'`)
4. Push branch (`git push origin feature/my-feature`)
5. Open pull request on GitHub

### Commit Messages
Use conventional commits:
- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation
- `test:` Tests
- `refactor:` Code refactoring
- `perf:` Performance improvements
```

## FAQ

```markdown
## Frequently Asked Questions

### What ROM formats are supported?
RustyNES supports iNES (.nes) and NES 2.0 formats.

### What mappers are supported?
Currently: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4). See MAPPERS.md for full list.

### Can I play online multiplayer?
Not yet. Netplay is planned for Phase 2 (v1.1.0).

### Does RustyNES support save states?
Yes! Use Ctrl+S to save, Ctrl+L to load.

### What is the accuracy of RustyNES?
RustyNES passes 95%+ of test ROMs (202/212) and is cycle-accurate for CPU, dot-accurate for PPU.

### Can I contribute?
Yes! See CONTRIBUTING.md for guidelines.
```

## Acceptance Criteria

- [ ] All docs updated to reflect current stack (eframe/egui 0.33, cpal 0.16, ron 0.12)
- [ ] MSRV updated to 1.88 in all documentation
- [ ] Rust 2024 edition mentioned where relevant
- [ ] User guide written (installation, quick start, features, troubleshooting)
- [ ] API documentation complete (rustdoc for all public APIs)
- [ ] Developer guide updated (architecture, testing, contributing)
- [ ] FAQ created (10+ common questions)
- [ ] Known limitations documented
- [ ] README.md updated (current status, screenshots, correct versions)
- [ ] ROADMAP.md updated (Phase 1.5 complete)
- [ ] CHANGELOG.md updated (v0.9.0/v1.0.0-alpha.1 entry)
- [ ] CLAUDE.md updated with current stack
- [ ] Documentation reviewed for clarity and accuracy

## Version Target

v0.9.0 / v1.0.0-alpha.1

---

## Technology Stack Reference

Update all documentation to reflect these versions:

| Component | Version | Notes |
|-----------|---------|-------|
| **Rust Edition** | 2024 | Required for latest patterns |
| **MSRV** | 1.88 | Required by egui 0.33 |
| **eframe** | 0.33 | Window + OpenGL via glow |
| **egui** | 0.33 | Modal, Atoms, Plugin trait |
| **cpal** | 0.16 | Buffer underrun reporting |
| **gilrs** | 0.11 | Gamepad support |
| **ron** | 0.12 | Configuration format |
| **thiserror** | 2.0 | no_std support |
| **bitflags** | 2.10 | Utility |
| **proptest** | 1.9 | Property testing |

---

**Status:** Pending
**Blocks:** M10-S3 (Release)
**Last Updated:** 2025-12-28
