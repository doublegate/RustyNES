# NES Mapper Overview

**Table of Contents**
- [Introduction](#introduction)
- [Why Mappers Exist](#why-mappers-exist)
- [Mapper Architecture](#mapper-architecture)
  - [Memory Banking](#memory-banking)
  - [PRG-ROM Banking](#prg-rom-banking)
  - [CHR Banking](#chr-banking)
- [ROM Formats](#rom-formats)
  - [iNES Format](#ines-format)
  - [NES 2.0 Format](#nes-20-format)
  - [UNIF Format](#unif-format)
- [Common Mappers](#common-mappers)
- [Mapper Categories](#mapper-categories)
- [Implementation Strategy](#implementation-strategy)
- [Testing Mappers](#testing-mappers)
- [References](#references)

---

## Introduction

**Mappers** are hardware circuits inside NES cartridges that extend the console's capabilities by providing additional memory, bank switching, and special features. The term "mapper" comes from **memory mapping**: translating cartridge hardware into the CPU's and PPU's address spaces.

### Key Concepts

- **Base NES**: 32KB PRG-ROM (CPU) + 8KB CHR-ROM/RAM (PPU)
- **Mappers**: Enable games larger than 40KB through **bank switching**
- **Mapper Number**: Standardized identifier (e.g., Mapper 0 = NROM, Mapper 1 = MMC1)
- **Over 300 mappers**: From simple discrete logic to complex ASICs

---

## Why Mappers Exist

The NES's memory architecture is limited:

### CPU Address Space Constraints

```
$8000-$FFFF: 32KB for PRG-ROM (program code/data)
```

**Problem**: Early games needed more than 32KB
**Solution**: Bank switching to swap different ROM banks into this space

### PPU Address Space Constraints

```
$0000-$1FFF: 8KB for CHR-ROM (graphics tiles)
```

**Problem**: 512 tiles (8KB) insufficient for large games
**Solution**: CHR banking to swap tile banks or use CHR-RAM

### Additional Features

Mappers also provide:
- **IRQ counters**: For split-screen effects, raster effects
- **Expansion audio**: Extra sound channels (VRC6, MMC5, N163)
- **RAM**: Battery-backed save RAM, work RAM
- **Mirroring control**: Dynamic horizontal/vertical/four-screen switching

---

## Mapper Architecture

### Memory Banking

**Banking** divides ROM into fixed-size chunks (banks) and selectively maps them into address space.

#### Example: 256KB PRG-ROM with 16KB Banks

```
Physical ROM:        Logical Banks:
$00000-$03FFF  →     Bank  0
$04000-$07FFF  →     Bank  1
$08000-$0BFFF  →     Bank  2
...
$3C000-$3FFFF  →     Bank 15
```

**Mapper Register**: Selects which bank appears at $8000-$BFFF

```
Write $05 to bank register:
CPU $8000-$BFFF now reads from physical ROM $14000-$17FFF (Bank 5)
```

### PRG-ROM Banking

**PRG-ROM** contains executable code and data for the CPU.

#### Common Banking Schemes

**Fixed + Switchable** (UxROM, MMC1):
```
$8000-$BFFF: Switchable 16KB bank
$C000-$FFFF: Fixed to last bank (interrupt vectors)
```

**Dual Switchable** (MMC3):
```
$8000-$9FFF: Switchable 8KB bank 0
$A000-$BFFF: Switchable 8KB bank 1
$C000-$DFFF: Fixed 8KB bank (-2)
$E000-$FFFF: Fixed 8KB bank (-1, last bank)
```

**Fully Switchable** (AxROM):
```
$8000-$FFFF: Single switchable 32KB bank
(Interrupt vectors must exist in every bank)
```

#### Bank Calculation Example

```rust
fn map_prg_address(&self, cpu_addr: u16, bank_num: usize, bank_size: usize) -> usize {
    let offset = (cpu_addr & (bank_size as u16 - 1)) as usize;
    (bank_num * bank_size) + offset
}

// Example: Reading from $9A00 in Bank 5 (8KB banks)
let physical = map_prg_address(0x9A00, 5, 0x2000);
// Result: (5 * 0x2000) + 0x1A00 = 0xBA00
```

### CHR Banking

**CHR** (Character ROM/RAM) contains graphics tile data for the PPU.

#### CHR-ROM vs CHR-RAM

**CHR-ROM**:
- Read-only graphics data
- Bank-switched for more than 8KB of tiles
- Common in early/mid-generation games

**CHR-RAM**:
- Writable RAM for dynamic graphics
- No banking needed (typically 8KB)
- Common in later games, all homebrew

#### Common CHR Banking Schemes

**8KB Banks** (CNROM):
```
$0000-$1FFF: Single switchable 8KB bank
```

**4KB Banks** (MMC1):
```
$0000-$0FFF: Switchable 4KB bank 0
$1000-$1FFF: Switchable 4KB bank 1
```

**2KB + 1KB Banks** (MMC3):
```
$0000-$07FF: Switchable 2KB bank 0 (2 tiles)
$0800-$0FFF: Switchable 2KB bank 1
$1000-$13FF: Switchable 1KB bank 2
$1400-$17FF: Switchable 1KB bank 3
$1800-$1BFF: Switchable 1KB bank 4
$1C00-$1FFF: Switchable 1KB bank 5
```

---

## ROM Formats

### iNES Format

The **iNES format** (.nes files) is the standard for distributing NES ROMs.

#### Header Structure (16 bytes)

```
Byte 0-3:   "NES" + $1A (magic number)
Byte 4:     PRG-ROM size (in 16KB units)
Byte 5:     CHR-ROM size (in 8KB units, 0 = CHR-RAM)
Byte 6:     Flags 6 (mapper low nibble, mirroring, battery, trainer)
Byte 7:     Flags 7 (mapper high nibble, VS System, PlayChoice-10)
Byte 8:     PRG-RAM size (rarely used)
Byte 9-15:  Padding (usually zero)
```

#### Flags 6 Breakdown

```
Bit 0:   Mirroring (0 = horizontal, 1 = vertical)
Bit 1:   Battery-backed RAM at $6000-$7FFF
Bit 2:   512-byte trainer at $7000-$71FF
Bit 3:   Four-screen VRAM
Bit 4-7: Mapper number (lower nibble)
```

#### Flags 7 Breakdown

```
Bit 0-1: Console type (0 = NES/Famicom)
Bit 2-3: NES 2.0 identifier (if 10b, use NES 2.0 format)
Bit 4-7: Mapper number (upper nibble)
```

#### Mapper Number Calculation

```rust
fn get_mapper_number(header: &[u8; 16]) -> u8 {
    let low_nibble = (header[6] & 0xF0) >> 4;
    let high_nibble = header[7] & 0xF0;
    high_nibble | low_nibble
}
```

### NES 2.0 Format

**NES 2.0** extends iNES to support:
- Mappers 0-4095 (vs iNES 0-255)
- **Submappers** (4-bit variant identifiers)
- Larger ROM sizes (up to exabytes theoretically)
- Console type (NES, Famicom, VS System, Playchoice)
- Default expansion devices
- Miscellaneous ROM chips

#### NES 2.0 Identification

```rust
fn is_nes20(header: &[u8; 16]) -> bool {
    (header[7] & 0x0C) == 0x08
}
```

#### Extended Mapper Number

```
Mapper number = (Byte 8 & 0x0F) << 8 | (Byte 7 & 0xF0) | (Byte 6 >> 4)
Submapper     = (Byte 8 & 0xF0) >> 4
```

#### Advantages Over iNES

- **Submappers**: Disambiguates UxROM variants, MMC3 revisions
- **Accurate ROM sizes**: Byte 9 for non-power-of-2 sizes
- **RAM specification**: Separate battery/non-battery RAM sizes
- **Region info**: NTSC vs PAL timing

**Recommendation**: Always output NES 2.0 headers for new ROMs

### UNIF Format

**UNIF** (Universal NES Image Format) describes cartridges by **board name** rather than mapper number.

**Example**: `NES-SLROM` (MMC1 board with specific PRG/CHR sizes)

**Advantages**:
- Precise hardware description
- Extensible chunk-based format

**Disadvantages**:
- Less emulator support than iNES/NES 2.0
- More complex parsing

**Status**: Largely superseded by NES 2.0

---

## Common Mappers

### Mapper Coverage by Game Library

| Mapper | Name | % of Games | Cumulative % |
|--------|------|------------|--------------|
| 1 | MMC1 (SxROM) | 27.9% | 27.9% |
| 4 | MMC3 (TxROM) | 23.4% | 51.3% |
| 0 | NROM | 9.5% | 60.8% |
| 2 | UxROM | 10.6% | 71.4% |
| 3 | CNROM | 6.3% | 77.7% |
| 7 | AxROM | 3.1% | 80.8% |
| 11 | Color Dreams | 2.1% | 82.9% |
| 9 | MMC2 (PxROM) | 1.8% | 84.7% |

**First 6 mappers** cover **80%** of the entire licensed NES library.

### Priority Implementation Order

**Phase 1** (Essential - 80% coverage):
1. Mapper 0 (NROM) - Baseline, no banking
2. Mapper 1 (MMC1) - Most common, complex serial interface
3. Mapper 2 (UxROM) - Simple PRG banking
4. Mapper 3 (CNROM) - Simple CHR banking
5. Mapper 4 (MMC3) - Complex, scanline IRQ
6. Mapper 7 (AxROM) - Full 32KB banking

**Phase 2** (Advanced - 95% coverage):
- 5 (MMC5) - Expansion audio, exotic features
- 9 (MMC2) - Punch-Out style latch
- 10 (MMC4) - Similar to MMC2
- 11 (Color Dreams) - Simple unlicensed
- 19 (Namco 163) - Expansion audio
- 23, 24, 25, 26 (VRC series) - Konami boards

**Phase 3** (Comprehensive - 99%+):
- Hundreds of unlicensed/obscure mappers
- Homebrew mappers (30, 31, 218)
- Multi-cart/educational boards

---

## Mapper Categories

### By Complexity

**Simple Discrete Logic**:
- Mappers 0, 2, 3, 7
- No IRQ, basic banking
- Easy to implement (< 100 lines)

**Moderate ASIC**:
- Mappers 1, 9, 10
- Serial registers, latches
- Medium complexity (100-300 lines)

**Complex ASIC**:
- Mappers 4, 5, 19
- IRQ counters, expansion audio, multiply/divide
- High complexity (300-1000+ lines)

### By Manufacturer

**Nintendo**:
- MMC1 (Mapper 1)
- MMC2 (Mapper 9)
- MMC3 (Mapper 4)
- MMC4 (Mapper 10)
- MMC5 (Mapper 5)

**Konami**:
- VRC1 (Mapper 75)
- VRC2/4 (Mappers 21-25)
- VRC3 (Mapper 73)
- VRC6 (Mapper 24/26)
- VRC7 (Mapper 85)

**Namco**:
- Namco 163 (Mapper 19)
- Namco 175/340 (Mapper 210)

**Sunsoft**:
- Sunsoft 4 (Mapper 68)
- Sunsoft 5B (Mapper 69)

**Unlicensed**:
- Color Dreams (Mapper 11)
- NINA-001 (Mapper 34)
- Camerica/Codemasters (Mappers 71, 232)

### By Features

**IRQ Support**:
- MMC3 (scanline counter)
- MMC5 (configurable)
- VRC series (CPU cycle counter)

**Expansion Audio**:
- VRC6 (3 channels: 2 pulse, 1 saw)
- VRC7 (FM synthesis, 6 channels)
- MMC5 (2 pulse + PCM)
- N163 (1-8 wavetable channels)
- Sunsoft 5B (AY-3-8910 PSG)

**Four-Screen VRAM**:
- MMC5 (ExRAM)
- Some unlicensed mappers

---

## Implementation Strategy

### Trait-Based Architecture

```rust
pub trait Mapper {
    // PRG-ROM access
    fn read_prg(&self, addr: u16) -> u8;
    fn write_prg(&mut self, addr: u16, value: u8);

    // CHR access
    fn read_chr(&self, addr: u16) -> u8;
    fn write_chr(&mut self, addr: u16, value: u8);

    // Mirroring
    fn mirroring(&self) -> Mirroring;

    // IRQ support
    fn irq_pending(&self) -> bool { false }
    fn clock(&mut self, _cycles: u8) {}

    // PPU notifications (for MMC3 scanline counter)
    fn notify_scanline(&mut self) {}

    // Save state support
    fn save_state(&self) -> Vec<u8>;
    fn load_state(&mut self, data: &[u8]);
}
```

### Factory Pattern

```rust
pub fn create_mapper(
    rom: Rom,
    mapper_number: u16,
    submapper: u8,
) -> Result<Box<dyn Mapper>, MapperError> {
    match mapper_number {
        0 => Ok(Box::new(NROM::new(rom))),
        1 => Ok(Box::new(MMC1::new(rom, submapper))),
        2 => Ok(Box::new(UxROM::new(rom, submapper))),
        3 => Ok(Box::new(CNROM::new(rom))),
        4 => Ok(Box::new(MMC3::new(rom, submapper))),
        7 => Ok(Box::new(AxROM::new(rom))),
        _ => Err(MapperError::UnsupportedMapper(mapper_number)),
    }
}
```

### Base Implementation Pattern

```rust
pub struct MapperNNN {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: Vec<u8>,

    prg_bank: usize,
    chr_bank: usize,

    mirroring: Mirroring,
    irq_pending: bool,
}

impl Mapper for MapperNNN {
    fn read_prg(&self, addr: u16) -> u8 {
        let mapped = self.map_prg_addr(addr);
        match addr {
            0x6000..=0x7FFF => self.prg_ram[mapped % self.prg_ram.len()],
            0x8000..=0xFFFF => self.prg_rom[mapped % self.prg_rom.len()],
            _ => 0, // Open bus
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                let mapped = addr as usize - 0x6000;
                self.prg_ram[mapped % self.prg_ram.len()] = value;
            }
            0x8000..=0xFFFF => {
                // Bank select register
                self.prg_bank = (value as usize) % self.num_prg_banks();
            }
            _ => {}
        }
    }

    fn map_prg_addr(&self, addr: u16) -> usize {
        // Mapper-specific logic
        match addr {
            0x8000..=0xBFFF => {
                // Switchable bank
                (self.prg_bank * 0x4000) + ((addr & 0x3FFF) as usize)
            }
            0xC000..=0xFFFF => {
                // Fixed to last bank
                let last = self.num_prg_banks() - 1;
                (last * 0x4000) + ((addr & 0x3FFF) as usize)
            }
            _ => 0,
        }
    }
}
```

---

## Testing Mappers

### Test ROM Suites

**Per-Mapper Test ROMs**:
- `mapper###_test.nes` - Basic functionality tests
- Bank switching verification
- Register write/read tests
- IRQ timing tests (if applicable)

**Game-Based Testing**:
| Mapper | Test Game | Tests |
|--------|-----------|-------|
| 0 | Super Mario Bros. | Baseline functionality |
| 1 | Mega Man 2 | MMC1 serial writes, banking |
| 2 | Mega Man | UxROM banking, bus conflicts |
| 3 | Super Mario Bros. | CNROM CHR banking |
| 4 | Super Mario Bros. 3 | MMC3 banking, IRQ |
| 7 | Battletoads | AxROM 32KB banking |

### Unit Testing Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper_creation() {
        let rom = create_test_rom(2, 1); // 32KB PRG, 8KB CHR
        let mapper = NROM::new(rom);
        assert_eq!(mapper.num_prg_banks(), 2);
    }

    #[test]
    fn test_prg_banking() {
        let mut mapper = UxROM::new(create_test_rom(8, 0));

        // Switch to bank 3
        mapper.write_prg(0x8000, 0x03);
        assert_eq!(mapper.prg_bank, 3);

        // Verify correct physical address
        let addr = mapper.map_prg_addr(0x8000);
        assert_eq!(addr, 3 * 0x4000);
    }

    #[test]
    fn test_mirroring() {
        let mapper = NROM::new_vertical();
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }
}
```

### Integration Testing

```rust
#[test]
fn test_full_rom_execution() {
    let rom = load_rom("test_roms/mapper002_test.nes");
    let mut console = Console::new(rom);

    // Run for 1 second (60 frames NTSC)
    for _ in 0..60 {
        console.step_frame();
    }

    // Check for test ROM success indicator
    let result = console.bus.read(0x6000);
    assert_eq!(result, 0x00); // 0x00 = pass
}
```

---

## References

- [NesDev Wiki: Mapper](https://www.nesdev.org/wiki/Mapper)
- [NesDev Wiki: NES 2.0](https://www.nesdev.org/wiki/NES_2.0)
- [NesDev Wiki: NES 2.0 Submappers](https://www.nesdev.org/wiki/NES_2.0_submappers)
- [NesDev Wiki: iNES](https://www.nesdev.org/wiki/INES)
- [MEMORY_MAP.md](../bus/MEMORY_MAP.md) - Memory architecture
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Bus conflict behavior

**Individual Mapper Documentation**:
- [MAPPER_NROM.md](MAPPER_NROM.md) - Mapper 0
- [MAPPER_MMC1.md](MAPPER_MMC1.md) - Mapper 1
- [MAPPER_UXROM.md](MAPPER_UXROM.md) - Mapper 2
- [MAPPER_CNROM.md](MAPPER_CNROM.md) - Mapper 3
- [MAPPER_MMC3.md](MAPPER_MMC3.md) - Mapper 4

---

**Related Documents**:
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Understanding bus conflicts
- [ARCHITECTURE.md](../ARCHITECTURE.md) - Overall system design
- [TESTING.md](../dev/TESTING.md) - Test strategy and test ROMs
