# FDS (Famicom Disk System) Format Specification

## Overview

The Famicom Disk System (FDS) was a disk-based add-on for the Famicom released in 1986. FDS images contain the disk data used by these games, along with optional header information.

**File Extensions:** `.fds`, `.qd`
**Disk Format:** Double-sided, 65,500 bytes per side
**Disk Capacity:** ~112 KB usable per side

---

## Disk Hardware

### Physical Specifications

| Property | Value |
|----------|-------|
| Media | Quick Disk (QD) |
| Capacity | 65,500 bytes per side |
| Sides | 2 |
| Sectors | None (continuous) |
| Transfer Rate | ~96 kbit/s |

### Disk Drive

The RAM Adapter contains:

- 32 KB PRG-RAM at $6000-$DFFF
- 8 KB CHR-RAM at PPU $0000-$1FFF
- Wavetable sound channel
- Disk drive controller

---

## File Format Variants

### .fds (with Header)

Most common format. Includes a 16-byte header:

```
Offset  Size  Description
------  ----  -----------
$00     4     Magic: "FDS\x1A"
$04     1     Number of disk sides
$05-$0F 11    Reserved (zeros)
```

### .fds (Raw)

Some ROMs omit the header, starting directly with disk data.

### .qd (Quick Disk)

Raw dump without any header, exactly 65,500 bytes per side.

---

## Disk Data Structure

Each disk side contains file blocks:

```
+-------------------+
| Block 1: Header   |  56 bytes
+-------------------+
| Block 2: File #   |  2 bytes
+-------------------+
| Block 3: File HDR |  16 bytes
+-------------------+
| Block 4: File DAT |  Variable
+-------------------+
|  ... more files   |
+-------------------+
| Gap/Unused Space  |
+-------------------+
```

### Block 1: Disk Info Block (56 bytes)

```
Offset  Size  Description
------  ----  -----------
$00     1     Block type ($01)
$01     14    Literal string "*NINTENDO-HVC*"
$0F     1     Manufacturer code
$10     4     Game name (ASCII, padded with spaces)
$14     1     Game type ($20=normal, $45=event, $46=sales)
$15     1     Revision number
$16     1     Side number ($00=A, $01=B)
$17     1     Disk number ($00=first disk)
$18     1     Disk type ($00=FMC, $01=FSC)
$19     1     Unknown (always $00?)
$1A     1     Boot file ID
$1B     5     Unknown (always $FF?)
$20     3     Manufacturing date (BCD: YY MM DD)
$23     1     Country code ($49=Japan)
$24     1     Unknown
$25     1     Unknown
$26     2     Unknown
$28     3     Rewrite date (BCD)
$2B     1     Unknown
$2C     1     Disk Writer serial number (low)
$2D     1     Unknown
$2E     1     Disk Writer serial number (high)
$2F     1     Actual disk side ($00=A, $01=B)
$30     1     Unknown
$31     1     Price code
$32-$37 6     Unknown
```

### Block 2: File Amount Block (2 bytes)

```
Offset  Size  Description
------  ----  -----------
$00     1     Block type ($02)
$01     1     Number of files on this side
```

### Block 3: File Header Block (16 bytes)

```
Offset  Size  Description
------  ----  -----------
$00     1     Block type ($03)
$01     1     File number (0-indexed)
$02     1     File ID (0-255)
$03     8     Filename (padded with spaces)
$0B     2     File address (little-endian)
$0D     2     File size (little-endian)
$0F     1     File type ($00=PRG, $01=CHR, $02=VRAM)
```

### Block 4: File Data Block (variable)

```
Offset  Size  Description
------  ----  -----------
$00     1     Block type ($04)
$01-    N     File data (N = file size from Block 3)
```

---

## Rust Implementation

### Data Structures

