# M9 Sprint 4: Bug Fixes & Polish

## Overview

Systematically resolve all known bugs, close GitHub issues, improve error handling, and validate save state robustness to prepare for v0.8.0 release.

## Objectives

- [ ] Close all critical GitHub issues (5 critical bugs)
- [ ] Fix edge case crashes (malformed ROMs, invalid states)
- [ ] Improve error handling and logging
- [ ] Validate save state robustness (corruption detection, versioning)
- [ ] Complete v0.8.0 release preparation

## Tasks

### Task 1: GitHub Issue Triage
- [ ] Review all open GitHub issues (categorize by severity)
- [ ] Close duplicate/invalid issues
- [ ] Assign priorities (critical, high, medium, low)
- [ ] Fix all critical issues (blockers for v0.8.0)
- [ ] Document known limitations (defer non-critical to Phase 2)

### Task 2: Crash Prevention
- [ ] Add bounds checking (array access, buffer writes)
- [ ] Validate ROM headers (iNES, NES 2.0 format)
- [ ] Handle invalid save states gracefully (corruption detection)
- [ ] Test with malformed ROMs (truncated, invalid headers)
- [ ] Add panic handlers (graceful degradation, error reporting)

### Task 3: Error Handling Improvements
- [ ] Replace panic! with Result<T, Error> in library code
- [ ] Improve error messages (actionable guidance for users)
- [ ] Add logging framework (tracing or log crate)
- [ ] Implement error recovery (don't crash on recoverable errors)
- [ ] Test error paths (simulate failures)

### Task 4: Save State Robustness
- [ ] Add save state versioning (compatibility across versions)
- [ ] Implement checksums (detect corruption)
- [ ] Test edge cases (mid-frame, mid-instruction)
- [ ] Validate backward compatibility (v0.5.0 → v0.8.0)
- [ ] Document save state format

### Task 5: Input Handling Edge Cases
- [ ] Fix rapid key press handling (debouncing)
- [ ] Handle controller disconnection gracefully
- [ ] Test with multiple controllers (2-player games)
- [ ] Validate keyboard/gamepad mapping
- [ ] Test with unusual input patterns (hold all buttons)

### Task 6: Release Preparation
- [ ] Update CHANGELOG.md (v0.8.0 changes)
- [ ] Update README.md (features, status)
- [ ] Update ROADMAP.md (completed milestones)
- [ ] Run full test suite (regression check)
- [ ] Create v0.8.0 git tag
- [ ] Publish GitHub release

## Bug Categories

### Critical Bugs (5 total)

| ID | Description | Severity | Status |
|----|-------------|----------|--------|
| #1 | Crash on malformed ROM header | Critical | [ ] Open |
| #2 | Panic on save state corruption | Critical | [ ] Open |
| #3 | Buffer overflow in audio subsystem | Critical | [ ] Open |
| #4 | Infinite loop on invalid opcode | Critical | [ ] Open |
| #5 | Memory leak in PPU rendering | Critical | [ ] Open |

### High Priority Bugs (10 total)

- Input handling edge cases (rapid presses)
- Save state backward compatibility
- Error messages not user-friendly
- Logging too verbose / not configurable
- ROM loading error handling

### Medium Priority Bugs (5 total)

- Minor visual glitches (specific games)
- Audio pops under high load
- Controller disconnection handling
- Window resize handling (desktop GUI)

## Error Handling Improvements

### Before (Panic-Heavy)

```rust
pub fn load_rom(path: &str) -> Cartridge {
    let bytes = std::fs::read(path).expect("Failed to read ROM file");
    let header = parse_header(&bytes).expect("Invalid ROM header");
    Cartridge::new(header, bytes)
}
```

### After (Result-Based)

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RomError {
    #[error("Failed to read ROM file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid ROM header: {0}")]
    InvalidHeader(String),
    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),
}

pub fn load_rom(path: &str) -> Result<Cartridge, RomError> {
    let bytes = std::fs::read(path)?;
    let header = parse_header(&bytes)?;
    Ok(Cartridge::new(header, bytes))
}

// Application code (desktop GUI)
match load_rom("game.nes") {
    Ok(cart) => { /* load successfully */ },
    Err(RomError::InvalidHeader(msg)) => {
        eprintln!("Error: The ROM file has an invalid header.");
        eprintln!("Details: {}", msg);
        eprintln!("Please verify the file is a valid NES ROM in iNES or NES 2.0 format.");
    },
    Err(e) => {
        eprintln!("Error loading ROM: {}", e);
    }
}
```

## Logging Framework

### Setup

```toml
# Cargo.toml
[dependencies]
tracing = "0.1"
tracing-subscriber = "0.3"
```

### Usage

```rust
use tracing::{info, warn, error, debug};

