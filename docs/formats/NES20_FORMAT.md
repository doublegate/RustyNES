# NES 2.0 ROM Format Specification

## Overview

NES 2.0 is a backward-compatible extension of the iNES format that addresses limitations in the original specification. It provides support for larger ROMs, submappers, accurate RAM sizes, and region information.

**Format Version:** NES 2.0
**File Extension:** `.nes`
**Magic Number:** `$4E $45 $53 $1A` (same as iNES)
**Identifier:** Bits 2-3 of byte 7 == `%10` (0x08)

---

## Detection

NES 2.0 ROMs are identified by checking bits 2-3 of header byte 7:

```rust
fn is_nes20_format(header: &[u8; 16]) -> bool {
    (header[7] & 0x0C) == 0x08
}
```

This check must be performed before parsing to determine which format specification to use.

---

## Header Structure

The NES 2.0 header is 16 bytes (same size as iNES), but with extended meaning:

```
Offset  Size  Description
------  ----  -----------
0-3     4     Magic number: "NES\x1A"
4       1     PRG-ROM size LSB (in 16KB units)
5       1     CHR-ROM size LSB (in 8KB units)
6       1     Flags 6: Mapper low, mirroring, battery, trainer
7       1     Flags 7: Mapper mid, NES 2.0 identifier ($x8)
8       1     Flags 8: Mapper high, submapper
9       1     Flags 9: PRG-ROM/CHR-ROM size MSB
10      1     Flags 10: PRG-RAM/EEPROM size
11      1     Flags 11: CHR-RAM size
12      1     Flags 12: CPU/PPU timing mode
13      1     Flags 13: VS System type / Extended console type
14      1     Flags 14: Miscellaneous ROMs
15      1     Flags 15: Default expansion device
```

---

## Byte-by-Byte Specification

### Bytes 0-3: Magic Number

Identical to iNES:

```rust
const NES_MAGIC: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A];
```

### Byte 4: PRG-ROM Size LSB

Lower 8 bits of PRG-ROM size. Combined with byte 9 bits 0-3 for full size.

### Byte 5: CHR-ROM Size LSB

Lower 8 bits of CHR-ROM size. Combined with byte 9 bits 4-7 for full size.

### Byte 6: Flags 6

Same as iNES 1.0:

```
7  bit  0
---------
NNNN FTBM

N: Mapper number bits 0-3
F: Four-screen VRAM
T: Trainer present
B: Battery-backed memory
M: Nametable mirroring (0=horizontal, 1=vertical)
```

### Byte 7: Flags 7

```
7  bit  0
---------
NNNN CCVS

N: Mapper number bits 4-7
C: Console type (must be %10 for NES 2.0)
V: VS Unisystem / Extended console flag
S: Playchoice-10 / Extended console flag
```

**Console Type (bits 2-3):**

| Value | Meaning |
|-------|---------|
| 0     | iNES 1.0 format |
| 1     | iNES 1.0 (archaic) |
| 2     | NES 2.0 format |
| 3     | iNES 1.0 (archaic) |

### Byte 8: Mapper MSB / Submapper

```
7  bit  0
---------
SSSS NNNN

S: Submapper number (0-15)
N: Mapper number bits 8-11
```

**Mapper Number Calculation:**

```rust
fn get_mapper_nes20(header: &[u8; 16]) -> u16 {
    let low = (header[6] >> 4) as u16;
    let mid = (header[7] & 0xF0) as u16;
    let high = ((header[8] & 0x0F) as u16) << 8;
    high | mid | low
}

fn get_submapper(header: &[u8; 16]) -> u8 {
    (header[8] >> 4) & 0x0F
}
```

**Total Mapper Range:** 0-4095 (12-bit)
**Submapper Range:** 0-15 (4-bit)

### Byte 9: ROM Size MSB

```
7  bit  0
---------
CCCC PPPP

C: CHR-ROM size MSB (bits 8-11)
P: PRG-ROM size MSB (bits 8-11)
```

**Size Calculation:**

For values 0-14 (exponent mode not used):