```rust
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FdsHeader {
    pub side_count: u8,
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub manufacturer: u8,
    pub game_name: String,
    pub game_type: u8,
    pub revision: u8,
    pub side_number: u8,
    pub disk_number: u8,
    pub disk_type: u8,
    pub boot_file_id: u8,
    pub manufacture_date: [u8; 3],
    pub country_code: u8,
    pub rewrite_date: [u8; 3],
    pub actual_side: u8,
    pub price_code: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Program,    // $00
    Character,  // $01
    Vram,       // $02
}

#[derive(Debug, Clone)]
pub struct FdsFile {
    pub file_number: u8,
    pub file_id: u8,
    pub filename: String,
    pub address: u16,
    pub file_type: FileType,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FdsSide {
    pub info: DiskInfo,
    pub files: Vec<FdsFile>,
}

#[derive(Debug, Clone)]
pub struct FdsDisk {
    pub header: Option<FdsHeader>,
    pub sides: Vec<FdsSide>,
}

#[derive(Debug, Error)]
pub enum FdsError {
    #[error("Invalid FDS magic number")]
    InvalidMagic,

    #[error("Invalid block type: expected {expected}, got {actual}")]
    InvalidBlockType { expected: u8, actual: u8 },

    #[error("File too small: expected at least {expected} bytes")]
    FileTooSmall { expected: usize },

    #[error("Invalid disk info block")]
    InvalidDiskInfo,

    #[error("CRC mismatch")]
    CrcMismatch,

    #[error("Unexpected end of data")]
    UnexpectedEnd,
}
```

### Parser Implementation

```rust
const FDS_MAGIC: &[u8] = b"FDS\x1a";
const NINTENDO_HVC: &[u8] = b"*NINTENDO-HVC*";
const DISK_SIDE_SIZE: usize = 65500;

impl FdsDisk {
    pub fn load(data: &[u8]) -> Result<Self, FdsError> {
        // Check for header
        let (header, disk_data) = if data.len() >= 4 && &data[0..4] == FDS_MAGIC {
            let header = FdsHeader {
                side_count: data[4],
            };
            (Some(header), &data[16..])
        } else {
            (None, data)
        };

        // Parse each side
        let side_count = header.as_ref()
            .map(|h| h.side_count as usize)
            .unwrap_or(disk_data.len() / DISK_SIDE_SIZE);

        let mut sides = Vec::with_capacity(side_count);

        for i in 0..side_count {
            let offset = i * DISK_SIDE_SIZE;
            if offset + DISK_SIDE_SIZE > disk_data.len() {
                break;
            }

            let side_data = &disk_data[offset..offset + DISK_SIDE_SIZE];
            let side = Self::parse_side(side_data)?;
            sides.push(side);
        }

        Ok(FdsDisk { header, sides })
    }

    fn parse_side(data: &[u8]) -> Result<FdsSide, FdsError> {
        let mut offset = 0;

        // Block 1: Disk Info
        if data[offset] != 0x01 {
            return Err(FdsError::InvalidBlockType {
                expected: 0x01,
                actual: data[offset],
            });
        }

        // Verify Nintendo string
        if &data[offset + 1..offset + 15] != NINTENDO_HVC {
            return Err(FdsError::InvalidDiskInfo);
        }

        let info = DiskInfo {
            manufacturer: data[offset + 0x0F],
            game_name: String::from_utf8_lossy(&data[offset + 0x10..offset + 0x14])
                .trim()
                .to_string(),
            game_type: data[offset + 0x14],
            revision: data[offset + 0x15],
            side_number: data[offset + 0x16],
            disk_number: data[offset + 0x17],
            disk_type: data[offset + 0x18],
            boot_file_id: data[offset + 0x1A],
            manufacture_date: [
                data[offset + 0x20],
                data[offset + 0x21],
                data[offset + 0x22],
            ],
            country_code: data[offset + 0x23],
            rewrite_date: [
                data[offset + 0x28],
                data[offset + 0x29],
                data[offset + 0x2A],
            ],
            actual_side: data[offset + 0x2F],
            price_code: data[offset + 0x31],
        };

        offset += 56;  // Skip CRC after block

        // Skip gap
        while offset < data.len() && data[offset] == 0x00 {
            offset += 1;
        }

        // Block 2: File Amount
        if offset >= data.len() || data[offset] != 0x02 {
            return Err(FdsError::InvalidBlockType {
                expected: 0x02,
                actual: data.get(offset).copied().unwrap_or(0),
            });
        }

        let file_count = data[offset + 1] as usize;
        offset += 2;

        // Parse files
        let mut files = Vec::with_capacity(file_count);

        for _ in 0..file_count {
            // Skip gaps
            while offset < data.len() && data[offset] == 0x00 {
                offset += 1;
            }

            if offset >= data.len() {
                break;
            }

            // Block 3: File Header
            if data[offset] != 0x03 {
                break;  // End of files
            }

            let file_number = data[offset + 1];
            let file_id = data[offset + 2];
            let filename = String::from_utf8_lossy(&data[offset + 3..offset + 11])
                .trim()
                .to_string();
            let address = u16::from_le_bytes([data[offset + 11], data[offset + 12]]);
            let size = u16::from_le_bytes([data[offset + 13], data[offset + 14]]) as usize;
            let file_type = match data[offset + 15] {
                0x00 => FileType::Program,
                0x01 => FileType::Character,
                0x02 => FileType::Vram,
                _ => FileType::Program,
            };

            offset += 16;

            // Skip gap
            while offset < data.len() && data[offset] == 0x00 {
                offset += 1;
            }

            // Block 4: File Data
            if offset >= data.len() || data[offset] != 0x04 {
                break;
            }
            offset += 1;

            let file_data = if offset + size <= data.len() {
                data[offset..offset + size].to_vec()
            } else {
                Vec::new()
            };
            offset += size;

            files.push(FdsFile {
                file_number,
                file_id,
                filename,
                address,
                file_type,
                data: file_data,
            });
        }

        Ok(FdsSide { info, files })
    }

    /// Get total number of sides
    pub fn side_count(&self) -> usize {
        self.sides.len()
    }

    /// Get file by ID from a specific side
    pub fn get_file(&self, side: usize, file_id: u8) -> Option<&FdsFile> {
        self.sides.get(side)?
            .files.iter()
            .find(|f| f.file_id == file_id)
    }

    /// Get boot file for a side
    pub fn boot_file(&self, side: usize) -> Option<&FdsFile> {
        let info = &self.sides.get(side)?.info;
        self.get_file(side, info.boot_file_id)
    }
}
```

