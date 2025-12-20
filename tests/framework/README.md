# RustyNES Test Framework

This directory contains test ROM validation tools and test harness utilities used during RustyNES development.

## Contents

### Test ROM Validators

#### test_rom_validator.rs

- Basic test ROM validator for validating NES ROM files
- Checks iNES header format, PRG/CHR ROM sizes, mapper compatibility
- Used during Phase 1 development

#### enhanced_rom_validator.rs

- Enhanced ROM validator with additional checks
- Validates ROM integrity, CRC checks, mapper detection
- Provides detailed diagnostic output

#### test_rom_runner.rs

- Test harness for automated test ROM execution
- Runs test ROMs and captures output for validation
- Supports blargg, nestest, and other test ROM suites

### Enhanced Validator Build

#### enhanced-validator/

- Standalone Cargo project for the enhanced ROM validator
- Can be built independently: `cd enhanced-validator && cargo build --release`
- Produces binary for command-line ROM validation

## Usage

These tools were primarily used during development and testing. The core emulator now includes integrated test ROM support through the main test suite.

For current test execution, see the main test suite:
```bash
cargo test --workspace
```

For test ROM specifics, see:
- `/test-roms/` - Test ROM files
- `/tests/` - Integration test suite
- `/docs/testing/` - Testing documentation

## Archive Status

These are reference implementations preserved from development sessions. The functionality has been integrated into the main codebase but these standalone tools may be useful for:
- ROM file debugging
- Standalone validation workflows
- Reference for test infrastructure patterns

**Note:** These files are preserved for reference but not actively maintained. See the main test suite for current testing infrastructure.
