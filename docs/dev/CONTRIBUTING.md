# Contributing to RustyNES

Thank you for your interest in contributing to RustyNES! This document provides guidelines for contributing code, documentation, and bug reports.

**Table of Contents**

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Style](#code-style)
- [Commit Messages](#commit-messages)
- [Pull Request Process](#pull-request-process)
- [Testing Requirements](#testing-requirements)
- [Documentation](#documentation)

---

## Code of Conduct

- Be respectful and inclusive
- Focus on constructive feedback
- Help others learn and grow
- Maintain professional communication

---

## Getting Started

### Fork and Clone

```bash
# Fork on GitHub, then:
git clone https://github.com/YOUR_USERNAME/rustynes.git
cd rustynes
git remote add upstream https://github.com/ORIGINAL_AUTHOR/rustynes.git
```

### Build and Test

```bash
cargo build
cargo test
```

See [BUILD.md](BUILD.md) for detailed build instructions.

---

## Development Workflow

### Creating a Feature Branch

```bash
git checkout -b feature/my-new-feature
```

**Branch Naming**:

- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation
- `refactor/` - Code refactoring
- `test/` - Test additions/improvements

### Making Changes

1. **Write tests first** (TDD approach)
2. **Implement feature/fix**
3. **Run tests**: `cargo test`
4. **Format code**: `cargo fmt`
5. **Lint code**: `cargo clippy -- -D warnings`
6. **Update documentation** if needed

### Committing Changes

```bash
git add .
git commit -m "feat: Add MMC5 mapper support"
```

See [Commit Messages](#commit-messages) below.

---

## Code Style

### Rust Style Guidelines

**Follow Rust conventions**:

- Use `cargo fmt` (rustfmt)
- Pass `cargo clippy` without warnings
- Use meaningful variable names
- Add documentation comments for public APIs

### Code Organization

```rust
// Imports
use std::collections::HashMap;

// Constants
const MAX_SPRITES: usize = 64;

// Structures
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    // ...
}

// Implementation
impl Cpu {
    pub fn new() -> Self {
        // ...
    }

    pub fn step(&mut self) -> u8 {
        // ...
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_initialization() {
        // ...
    }
}
```

### Documentation Comments

```rust
/// Executes one CPU instruction
///
/// # Returns
///
/// Number of CPU cycles consumed
///
/// # Examples
///
/// ```
/// let mut cpu = Cpu::new();
/// let cycles = cpu.step();
/// assert!(cycles >= 2);
/// ```
pub fn step(&mut self) -> u8 {
    // ...
}
```

---

## Commit Messages

### Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types

- **feat**: New feature
- **fix**: Bug fix
- **docs**: Documentation changes
- **style**: Code style changes (formatting)
- **refactor**: Code refactoring
- **test**: Adding/updating tests
- **chore**: Build/tooling changes

### Examples

**Feature**:

```
feat(mapper): Add MMC5 mapper support

Implements iNES mapper 5 with PRG/CHR banking,
expansion audio, and ExRAM functionality.

Closes #42
```

**Bug Fix**:

```
fix(ppu): Correct sprite zero hit timing

Sprite 0 hit was occurring one cycle too late,
causing SMB1 scrolling glitches. Fixed by adjusting
the hit detection to occur at cycle 257.

Fixes #85
```

**Documentation**:

```
docs(cpu): Add cycle timing tables

Documents cycle counts for all 6502 instructions
including page-crossing penalties.
```

---

## Pull Request Process

### Before Submitting

1. **Update from upstream**:

   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Run full test suite**:

   ```bash
   cargo test --all-features
   cargo clippy -- -D warnings
   cargo fmt --check
   ```

3. **Update documentation**:
   - Update relevant `.md` files
   - Add/update code comments
   - Update CHANGELOG.md if applicable

### Submitting PR

1. **Push branch**:

   ```bash
   git push origin feature/my-new-feature
   ```

2. **Create Pull Request** on GitHub

3. **PR Description Template**:

   ```markdown
   ## Description
   Brief description of changes

   ## Type of Change
   - [ ] Bug fix
   - [ ] New feature
   - [ ] Breaking change
   - [ ] Documentation update

   ## Testing
   - [ ] Unit tests pass
   - [ ] Integration tests pass
   - [ ] Test ROMs pass (if applicable)

   ## Checklist
   - [ ] Code follows style guidelines
   - [ ] Self-reviewed code
   - [ ] Commented complex sections
   - [ ] Updated documentation
   - [ ] No new warnings
   ```

### Review Process

- **Automated checks** must pass
- **Code review** by maintainer(s)
- **Requested changes** should be addressed
- **Approval** from at least one maintainer

---

## Testing Requirements

### Minimum Requirements

**All PRs must**:

- Pass existing unit tests
- Pass existing integration tests
- Include new tests for new functionality
- Maintain or improve test coverage

### Test Coverage

**Aim for**:

- 80%+ line coverage for new code
- 100% coverage for critical paths (CPU, PPU core)

**Check coverage**:

```bash
cargo tarpaulin --out Html
open tarpaulin-report.html
```

### Test ROM Validation

**For mapper changes**:

```bash
cargo test --test mapper_test_suite
```

**For CPU/PPU changes**:

```bash
cargo test --test nestest
cargo test --test blargg_suite
```

---

## Documentation

### Required Documentation

**For new features**:

- Code documentation (rustdoc comments)
- User-facing documentation (docs/ folder)
- Examples in docs/examples/ (if applicable)

**For bug fixes**:

- Comment explaining the fix
- Update relevant documentation if behavior changes

### Documentation Style

**Clear and concise**:

- Use simple language
- Provide examples
- Link to related documents
- Include diagrams where helpful (Mermaid format)

---

## Questions?

- **GitHub Issues**: For bugs and feature requests
- **GitHub Discussions**: For questions and general discussion
- **Documentation**: Check docs/ folder first

---

**Thank you for contributing to RustyNES!**
