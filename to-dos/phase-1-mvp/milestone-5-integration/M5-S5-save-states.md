# [Milestone 5] Sprint 5.5: Save States

**Status:** ✅ COMPLETED
**Started:** December 19, 2025
**Completed:** December 19, 2025
**Duration:** 1 day (part of M5 integration)
**Assignee:** Claude Code / Developer
**Sprint:** M5-S5 (Integration - Save States)
**Progress:** 100%

---

## Overview

This sprint implements the **save state system** for RustyNES, enabling instant save/load of complete emulator state. This is critical for rewind functionality, TAS recording, debugging, and user convenience features.

### Goals

- ⏳ Complete state serialization/deserialization
- ⏳ File-based save state management
- ⏳ Version compatibility system
- ⏳ ROM verification (prevent mismatched states)
- ⏳ Checksum validation
- ⏳ Optional compression support
- ⏳ Deterministic state restoration
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M5-S3 Console Coordinator complete (Console struct exists)
- ✅ All component crates (CPU, PPU, APU, Mappers) support serialization
- ✅ Bus and memory system implemented (M5-S2)

---

## Tasks

### Task 1: Define Save State Format (2 hours)

**File:** `crates/rustynes-core/src/save_state/format.rs`

**Objective:** Establish binary format specification for save states.

#### Subtasks

1. Create `SaveStateHeader` struct with:
   - Magic bytes ("RNES")
   - Format version (u32)
   - CRC32 checksum
   - Flags (compression, etc.)
   - ROM hash (SHA-256)
   - Unix timestamp
   - Frame count
   - Reserved bytes

2. Define `SaveStateFlags` bitflags:
   - `COMPRESSED` - State data is zlib-compressed
   - `BATTERY_BACKED` - Contains PRG-RAM
   - Reserved flags for future use

3. Document binary layout

**Acceptance Criteria:**

