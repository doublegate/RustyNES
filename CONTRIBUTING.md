# Contributing to RustyNES

Thank you for your interest in contributing to RustyNES! We welcome contributions from developers of all skill levels.

## Quick Links

- [Code of Conduct](#code-of-conduct)
- [How Can I Contribute?](#how-can-i-contribute)
- [Development Setup](#development-setup)
- [Coding Standards](#coding-standards)
- [Pull Request Process](#pull-request-process)
- [Getting Help](#getting-help)

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior to <parobek@gmail.com>.

## How Can I Contribute?

### Reporting Bugs

- Use the [Bug Report template](.github/ISSUE_TEMPLATE/bug_report.md)
- Search existing issues first to avoid duplicates
- Include system information, ROM details, and reproduction steps
- Provide logs when possible (`RUST_LOG=debug`)

### Suggesting Features

- Use the [Feature Request template](.github/ISSUE_TEMPLATE/feature_request.md)
- Clearly describe the problem and proposed solution
- Explain use cases and benefits
- Check the [ROADMAP](ROADMAP.md) for alignment with project goals

### Requesting Mappers

- Use the [Mapper Request template](.github/ISSUE_TEMPLATE/mapper_request.md)
- Provide mapper number, games that use it, and technical references
- Include NESdev wiki links and test ROM information

### Contributing Code

Areas where we especially need help:

- **Core Emulation** (Phase 1 focus):
  - CPU: 6502 instruction implementation
  - PPU: Rendering pipeline and timing
  - APU: Audio channel synthesis
  - Mappers: NROM, MMC1, MMC3 implementation

- **Testing**:
  - Test ROM integration
  - Property-based tests
  - Game compatibility testing
  - Benchmarking

- **Documentation**:
  - Code documentation (rustdoc comments)
  - Examples and tutorials
  - Technical specification clarifications

- **Tooling**:
  - CI/CD improvements
  - Build scripts
  - Development utilities

## Development Setup

### Prerequisites

- **Rust 1.75 or newer** ([install via rustup](https://rustup.rs))
- **SDL2 development libraries**
- **Git**

### Platform-Specific Setup

**Ubuntu/Debian:**

```bash
sudo apt-get update
sudo apt-get install -y build-essential git libsdl2-dev
```

**Fedora:**

```bash
sudo dnf install gcc git SDL2-devel
```

**macOS:**

```bash
brew install git sdl2
```

**Windows:**

- Install [Visual Studio 2019+](https://visualstudio.microsoft.com/) with C++ tools
- Download SDL2 development libraries from [libsdl.org](https://libsdl.org)
- Set `SDL2_PATH` environment variable

### Fork and Clone

```bash
# Fork the repository on GitHub first, then:
git clone https://github.com/YOUR_USERNAME/RustyNES.git
cd RustyNES
git remote add upstream https://github.com/doublegate/RustyNES.git
```

### Build and Test

```bash
# Build the project
cargo build --workspace

# Run tests
cargo test --workspace

# Run a specific crate's tests
cargo test -p rustynes-cpu

# Run with optimizations
cargo build --release --workspace
```

### Verify Your Setup

```bash
# Check formatting
cargo fmt --all -- --check

# Run linter
cargo clippy --workspace -- -D warnings

# Generate documentation
cargo doc --workspace --no-deps --open
```

## Coding Standards

### Rust Style

- **Format**: Use `cargo fmt` (rustfmt default settings)
- **Lint**: Pass `cargo clippy -- -D warnings` without warnings
- **Edition**: Rust 2021
- **MSRV**: Minimum Supported Rust Version is 1.75

### Code Organization

- Follow Rust naming conventions (snake_case, PascalCase)
- Use meaningful variable and function names
- Keep functions focused and concise
- Organize imports: std → external crates → internal modules

### Documentation

All public APIs must have documentation comments:

```rust
/// Executes one CPU instruction
///
/// This function reads the opcode at the program counter, decodes it,
/// executes the corresponding instruction, and returns the number of
/// cycles consumed.
///
/// # Arguments
///
/// * `bus` - The memory bus for reading/writing
///
/// # Returns
///
/// Number of CPU cycles consumed by the instruction
///
/// # Examples
///
/// ```
/// let mut cpu = Cpu::new();
/// let mut bus = Bus::new();
/// let cycles = cpu.step(&mut bus);
/// assert!(cycles >= 2);
/// ```
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    // Implementation
}
```

### Testing Requirements

- **Unit tests** for new functions and modules
- **Integration tests** for component interactions
- **Test ROMs** for emulation accuracy
- **Minimum 80% code coverage** for new code

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting)
- `refactor`: Code refactoring
- `test`: Adding/updating tests
- `chore`: Build/tooling changes

**Examples:**

```
feat(cpu): implement ADC instruction with decimal mode

Adds the ADC (Add with Carry) instruction including proper
handling of the decimal flag for BCD arithmetic.

Closes #42
```

```
fix(ppu): correct sprite 0 hit timing

Sprite 0 hit was occurring one cycle too late. Adjusted
detection to occur at cycle 257 of the scanline.

Fixes #85
```

## Pull Request Process

### Before Submitting

1. **Update from upstream:**

   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Run all checks:**

   ```bash
   cargo fmt --all
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```

3. **Update documentation:**
   - Add/update rustdoc comments
   - Update relevant markdown files in `docs/`
   - Add changelog entry if significant change

### Creating a Pull Request

1. **Push your branch:**

   ```bash
   git push origin feature/my-feature
   ```

2. **Open a Pull Request** on GitHub

3. **Fill out the PR template** completely

4. **Ensure CI passes** (automated checks must succeed)

5. **Respond to review feedback** promptly

### Review Process

- Maintainers will review your PR within 3-14 days
- Automated checks (CI) must pass
- At least one maintainer approval required
- Address all review comments
- Keep the PR scope focused

### After Merge

Your contribution will be:

- Included in the next release
- Documented in CHANGELOG.md
- Credited in release notes

## Getting Help

### Resources

- **Documentation**: [docs/](docs/) folder
- **Detailed Contributing Guide**: [docs/dev/CONTRIBUTING.md](docs/dev/CONTRIBUTING.md)
- **Build Instructions**: [docs/dev/BUILD.md](docs/dev/BUILD.md)
- **Testing Guide**: [docs/dev/TESTING.md](docs/dev/TESTING.md)
- **Style Guide**: [docs/dev/STYLE_GUIDE.md](docs/dev/STYLE_GUIDE.md)

### Ask Questions

- **GitHub Discussions**: [General questions and ideas](https://github.com/doublegate/RustyNES/discussions)
- **GitHub Issues**: [Bug reports and feature requests](https://github.com/doublegate/RustyNES/issues)
- **NESdev Forums**: [NES hardware questions](https://forums.nesdev.org/)

### Finding Work

- Check issues labeled [`good first issue`](https://github.com/doublegate/RustyNES/labels/good%20first%20issue)
- Check issues labeled [`help wanted`](https://github.com/doublegate/RustyNES/labels/help%20wanted)
- Review the [ROADMAP](ROADMAP.md) for upcoming features
- Ask in [Discussions](https://github.com/doublegate/RustyNES/discussions) what needs help

## Development Workflow

### Feature Development

```bash
# 1. Create a feature branch
git checkout -b feature/my-feature

# 2. Make changes
# ... edit files ...

# 3. Write tests (TDD approach recommended)
# ... add tests in src/ or tests/ ...

# 4. Run tests
cargo test

# 5. Format and lint
cargo fmt
cargo clippy -- -D warnings

# 6. Commit changes
git add .
git commit -m "feat(component): add my feature"

# 7. Push and create PR
git push origin feature/my-feature
```

### Test-Driven Development

We strongly encourage TDD:

1. Write a failing test
2. Implement the feature
3. Make the test pass
4. Refactor if needed
5. Repeat

**Example:**

```rust
#[test]
fn test_lda_immediate() {
    let mut cpu = Cpu::new();
    let mut bus = Bus::new();

    // LDA #$42
    bus.write(0x0000, 0xA9);
    bus.write(0x0001, 0x42);

    let cycles = cpu.step(&mut bus);

    assert_eq!(cpu.a, 0x42);
    assert_eq!(cycles, 2);
    assert!(!cpu.p.zero);
    assert!(!cpu.p.negative);
}
```

## Project Structure

```
rustynes/
├── crates/
│   ├── rustynes-core/     # Core emulation engine
│   ├── rustynes-cpu/      # 6502 CPU
│   ├── rustynes-ppu/      # 2C02 PPU
│   ├── rustynes-apu/      # 2A03 APU
│   ├── rustynes-mappers/  # Mapper implementations
│   ├── rustynes-desktop/  # Desktop GUI
│   └── ...
├── docs/                  # Documentation
├── tests/                 # Integration tests
├── benches/               # Benchmarks
└── examples/              # Usage examples
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed system design.

## Recognition

Contributors will be recognized in:

- Release notes for each version
- CHANGELOG.md for significant contributions
- Project README (for substantial contributions)

## License

By contributing to RustyNES, you agree that your contributions will be licensed under both:

- MIT License
- Apache License 2.0

See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE) for details.

---

**Thank you for contributing to RustyNES! Your efforts help preserve video game history and create an amazing emulation platform.**

For more detailed guidelines, see [docs/dev/CONTRIBUTING.md](docs/dev/CONTRIBUTING.md).