```rust
fn get_prg_size_nes20(header: &[u8; 16]) -> usize {
    let lsb = header[4] as usize;
    let msb = (header[9] & 0x0F) as usize;
    let size_units = (msb << 8) | lsb;

    if msb == 0x0F {
        // Exponent mode: size = 2^E × (MM×2+1)
        let exponent = (lsb >> 2) & 0x3F;
        let multiplier = lsb & 0x03;
        (1 << exponent) * (multiplier * 2 + 1)
    } else {
        // Linear mode: size = units × 16KB
        size_units * 16384
    }
}

fn get_chr_size_nes20(header: &[u8; 16]) -> usize {
    let lsb = header[5] as usize;
    let msb = ((header[9] >> 4) & 0x0F) as usize;
    let size_units = (msb << 8) | lsb;

    if msb == 0x0F {
        // Exponent mode: size = 2^E × (MM×2+1)
        let exponent = (lsb >> 2) & 0x3F;
        let multiplier = lsb & 0x03;
        (1 << exponent) * (multiplier * 2 + 1)
    } else {
        // Linear mode: size = units × 8KB
        size_units * 8192
    }
}
```

**Exponent Mode (when MSB = $F):**

When the MSB nibble is $F, the LSB byte uses exponent notation:

```
LSB: EEEE EEMM
E: Exponent (6 bits)
M: Multiplier (2 bits)
Size = 2^E × (M×2 + 1)
```

This allows expressing sizes like 3MB, 5MB, etc. that aren't powers of 2.

### Byte 10: PRG-RAM / PRG-NVRAM Size

```
7  bit  0
---------
pppp PPPP

p: PRG-NVRAM (battery-backed) shift count
P: PRG-RAM (volatile) shift count
```

**Size Calculation:**

```rust
fn get_prg_ram_size(header: &[u8; 16]) -> usize {
    let shift = header[10] & 0x0F;
    if shift == 0 { 0 } else { 64 << shift }
}

fn get_prg_nvram_size(header: &[u8; 16]) -> usize {
    let shift = (header[10] >> 4) & 0x0F;
    if shift == 0 { 0 } else { 64 << shift }
}
```

**Shift Count Table:**

| Shift | Size |
|-------|------|
| 0     | 0 (none) |
| 1     | 128 bytes |
| 2     | 256 bytes |
| 3     | 512 bytes |
| 4     | 1 KB |
| 5     | 2 KB |
| 6     | 4 KB |
| 7     | 8 KB |
| 8     | 16 KB |
| 9     | 32 KB |
| 10    | 64 KB |
| 11    | 128 KB |
| 12    | 256 KB |
| 13    | 512 KB |
| 14    | 1 MB |
| 15    | Reserved |

### Byte 11: CHR-RAM / CHR-NVRAM Size

```
7  bit  0
---------
cccc CCCC

c: CHR-NVRAM (battery-backed) shift count
C: CHR-RAM (volatile) shift count
```

**Size Calculation:**

```rust
fn get_chr_ram_size(header: &[u8; 16]) -> usize {
    let shift = header[11] & 0x0F;
    if shift == 0 { 0 } else { 64 << shift }
}

fn get_chr_nvram_size(header: &[u8; 16]) -> usize {
    let shift = (header[11] >> 4) & 0x0F;
    if shift == 0 { 0 } else { 64 << shift }
}
```

### Byte 12: CPU/PPU Timing Mode

```
7  bit  0
---------
xxxx xxTT

T: Timing mode
```

| Value | Mode | Frame Rate | CPU Clock |
|-------|------|------------|-----------|
| 0     | NTSC | 60.0988 Hz | 1.789773 MHz |
| 1     | PAL | 50.0070 Hz | 1.662607 MHz |
| 2     | Multi-region | Both | Variable |
| 3     | Dendy | 50.0070 Hz | 1.773448 MHz |

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingMode {
    Ntsc,
    Pal,
    MultiRegion,
    Dendy,
}

fn get_timing_mode(header: &[u8; 16]) -> TimingMode {
    match header[12] & 0x03 {
        0 => TimingMode::Ntsc,
        1 => TimingMode::Pal,
        2 => TimingMode::MultiRegion,
        3 => TimingMode::Dendy,
        _ => unreachable!(),
    }
}
```

**Dendy:** Soviet NES clone with PAL-like timing but different CPU divider.

### Byte 13: VS System / Extended Console Type

When byte 7 bits 0-1 are non-zero, this byte has meaning:

**VS System (byte 7 bit 0 set):**

```
7  bit  0
---------
MMMM PPPP