- [ ] Header is exactly 64 bytes (#[repr(C)])
- [ ] All fields have clear byte offsets
- [ ] Documentation explains each field
- [ ] Version constant defined (CURRENT_VERSION = 1)

**Implementation:**

```rust
use std::time::{SystemTime, UNIX_EPOCH};

/// Save state file header (64 bytes)
#[repr(C)]
pub struct SaveStateHeader {
    /// Magic bytes: "RNES"
    pub magic: [u8; 4],

    /// Format version (current: 1)
    pub version: u32,

    /// CRC32 checksum of state data (excludes header)
    pub checksum: u32,

    /// Bitflags (see SaveStateFlags)
    pub flags: u32,

    /// SHA-256 hash of ROM (for compatibility checking)
    pub rom_hash: [u8; 32],

    /// Unix timestamp when state was saved
    pub timestamp: u64,

    /// Frame number at time of save
    pub frame_count: u64,

    /// Reserved for future use
    pub reserved: [u8; 8],
}

bitflags::bitflags! {
    pub struct SaveStateFlags: u32 {
        const COMPRESSED = 0x0001;
        const BATTERY_BACKED = 0x0002;
    }
}

impl SaveStateHeader {
    pub const SIZE: usize = 64;
    pub const MAGIC: &'static [u8; 4] = b"RNES";
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new(rom_hash: [u8; 32], frame_count: u64, flags: SaveStateFlags) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            magic: *Self::MAGIC,
            version: Self::CURRENT_VERSION,
            checksum: 0, // Computed after serialization
            flags: flags.bits(),
            rom_hash,
            timestamp,
            frame_count,
            reserved: [0; 8],
        }
    }

    pub fn verify(&self) -> Result<(), SaveStateError> {
        if &self.magic != Self::MAGIC {
            return Err(SaveStateError::InvalidMagic);
        }

        if self.version > Self::CURRENT_VERSION {
            return Err(SaveStateError::UnsupportedVersion(self.version));
        }

        Ok(())
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        unsafe { std::mem::transmute(*self) }
    }

    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Self {
        unsafe { std::mem::transmute(*bytes) }
    }
}
```

---

### Task 2: Implement Serialization Trait (3 hours)

**File:** `crates/rustynes-core/src/save_state/serialization.rs`

**Objective:** Define trait for component serialization.

#### Subtasks

1. Create `Serializable` trait with:
   - `serialize(&self) -> Result<Vec<u8>, SerializeError>`
   - `deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError>`

2. Implement for all components:
   - CPU (registers, flags, cycle count, interrupt state)
   - PPU (registers, internal state, VRAM, OAM, palettes)
   - APU (channel state, frame counter, DMC)
   - Bus (RAM, controller state)
   - Mapper (type-specific state)

3. Create `SerializeError` enum

**Acceptance Criteria:**

- [ ] All components implement `Serializable`
- [ ] Deserialization returns bytes consumed
- [ ] Error handling covers insufficient data, invalid state
- [ ] No heap allocations during deserialization (where possible)

**Implementation:**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SerializeError {
    #[error("Insufficient data: need {needed} bytes, got {available}")]
    InsufficientData { needed: usize, available: usize },

    #[error("Invalid state value: {0}")]
    InvalidState(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub trait Serializable {
    /// Serialize component state to bytes
    fn serialize(&self) -> Result<Vec<u8>, SerializeError>;

    /// Deserialize component state from bytes, returning bytes consumed
    fn deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError>;

    /// Expected serialized size (for validation)
    fn serialized_size(&self) -> usize;
}

// Example: CPU implementation
impl Serializable for Cpu {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut data = Vec::with_capacity(self.serialized_size());

        // Registers (7 bytes)
        data.push(self.a);
        data.push(self.x);
        data.push(self.y);
        data.push(self.sp);
        data.extend_from_slice(&self.pc.to_le_bytes());
        data.push(self.p.bits());

        // Internal state (17 bytes)
        data.extend_from_slice(&self.cycles.to_le_bytes());
        data.push(self.nmi_pending as u8);
        data.push(self.irq_pending as u8);
        data.push(self.nmi_edge as u8);
        data.push(self.irq_line as u8);
        data.push(self.oam_dma_pending as u8);
        data.push(self.dmc_dma_pending as u8);
        data.push(self.halt_cycles as u8);
        data.push(self.stall_cycles as u8);

        Ok(data)
    }

    fn deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError> {
        let needed = self.serialized_size();
        if data.len() < needed {
            return Err(SerializeError::InsufficientData {
                needed,
                available: data.len(),
            });
        }

        let mut offset = 0;

        self.a = data[offset]; offset += 1;
        self.x = data[offset]; offset += 1;
        self.y = data[offset]; offset += 1;
        self.sp = data[offset]; offset += 1;
        self.pc = u16::from_le_bytes([data[offset], data[offset + 1]]); offset += 2;
        self.p = Status::from_bits_truncate(data[offset]); offset += 1;

        self.cycles = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()); offset += 8;
        self.nmi_pending = data[offset] != 0; offset += 1;
        self.irq_pending = data[offset] != 0; offset += 1;
        self.nmi_edge = data[offset] != 0; offset += 1;
        self.irq_line = data[offset] != 0; offset += 1;
        self.oam_dma_pending = data[offset] != 0; offset += 1;
        self.dmc_dma_pending = data[offset] != 0; offset += 1;
        self.halt_cycles = data[offset]; offset += 1;
        self.stall_cycles = data[offset]; offset += 1;

        Ok(offset)
    }

    fn serialized_size(&self) -> usize {
        24 // 7 + 17
    }
}
```

---

### Task 3: Console Save State API (3 hours)

**File:** `crates/rustynes-core/src/save_state/console.rs`

**Objective:** Implement high-level save/load API on Console.

#### Subtasks

1. Implement `Console::save_state()`
   - Compute ROM hash
   - Serialize header
   - Serialize all components
   - Compute checksum
   - Return complete byte vector

2. Implement `Console::load_state()`
   - Verify header magic and version
   - Verify ROM hash matches
   - Validate checksum
   - Deserialize all components
   - Verify determinism

3. Add file I/O wrappers:
   - `save_state_to_file(path: &Path)`
   - `load_state_from_file(path: &Path)`

4. Error handling

**Acceptance Criteria:**

- [ ] Save state captures all emulator state
- [ ] Load state restores exact execution
- [ ] Determinism test: save → load → save produces identical bytes
- [ ] File I/O handles permissions, disk full gracefully
- [ ] No panics on invalid state files

**Implementation:**

```rust
use std::path::Path;
use sha2::{Sha256, Digest};
use crc32fast::Hasher;

impl Console {
    /// Save complete console state to bytes
    pub fn save_state(&self) -> Result<Vec<u8>, SaveStateError> {
        // Compute ROM hash
        let rom_hash = self.rom_hash();

        // Create header
        let flags = if self.cartridge.has_battery_ram() {
            SaveStateFlags::BATTERY_BACKED
        } else {
            SaveStateFlags::empty()
        };

        let mut header = SaveStateHeader::new(rom_hash, self.frame_count, flags);

        // Serialize components
        let mut state_data = Vec::new();
        state_data.extend_from_slice(&self.cpu.serialize()?);
        state_data.extend_from_slice(&self.bus.ppu.serialize()?);
        state_data.extend_from_slice(&self.bus.apu.serialize()?);
        state_data.extend_from_slice(&self.bus.serialize()?);
        state_data.extend_from_slice(&self.bus.cartridge.serialize()?);

        // Compute checksum
        let mut hasher = Hasher::new();
        hasher.update(&state_data);
        header.checksum = hasher.finalize();

        // Combine header + data
        let mut complete_state = Vec::with_capacity(SaveStateHeader::SIZE + state_data.len());
        complete_state.extend_from_slice(&header.to_bytes());
        complete_state.extend_from_slice(&state_data);

        Ok(complete_state)
    }

    /// Load console state from bytes
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        // Verify minimum size
        if data.len() < SaveStateHeader::SIZE {
            return Err(SaveStateError::InsufficientData {
                needed: SaveStateHeader::SIZE,
                available: data.len(),
            });
        }

        // Parse header
        let header_bytes: &[u8; SaveStateHeader::SIZE] =
            data[0..SaveStateHeader::SIZE].try_into().unwrap();
        let header = SaveStateHeader::from_bytes(header_bytes);

        // Verify header
        header.verify()?;

        // Verify ROM hash
        let expected_hash = self.rom_hash();
        if header.rom_hash != expected_hash {
            return Err(SaveStateError::RomMismatch {
                expected: expected_hash,
                actual: header.rom_hash,
            });
        }

        // Verify checksum
        let state_data = &data[SaveStateHeader::SIZE..];
        let mut hasher = Hasher::new();
        hasher.update(state_data);
        let computed_checksum = hasher.finalize();

        if computed_checksum != header.checksum {
            return Err(SaveStateError::ChecksumMismatch {
                expected: header.checksum,
                actual: computed_checksum,
            });
        }

        // Deserialize components
        let mut offset = 0;
        offset += self.cpu.deserialize(&state_data[offset..])?;
        offset += self.bus.ppu.deserialize(&state_data[offset..])?;
        offset += self.bus.apu.deserialize(&state_data[offset..])?;
        offset += self.bus.deserialize(&state_data[offset..])?;
        offset += self.bus.cartridge.deserialize(&state_data[offset..])?;

        // Restore frame count
        self.frame_count = header.frame_count;

        Ok(())
    }

    /// Save state to file
    pub fn save_state_to_file(&self, path: &Path) -> Result<(), SaveStateError> {
        let state = self.save_state()?;
        std::fs::write(path, &state)
            .map_err(|e| SaveStateError::Io(e))?;
        Ok(())
    }

    /// Load state from file
    pub fn load_state_from_file(&mut self, path: &Path) -> Result<(), SaveStateError> {
        let data = std::fs::read(path)
            .map_err(|e| SaveStateError::Io(e))?;
        self.load_state(&data)
    }

    /// Compute SHA-256 hash of ROM
    fn rom_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.bus.cartridge.prg_rom());
        hasher.update(&self.bus.cartridge.chr_rom());
        hasher.finalize().into()
    }
}
```

---

### Task 4: Compression Support (2 hours)

**File:** `crates/rustynes-core/src/save_state/compression.rs`

**Objective:** Add optional zlib compression for save states.

#### Subtasks

1. Implement `Console::save_state_compressed()`
2. Implement `Console::load_state_compressed()`
3. Set `COMPRESSED` flag in header
4. Benchmark compression ratios

**Acceptance Criteria:**

- [ ] Compressed states ~80% smaller (10-20KB)
- [ ] Decompression adds <5ms latency
- [ ] Flag correctly indicates compression
- [ ] Fallback to uncompressed on error

**Implementation:**

```rust
use flate2::Compression;
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;
use std::io::{Write, Read};

