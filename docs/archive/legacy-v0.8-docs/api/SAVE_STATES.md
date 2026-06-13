# Save States

**Table of Contents**

- [Overview](#overview)
- [Save State Format](#save-state-format)
- [API](#api)
- [Implementation](#implementation)
- [Versioning](#versioning)
- [Compatibility](#compatibility)
- [Best Practices](#best-practices)

---

## Overview

**Save states** allow instant saving and loading of complete emulator state, enabling features like rewind, quick save/load, and tool-assisted speedruns (TAS).

### Features

- **Instant save/load**: Capture complete system state
- **Rewind**: Ring buffer of recent states
- **TAS support**: Frame-perfect state management
- **Versioning**: Forward/backward compatibility
- **Compression**: Optional zlib compression

---

## Save State Format

### Structure

```
Save State File (.state):
├── Header (64 bytes)
│   ├── Magic ("RNES")
│   ├── Version (u32)
│   ├── Checksum (CRC32)
│   ├── Flags (compressed, etc.)
│   └── Metadata (ROM hash, timestamp)
├── CPU State (32 bytes)
├── PPU State (512 bytes)
├── APU State (128 bytes)
├── Bus State (RAM, 2KB)
├── Cartridge State (PRG-RAM, varies)
└── Mapper State (varies by mapper)
```

### Header Format

```rust
#[repr(C)]
pub struct SaveStateHeader {
    magic: [u8; 4],           // "RNES"
    version: u32,             // Format version (current: 1)
    checksum: u32,            // CRC32 of remaining data
    flags: u32,               // Bit flags (compressed, etc.)
    rom_hash: [u8; 32],       // SHA-256 of ROM
    timestamp: u64,           // Unix timestamp
    frame_count: u64,         // Frame number when saved
    reserved: [u8; 8],        // Future use
}
```

---

## API

### Saving State

```rust
impl Console {
    /// Save complete console state to bytes
    pub fn save_state(&self) -> Result<Vec<u8>, SaveStateError> {
        let mut state = Vec::new();

        // Write header
        state.extend_from_slice(&self.serialize_header()?);

        // Write components
        state.extend_from_slice(&self.cpu.serialize()?);
        state.extend_from_slice(&self.ppu.serialize()?);
        state.extend_from_slice(&self.apu.serialize()?);
        state.extend_from_slice(&self.bus.serialize()?);
        state.extend_from_slice(&self.cartridge.serialize()?);

        Ok(state)
    }

    /// Save state to file
    pub fn save_state_to_file(&self, path: &Path) -> Result<(), SaveStateError> {
        let state = self.save_state()?;
        std::fs::write(path, &state)?;
        Ok(())
    }
}
```

### Loading State

```rust
impl Console {
    /// Load console state from bytes
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        // Verify header
        let header = SaveStateHeader::deserialize(&data[0..64])?;
        header.verify()?;

        // Check ROM compatibility
        if header.rom_hash != self.rom_hash() {
            return Err(SaveStateError::RomMismatch);
        }

        // Load components
        let mut offset = 64;
        offset += self.cpu.deserialize(&data[offset..])?;
        offset += self.ppu.deserialize(&data[offset..])?;
        offset += self.apu.deserialize(&data[offset..])?;
        offset += self.bus.deserialize(&data[offset..])?;
        offset += self.cartridge.deserialize(&data[offset..])?;

        Ok(())
    }

    /// Load state from file
    pub fn load_state_from_file(&mut self, path: &Path) -> Result<(), SaveStateError> {
        let data = std::fs::read(path)?;
        self.load_state(&data)
    }
}
```

---

## Implementation

### Component Serialization

```rust
pub trait Serializable {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError>;
    fn deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError>;
}

impl Serializable for Cpu {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut data = Vec::with_capacity(32);

        // Registers
        data.push(self.a);
        data.push(self.x);
        data.push(self.y);
        data.push(self.sp);
        data.extend_from_slice(&self.pc.to_le_bytes());
        data.push(self.p.bits());

        // State
        data.extend_from_slice(&self.cycles.to_le_bytes());
        data.push(self.nmi_pending as u8);
        data.push(self.irq_pending as u8);

        Ok(data)
    }

    fn deserialize(&mut self, data: &[u8]) -> Result<usize, SerializeError> {
        if data.len() < 32 {
            return Err(SerializeError::InsufficientData);
        }

        let mut offset = 0;

        self.a = data[offset]; offset += 1;
        self.x = data[offset]; offset += 1;
        self.y = data[offset]; offset += 1;
        self.sp = data[offset]; offset += 1;
        self.pc = u16::from_le_bytes([data[offset], data[offset + 1]]); offset += 2;
        self.p = Status::from_bits_truncate(data[offset]); offset += 1;

        self.cycles = u64::from_le_bytes(data[offset..offset + 8].try_into()?); offset += 8;
        self.nmi_pending = data[offset] != 0; offset += 1;
        self.irq_pending = data[offset] != 0; offset += 1;

        Ok(offset)
    }
}
```

### Compression

```rust
use flate2::Compression;
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;

impl Console {
    pub fn save_state_compressed(&self) -> Result<Vec<u8>, SaveStateError> {
        let state = self.save_state()?;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&state)?;
        let compressed = encoder.finish()?;

        Ok(compressed)
    }

    pub fn load_state_compressed(&mut self, compressed: &[u8]) -> Result<(), SaveStateError> {
        let mut decoder = ZlibDecoder::new(compressed);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;

        self.load_state(&decompressed)
    }
}
```

---

## Versioning

### Version History

| Version | Changes | Compatibility |
|---------|---------|---------------|
| 1 | Initial format | N/A |
| 2 | Added APU envelope state | Backward compatible |
| 3 | Extended mapper state | Breaking change |

### Version Checking

```rust
impl SaveStateHeader {
    pub fn verify(&self) -> Result<(), SaveStateError> {
        // Check magic
        if &self.magic != b"RNES" {
            return Err(SaveStateError::InvalidMagic);
        }

        // Check version
        match self.version {
            1..=CURRENT_VERSION => Ok(()),
            _ => Err(SaveStateError::UnsupportedVersion(self.version)),
        }
    }
}
```

### Migration

```rust
impl Console {
    fn migrate_save_state(&self, data: &[u8], from_version: u32) -> Result<Vec<u8>, SaveStateError> {
        match (from_version, CURRENT_VERSION) {
            (1, 2) => self.migrate_v1_to_v2(data),
            (2, 3) => self.migrate_v2_to_v3(data),
            _ => Err(SaveStateError::UnsupportedMigration),
        }
    }
}
```

---

## Compatibility

### ROM Verification

**Ensure save state matches ROM**:

```rust
use sha2::{Sha256, Digest};

impl Console {
    fn rom_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.cartridge.prg_rom);
        hasher.update(&self.cartridge.chr_rom);
        hasher.finalize().into()
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        let header = SaveStateHeader::deserialize(&data[0..64])?;

        // Verify ROM matches
        if header.rom_hash != self.rom_hash() {
            return Err(SaveStateError::RomMismatch {
                expected: header.rom_hash,
                actual: self.rom_hash(),
            });
        }

        // Load state...
    }
}
```

### Checksum Validation

```rust
use crc32fast::Hasher;

impl SaveStateHeader {
    fn compute_checksum(data: &[u8]) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&data[64..]); // Skip header
        hasher.finalize()
    }

    pub fn verify_checksum(&self, data: &[u8]) -> Result<(), SaveStateError> {
        let computed = Self::compute_checksum(data);
        if computed != self.checksum {
            return Err(SaveStateError::ChecksumMismatch);
        }
        Ok(())
    }
}
```

---

## Best Practices

### When to Save

**Good times**:

- At frame boundaries (after `step_frame()`)
- During VBlank (deterministic state)
- User-triggered (explicit save/load)

**Avoid**:

- Mid-instruction
- During DMA
- Arbitrary cycle counts

### Rewind Implementation

**Ring buffer of states**:

```rust
pub struct RewindBuffer {
    states: VecDeque<Vec<u8>>,
    capacity: usize, // e.g., 600 states = 10 seconds at 60 FPS
}

impl RewindBuffer {
    pub fn push(&mut self, state: Vec<u8>) {
        if self.states.len() >= self.capacity {
            self.states.pop_front();
        }
        self.states.push_back(state);
    }

    pub fn pop(&mut self) -> Option<Vec<u8>> {
        self.states.pop_back()
    }
}

// Usage
let mut rewind = RewindBuffer::new(600);

// Every frame
rewind.push(console.save_state()?);

// Rewind one frame
if let Some(state) = rewind.pop() {
    console.load_state(&state)?;
}
```

### Performance

**State size** (approximate):

- Uncompressed: ~50KB
- Compressed: ~10-20KB (80% reduction)

**Save/load time**:

- Uncompressed: ~0.1ms
- Compressed: ~2-5ms

**For rewind**: Use uncompressed states (speed critical)
**For storage**: Use compressed states (save disk space)

---

## References

- [CORE_API.md](CORE_API.md) - Console API
- [CONFIGURATION.md](CONFIGURATION.md) - Configuration options

---

**Related Documents**:

- [CORE_API.md](CORE_API.md) - Embedding guide
- [ARCHITECTURE.md](../ARCHITECTURE.md) - System design
