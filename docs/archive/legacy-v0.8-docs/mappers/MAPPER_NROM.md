# Mapper 0: NROM

**Table of Contents**

- [Overview](#overview)
- [Board Variants](#board-variants)
- [Memory Map](#memory-map)
- [Technical Specifications](#technical-specifications)
- [Implementation](#implementation)
- [Games Using NROM](#games-using-nrom)
- [Testing](#testing)
- [References](#references)

---

## Overview

**NROM** (Mapper 0) is the simplest NES mapper, representing cartridges with **no bank switching**. All memory is fixed and directly mapped to the CPU and PPU address spaces. NROM serves as the baseline for NES emulator development and represents the console's native capabilities without enhancement.

### Key Characteristics

- **Mapper Number**: 0
- **Board Names**: NES-NROM-128, NES-NROM-256, HVC-NROM-256
- **PRG-ROM**: 16KB or 32KB (fixed, no banking)
- **CHR**: 8KB CHR-ROM (fixed) or CHR-RAM
- **Mirroring**: Fixed horizontal or vertical (no dynamic control)
- **Bank Switching**: None
- **Bus Conflicts**: Irrelevant (no writable registers)

**Coverage**: ~9.5% of licensed NES library

---

## Board Variants

### NROM-128

**PRG-ROM**: 16KB (single bank)

**Memory Layout**:

```
CPU $8000-$BFFF: First 16KB of PRG-ROM
CPU $C000-$FFFF: Mirror of $8000-$BFFF
```

**Characteristic**: The same 16KB appears twice in CPU address space.

**Example Games**:

- Donkey Kong
- Mario Bros.
- Pinball

### NROM-256

**PRG-ROM**: 32KB (two banks)

**Memory Layout**:

```
CPU $8000-$BFFF: First 16KB of PRG-ROM
CPU $C000-$FFFF: Second 16KB of PRG-ROM (unique)
```

**Characteristic**: Full 32KB address space utilized.

**Example Games**:

- Super Mario Bros.
- Excitebike
- Ice Climber
- Balloon Fight

### NROM-368 (Modern)

**PRG-ROM**: 40KB + 8KB (recent invention for homebrew)

**Memory Layout**:

```
CPU $6000-$7FFF: 8KB PRG-ROM (non-standard)
CPU $8000-$FFFF: 32KB PRG-ROM (standard)
```

**Use Case**: Homebrew games exceeding 32KB without full mapper complexity.

---

## Memory Map

### CPU Address Space

```
$0000-$07FF: 2KB Internal RAM (NES, not cartridge)
$0800-$1FFF: Mirrors of $0000-$07FF
$2000-$2007: PPU Registers (NES, not cartridge)
$2008-$3FFF: Mirrors of PPU registers
$4000-$4017: APU & I/O Registers (NES, not cartridge)
$4018-$401F: APU test mode (disabled)
$4020-$5FFF: Expansion ROM (unused on NROM)
$6000-$7FFF: PRG-RAM (optional, 2-4KB, battery-backed for saves)
$8000-$BFFF: PRG-ROM Lower 16KB
$C000-$FFFF: PRG-ROM Upper 16KB (or mirror of $8000-$BFFF)
```

### PPU Address Space

```
$0000-$1FFF: 8KB CHR-ROM or CHR-RAM (fixed)
$2000-$3FFF: VRAM (nametables, palette) (NES internal)
```

**Mirroring**: Set by solder pad/trace on cartridge (horizontal or vertical)

---

## Technical Specifications

### PRG-ROM

**Size**: 16KB or 32KB
**Banks**: None (all fixed)
**Writable**: No (ROM)

**Access**:

- **NROM-128**: $8000-$FFFF both read from same 16KB
- **NROM-256**: $8000-$BFFF (first 16KB), $C000-$FFFF (second 16KB)

### CHR

**Size**: 8KB
**Type**: CHR-ROM (older games) or CHR-RAM (modern/homebrew)
**Banks**: None (all fixed)

**CHR-ROM**: Read-only, fixed graphics
**CHR-RAM**: Writable, allows dynamic tile updates

### PRG-RAM

**Size**: 0-8KB (optional)
**Battery-backed**: Optional (for save data)
**Location**: $6000-$7FFF

**Family Basic**: Uses 2KB or 4KB RAM with external write-protect switch

### Mirroring

**Type**: Fixed (set by cartridge hardware)
**Modes**: Horizontal or Vertical

**No dynamic control**: Unlike MMC1/MMC3, mirroring cannot change at runtime

---

## Implementation

### Rust Structure

```rust
pub struct NROM {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_mem: Vec<u8>,
    mirroring: Mirroring,
    chr_is_ram: bool,
}

impl NROM {
    pub fn new(rom: Rom) -> Self {
        let chr_is_ram = rom.chr_rom.is_empty();
        let chr_mem = if chr_is_ram {
            vec![0; 0x2000] // 8KB CHR-RAM
        } else {
            rom.chr_rom
        };

        Self {
            prg_rom: rom.prg_rom,
            prg_ram: vec![0; 0x2000], // 8KB PRG-RAM
            chr_mem,
            mirroring: rom.mirroring,
            chr_is_ram,
        }
    }
}
```

### Mapper Trait Implementation

```rust
impl Mapper for NROM {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM (battery-backed save RAM)
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset % self.prg_ram.len()]
            }
            0x8000..=0xFFFF => {
                // PRG-ROM
                let offset = (addr - 0x8000) as usize;
                if self.prg_rom.len() == 0x4000 {
                    // NROM-128: Mirror 16KB
                    self.prg_rom[offset % 0x4000]
                } else {
                    // NROM-256: Full 32KB
                    self.prg_rom[offset % self.prg_rom.len()]
                }
            }
            _ => 0, // Open bus
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM is writable
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset % self.prg_ram.len()] = value;
            }
            0x8000..=0xFFFF => {
                // PRG-ROM writes ignored (no bank switching registers)
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_mem[(addr as usize) % self.chr_mem.len()]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_is_ram {
            self.chr_mem[(addr as usize) % self.chr_mem.len()] = value;
        }
        // CHR-ROM writes ignored
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
```

### Save State Support

```rust
impl NROM {
    pub fn save_battery_ram(&self) -> Option<Vec<u8>> {
        // Only save if battery-backed RAM exists
        if self.has_battery {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    pub fn load_battery_ram(&mut self, data: &[u8]) {
        if self.has_battery && !data.is_empty() {
            self.prg_ram.copy_from_slice(&data[..self.prg_ram.len().min(data.len())]);
        }
    }
}
```

---

## Games Using NROM

### Notable NROM-128 Games

| Game | Size | Features |
|------|------|----------|
| Donkey Kong | 16KB PRG, 8KB CHR-ROM | First NES game in NA |
| Mario Bros. | 16KB PRG, 8KB CHR-ROM | Classic arcade port |
| Pinball | 16KB PRG, 8KB CHR-ROM | Simple physics |

### Notable NROM-256 Games

| Game | Size | Features |
|------|------|----------|
| **Super Mario Bros.** | 32KB PRG, 8KB CHR-ROM | Iconic launch title |
| Excitebike | 32KB PRG, 8KB CHR-ROM | Track editor (battery RAM) |
| Ice Climber | 32KB PRG, 8KB CHR-ROM | Two-player co-op |
| Balloon Fight | 32KB PRG, 8KB CHR-ROM | Flapping mechanic |
| Popeye | 32KB PRG, 8KB CHR-ROM | Early platformer |

**Total NROM Games**: ~200 commercial releases (~9.5% of library)

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nrom_128_mirroring() {
        let rom = create_test_rom(1, 1, Mirroring::Horizontal);
        let mapper = NROM::new(rom);

        // Write to first 16KB
        let value_at_8000 = mapper.read_prg(0x8000);

        // Should mirror at $C000
        assert_eq!(mapper.read_prg(0xC000), value_at_8000);
    }

    #[test]
    fn test_nrom_256_no_mirroring() {
        let rom = create_test_rom(2, 1, Mirroring::Horizontal);
        let mapper = NROM::new(rom);

        // Upper 16KB should be different from lower 16KB
        let value_at_8000 = mapper.read_prg(0x8000);
        let value_at_c000 = mapper.read_prg(0xC000);

        // May or may not be equal, but should address different regions
        // Test by writing known patterns
        assert_ne!(
            mapper.map_address(0x8000),
            mapper.map_address(0xC000)
        );
    }

    #[test]
    fn test_prg_ram_readwrite() {
        let mut mapper = NROM::new(create_test_rom(2, 1, Mirroring::Horizontal));

        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        mapper.write_prg(0x7FFF, 0xAA);
        assert_eq!(mapper.read_prg(0x7FFF), 0xAA);
    }

    #[test]
    fn test_chr_ram_write() {
        let mut rom = create_test_rom(2, 0, Mirroring::Horizontal); // 0 CHR = CHR-RAM
        let mut mapper = NROM::new(rom);

        assert!(mapper.chr_is_ram);

        mapper.write_chr(0x0000, 0xFF);
        assert_eq!(mapper.read_chr(0x0000), 0xFF);
    }

    #[test]
    fn test_chr_rom_readonly() {
        let rom = create_test_rom(2, 1, Mirroring::Horizontal); // 1 CHR = CHR-ROM
        let mut mapper = NROM::new(rom);

        assert!(!mapper.chr_is_ram);

        let original = mapper.read_chr(0x0000);
        mapper.write_chr(0x0000, 0xFF);
        assert_eq!(mapper.read_chr(0x0000), original); // Unchanged
    }
}
```

### Integration Tests

**Test ROM**: Run Super Mario Bros. (NROM-256)

```rust
#[test]
fn test_super_mario_bros_boots() {
    let rom = load_rom("roms/Super Mario Bros (USA).nes");
    let mut console = Console::new(rom);

    // Run for 60 frames (1 second)
    for _ in 0..60 {
        console.step_frame();
    }

    // SMB should reach title screen
    // (Verify via PPU state or known RAM location)
    let title_screen_flag = console.read_cpu(0x0770);
    assert!(title_screen_flag > 0);
}
```

---

## References

- [NesDev Wiki: NROM](https://www.nesdev.org/wiki/NROM)
- [NesDev Wiki: Mapper](https://www.nesdev.org/wiki/Mapper)
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - General mapper architecture
- [MEMORY_MAP.md](../bus/MEMORY_MAP.md) - NES memory layout
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Bus conflict behavior (irrelevant for NROM)

---

**Related Documents**:

- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - Mapper introduction
- [MAPPER_UXROM.md](MAPPER_UXROM.md) - Next step: PRG banking
- [MAPPER_CNROM.md](MAPPER_CNROM.md) - Next step: CHR banking
- [ARCHITECTURE.md](../ARCHITECTURE.md) - Overall system design