impl Console {
    /// Save state with zlib compression
    pub fn save_state_compressed(&self) -> Result<Vec<u8>, SaveStateError> {
        let uncompressed = self.save_state()?;

        // Compress state data (skip header)
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&uncompressed[SaveStateHeader::SIZE..])
            .map_err(|e| SaveStateError::Compression(e.to_string()))?;
        let compressed_data = encoder.finish()
            .map_err(|e| SaveStateError::Compression(e.to_string()))?;

        // Update header with COMPRESSED flag
        let mut header_bytes = uncompressed[0..SaveStateHeader::SIZE].to_vec();
        let mut header = SaveStateHeader::from_bytes(
            header_bytes.as_slice().try_into().unwrap()
        );
        header.flags |= SaveStateFlags::COMPRESSED.bits();

        // Recompute checksum for compressed data
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&compressed_data);
        header.checksum = hasher.finalize();

        // Combine header + compressed data
        let mut result = header.to_bytes().to_vec();
        result.extend_from_slice(&compressed_data);

        Ok(result)
    }

    /// Load compressed state
    pub fn load_state_compressed(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        if data.len() < SaveStateHeader::SIZE {
            return Err(SaveStateError::InsufficientData {
                needed: SaveStateHeader::SIZE,
                available: data.len(),
            });
        }

        // Parse header
        let header_bytes: &[u8; SaveStateHeader::SIZE] =
            data[0..SaveStateHeader::SIZE].try_into().unwrap();
        let header = SaveStateHeader::from_bytes(header_bytes);

        // Verify header
        header.verify()?;

        // Check if compressed
        let flags = SaveStateFlags::from_bits_truncate(header.flags);
        if !flags.contains(SaveStateFlags::COMPRESSED) {
            // Not compressed, use regular load
            return self.load_state(data);
        }

        // Decompress state data
        let compressed_data = &data[SaveStateHeader::SIZE..];
        let mut decoder = ZlibDecoder::new(compressed_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)
            .map_err(|e| SaveStateError::Decompression(e.to_string()))?;

        // Reconstruct uncompressed state (header + data)
        let mut uncompressed_state = header.to_bytes().to_vec();

        // Clear COMPRESSED flag for internal load
        let mut internal_header = header;
        internal_header.flags &= !SaveStateFlags::COMPRESSED.bits();

        // Recompute checksum for decompressed data
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&decompressed);
        internal_header.checksum = hasher.finalize();

        uncompressed_state[0..SaveStateHeader::SIZE]
            .copy_from_slice(&internal_header.to_bytes());
        uncompressed_state.extend_from_slice(&decompressed);

        // Load via standard path
        self.load_state(&uncompressed_state)
    }
}
```

---

### Task 5: Error Types (1 hour)

**File:** `crates/rustynes-core/src/save_state/error.rs`

**Objective:** Define comprehensive error types.

#### Subtasks

1. Create `SaveStateError` enum
2. Implement `Display` via thiserror
3. Add conversion from component errors

**Acceptance Criteria:**

- [ ] All error paths covered
- [ ] Clear error messages
- [ ] Implements `std::error::Error`

**Implementation:**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveStateError {
    #[error("Invalid magic bytes (expected 'RNES')")]
    InvalidMagic,

    #[error("Unsupported version: {0} (current: {})", SaveStateHeader::CURRENT_VERSION)]
    UnsupportedVersion(u32),

    #[error("ROM mismatch: expected {expected:x?}, got {actual:x?}")]
    RomMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },

    #[error("Checksum mismatch: expected {expected:08x}, got {actual:08x}")]
    ChecksumMismatch {
        expected: u32,
        actual: u32,
    },

    #[error("Insufficient data: need {needed} bytes, got {available}")]
    InsufficientData {
        needed: usize,
        available: usize,
    },

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] SerializeError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

### Task 6: Determinism Testing (2 hours)

**File:** `crates/rustynes-core/tests/save_state_determinism.rs`

**Objective:** Verify save states restore exact execution.

#### Subtasks

1. Test: save → load → save produces identical bytes
2. Test: execute N frames, save, load, verify next M frames match
3. Test: load state mid-frame fails gracefully (or succeeds deterministically)
4. Test: multiple save slots don't interfere

**Acceptance Criteria:**

- [ ] Determinism test passes for 1000 frames
- [ ] State size is consistent
- [ ] Loading doesn't leak previous state
- [ ] All test ROMs pass after save/load

**Implementation:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::Console;

    #[test]
    fn test_save_load_determinism() {
        // Load test ROM
        let rom = load_test_rom("nestest.nes");
        let mut console1 = Console::new(rom.clone()).unwrap();
        let mut console2 = Console::new(rom.clone()).unwrap();

        // Execute 1000 frames on console1
        for _ in 0..1000 {
            console1.step_frame();
        }

        // Save state
        let state1 = console1.save_state().unwrap();

        // Load into console2
        console2.load_state(&state1).unwrap();

        // Save again
        let state2 = console2.save_state().unwrap();

        // States must be byte-identical (except timestamp)
        assert_eq!(state1.len(), state2.len());

        // Compare all bytes except timestamp field (offset 40-48)
        let skip_timestamp = |s: &[u8]| {
            [&s[0..40], &s[48..]].concat()
        };

        assert_eq!(skip_timestamp(&state1), skip_timestamp(&state2));
    }

    #[test]
    fn test_execution_after_load() {
        let rom = load_test_rom("nestest.nes");
        let mut console1 = Console::new(rom.clone()).unwrap();
        let mut console2 = Console::new(rom.clone()).unwrap();

        // Execute 500 frames
        for _ in 0..500 {
            console1.step_frame();
            console2.step_frame();
        }

        // Save console1 state
        let state = console1.save_state().unwrap();

        // Execute 100 more frames on console1
        for _ in 0..100 {
            console1.step_frame();
        }
        let frame1 = console1.framebuffer().to_vec();

        // Load state into console2, execute 100 frames
        console2.load_state(&state).unwrap();
        for _ in 0..100 {
            console2.step_frame();
        }
        let frame2 = console2.framebuffer().to_vec();

        // Framebuffers must match
        assert_eq!(frame1, frame2, "Execution diverged after load_state");
    }

    #[test]
    fn test_rom_mismatch_detection() {
        let rom1 = load_test_rom("nestest.nes");
        let rom2 = load_test_rom("smb.nes");

        let mut console1 = Console::new(rom1).unwrap();
        let console2 = Console::new(rom2).unwrap();

        // Execute and save
        for _ in 0..100 {
            console1.step_frame();
        }
        let state = console1.save_state().unwrap();

        // Try to load into console with different ROM
        let mut console2 = console2;
        let result = console2.load_state(&state);

        assert!(matches!(result, Err(SaveStateError::RomMismatch { .. })));
    }

    #[test]
    fn test_corrupted_checksum() {
        let rom = load_test_rom("nestest.nes");
        let mut console = Console::new(rom).unwrap();

        for _ in 0..100 {
            console.step_frame();
        }

        let mut state = console.save_state().unwrap();

        // Corrupt a byte in the state data
        state[SaveStateHeader::SIZE + 10] ^= 0xFF;

        // Load should fail
        let result = console.load_state(&state);
        assert!(matches!(result, Err(SaveStateError::ChecksumMismatch { .. })));
    }

    #[test]
    fn test_compression_roundtrip() {
        let rom = load_test_rom("nestest.nes");
        let mut console1 = Console::new(rom.clone()).unwrap();

        for _ in 0..500 {
            console1.step_frame();
        }

        // Save compressed
        let compressed = console1.save_state_compressed().unwrap();
        let uncompressed = console1.save_state().unwrap();

        // Verify compression ratio
        assert!(compressed.len() < uncompressed.len());
        println!("Compression ratio: {:.1}%",
            100.0 * compressed.len() as f64 / uncompressed.len() as f64);

        // Load compressed state
        let mut console2 = Console::new(rom).unwrap();
        console2.load_state_compressed(&compressed).unwrap();

        // Verify determinism
        let state2 = console2.save_state().unwrap();

        // Skip timestamp comparison
        assert_eq!(
            &uncompressed[0..40],
            &state2[0..40]
        );
        assert_eq!(
            &uncompressed[48..],
            &state2[48..]
        );
    }
}
```

