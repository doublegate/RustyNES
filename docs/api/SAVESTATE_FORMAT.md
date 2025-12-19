# RustyNES Save State Format Specification

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete specification for RustyNES save state format, versioning, and serialization

---

## Table of Contents

- [Overview](#overview)
- [Design Philosophy](#design-philosophy)
- [File Format Structure](#file-format-structure)
- [Versioning Strategy](#versioning-strategy)
- [Component Serialization](#component-serialization)
- [Compression and Encoding](#compression-and-encoding)
- [Compatibility](#compatibility)
- [Implementation Guide](#implementation-guide)
- [Testing and Validation](#testing-and-validation)

---

## Overview

RustyNES save states capture the **complete emulator state** at a specific moment, allowing instant save/load functionality. Unlike cartridge battery saves (SRAM), save states include all system registers, RAM, VRAM, and timing information.

**Key Characteristics:**

- Binary format with magic header
- Version-tagged blocks for forward/backward compatibility
- Optional compression (zstd recommended)
- Integrity checking (CRC32 or SHA256)
- Human-readable metadata (JSON header)
- Atomic write operations (write-verify-rename)

**File Extension:** `.rnss` (RustyNES Save State)

---

## Design Philosophy

### 1. Version Tolerance

**Goal:** New emulator versions can load old save states, with graceful degradation for missing features.

**Strategy:**

- Block-based format with version tags
- Unknown blocks are skipped (forward compatibility)
- Missing blocks use default values (backward compatibility)

### 2. Determinism

**Goal:** Loading a save state reproduces **exact** emulator behavior.

**Requirements:**

- All component state must be saved
- Timing counters must be preserved
- Random number generator state (if used)
- Input state and controller latches

### 3. Safety

**Goal:** Prevent data corruption from crashes or power loss.

**Strategy:**

- Atomic writes (temporary file + rename)
- Integrity checksums
- Backup rotation (keep 3 most recent)

### 4. Efficiency

**Goal:** Fast save/load times, reasonable file sizes.

**Strategy:**

- Binary format (not JSON/XML for bulk data)
- Compression (zstd: fast + good ratio)
- Lazy loading (defer PPU CHR decompression)

---

## File Format Structure

### Binary Layout

```
┌─────────────────────────────────────────────────────┐
│ Magic Header (12 bytes)                             │
│   - Magic:    "RNSS" (4 bytes)                      │
│   - Version:  u16 (2 bytes)                         │
│   - Flags:    u16 (2 bytes)                         │
│   - CRC32:    u32 (4 bytes) - header checksum       │
├─────────────────────────────────────────────────────┤
│ Metadata Block (variable, JSON)                     │
│   - Block ID:     0x0001                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - JSON Data:    UTF-8 string                      │
├─────────────────────────────────────────────────────┤
│ CPU State Block (fixed size)                        │
│   - Block ID:     0x0010                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - CPU registers, flags, cycle count              │
├─────────────────────────────────────────────────────┤
│ PPU State Block (variable size)                     │
│   - Block ID:     0x0020                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - PPU registers, VRAM, OAM, palettes             │
├─────────────────────────────────────────────────────┤
│ APU State Block (variable size)                     │
│   - Block ID:     0x0030                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - APU registers, channel state                   │
├─────────────────────────────────────────────────────┤
│ Memory Block (variable size)                        │
│   - Block ID:     0x0040                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - Internal RAM (2 KB)                            │
│   - Cartridge SRAM (if present)                    │
├─────────────────────────────────────────────────────┤
│ Mapper State Block (variable size)                  │
│   - Block ID:     0x0050                            │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - Mapper-specific state                          │
├─────────────────────────────────────────────────────┤
│ Optional Extension Blocks                           │
│   - Block ID:     0x1000+ (extensions)              │
│   - Block Size:   u32                               │
│   - Block Version: u16                              │
│   - Rewind, TAS input, debugger state              │
├─────────────────────────────────────────────────────┤
│ End Marker (8 bytes)                                │
│   - Magic:    "RNSSEND\0" (8 bytes)                 │
│   - Checksum: CRC32 of entire file (in header)     │
└─────────────────────────────────────────────────────┘
```

### Block Structure

Every block follows this format:

```rust
struct BlockHeader {
    block_id: u16,       // Unique block identifier
    block_version: u16,  // Block format version
    block_size: u32,     // Payload size in bytes (excluding header)
}
```

**Total Block Size:** `6 + block_size` bytes

---

## Versioning Strategy

### Version Numbers

**Format Version (in header):**

```
Major.Minor → u16 = (Major << 8) | Minor

Example: Version 1.2 → 0x0102
```

**Compatibility Rules:**

- **Major version change:** Breaking changes (incompatible)
- **Minor version change:** New blocks or fields (compatible)

### Block Versioning

Each block has its own version:

```rust
match block_id {
    0x0010 => {  // CPU State
        match block_version {
            1 => deserialize_cpu_v1(data),
            2 => deserialize_cpu_v2(data),
            _ => Err("Unsupported CPU block version"),
        }
    }
    // ...
}
```

### Forward Compatibility

**Unknown Blocks:** Skip and continue

```rust
if !supported_block_ids.contains(&block_id) {
    reader.skip(block_size)?;  // Skip unknown block
    continue;
}
```

### Backward Compatibility

**Missing Blocks:** Use defaults

```rust
let cpu_state = match find_block(0x0010) {
    Some(block) => deserialize_cpu(block)?,
    None => CpuState::default(),  // Default state
};
```

---

## Component Serialization

### Metadata Block (0x0001)

**JSON format for human-readable metadata:**

```json
{
  "emulator": "RustyNES",
  "emulator_version": "0.1.0",
  "format_version": "1.0",
  "created_at": "2025-12-18T14:30:00Z",
  "rom": {
    "name": "Super Mario Bros.",
    "sha256": "ea343f4e445a9050d4b4fbac2c77d0693b1d0922ffb8b2be75e4a3a2bf81f8b5",
    "mapper": 0,
    "prg_rom_size": 32768,
    "chr_rom_size": 8192
  },
  "screenshot": null,  // Optional base64-encoded thumbnail
  "description": "Before first Bowser",
  "slot": 1
}
```

### CPU State Block (0x0010)

**Fixed-size structure:**

```rust
struct CpuState {
    // Registers (7 bytes)
    a: u8,
    x: u8,
    y: u8,
    sp: u8,
    p: u8,           // Status flags
    pc: u16,

    // Timing (16 bytes)
    cycles: u64,     // Total cycles executed
    cycle_count: u8, // Cycles remaining in current instruction

    // Interrupt state (4 bytes)
    nmi_pending: bool,
    irq_pending: bool,
    irq_line: bool,
    nmi_line: bool,

    // DMA state (8 bytes)
    dma_active: bool,
    dma_page: u8,
    dma_addr: u8,
    dma_cycles: u16,
    oam_dma_offset: u8,
    _padding: [u8; 1],
}
```

**Total Size:** 36 bytes

### PPU State Block (0x0020)

```rust
struct PpuState {
    // Registers (8 bytes)
    ctrl: u8,
    mask: u8,
    status: u8,
    oam_addr: u8,
    scroll_x: u8,
    scroll_y: u8,
    data_buffer: u8,
    _padding: u8,

    // Internal registers (8 bytes)
    v: u16,              // Current VRAM address
    t: u16,              // Temporary VRAM address
    x: u8,               // Fine X scroll
    w: bool,             // Write toggle
    _padding2: [u8; 3],

    // Timing (16 bytes)
    scanline: u16,
    dot: u16,
    frame_count: u64,
    odd_frame: bool,
    _padding3: [u8; 3],

    // Memory (8,512 bytes)
    vram: [u8; 2048],        // Nametables
    palette_ram: [u8; 32],   // Palette RAM
    oam: [u8; 256],          // Object Attribute Memory
    secondary_oam: [u8; 32], // Secondary OAM

    // State flags (4 bytes)
    nmi_occurred: bool,
    nmi_output: bool,
    sprite_0_hit: bool,
    sprite_overflow: bool,
}
```

**Total Size:** ~8,556 bytes

### APU State Block (0x0030)

```rust
struct ApuState {
    // Frame counter (8 bytes)
    frame_counter_mode: u8,
    frame_counter_irq_inhibit: bool,
    frame_counter_step: u8,
    frame_counter_cycles: u32,
    _padding: u8,

    // Pulse 1 (32 bytes)
    pulse1: PulseChannelState,

    // Pulse 2 (32 bytes)
    pulse2: PulseChannelState,

    // Triangle (24 bytes)
    triangle: TriangleChannelState,

    // Noise (24 bytes)
    noise: NoiseChannelState,

    // DMC (48 bytes)
    dmc: DmcChannelState,

    // Enable flags (4 bytes)
    channel_enable: u8,
    frame_irq: bool,
    dmc_irq: bool,
    _padding2: u8,
}

struct PulseChannelState {
    // Envelope (8 bytes)
    envelope_start: bool,
    envelope_loop: bool,
    envelope_constant: bool,
    envelope_reload: u8,
    envelope_decay: u8,
    envelope_divider: u8,
    _padding: [u8; 2],

    // Sweep (8 bytes)
    sweep_enabled: bool,
    sweep_negate: bool,
    sweep_reload: bool,
    sweep_shift: u8,
    sweep_period: u8,
    sweep_divider: u8,
    _padding2: [u8; 2],

    // Timer (8 bytes)
    timer_period: u16,
    timer_counter: u16,
    _padding3: [u8; 4],

    // Sequencer (4 bytes)
    duty: u8,
    sequence_step: u8,
    _padding4: [u8; 2],

    // Length counter (4 bytes)
    length_counter: u8,
    length_halt: bool,
    _padding5: [u8; 2],
}
```

**Total Size:** ~176 bytes

### Memory Block (0x0040)

```rust
struct MemoryState {
    // Internal RAM (2,048 bytes)
    internal_ram: [u8; 0x800],

    // Cartridge SRAM (variable)
    sram_size: u32,
    sram_data: Vec<u8>,  // 0-32768 bytes typical
}
```

**Size:** 2,048 + sram_size + 4 bytes

### Mapper State Block (0x0050)

**Variable format depending on mapper:**

```rust
// Mapper ID (2 bytes)
let mapper_id: u16;

// Mapper-specific state (variable)
match mapper_id {
    0 => {
        // NROM: No state (0 bytes)
    }
    1 => {
        // MMC1: 16 bytes
        // Shift register, bank registers, mirroring
    }
    4 => {
        // MMC3: 32 bytes
        // Bank registers, IRQ counter, IRQ enabled, mirroring
    }
    // ... other mappers
}
```

---

## Compression and Encoding

### Compression Options

**Recommended:** zstd (Zstandard)

- Fast compression/decompression
- Good ratio (typically 60-80% reduction)
- Adjustable levels (1-22)

**Alternative:** LZ4 (fastest) or Deflate (most compatible)

### Compression Implementation

**Level Selection:**

```rust
match priority {
    Priority::Speed => zstd::compress(data, 1),   // Level 1: Fast
    Priority::Balanced => zstd::compress(data, 3), // Level 3: Default
    Priority::Size => zstd::compress(data, 10),    // Level 10: Maximum
}
```

**Flags Field:**

```
Bit 0: Compressed (1 = yes, 0 = no)
Bit 1-2: Algorithm (00 = none, 01 = zstd, 10 = lz4, 11 = deflate)
Bit 3-15: Reserved
```

### Integrity Checking

**CRC32 (fast, good for corruption detection):**

```rust
let checksum = crc32::checksum(&savestate_data);
```

**SHA256 (secure, slower):**

```rust
let hash = sha256::digest(&savestate_data);
```

**Location:** Stored in file header

---

## Compatibility

### ROM Matching

**Problem:** Save states are tied to specific ROM versions.

**Solution:**

1. Store ROM SHA256 hash in metadata
2. Warn if hash doesn't match
3. Allow loading with user confirmation (risk of desyncs)

### Emulator Version Mismatch

**Problem:** Different emulator versions may have incompatible states.

**Solution:**

- Minor version differences: Load with warnings
- Major version differences: Reject or migrate

### Mapper Changes

**Problem:** Mapper implementation changes break old saves.

**Solution:**

- Version mapper state blocks independently
- Provide migration functions for common changes

---

## Implementation Guide

### Save State Creation

```rust
pub struct SaveState {
    metadata: Metadata,
    cpu: CpuState,
    ppu: PpuState,
    apu: ApuState,
    memory: MemoryState,
    mapper: MapperState,
}

impl SaveState {
    pub fn create(console: &Console) -> Self {
        Self {
            metadata: Metadata::from_console(console),
            cpu: console.cpu.save_state(),
            ppu: console.ppu.save_state(),
            apu: console.apu.save_state(),
            memory: console.bus.save_state(),
            mapper: console.cartridge.save_state(),
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        // Atomic write: temp file + rename
        let temp_path = path.with_extension(".rnss.tmp");

        // Serialize
        let mut buffer = Vec::new();
        self.serialize(&mut buffer)?;

        // Compress
        let compressed = zstd::compress(&buffer, 3)?;

        // Write atomically
        fs::write(&temp_path, &compressed)?;
        fs::rename(&temp_path, path)?;

        Ok(())
    }

    fn serialize(&self, writer: &mut impl Write) -> Result<()> {
        // Write header
        writer.write_all(b"RNSS")?;
        writer.write_u16::<LE>(SAVESTATE_VERSION)?;
        writer.write_u16::<LE>(self.flags())?;
        writer.write_u32::<LE>(0)?;  // CRC placeholder

        // Write metadata block
        self.write_block(writer, 0x0001, 1, |w| {
            let json = serde_json::to_string(&self.metadata)?;
            w.write_all(json.as_bytes())?;
            Ok(())
        })?;

        // Write component blocks
        self.write_block(writer, 0x0010, 1, |w| self.cpu.serialize(w))?;
        self.write_block(writer, 0x0020, 1, |w| self.ppu.serialize(w))?;
        self.write_block(writer, 0x0030, 1, |w| self.apu.serialize(w))?;
        self.write_block(writer, 0x0040, 1, |w| self.memory.serialize(w))?;
        self.write_block(writer, 0x0050, 1, |w| self.mapper.serialize(w))?;

        // Write end marker
        writer.write_all(b"RNSSEND\0")?;

        Ok(())
    }

    fn write_block<F>(&self, writer: &mut impl Write, id: u16, version: u16, f: F) -> Result<()>
    where
        F: FnOnce(&mut Vec<u8>) -> Result<()>,
    {
        let mut block_data = Vec::new();
        f(&mut block_data)?;

        writer.write_u16::<LE>(id)?;
        writer.write_u16::<LE>(version)?;
        writer.write_u32::<LE>(block_data.len() as u32)?;
        writer.write_all(&block_data)?;

        Ok(())
    }
}
```

### Save State Loading

```rust
impl SaveState {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        // Read file
        let compressed = fs::read(path)?;

        // Decompress
        let buffer = zstd::decompress(&compressed, 1024 * 1024)?;  // 1 MB max

        // Deserialize
        let mut reader = Cursor::new(buffer);
        Self::deserialize(&mut reader)
    }

    fn deserialize(reader: &mut impl Read) -> Result<Self> {
        // Read header
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != b"RNSS" {
            return Err("Invalid save state file");
        }

        let version = reader.read_u16::<LE>()?;
        let flags = reader.read_u16::<LE>()?;
        let _crc = reader.read_u32::<LE>()?;

        // Read blocks
        let mut blocks = HashMap::new();
        loop {
            let block_id = reader.read_u16::<LE>()?;

            // Check for end marker
            if block_id == 0xFFFF || reader.is_at_end() {
                break;
            }

            let block_version = reader.read_u16::<LE>()?;
            let block_size = reader.read_u32::<LE>()?;

            let mut block_data = vec![0u8; block_size as usize];
            reader.read_exact(&mut block_data)?;

            blocks.insert(block_id, (block_version, block_data));
        }

        // Deserialize components
        let metadata = Self::load_block(&blocks, 0x0001, |data| {
            let json = str::from_utf8(data)?;
            Ok(serde_json::from_str(json)?)
        })?;

        let cpu = Self::load_block(&blocks, 0x0010, CpuState::deserialize)?;
        let ppu = Self::load_block(&blocks, 0x0020, PpuState::deserialize)?;
        let apu = Self::load_block(&blocks, 0x0030, ApuState::deserialize)?;
        let memory = Self::load_block(&blocks, 0x0040, MemoryState::deserialize)?;
        let mapper = Self::load_block(&blocks, 0x0050, MapperState::deserialize)?;

        Ok(Self { metadata, cpu, ppu, apu, memory, mapper })
    }

    fn load_block<T, F>(blocks: &HashMap<u16, (u16, Vec<u8>)>, id: u16, f: F) -> Result<T>
    where
        F: FnOnce(&[u8]) -> Result<T>,
    {
        match blocks.get(&id) {
            Some((version, data)) => f(data),
            None => Err(format!("Missing required block: 0x{:04X}", id)),
        }
    }
}
```

### Backup Rotation

```rust
pub fn save_with_backup(path: &Path, state: &SaveState) -> Result<()> {
    // Rotate backups: file.rnss.2 → file.rnss.3
    for i in (1..3).rev() {
        let old_backup = path.with_extension(format!("rnss.{}", i));
        let new_backup = path.with_extension(format!("rnss.{}", i + 1));
        if old_backup.exists() {
            fs::rename(old_backup, new_backup)?;
        }
    }

    // Move current to backup: file.rnss → file.rnss.1
    if path.exists() {
        let backup = path.with_extension("rnss.1");
        fs::rename(path, backup)?;
    }

    // Save new state
    state.save_to_file(path)?;

    Ok(())
}
```

---

## Testing and Validation

### Unit Tests

```rust
#[test]
fn test_savestate_roundtrip() {
    let console = create_test_console();
    let state = SaveState::create(&console);

    let path = temp_file();
    state.save_to_file(&path).unwrap();

    let loaded = SaveState::load_from_file(&path).unwrap();

    assert_eq!(state.cpu.pc, loaded.cpu.pc);
    assert_eq!(state.cpu.a, loaded.cpu.a);
    // ... validate all fields
}

#[test]
fn test_forward_compatibility() {
    // Create v1.0 save state
    let v1_state = create_v1_savestate();

    // Load with v1.1 emulator (should succeed)
    let loaded = SaveState::load_from_file(&v1_state).unwrap();

    assert!(loaded.is_valid());
}
```

### Integration Tests

Test with actual games:

- Create save state mid-game
- Load and verify game continues correctly
- Test across different mappers
- Test with and without compression

---

## Related Documentation

- [CORE_API.md](CORE_API.md) - Emulator core API
- [CONFIGURATION.md](CONFIGURATION.md) - Configuration options
- [../dev/CONTRIBUTING.md](../dev/CONTRIBUTING.md) - Contribution guidelines

---

## References

- [Adding Save States to an Emulator](https://www.gregorygaines.com/blog/adding-save-states-to-an-emulator/)
- [Best Practices for Preserving Save States](https://greatestjournal.com/entertainment/best-practices-for-preserving-save-states-in-emulators/)
- [Implementing Saving and Loading States in a C# GB Emulator](https://intodot.net/implementing-saving-and-loading-of-states-in-a-c-gb-emulator/)
- [Game Save Systems: Complete Data Persistence Guide 2025](https://generalistprogrammer.com/tutorials/game-save-systems-complete-data-persistence-guide-2025)

---

**Document Status:** Complete specification for save state format with versioning and compatibility.