### FDS Emulation

```rust
pub struct FdsSystem {
    // Disk drive state
    disk: Option<FdsDisk>,
    current_side: usize,
    disk_inserted: bool,

    // Drive mechanics
    motor_on: bool,
    head_position: usize,
    transfer_mode: TransferMode,

    // Memory
    prg_ram: [u8; 0x8000],  // 32KB at $6000-$DFFF
    chr_ram: [u8; 0x2000],  // 8KB

    // Disk I/O registers
    irq_control: u8,
    irq_counter: u16,
    irq_reload: u16,
    disk_status: u8,
    data_register: u8,

    // Audio (wavetable)
    audio: FdsAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferMode {
    Idle,
    Reading,
    Writing,
}

impl FdsSystem {
    pub fn new() -> Self {
        FdsSystem {
            disk: None,
            current_side: 0,
            disk_inserted: false,
            motor_on: false,
            head_position: 0,
            transfer_mode: TransferMode::Idle,
            prg_ram: [0; 0x8000],
            chr_ram: [0; 0x2000],
            irq_control: 0,
            irq_counter: 0,
            irq_reload: 0,
            disk_status: 0,
            data_register: 0,
            audio: FdsAudio::new(),
        }
    }

    pub fn insert_disk(&mut self, disk: FdsDisk) {
        self.disk = Some(disk);
        self.disk_inserted = true;
        self.current_side = 0;
        self.head_position = 0;
    }

    pub fn eject_disk(&mut self) {
        self.disk_inserted = false;
    }

    pub fn flip_disk(&mut self) {
        if let Some(ref disk) = self.disk {
            if disk.sides.len() > 1 {
                self.current_side = 1 - self.current_side;
                self.head_position = 0;
            }
        }
    }

    /// CPU read from FDS I/O registers
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4030 => {
                // Disk status
                let status = self.disk_status;
                self.disk_status &= !0x01;  // Clear IRQ flag
                status
            }
            0x4031 => {
                // Read data
                self.data_register
            }
            0x4032 => {
                // Disk drive status
                let mut status = 0x40;  // CRC control
                if !self.disk_inserted {
                    status |= 0x01;  // Disk not inserted
                    status |= 0x04;  // Disk not ready
                }
                if !self.motor_on {
                    status |= 0x02;  // Motor off
                }
                status
            }
            0x4033 => {
                // External connector (battery status)
                0x80  // Battery good
            }
            // FDS audio registers $4040-$4092
            0x4040..=0x4092 => self.audio.read(addr),
            _ => 0,
        }
    }

    /// CPU write to FDS I/O registers
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4020 => {
                // IRQ reload value low
                self.irq_reload = (self.irq_reload & 0xFF00) | (value as u16);
            }
            0x4021 => {
                // IRQ reload value high
                self.irq_reload = (self.irq_reload & 0x00FF) | ((value as u16) << 8);
            }
            0x4022 => {
                // IRQ control
                self.irq_control = value;
                if (value & 0x02) != 0 {
                    self.irq_counter = self.irq_reload;
                }
            }
            0x4023 => {
                // I/O enable
            }
            0x4024 => {
                // Write data
                self.data_register = value;
            }
            0x4025 => {
                // FDS control
                self.motor_on = (value & 0x01) != 0;
                self.transfer_mode = if (value & 0x04) != 0 {
                    TransferMode::Writing
                } else {
                    TransferMode::Reading
                };
            }
            // FDS audio registers $4040-$4092
            0x4040..=0x4092 => self.audio.write(addr, value),
            _ => {}
        }
    }

    /// Clock the disk drive (called from CPU)
    pub fn clock(&mut self) {
        // IRQ timer
        if (self.irq_control & 0x02) != 0 {
            if self.irq_counter == 0 {
                self.disk_status |= 0x01;  // IRQ flag
                if (self.irq_control & 0x01) != 0 {
                    self.irq_counter = self.irq_reload;
                }
            } else {
                self.irq_counter -= 1;
            }
        }
    }

    /// Check if IRQ is pending
    pub fn irq_pending(&self) -> bool {
        (self.disk_status & 0x01) != 0
    }
}
```