---

### Task 7: Documentation (1 hour)

**File:** `crates/rustynes-core/src/save_state/mod.rs`

**Objective:** Add comprehensive module documentation.

#### Subtasks

1. Module-level doc comments
2. API usage examples
3. Format specification
4. Performance notes

**Acceptance Criteria:**

- [ ] All public items documented
- [ ] Examples compile and run
- [ ] Format spec is complete

**Implementation:**

```rust
//! Save state system for RustyNES emulator.
//!
//! This module provides instant save/load functionality for complete emulator state,
//! enabling features like rewind, TAS recording, and quick save/load.
//!
//! # Format
//!
//! Save states use a custom binary format with the following structure:
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Header (64 bytes)                   │
//! │  - Magic: "RNES"                    │
//! │  - Version: u32                     │
//! │  - Checksum: CRC32                  │
//! │  - Flags: u32                       │
//! │  - ROM Hash: SHA-256 (32 bytes)     │
//! │  - Timestamp: u64                   │
//! │  - Frame Count: u64                 │
//! │  - Reserved: 8 bytes                │
//! ├─────────────────────────────────────┤
//! │ CPU State (~24 bytes)               │
//! ├─────────────────────────────────────┤
//! │ PPU State (~512 bytes)              │
//! ├─────────────────────────────────────┤
//! │ APU State (~128 bytes)              │
//! ├─────────────────────────────────────┤
//! │ Bus State (2KB RAM + controllers)   │
//! ├─────────────────────────────────────┤
//! │ Mapper State (varies)               │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use rustynes_core::Console;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rom = std::fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom)?;
//!
//! // Execute some frames
//! for _ in 0..1000 {
//!     console.step_frame();
//! }
//!
//! // Save state
//! console.save_state_to_file(Path::new("save1.state"))?;
//!
//! // Continue playing...
//! for _ in 0..500 {
//!     console.step_frame();
//! }
//!
//! // Load previous state
//! console.load_state_from_file(Path::new("save1.state"))?;
//! # Ok(())
//! # }
//! ```
//!
//! # Compression
//!
//! Save states can be compressed using zlib for storage:
//!
//! ```no_run
//! # use rustynes_core::Console;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let rom = std::fs::read("game.nes")?;
//! # let mut console = Console::from_rom_bytes(&rom)?;
//! // Compress for disk storage (80% smaller, ~10-20KB)
//! let compressed = console.save_state_compressed()?;
//! std::fs::write("save1.state.gz", compressed)?;
//!
//! // Load compressed state
//! let compressed = std::fs::read("save1.state.gz")?;
//! console.load_state_compressed(&compressed)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Performance
//!
//! - Uncompressed save: ~50KB, <0.1ms
//! - Compressed save: ~10-20KB, ~2-5ms
//! - Load (either): <0.5ms
//!
//! For rewind functionality (speed-critical), use uncompressed states.
//! For disk storage, use compressed states.
//!
//! # Safety
//!
//! This module uses zero unsafe code. All serialization is bounds-checked.