M: VS hardware type
P: PPU type
```

**VS PPU Types:**

| Value | PPU |
|-------|-----|
| 0 | RP2C03B |
| 1 | RP2C03G |
| 2 | RP2C04-0001 |
| 3 | RP2C04-0002 |
| 4 | RP2C04-0003 |
| 5 | RP2C04-0004 |
| 6 | RC2C03B |
| 7 | RC2C03C |
| 8 | RC2C05-01 |
| 9 | RC2C05-02 |
| 10 | RC2C05-03 |
| 11 | RC2C05-04 |
| 12 | RC2C05-05 |
| 13-15 | Reserved |

**VS Hardware Types:**

| Value | Type |
|-------|------|
| 0 | VS Unisystem (normal) |
| 1 | VS Unisystem (RBI Baseball protection) |
| 2 | VS Unisystem (TKO Boxing protection) |
| 3 | VS Unisystem (Super Xevious protection) |
| 4 | VS Unisystem (Ice Climber Japan protection) |
| 5 | VS Dual System |
| 6 | VS Dual System (Raid on Bungeling Bay protection) |

**Extended Console Type (byte 7 bits 0-1 == 3):**

| Value | Console |
|-------|---------|
| 0 | Regular NES/Famicom/Dendy |
| 1 | VS System |
| 2 | Playchoice 10 |
| 3 | Regular Famiclone (decimal mode) |
| 4 | EPSM (VT01 mono) |
| 5 | VT01 (red/cyan STN) |
| 6 | VT02 |
| 7 | VT03 |
| 8 | VT09 |
| 9 | VT32 |
| 10 | VT369 |
| 11 | UM6578 |
| 12 | Famicom Network System |

### Byte 14: Miscellaneous ROMs

```
7  bit  0
---------
xxxx xxNN

N: Number of miscellaneous ROM images
```

This indicates additional ROM areas after PRG-ROM and CHR-ROM. Used for:

- VS System character ROM
- PlayChoice-10 INST-ROM and PROM

### Byte 15: Default Expansion Device

```
7  bit  0
---------
xxDD DDDD