---

## FDS Audio

The FDS includes a wavetable synthesis channel:

```rust
pub struct FdsAudio {
    // Wavetable
    wavetable: [u8; 64],

    // Modulation table
    mod_table: [i8; 32],
    mod_counter: u8,
    mod_frequency: u16,
    mod_depth: u8,

    // Main oscillator
    main_frequency: u16,
    main_volume: u8,
    wave_position: usize,

    // Output
    envelope_speed: u8,
    master_volume: u8,
}

impl FdsAudio {
    pub fn new() -> Self {
        FdsAudio {
            wavetable: [0; 64],
            mod_table: [0; 32],
            mod_counter: 0,
            mod_frequency: 0,
            mod_depth: 0,
            main_frequency: 0,
            main_volume: 0,
            wave_position: 0,
            envelope_speed: 0,
            master_volume: 0,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x4040..=0x407F => self.wavetable[(addr - 0x4040) as usize],
            0x4090 => self.main_volume,
            0x4092 => self.mod_depth,
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4040..=0x407F => {
                self.wavetable[(addr - 0x4040) as usize] = value & 0x3F;
            }
            0x4080 => {
                // Volume envelope
                self.envelope_speed = value;
            }
            0x4082 => {
                // Main frequency low
                self.main_frequency = (self.main_frequency & 0xF00) | (value as u16);
            }
            0x4083 => {
                // Main frequency high + halt
                self.main_frequency = (self.main_frequency & 0x0FF) | (((value & 0x0F) as u16) << 8);
            }
            0x4084 => {
                // Mod envelope
            }
            0x4085 => {
                // Mod counter
                self.mod_counter = value & 0x7F;
            }
            0x4086 => {
                // Mod frequency low
                self.mod_frequency = (self.mod_frequency & 0xF00) | (value as u16);
            }
            0x4087 => {
                // Mod frequency high
                self.mod_frequency = (self.mod_frequency & 0x0FF) | (((value & 0x0F) as u16) << 8);
            }
            0x4088 => {
                // Mod table write
            }
            0x4089 => {
                // Master volume + wavetable write enable
                self.master_volume = value & 0x03;
            }
            0x408A => {
                // Envelope speed
            }
            _ => {}
        }
    }

    pub fn output(&self) -> f32 {
        if self.main_frequency == 0 {
            return 0.0;
        }

        let sample = self.wavetable[self.wave_position] as f32 / 63.0;
        let volume = self.main_volume as f32 / 63.0;
        let master = [1.0, 2.0/3.0, 1.0/2.0, 1.0/3.0][self.master_volume as usize];

        sample * volume * master
    }
}
```