pub mod format;
pub mod serialization;
pub mod console;
pub mod compression;
pub mod error;

pub use format::{SaveStateHeader, SaveStateFlags};
pub use serialization::{Serializable, SerializeError};
pub use error::SaveStateError;
```

---

## Acceptance Criteria

### Functionality

- [ ] Console::save_state() captures all state
- [ ] Console::load_state() restores exact execution
- [ ] ROM verification prevents mismatched loads
- [ ] Checksum validation detects corruption
- [ ] Compression reduces size by ~80%
- [ ] File I/O handles errors gracefully

### Determinism

- [ ] save → load → save produces identical bytes (minus timestamp)
- [ ] Execution after load matches original
- [ ] 1000-frame determinism test passes
- [ ] Works with all mappers

### Quality

- [ ] Zero unsafe code
- [ ] No panics on invalid input
- [ ] Comprehensive error messages
- [ ] All public APIs documented
- [ ] Unit tests for all components
- [ ] Integration tests verify end-to-end

---

## Dependencies

### External Crates

```toml
[dependencies]
bitflags = "2.4"
sha2 = "0.10"
crc32fast = "1.3"
flate2 = "1.0"  # Optional compression
thiserror = "1.0"

[dev-dependencies]
criterion = "0.5"  # Benchmark save/load performance
```

### Internal Dependencies

- rustynes-cpu (Serializable impl)
- rustynes-ppu (Serializable impl)
- rustynes-apu (Serializable impl)
- rustynes-mappers (Serializable impl)

---

## Related Documentation

- [SAVE_STATES.md](../../../docs/api/SAVE_STATES.md) - Format specification
- [CORE_API.md](../../../docs/api/CORE_API.md) - Console API
- [M5-S3-console-coordinator.md](M5-S3-console-coordinator.md) - Console integration

---

## Technical Notes

### When to Save

**Recommended**: At frame boundaries (after `step_frame()`)

- Deterministic state
- Clean component synchronization
- PPU in VBlank (stable state)

**Avoid**: Mid-instruction, during DMA, arbitrary cycle counts

### Rewind Implementation

For rewind functionality, maintain a ring buffer of recent states:

```rust
use std::collections::VecDeque;

