# Mapper Implementation Guide

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** How to implement a new NES mapper

---

## Table of Contents

- [Overview](#overview)
- [Mapper Trait](#mapper-trait)
- [Implementation Steps](#implementation-steps)
- [Common Patterns](#common-patterns)
- [Banking Systems](#banking-systems)
- [IRQ Implementation](#irq-implementation)
- [Testing Checklist](#testing-checklist)
- [Example Implementations](#example-implementations)

---

## Overview

NES mappers expand the console's capabilities by providing additional ROM/RAM, banking, IRQs, and other features. Implementing a mapper involves:

1. Understanding the hardware specifications
2. Implementing the Mapper trait
3. Handling banking and memory access
4. Implementing special features (IRQs, audio, etc.)
5. Testing with ROM test suite and commercial games

---

## Mapper Trait

### Core Trait Definition

```rust
pub trait Mapper {
    /// Read from CPU address space ($6000-$FFFF)
    fn cpu_read(&mut self, addr: u16) -> u8;

    /// Write to CPU address space ($6000-$FFFF)
    fn cpu_write(&mut self, addr: u16, value: u8);

    /// Read from PPU address space ($0000-$3FFF)
    fn ppu_read(&mut self, addr: u16) -> u8;

    /// Write to PPU address space ($0000-$3FFF)
    fn ppu_write(&mut self, addr: u16, value: u8);

    /// Clock the mapper (for IRQ counters, audio, etc.)
    fn tick(&mut self) {}

    /// PPU scanline tick (for scanline counters like MMC3)
    fn ppu_scanline_tick(&mut self) {}

    /// Check if mapper is requesting IRQ
    fn irq_pending(&self) -> bool {
        false
    }

    /// Get mirroring mode
    fn mirroring(&self) -> Mirroring;

    /// Save state for savestates
    fn save_state(&self) -> MapperState;

    /// Load state from savestates
    fn load_state(&mut self, state: MapperState);
}
```

---

## Implementation Steps

### Step 1: Research the Mapper

**Resources:**

- [NESdev Wiki Mapper Page](https://www.nesdev.org/wiki/Mapper)
- NesDev forum discussions
- Other emulator implementations (Mesen2, puNES)
- Bootgod's ROM database

**Key Information:**

- PRG ROM banking configuration
- CHR ROM/RAM banking
- Mirroring control
- IRQ generation method
- Special features (audio, expansion RAM, etc.)

### Step 2: Create Mapper Struct

```rust
pub struct MapperXXX {
    // ROM data
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,

    // RAM
    prg_ram: Vec<u8>,
    chr_ram: Vec<u8>,

    // Banking registers
    prg_bank0: usize,
    prg_bank1: usize,
    chr_bank0: usize,
    chr_bank1: usize,

    // Control registers
    mirroring: Mirroring,
    irq_enabled: bool,
    irq_counter: u16,

    // Metadata
    prg_rom_size: usize,
    chr_rom_size: usize,
}

impl MapperXXX {
    pub fn new(rom: &NesRom) -> Self {
        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            prg_ram: vec![0; rom.prg_ram_size],
            chr_ram: vec![0; rom.chr_ram_size],
            prg_bank0: 0,
            prg_bank1: rom.prg_rom_banks - 1,
            chr_bank0: 0,
            chr_bank1: 0,
            mirroring: rom.mirroring,
            irq_enabled: false,
            irq_counter: 0,
            prg_rom_size: rom.prg_rom.len(),
            chr_rom_size: rom.chr_rom.len(),
        }
    }
}
```

### Step 3: Implement Memory Access

```rust
impl Mapper for MapperXXX {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset]
            }

            // PRG ROM Bank 0: $8000-$BFFF
            0x8000..=0xBFFF => {
                let offset = (addr - 0x8000) as usize;
                let bank_offset = self.prg_bank0 * 0x4000 + offset;
                self.prg_rom[bank_offset % self.prg_rom_size]
            }

            // PRG ROM Bank 1: $C000-$FFFF
            0xC000..=0xFFFF => {
                let offset = (addr - 0xC000) as usize;
                let bank_offset = self.prg_bank1 * 0x4000 + offset;
                self.prg_rom[bank_offset % self.prg_rom_size]
            }

            _ => 0,  // Open bus
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // PRG RAM: $6000-$7FFF
            0x6000..=0x7FFF => {
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset] = value;
            }

            // Mapper registers: $8000-$FFFF
            0x8000..=0xFFFF => {
                self.write_register(addr, value);
            }

            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let offset = (addr & 0x1FFF) as usize;
                if self.chr_rom_size > 0 {
                    let bank_offset = self.chr_bank0 * 0x2000 + offset;
                    self.chr_rom[bank_offset % self.chr_rom_size]
                } else {
                    self.chr_ram[offset]
                }
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_rom_size == 0 {
                    // CHR RAM
                    let offset = (addr & 0x1FFF) as usize;
                    self.chr_ram[offset] = value;
                }
            }
            _ => {}
        }
    }
}
```

### Step 4: Implement Register Writes

```rust
impl MapperXXX {
    fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // Bank select
                self.prg_bank0 = (value & 0x0F) as usize;
            }

            0xA000..=0xBFFF => {
                // CHR bank select
                self.chr_bank0 = (value & 0x0F) as usize;
            }

            0xC000..=0xDFFF => {
                // IRQ latch
                self.irq_counter = value as u16;
            }

            0xE000..=0xFFFF => {
                // IRQ enable/disable
                self.irq_enabled = (value & 0x01) != 0;
            }

            _ => {}
        }
    }
}
```

---

## Common Patterns

### Fixed Bank + Switchable Bank

```rust
// Pattern: First bank switchable, last bank fixed
fn cpu_read_fixed_last(&self, addr: u16) -> u8 {
    match addr {
        0x8000..=0xBFFF => {
            // Switchable bank
            let offset = (addr - 0x8000) as usize;
            let bank_offset = self.prg_bank * 0x4000 + offset;
            self.prg_rom[bank_offset]
        }
        0xC000..=0xFFFF => {
            // Fixed to last bank
            let offset = (addr - 0xC000) as usize;
            let last_bank = (self.prg_rom_size / 0x4000) - 1;
            let bank_offset = last_bank * 0x4000 + offset;
            self.prg_rom[bank_offset]
        }
        _ => 0,
    }
}
```

### Bus Conflicts

Some mappers (NROM, CNROM, UXROM) have bus conflicts:

```rust
fn cpu_write_with_bus_conflict(&mut self, addr: u16, value: u8) {
    if addr >= 0x8000 {
        // Read current value from ROM
        let rom_value = self.cpu_read(addr);

        // AND value with ROM value (bus conflict)
        let actual_value = value & rom_value;

        self.write_register(addr, actual_value);
    }
}
```

### CHR Banking Granularity

```rust
// 1KB CHR banks (8 banks)
fn map_chr_1kb(&self, addr: u16) -> usize {
    let bank = (addr / 0x400) as usize;  // 0-7
    let offset = (addr % 0x400) as usize;
    self.chr_banks[bank] * 0x400 + offset
}

// 2KB CHR banks (4 banks)
fn map_chr_2kb(&self, addr: u16) -> usize {
    let bank = (addr / 0x800) as usize;  // 0-3
    let offset = (addr % 0x800) as usize;
    self.chr_banks[bank] * 0x800 + offset
}

// 4KB CHR banks (2 banks)
fn map_chr_4kb(&self, addr: u16) -> usize {
    let bank = (addr / 0x1000) as usize;  // 0-1
    let offset = (addr % 0x1000) as usize;
    self.chr_banks[bank] * 0x1000 + offset
}
```

---

## Banking Systems

### Simple Banking (NROM, CNROM, UXROM)

```rust
// UXROM: 16KB switchable + 16KB fixed
self.prg_bank = value & 0x0F;

// CNROM: 8KB CHR banking
self.chr_bank = value & 0x03;
```

### MMC1 - Sequential Writes

```rust
struct Mmc1Shift {
    shift_register: u8,
    write_count: u8,
}

fn write_mmc1(&mut self, addr: u16, value: u8) {
    if value & 0x80 != 0 {
        // Reset
        self.shift_register = 0;
        self.write_count = 0;
        self.control |= 0x0C;  // Set both PRG mode bits
    } else {
        // Accumulate bit
        self.shift_register = (self.shift_register >> 1) | ((value & 0x01) << 4);
        self.write_count += 1;

        if self.write_count == 5 {
            // Full write complete
            self.update_register(addr, self.shift_register);
            self.shift_register = 0;
            self.write_count = 0;
        }
    }
}
```

### MMC3 - Bank Select + Bank Data

```rust
fn write_mmc3(&mut self, addr: u16, value: u8) {
    match addr & 0xE001 {
        0x8000 => {
            // Bank select
            self.bank_select = value & 0x07;
            self.prg_bank_mode = (value & 0x40) != 0;
            self.chr_bank_mode = (value & 0x80) != 0;
        }
        0x8001 => {
            // Bank data
            self.bank_registers[self.bank_select as usize] = value;
            self.update_banks();
        }
        0xA000 => {
            // Mirroring
            self.mirroring = if value & 0x01 != 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        }
        0xC000 => self.irq_latch = value,
        0xC001 => self.irq_reload = true,
        0xE000 => {
            self.irq_enabled = false;
            self.irq_pending = false;
        }
        0xE001 => self.irq_enabled = true,
        _ => {}
    }
}
```

---

## IRQ Implementation

### Scanline Counter (MMC3)

```rust
impl Mapper for MapperMMC3 {
    fn ppu_scanline_tick(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }
}
```

### CPU Cycle Counter (VRC6/7)

```rust
impl Mapper for MapperVRC6 {
    fn tick(&mut self) {
        if self.irq_enabled {
            if self.irq_mode == IrqMode::Scanline {
                // Scanline mode: increment every 114 CPU cycles
                self.irq_prescaler += 1;
                if self.irq_prescaler >= 114 {
                    self.irq_prescaler -= 114;
                    self.irq_counter = self.irq_counter.wrapping_add(1);
                }
            } else {
                // Cycle mode: increment every CPU cycle
                self.irq_counter = self.irq_counter.wrapping_add(1);
            }

            if self.irq_counter == 0 {
                self.irq_pending = true;
            }
        }
    }
}
```

---

## Testing Checklist

### Basic Functionality

- [ ] ROM loads without crashing
- [ ] Title screen displays correctly
- [ ] Basic gameplay works
- [ ] No visual glitches in background
- [ ] Sprites display correctly

### Banking

- [ ] All PRG banks accessible
- [ ] All CHR banks accessible
- [ ] Bank switching doesn't cause corruption
- [ ] Fixed banks remain fixed

### Special Features

- [ ] Mirroring changes work correctly
- [ ] IRQs trigger at correct time
- [ ] PRG RAM saves/loads correctly
- [ ] Bus conflicts handled (if applicable)

### Test ROMs

- [ ] Mapper-specific test ROMs pass
- [ ] Holy Diver (MMC3 IRQ stress test)
- [ ] Mapper behavior matches hardware

### Commercial Games

- [ ] 3-5 popular games work correctly
- [ ] No crashes or freezes
- [ ] Audio plays correctly
- [ ] Game is completable

---

## Example Implementations

### NROM (Mapper 000)

See [MAPPER_000_NROM.md](MAPPER_000_NROM.md)

### MMC1 (Mapper 001)

See [MAPPER_001_MMC1.md](MAPPER_001_MMC1.md)

### UxROM (Mapper 002)

See [MAPPER_002_UXROM.md](MAPPER_002_UXROM.md)

### MMC3 (Mapper 004)

See [MAPPER_004_MMC3.md](MAPPER_004_MMC3.md)

---

## Related Documentation

- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - Mapper system overview
- [MAPPER_SUBMAPPER_GUIDE.md](MAPPER_SUBMAPPER_GUIDE.md) - Submapper handling
- [../bus/BUS_ARCHITECTURE.md](../bus/BUS_ARCHITECTURE.md) - Bus system
- [../bus/MEMORY_MAP.md](../bus/MEMORY_MAP.md) - Memory layout

---

## References

- [NESdev Wiki: Mapper](https://www.nesdev.org/wiki/Mapper)
- [NESdev Wiki: List of Mappers](https://www.nesdev.org/wiki/List_of_mappers)
- Bootgod's NES Cartridge Database
- Mesen2 mapper implementations
- puNES mapper implementations

---

**Document Status:** Complete guide for implementing new NES mappers.