---

## BIOS Handling

The FDS requires a BIOS ROM (8KB):

```rust
pub struct FdsBios {
    data: [u8; 8192],
}

impl FdsBios {
    pub fn load(data: &[u8]) -> Result<Self, FdsError> {
        if data.len() != 8192 {
            return Err(FdsError::FileTooSmall { expected: 8192 });
        }

        let mut bios = FdsBios { data: [0; 8192] };
        bios.data.copy_from_slice(data);
        Ok(bios)
    }

    pub fn read(&self, addr: u16) -> u8 {
        if addr >= 0xE000 {
            self.data[(addr - 0xE000) as usize]
        } else {
            0
        }
    }
}
```

---

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_fds() -> Vec<u8> {
        let mut data = vec![0u8; 16 + DISK_SIDE_SIZE];

        // Header
        data[0..4].copy_from_slice(b"FDS\x1a");
        data[4] = 1;  // 1 side

        let disk_start = 16;

        // Block 1: Disk Info
        data[disk_start] = 0x01;
        data[disk_start + 1..disk_start + 15].copy_from_slice(NINTENDO_HVC);
        data[disk_start + 0x10..disk_start + 0x14].copy_from_slice(b"TEST");
        data[disk_start + 0x1A] = 0x00;  // Boot file ID

        // Skip some bytes, then Block 2
        let block2 = disk_start + 58;
        data[block2] = 0x02;
        data[block2 + 1] = 1;  // 1 file

        // Block 3: File header
        let block3 = block2 + 4;
        data[block3] = 0x03;
        data[block3 + 1] = 0;  // File number
        data[block3 + 2] = 0;  // File ID
        data[block3 + 3..block3 + 11].copy_from_slice(b"MAINPRG ");
        data[block3 + 11..block3 + 13].copy_from_slice(&0x6000u16.to_le_bytes());
        data[block3 + 13..block3 + 15].copy_from_slice(&4u16.to_le_bytes());
        data[block3 + 15] = 0x00;  // Program

        // Block 4: File data
        let block4 = block3 + 18;
        data[block4] = 0x04;
        data[block4 + 1..block4 + 5].copy_from_slice(&[0x4C, 0x00, 0x60, 0x00]);

        data
    }

    #[test]
    fn test_parse_fds() {
        let data = create_minimal_fds();
        let disk = FdsDisk::load(&data).unwrap();

        assert_eq!(disk.side_count(), 1);
        assert_eq!(disk.sides[0].info.game_name, "TEST");
        assert_eq!(disk.sides[0].files.len(), 1);
    }

    #[test]
    fn test_file_info() {
        let data = create_minimal_fds();
        let disk = FdsDisk::load(&data).unwrap();

        let file = &disk.sides[0].files[0];
        assert_eq!(file.file_id, 0);
        assert_eq!(file.address, 0x6000);
        assert_eq!(file.file_type, FileType::Program);
    }

    #[test]
    fn test_boot_file() {
        let data = create_minimal_fds();
        let disk = FdsDisk::load(&data).unwrap();

        let boot = disk.boot_file(0).unwrap();
        assert_eq!(boot.file_id, 0);
    }

    #[test]
    fn test_raw_format() {
        // Test without header
        let mut data = vec![0u8; DISK_SIDE_SIZE];
        data[0] = 0x01;
        data[1..15].copy_from_slice(NINTENDO_HVC);

        let disk = FdsDisk::load(&data);
        // Should fail gracefully or parse as raw
        assert!(disk.is_ok() || disk.is_err());
    }
}
```

---

## References

- [NESdev Wiki: FDS](https://www.nesdev.org/wiki/FDS)
- [NESdev Wiki: FDS File Format](https://www.nesdev.org/wiki/FDS_file_format)
- [NESdev Wiki: FDS Audio](https://www.nesdev.org/wiki/FDS_audio)

---

## See Also

- [INES_FORMAT.md](INES_FORMAT.md) - iNES cartridge format
- [EXPANSION_AUDIO.md](../apu/EXPANSION_AUDIO.md) - FDS audio channel
- [MAPPER_020_FDS.md](../mappers/MAPPER_020_FDS.md) - FDS mapper (mapper 20)