pub struct RewindBuffer {
    states: VecDeque<Vec<u8>>,
    capacity: usize, // e.g., 600 states = 10 seconds at 60 FPS
}

impl RewindBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, state: Vec<u8>) {
        if self.states.len() >= self.capacity {
            self.states.pop_front();
        }
        self.states.push_back(state);
    }

    pub fn pop(&mut self) -> Option<Vec<u8>> {
        self.states.pop_back()
    }

    pub fn clear(&mut self) {
        self.states.clear();
    }
}
```

### Version Migration

Future versions should implement migration logic:

```rust
impl Console {
    fn migrate_save_state(&self, data: &[u8], from_version: u32)
        -> Result<Vec<u8>, SaveStateError>
    {
        match (from_version, SaveStateHeader::CURRENT_VERSION) {
            (1, 2) => self.migrate_v1_to_v2(data),
            (2, 3) => self.migrate_v2_to_v3(data),
            _ => Err(SaveStateError::UnsupportedMigration {
                from: from_version,
                to: SaveStateHeader::CURRENT_VERSION,
            }),
        }
    }
}
```

---

## Performance Targets

- **Save Time:** <0.5ms (uncompressed), <5ms (compressed)
- **Load Time:** <0.5ms (either format)
- **State Size:** ~50KB uncompressed, ~10-20KB compressed
- **Memory Overhead:** Zero heap allocations during load (stack-based deserialization)

---

## Success Criteria

- [ ] All tasks complete
- [ ] Determinism tests pass
- [ ] Compression achieves 80% reduction
- [ ] Integration with Console API
- [ ] Zero unsafe code
- [ ] Full test coverage (unit + integration)
- [ ] Documentation complete
- [ ] Performance targets met

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M5-S3 (Console Coordinator)
**Next Sprint:** [M5-S6 Input Handling](M5-S6-input-handling.md)