// Initialize logging (desktop app)
fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
}

// Usage throughout codebase
fn load_rom(path: &str) -> Result<Cartridge, RomError> {
    info!("Loading ROM: {}", path);
    let bytes = std::fs::read(path)?;
    debug!("Read {} bytes from ROM file", bytes.len());

    let header = parse_header(&bytes)?;
    info!("ROM loaded successfully: {} KB PRG, {} KB CHR, Mapper {}",
          header.prg_rom_size / 1024,
          header.chr_rom_size / 1024,
          header.mapper);

    Ok(Cartridge::new(header, bytes))
}
```

## Save State Versioning

### Format

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct SaveState {
    version: u32, // Save state format version
    checksum: u32, // CRC32 checksum (corruption detection)
    timestamp: u64, // Unix timestamp
    cpu_state: CpuState,
    ppu_state: PpuState,
    apu_state: ApuState,
    mapper_state: Vec<u8>, // Mapper-specific state
}

const CURRENT_VERSION: u32 = 3; // v0.8.0 format

impl SaveState {
    fn save(&self, path: &str) -> Result<(), SaveStateError> {
        let serialized = bincode::serialize(self)?;
        std::fs::write(path, serialized)?;
        Ok(())
    }

    fn load(path: &str) -> Result<Self, SaveStateError> {
        let bytes = std::fs::read(path)?;
        let state: SaveState = bincode::deserialize(&bytes)?;

        // Verify checksum
        if !state.verify_checksum() {
            return Err(SaveStateError::CorruptedState);
        }

        // Handle version compatibility
        if state.version < 2 {
            return Err(SaveStateError::IncompatibleVersion(state.version));
        } else if state.version < CURRENT_VERSION {
            // Migrate old save state to current version
            state.migrate(CURRENT_VERSION)
        } else {
            Ok(state)
        }
    }

    fn verify_checksum(&self) -> bool {
        // Compute CRC32 of serialized state (excluding checksum field)
        // Compare to stored checksum
        true // Placeholder
    }
}
```

## Test Cases

| Test | Description | Expected Result |
|------|-------------|-----------------|
| **Malformed ROM** | Load truncated ROM file | Graceful error message |
| **Invalid Header** | Load non-NES file | Clear error (not a valid ROM) |
| **Corrupted Save State** | Load corrupted save file | Detect corruption, reject load |
| **Version Mismatch** | Load v0.5.0 save in v0.8.0 | Migrate or clear error |
| **Rapid Input** | Mash all buttons rapidly | No crashes, correct input |
| **Controller Disconnect** | Disconnect mid-game | Pause or handle gracefully |
| **Invalid Opcode** | Execute undefined opcode | Log warning, don't crash |
| **Out of Bounds** | Access invalid memory | Return default value, log error |

## Acceptance Criteria

- [ ] All 5 critical bugs fixed
- [ ] 66% reduction in minor bugs (15 → 5)
- [ ] All GitHub issues closed or triaged
- [ ] Error handling improved (Result-based, user-friendly messages)
- [ ] Logging framework integrated
- [ ] Save state versioning implemented
- [ ] Checksum validation for save states
- [ ] Tested with malformed inputs (ROMs, save states)
- [ ] Full regression test suite passing
- [ ] v0.8.0 release complete

## Release Checklist

- [ ] All critical bugs fixed
- [ ] Full test suite passing (regression check)
- [ ] Test ROM pass rate maintained (202/212, 95%+)
- [ ] Performance benchmarks passing (120+ FPS)
- [ ] Documentation updated:
  - [ ] CHANGELOG.md (v0.8.0 entry)
  - [ ] README.md (features, status)
  - [ ] ROADMAP.md (M7-M9 complete)
  - [ ] VERSION-PLAN.md (v0.8.0 tagged)
- [ ] Version bumped in Cargo.toml files
- [ ] Git tag created: `v0.8.0`
- [ ] GitHub release published
- [ ] Release notes written

## Known Limitations (Documented)

**Defer to Phase 2:**
- Expansion audio (VRC6, FDS, MMC5)
- Rare mappers (15, 19, 24+)
- Sub-cycle timing precision (<±1 cycle)
- Netplay functionality
- TAS tools (rewind, slowdown)

## Version Target

v0.8.0 (Final Release for M9)