D: Default expansion device (0-63)
```

**Common Expansion Devices:**

| Value | Device |
|-------|--------|
| 0 | Unspecified |
| 1 | Standard NES/Famicom controllers |
| 2 | NES Four Score / Satellite (4-player) |
| 3 | Famicom Four Players Adapter |
| 4 | VS System (1P via $4016) |
| 5 | VS System (1P via $4017) |
| 6 | Reserved |
| 7 | VS Zapper |
| 8 | Zapper |
| 9 | Two Zappers |
| 10 | Bandai Hyper Shot |
| 11 | Power Pad Side A |
| 12 | Power Pad Side B |
| 13 | Family Trainer Side A |
| 14 | Family Trainer Side B |
| 15 | Arkanoid Vaus (NES) |
| 16 | Arkanoid Vaus (Famicom) |
| 17 | Two Vaus + Famicom Data Recorder |
| ... | (many more) |

---

## Rust Implementation

### Header Structure

```rust
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenA,
    SingleScreenB,
    MapperControlled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingMode {
    Ntsc,
    Pal,
    MultiRegion,
    Dendy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleType {
    Nes,
    VsSystem,
    Playchoice10,
    ExtendedConsole(u8),
}

#[derive(Debug, Clone)]
pub struct Nes20Header {
    // ROM sizes
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,

    // RAM sizes
    pub prg_ram_size: usize,
    pub prg_nvram_size: usize,
    pub chr_ram_size: usize,
    pub chr_nvram_size: usize,

    // Mapper
    pub mapper: u16,
    pub submapper: u8,

    // Flags
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,

    // System info
    pub timing_mode: TimingMode,
    pub console_type: ConsoleType,
    pub vs_ppu_type: Option<u8>,
    pub vs_hardware_type: Option<u8>,

    // Misc
    pub misc_roms: u8,
    pub default_expansion: u8,
}

#[derive(Debug, Error)]
pub enum Nes20Error {
    #[error("Invalid magic number")]
    InvalidMagic,

    #[error("Not NES 2.0 format (byte 7 bits 2-3 != 2)")]
    NotNes20Format,

    #[error("File too small: expected {expected}, got {actual}")]
    FileTooSmall { expected: usize, actual: usize },

    #[error("Invalid size specification")]
    InvalidSize,
}
```

### Header Parser

```rust
impl Nes20Header {
    pub fn parse(data: &[u8]) -> Result<Self, Nes20Error> {
        if data.len() < 16 {
            return Err(Nes20Error::FileTooSmall {
                expected: 16,
                actual: data.len(),
            });
        }

        // Validate magic
        if &data[0..4] != b"NES\x1A" {
            return Err(Nes20Error::InvalidMagic);
        }

        // Verify NES 2.0 format
        if (data[7] & 0x0C) != 0x08 {
            return Err(Nes20Error::NotNes20Format);
        }

        let flags6 = data[6];
        let flags7 = data[7];

        // Calculate mapper number (12-bit)
        let mapper = {
            let low = ((flags6 >> 4) & 0x0F) as u16;
            let mid = ((flags7 & 0xF0) >> 0) as u16;
            let high = ((data[8] & 0x0F) as u16) << 8;
            high | mid | low
        };

        let submapper = (data[8] >> 4) & 0x0F;

        // Calculate ROM sizes
        let prg_rom_size = Self::calculate_prg_size(data[4], data[9] & 0x0F);
        let chr_rom_size = Self::calculate_chr_size(data[5], (data[9] >> 4) & 0x0F);

        // Calculate RAM sizes
        let prg_ram_size = Self::shift_to_size(data[10] & 0x0F);
        let prg_nvram_size = Self::shift_to_size((data[10] >> 4) & 0x0F);
        let chr_ram_size = Self::shift_to_size(data[11] & 0x0F);
        let chr_nvram_size = Self::shift_to_size((data[11] >> 4) & 0x0F);

        // Mirroring
        let mirroring = if (flags6 & 0x08) != 0 {
            Mirroring::FourScreen
        } else if (flags6 & 0x01) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        // Timing mode
        let timing_mode = match data[12] & 0x03 {
            0 => TimingMode::Ntsc,
            1 => TimingMode::Pal,
            2 => TimingMode::MultiRegion,
            3 => TimingMode::Dendy,
            _ => unreachable!(),
        };

        // Console type
        let console_bits = flags7 & 0x03;
        let (console_type, vs_ppu_type, vs_hardware_type) = match console_bits {
            0 => (ConsoleType::Nes, None, None),
            1 => {
                let ppu = data[13] & 0x0F;
                let hw = (data[13] >> 4) & 0x0F;
                (ConsoleType::VsSystem, Some(ppu), Some(hw))
            }
            2 => (ConsoleType::Playchoice10, None, None),
            3 => (ConsoleType::ExtendedConsole(data[13]), None, None),
            _ => unreachable!(),
        };

        Ok(Nes20Header {
            prg_rom_size,
            chr_rom_size,
            prg_ram_size,
            prg_nvram_size,
            chr_ram_size,
            chr_nvram_size,
            mapper,
            submapper,
            mirroring,
            has_battery: (flags6 & 0x02) != 0,
            has_trainer: (flags6 & 0x04) != 0,
            timing_mode,
            console_type,
            vs_ppu_type,
            vs_hardware_type,
            misc_roms: data[14] & 0x03,
            default_expansion: data[15] & 0x3F,
        })
    }

    fn calculate_prg_size(lsb: u8, msb: u8) -> usize {
        if msb == 0x0F {
            // Exponent mode
            let exponent = ((lsb >> 2) & 0x3F) as u32;
            let multiplier = (lsb & 0x03) as usize;
            (1usize << exponent) * (multiplier * 2 + 1)
        } else {
            // Linear mode
            let units = ((msb as usize) << 8) | (lsb as usize);
            units * 16384
        }
    }

    fn calculate_chr_size(lsb: u8, msb: u8) -> usize {
        if msb == 0x0F {
            // Exponent mode
            let exponent = ((lsb >> 2) & 0x3F) as u32;
            let multiplier = (lsb & 0x03) as usize;
            (1usize << exponent) * (multiplier * 2 + 1)
        } else {
            // Linear mode
            let units = ((msb as usize) << 8) | (lsb as usize);
            units * 8192
        }
    }

    fn shift_to_size(shift: u8) -> usize {
        if shift == 0 {
            0
        } else {
            64 << shift
        }
    }
}
```

### ROM Loader

```rust
pub struct Nes20Rom {
    pub header: Nes20Header,
    pub trainer: Option<Vec<u8>>,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub misc_rom: Option<Vec<u8>>,
}

impl Nes20Rom {
    pub fn load(data: &[u8]) -> Result<Self, Nes20Error> {
        let header = Nes20Header::parse(data)?;

        let mut offset = 16;

        // Trainer
        let trainer = if header.has_trainer {
            let t = data.get(offset..offset + 512)
                .ok_or(Nes20Error::FileTooSmall {
                    expected: offset + 512,
                    actual: data.len(),
                })?
                .to_vec();
            offset += 512;
            Some(t)
        } else {
            None
        };

        // PRG-ROM
        let prg_end = offset + header.prg_rom_size;
        if data.len() < prg_end {
            return Err(Nes20Error::FileTooSmall {
                expected: prg_end,
                actual: data.len(),
            });
        }
        let prg_rom = data[offset..prg_end].to_vec();
        offset = prg_end;

        // CHR-ROM
        let chr_rom = if header.chr_rom_size > 0 {
            let chr_end = offset + header.chr_rom_size;
            if data.len() < chr_end {
                return Err(Nes20Error::FileTooSmall {
                    expected: chr_end,
                    actual: data.len(),
                });
            }
            let chr = data[offset..chr_end].to_vec();
            offset = chr_end;
            chr
        } else {
            Vec::new()
        };

        // Miscellaneous ROMs (if any)
        let misc_rom = if header.misc_roms > 0 && offset < data.len() {
            Some(data[offset..].to_vec())
        } else {
            None
        };

        Ok(Nes20Rom {
            header,
            trainer,
            prg_rom,
            chr_rom,
            misc_rom,
        })
    }

    /// Get total volatile RAM needed
    pub fn volatile_ram_size(&self) -> usize {
        self.header.prg_ram_size + self.header.chr_ram_size
    }

    /// Get total battery-backed RAM needed
    pub fn nvram_size(&self) -> usize {
        self.header.prg_nvram_size + self.header.chr_nvram_size
    }

    /// Check if CHR data comes from RAM
    pub fn uses_chr_ram(&self) -> bool {
        self.header.chr_rom_size == 0 && self.header.chr_ram_size > 0
    }
}
```

---

## Submapper Reference

Submappers provide finer granularity for mapper variants:

### Mapper 1 (MMC1) Submappers

| Submapper | Variant | Description |
|-----------|---------|-------------|
| 0 | MMC1 | Standard MMC1 |
| 1 | SUROM | 512KB PRG-ROM |
| 2 | SOROM | 16KB PRG-RAM |
| 3 | SXROM | 32KB PRG-RAM |
| 4 | SEROM/SHROM/SH1ROM | Reduced PRG-RAM |
| 5 | SNROM | 8KB PRG-RAM, battery |

### Mapper 2 (UxROM) Submappers

| Submapper | Variant | Description |
|-----------|---------|-------------|
| 0 | UxROM | Standard |
| 1 | UN1ROM | No bus conflicts |
| 2 | UOROM | 256KB, bus conflicts |

### Mapper 3 (CNROM) Submappers

| Submapper | Variant | Description |
|-----------|---------|-------------|
| 0 | CNROM | Standard |
| 1 | CNROM + security | Copy protection |
| 2 | CNROM + WRAM | With work RAM |

### Mapper 4 (MMC3) Submappers

| Submapper | Variant | Description |
|-----------|---------|-------------|
| 0 | MMC3C | Standard |
| 1 | MMC6 | Startropics variant |
| 2 | MC-ACC | Different IRQ behavior |
| 3 | MMC3A | Early MMC3 with bugs |
| 4 | MMC3 + ACCLAIM | Acclaim IRQ variant |

### Mapper 7 (AxROM) Submappers

| Submapper | Variant | Description |
|-----------|---------|-------------|
| 0 | AxROM | Standard |
| 1 | AxROM | Bank order variant |
| 2 | AxROM | Battletoads wiring |

---

## Converting iNES to NES 2.0

```rust
use super::ines::{INesHeader, INesRom};

impl Nes20Header {
    /// Convert iNES header to NES 2.0 with default values
    pub fn from_ines(ines: &INesHeader) -> Self {
        Nes20Header {
            prg_rom_size: ines.prg_rom_size,
            chr_rom_size: ines.chr_rom_size,
            prg_ram_size: if ines.prg_ram_size > 0 {
                ines.prg_ram_size
            } else if ines.has_battery {
                8192  // Default 8KB
            } else {
                0
            },
            prg_nvram_size: if ines.has_battery {
                ines.prg_ram_size.max(8192)
            } else {
                0
            },
            chr_ram_size: if ines.chr_rom_size == 0 {
                8192  // Default 8KB CHR-RAM
            } else {
                0
            },
            chr_nvram_size: 0,
            mapper: ines.mapper as u16,
            submapper: 0,  // Unknown
            mirroring: ines.mirroring,
            has_battery: ines.has_battery,
            has_trainer: ines.has_trainer,
            timing_mode: match ines.tv_system {
                super::ines::TvSystem::Ntsc => TimingMode::Ntsc,
                super::ines::TvSystem::Pal => TimingMode::Pal,
                super::ines::TvSystem::DualCompatible => TimingMode::MultiRegion,
            },
            console_type: if ines.is_vs_unisystem {
                ConsoleType::VsSystem
            } else if ines.is_playchoice {
                ConsoleType::Playchoice10
            } else {
                ConsoleType::Nes
            },
            vs_ppu_type: None,
            vs_hardware_type: None,
            misc_roms: 0,
            default_expansion: 1,  // Standard controllers
        }
    }
}
```

---

## Timing Constants by Mode

```rust
pub struct TimingConstants {
    pub master_clock_hz: f64,
    pub cpu_divider: u32,
    pub ppu_divider: u32,
    pub frame_rate: f64,
    pub scanlines: u32,
    pub dots_per_scanline: u32,
    pub cpu_cycles_per_frame: u32,
    pub vblank_scanline: u32,
}

impl TimingMode {
    pub fn constants(&self) -> TimingConstants {
        match self {
            TimingMode::Ntsc => TimingConstants {
                master_clock_hz: 21_477_272.0,
                cpu_divider: 12,
                ppu_divider: 4,
                frame_rate: 60.0988,
                scanlines: 262,
                dots_per_scanline: 341,
                cpu_cycles_per_frame: 29780,
                vblank_scanline: 241,
            },
            TimingMode::Pal => TimingConstants {
                master_clock_hz: 26_601_712.0,
                cpu_divider: 16,
                ppu_divider: 5,
                frame_rate: 50.0070,
                scanlines: 312,
                dots_per_scanline: 341,
                cpu_cycles_per_frame: 33247,
                vblank_scanline: 241,
            },
            TimingMode::Dendy => TimingConstants {
                master_clock_hz: 26_601_712.0,
                cpu_divider: 15,
                ppu_divider: 5,
                frame_rate: 50.0070,
                scanlines: 312,
                dots_per_scanline: 341,
                cpu_cycles_per_frame: 35464,
                vblank_scanline: 291,  // Later than PAL
            },
            TimingMode::MultiRegion => {
                // Default to NTSC for multi-region
                TimingMode::Ntsc.constants()
            }
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

    fn create_nes20_header() -> [u8; 16] {
        [
            0x4E, 0x45, 0x53, 0x1A,  // Magic
            0x02,                      // PRG LSB: 2 units
            0x01,                      // CHR LSB: 1 unit
            0x11,                      // Flags 6: mapper 1 low, vertical mirroring
            0x08,                      // Flags 7: NES 2.0 identifier
            0x00,                      // Flags 8: mapper high = 0, submapper = 0
            0x00,                      // Flags 9: size MSB = 0
            0x07,                      // Flags 10: 8KB PRG-RAM
            0x07,                      // Flags 11: 8KB CHR-RAM
            0x00,                      // Flags 12: NTSC
            0x00,                      // Flags 13: Regular NES
            0x00,                      // Flags 14: No misc ROMs
            0x01,                      // Flags 15: Standard controllers
        ]
    }

    #[test]
    fn test_nes20_detection() {
        let header = create_nes20_header();
        assert!((header[7] & 0x0C) == 0x08);
    }

    #[test]
    fn test_parse_valid_header() {
        let header = create_nes20_header();
        let result = Nes20Header::parse(&header);
        assert!(result.is_ok());

        let h = result.unwrap();
        assert_eq!(h.prg_rom_size, 32768);  // 2 × 16KB
        assert_eq!(h.chr_rom_size, 8192);   // 1 × 8KB
        assert_eq!(h.mapper, 1);            // MMC1
        assert_eq!(h.submapper, 0);
        assert_eq!(h.prg_ram_size, 8192);   // shift 7 = 8KB
        assert_eq!(h.timing_mode, TimingMode::Ntsc);
    }

    #[test]
    fn test_large_mapper_number() {
        let mut header = create_nes20_header();
        header[6] = 0xF0;  // Low nibble = 15
        header[7] = 0xF8;  // Mid nibble = 15, NES 2.0
        header[8] = 0x0F;  // High nibble = 15

        let h = Nes20Header::parse(&header).unwrap();
        assert_eq!(h.mapper, 0xFFF);  // 4095
    }

    #[test]
    fn test_submapper() {
        let mut header = create_nes20_header();
        header[8] = 0x50;  // Submapper 5, mapper high = 0

        let h = Nes20Header::parse(&header).unwrap();
        assert_eq!(h.submapper, 5);
    }

    #[test]
    fn test_exponent_size() {
        // Test exponent mode for PRG-ROM
        // MSB = 0xF, LSB = 0b00001000 = exponent 2, multiplier 0
        // Size = 2^2 × (0×2+1) = 4 × 1 = 4 bytes
        assert_eq!(
            Nes20Header::calculate_prg_size(0b00001000, 0x0F),
            4
        );

        // exponent 10, multiplier 1 = 2^10 × 3 = 3072 bytes
        assert_eq!(
            Nes20Header::calculate_prg_size(0b00101001, 0x0F),
            3072
        );
    }

    #[test]
    fn test_timing_modes() {
        let mut header = create_nes20_header();

        header[12] = 0x00;
        assert_eq!(Nes20Header::parse(&header).unwrap().timing_mode, TimingMode::Ntsc);

        header[12] = 0x01;
        assert_eq!(Nes20Header::parse(&header).unwrap().timing_mode, TimingMode::Pal);

        header[12] = 0x02;
        assert_eq!(Nes20Header::parse(&header).unwrap().timing_mode, TimingMode::MultiRegion);

        header[12] = 0x03;
        assert_eq!(Nes20Header::parse(&header).unwrap().timing_mode, TimingMode::Dendy);
    }

    #[test]
    fn test_vs_system() {
        let mut header = create_nes20_header();
        header[7] = 0x09;  // VS System bit + NES 2.0
        header[13] = 0x23; // PPU type 3, hardware type 2

        let h = Nes20Header::parse(&header).unwrap();
        assert_eq!(h.console_type, ConsoleType::VsSystem);
        assert_eq!(h.vs_ppu_type, Some(3));
        assert_eq!(h.vs_hardware_type, Some(2));
    }

    #[test]
    fn test_ram_sizes() {
        let mut header = create_nes20_header();
        header[10] = 0x97;  // PRG-NVRAM shift 9 (32KB), PRG-RAM shift 7 (8KB)
        header[11] = 0x87;  // CHR-NVRAM shift 8 (16KB), CHR-RAM shift 7 (8KB)

        let h = Nes20Header::parse(&header).unwrap();
        assert_eq!(h.prg_ram_size, 8192);
        assert_eq!(h.prg_nvram_size, 32768);
        assert_eq!(h.chr_ram_size, 8192);
        assert_eq!(h.chr_nvram_size, 16384);
    }
}
```

---

## References

- [NESdev Wiki: NES 2.0](https://www.nesdev.org/wiki/NES_2.0)
- [NESdev Wiki: NES 2.0 Submappers](https://www.nesdev.org/wiki/NES_2.0_submappers)
- [NESdev Wiki: VS System](https://www.nesdev.org/wiki/VS_System)

---

## See Also

- [INES_FORMAT.md](INES_FORMAT.md) - Original iNES format
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper implementations
- [ROM_LOADING.md](ROM_LOADING.md) - Complete ROM loading pipeline
