# NES Memory Map Reference

**Table of Contents**
- [Overview](#overview)
- [CPU Address Space](#cpu-address-space)
  - [Internal RAM](#internal-ram)
  - [PPU Registers](#ppu-registers)
  - [APU and I/O Registers](#apu-and-io-registers)
  - [Cartridge Space](#cartridge-space)
- [PPU Address Space](#ppu-address-space)
  - [Pattern Tables (CHR)](#pattern-tables-chr)
  - [Nametables](#nametables)
  - [Palette RAM](#palette-ram)
- [Mirroring Behavior](#mirroring-behavior)
- [Open Bus Behavior](#open-bus-behavior)
- [Implementation Guidelines](#implementation-guidelines)
- [References](#references)

---

## Overview

The NES has two separate address spaces: the **CPU address space** (16-bit, $0000-$FFFF) and the **PPU address space** (14-bit, $0000-$3FFF). These spaces are completely independent, though the CPU can indirectly access PPU memory through memory-mapped registers.

### Key Characteristics

- **CPU**: 16-bit address bus, 64KB addressable space
- **PPU**: 14-bit address bus, 16KB addressable space
- **Separation**: CPU and PPU buses are electrically independent
- **Communication**: CPU accesses PPU via memory-mapped I/O registers at $2000-$2007
- **Mirroring**: Extensive mirroring used to conserve RAM (2KB internal RAM appears in 8KB space)

---

## CPU Address Space

The CPU's 16-bit address space ($0000-$FFFF) is divided into several regions:

```
$0000-$07FF   Internal RAM (2KB)
$0800-$1FFF   Mirrors of $0000-$07FF (3 times)
$2000-$2007   PPU Registers
$2008-$3FFF   Mirrors of $2000-$2007 (1,023 times)
$4000-$4017   APU and I/O Registers
$4018-$401F   APU and I/O functionality (disabled on retail consoles)
$4020-$FFFF   Cartridge space (PRG-ROM, PRG-RAM, mapper registers)
```

### Internal RAM

**Address Range**: $0000-$07FF (2KB physical RAM)
**Mirrored To**: $0800-$1FFF (appears 4 times total in first 8KB)

The NES contains 2KB of internal static RAM (SRAM) for general-purpose use. This RAM is mirrored three additional times to fill the first 8KB of CPU address space.

#### Mirroring Pattern

```
$0000-$07FF: RAM
$0800-$0FFF: Mirror of $0000-$07FF
$1000-$17FF: Mirror of $0000-$07FF
$1800-$1FFF: Mirror of $0000-$07FF
```

**Example**: Writing $42 to $0123 makes it visible at $0923, $1123, and $1923.

#### Common Usage Conventions

While the RAM can be used for any purpose, several areas have common uses:

| Address Range | Common Usage |
|---------------|--------------|
| $0000-$00FF | Zero Page (fast access variables) |
| $0100-$01FF | Stack (grows downward from $01FF) |
| $0200-$02FF | OAM buffer (sprite data for DMA transfer) |
| $0300-$07FF | General purpose variables and buffers |

**Implementation Note**: The mirroring is handled by simply masking the address:
```rust
fn read_ram(&self, addr: u16) -> u8 {
    self.ram[(addr & 0x07FF) as usize]
}

fn write_ram(&mut self, addr: u16, value: u8) {
    self.ram[(addr & 0x07FF) as usize] = value;
}
```

### PPU Registers

**Address Range**: $2000-$2007 (8 registers)
**Mirrored To**: $2008-$3FFF (repeats every 8 bytes)

The PPU exposes eight memory-mapped registers to the CPU. Because the address decoding is incomplete (only uses A0-A2), these registers appear mirrored throughout the $2000-$3FFF range.

#### Register Map

| Address | Register | Name | Access |
|---------|----------|------|--------|
| $2000 | PPUCTRL | PPU Control | Write |
| $2001 | PPUMASK | PPU Mask | Write |
| $2002 | PPUSTATUS | PPU Status | Read |
| $2003 | OAMADDR | OAM Address | Write |
| $2004 | OAMDATA | OAM Data | Read/Write |
| $2005 | PPUSCROLL | PPU Scroll | Write (2x) |
| $2006 | PPUADDR | PPU Address | Write (2x) |
| $2007 | PPUDATA | PPU Data | Read/Write |

**Mirroring**: Any address in $2008-$3FFF maps to one of the eight registers:
```
$2008 → $2000 (PPUCTRL)
$2009 → $2001 (PPUMASK)
$200A → $2002 (PPUSTATUS)
...
$3FF8 → $2000 (PPUCTRL)
$3FF9 → $2001 (PPUMASK)
...
```

**Implementation**:
```rust
fn map_ppu_register(addr: u16) -> u16 {
    0x2000 + (addr & 0x0007)
}
```

See [PPU_OVERVIEW.md](../ppu/PPU_OVERVIEW.md) for detailed register specifications.

### APU and I/O Registers

**Address Range**: $4000-$4017 (24 registers)

This region contains registers for the APU (Audio Processing Unit) and I/O devices (controllers, DMA).

#### APU Registers ($4000-$4013, $4015, $4017)

| Address | Register | Description |
|---------|----------|-------------|
| $4000-$4003 | Pulse 1 | Pulse wave channel 1 (duty, volume, sweep, timer) |
| $4004-$4007 | Pulse 2 | Pulse wave channel 2 |
| $4008-$400B | Triangle | Triangle wave channel |
| $400C-$400F | Noise | Noise channel |
| $4010-$4013 | DMC | Delta Modulation Channel |
| $4015 | SND_CHN | Sound channel enable/status |
| $4017 | FRAME_COUNTER | Frame counter control |

#### I/O Registers

| Address | Register | Description | Access |
|---------|----------|-------------|--------|
| $4014 | OAMDMA | OAM DMA transfer | Write |
| $4016 | JOY1 | Controller 1 / Expansion port | Read/Write |
| $4017 | JOY2/FRAME | Controller 2 / Frame counter | Read/Write |

**Note**: $4017 is dual-purpose:
- **Write**: APU Frame Counter mode
- **Read**: Controller 2 data

See [APU_OVERVIEW.md](../apu/APU_OVERVIEW.md) and [INPUT_HANDLING.md](../input/INPUT_HANDLING.md) for details.

#### Disabled Test Registers ($4018-$401F)

These registers were used for testing APU/CPU functionality at the factory but are disabled on retail consoles. Reading returns open bus; writes have no effect.

**Implementation**: Typically ignored in emulators unless emulating development hardware.

### Cartridge Space

**Address Range**: $4020-$FFFF (nearly 64KB)

This is the largest region and contains cartridge ROM, RAM, and mapper-specific registers.

#### Standard Layout (Mapper 0 - NROM)

```
$4020-$5FFF   Expansion ROM (rarely used)
$6000-$7FFF   Battery-backed Save RAM (SRAM, 8KB typical)
$8000-$BFFF   PRG-ROM Lower Bank (16KB)
$C000-$FFFF   PRG-ROM Upper Bank (16KB)
```

#### Mapper Variations

Different mappers remap this space:
- **UxROM (Mapper 2)**: $8000-$BFFF switchable, $C000-$FFFF fixed to last bank
- **MMC1 (Mapper 1)**: Configurable 16KB or 32KB banking
- **MMC3 (Mapper 4)**: Two 8KB banks at $8000/$A000, two 8KB fixed banks at $C000/$E000

See [Mapper Documentation](../mappers/MAPPER_OVERVIEW.md) for mapper-specific memory layouts.

#### Interrupt Vectors ($FFFA-$FFFF)

The top 6 bytes of CPU address space contain three 16-bit vectors:

| Address | Vector | Purpose |
|---------|--------|---------|
| $FFFA-$FFFB | NMI | Non-Maskable Interrupt (VBlank) |
| $FFFC-$FFFD | RESET | Power-on and reset initialization |
| $FFFE-$FFFF | IRQ/BRK | Interrupt Request / Break instruction |

**Little-Endian**: Vectors are stored low byte first (e.g., $FFFC = low byte, $FFFD = high byte).

**Example**:
```
$FFFC: $00
$FFFD: $80
→ Reset vector points to $8000
```

These vectors are always supplied by the cartridge ROM (typically in the fixed bank).

---

## PPU Address Space

The PPU addresses a 14-bit (16KB) address space, $0000-$3FFF, completely separate from the CPU's address bus.

```
$0000-$0FFF   Pattern Table 0 (4KB)
$1000-$1FFF   Pattern Table 1 (4KB)
$2000-$23FF   Nametable 0 (1KB)
$2400-$27FF   Nametable 1 (1KB)
$2800-$2BFF   Nametable 2 (1KB)
$2C00-$2FFF   Nametable 3 (1KB)
$3000-$3EFF   Mirrors of $2000-$2EFF
$3F00-$3F1F   Palette RAM (32 bytes)
$3F20-$3FFF   Mirrors of $3F00-$3F1F
```

### Pattern Tables (CHR)

**Address Range**: $0000-$1FFF (8KB)

Pattern tables store sprite and background tile graphics. Each tile is 8x8 pixels with 2 bits per pixel (4 colors), requiring 16 bytes per tile.

- **$0000-$0FFF**: Pattern Table 0 (256 tiles)
- **$1000-$1FFF**: Pattern Table 1 (256 tiles)

#### Cartridge Mapping

Pattern tables are usually mapped by the cartridge:
- **CHR-ROM**: Read-only graphics data on cartridge
- **CHR-RAM**: Writable RAM for dynamic graphics (common in later games)

Mappers can bank-switch pattern table regions for more than 512 tiles.

### Nametables

**Address Range**: $2000-$2FFF (4KB logical, 2KB physical)

Nametables define the layout of background tiles. Each nametable is 1024 bytes:
- **960 bytes**: 30 rows × 32 columns of tile indices
- **64 bytes**: Attribute table (2×2 tile color groups)

#### Nametable Layout

```
$2000-$23BF: Tile indices (32×30 = 960 bytes)
$23C0-$23FF: Attribute table (64 bytes)
$2400-$27BF: Nametable 1 tile indices
$27C0-$27FF: Nametable 1 attributes
... (similar for nametables 2 and 3)
```

#### Physical RAM

The NES has only **2KB of internal VRAM**, enough for two nametables. The other two are either mirrored or provided by cartridge RAM, depending on the **mirroring mode**:

**Horizontal Mirroring** (vertical scrolling games):
```
$2000 = $2400 (Nametable 0 = Nametable 1)
$2800 = $2C00 (Nametable 2 = Nametable 3)
```

**Vertical Mirroring** (horizontal scrolling games):
```
$2000 = $2800 (Nametable 0 = Nametable 2)
$2400 = $2C00 (Nametable 1 = Nametable 3)
```

**Single-Screen** (stationary screens):
```
All nametables mirror the same 1KB
```

**Four-Screen** (advanced mappers like MMC5):
```
Cartridge provides 2KB extra RAM for independent nametables
```

### Palette RAM

**Address Range**: $3F00-$3F1F (32 bytes)
**Mirrored To**: $3F20-$3FFF

Palette RAM stores color indices (pointing to the NES's master palette of 64 colors):
- **$3F00-$3F0F**: Background palettes (4 palettes × 4 colors)
- **$3F10-$3F1F**: Sprite palettes (4 palettes × 4 colors)

#### Palette Structure

Each palette consists of 4 colors:
- **Color 0**: Transparent for sprites, backdrop for backgrounds
- **Colors 1-3**: Opaque colors for the palette

**Special Mirroring**: $3F00, $3F04, $3F08, $3F0C (background color 0) all mirror $3F00.
Similarly, $3F10, $3F14, $3F18, $3F1C mirror $3F00 (not $3F10).

**Implementation**:
```rust
fn read_palette(&self, addr: u16) -> u8 {
    let addr = addr & 0x1F;
    // Mirror backdrop colors
    let addr = if addr >= 0x10 && (addr & 0x03) == 0 {
        addr & 0x0F
    } else {
        addr
    };
    self.palette[addr as usize]
}
```

---

## Mirroring Behavior

### RAM Mirroring

The 2KB internal RAM ($0000-$07FF) appears four times in the first 8KB:

```
Physical:  $0000-$07FF
Mirrors:   $0800-$0FFF
           $1000-$17FF
           $1800-$1FFF
```

**Mask**: `addr & 0x07FF`

### PPU Register Mirroring

The 8 PPU registers ($2000-$2007) repeat every 8 bytes through $3FFF:

```
$2000-$2007: Original
$2008-$200F: Mirror
...
$3FF8-$3FFF: Mirror
```

**Mask**: `0x2000 + (addr & 0x0007)`

### Nametable Mirroring

Controlled by cartridge hardware (mirroring mode set by mapper):

**Horizontal** (bit A10 ignored):
```rust
fn horizontal_mirror(addr: u16) -> u16 {
    match (addr >> 10) & 0x03 {
        0 | 1 => addr & 0x03FF,        // $2000/$2400 → VRAM $0000
        2 | 3 => 0x0400 + (addr & 0x03FF), // $2800/$2C00 → VRAM $0400
        _ => unreachable!(),
    }
}
```

**Vertical** (bit A11 ignored):
```rust
fn vertical_mirror(addr: u16) -> u16 {
    addr & 0x07FF
}
```

### Palette Mirroring

$3F00-$3F1F repeats every 32 bytes to $3FFF:

```
$3F00-$3F1F: Original
$3F20-$3F3F: Mirror
...
$3FE0-$3FFF: Mirror
```

**Mask**: `0x3F00 + (addr & 0x1F)`

Additionally, backdrop colors mirror universally to $3F00.

---

## Open Bus Behavior

Reading from unmapped addresses returns the **last value on the data bus** (open bus). This value decays over time (milliseconds) but persists temporarily.

### Common Open Bus Scenarios

| Address Range | Behavior |
|---------------|----------|
| $2000-$2007 (write-only registers) | Returns open bus |
| $4018-$401F | Returns open bus (test registers disabled) |
| Unmapped cartridge space | May return open bus or cartridge-specific behavior |

### PPUSTATUS ($2002) Special Case

Reading $2002 returns:
- **Bits 7-5**: Actual PPU status flags
- **Bits 4-0**: Open bus (last value on bus)

**Implementation**:
```rust
fn read_ppustatus(&mut self) -> u8 {
    let status = (self.ppu_status & 0xE0) | (self.open_bus & 0x1F);
    self.vblank_flag = false; // Reading $2002 clears VBlank flag
    status
}
```

### Cartridge Open Bus

Behavior varies by mapper:
- **NROM**: Reads from unmapped regions typically return open bus
- **Some mappers**: Pull certain bits high or low
- **Advanced mappers**: May have complex open bus interactions

**Testing**: Test ROMs like `open_bus_test` validate correct implementation.

---

## Implementation Guidelines

### Bus Structure

```rust
pub struct Bus {
    // Internal RAM (2KB)
    ram: [u8; 0x0800],

    // Components
    ppu: Ppu,
    apu: Apu,
    cartridge: Box<dyn Mapper>,
    controller1: Controller,
    controller2: Controller,

    // Open bus value
    open_bus: u8,
}

impl Bus {
    pub fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(0x2000 + (addr & 0x07)),
            0x4000..=0x4013 | 0x4015 => self.apu.read_register(addr),
            0x4016 => self.controller1.read(),
            0x4017 => self.controller2.read(),
            0x4020..=0xFFFF => self.cartridge.read_prg(addr),
            _ => self.open_bus, // Unmapped regions
        };

        self.open_bus = value;
        value
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.open_bus = value;

        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(0x2000 + (addr & 0x07), value),
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write_register(addr, value),
            0x4014 => self.oam_dma(value),
            0x4016 => {
                self.controller1.write(value);
                self.controller2.write(value);
            }
            0x4020..=0xFFFF => self.cartridge.write_prg(addr, value),
            _ => {} // Unmapped regions (ignore writes)
        }
    }
}
```

### Performance Considerations

1. **Avoid dynamic dispatch for RAM**: Use direct array indexing
2. **Inline hot paths**: Mark memory access functions with `#[inline]`
3. **Minimize bounds checks**: Use unsafe indexing if profiling shows benefit (after validation)
4. **Separate PPU bus**: PPU memory accesses should not go through CPU bus

### Testing

**Test ROMs**:
- `nestest.nes`: Comprehensive CPU bus testing
- `ppu_vbl_nmi`: PPU register timing
- `oam_read/write`: OAM access behavior
- `open_bus_test`: Open bus decay

**Unit Tests**:
```rust
#[test]
fn test_ram_mirroring() {
    let mut bus = Bus::new();
    bus.write(0x0000, 0x42);
    assert_eq!(bus.read(0x0000), 0x42);
    assert_eq!(bus.read(0x0800), 0x42); // Mirror 1
    assert_eq!(bus.read(0x1000), 0x42); // Mirror 2
    assert_eq!(bus.read(0x1800), 0x42); // Mirror 3
}

#[test]
fn test_ppu_register_mirroring() {
    let mut bus = Bus::new();
    // Writing to $2000 should be readable from mirrors
    bus.write(0x2000, 0x80);
    // (Note: PPUCTRL is write-only, this is just testing routing)
    assert_eq!(bus.read(0x2008) & 0x80, 0x80); // Should mirror
}
```

---

## References

- [NesDev Wiki: CPU Memory Map](https://www.nesdev.org/wiki/CPU_memory_map)
- [NesDev Wiki: PPU Memory Map](https://www.nesdev.org/wiki/PPU_memory_map)
- [NesDev Wiki: Mirroring](https://www.nesdev.org/wiki/Mirroring)
- [CPU_6502.md](../cpu/CPU_6502.md) - CPU architecture
- [PPU_OVERVIEW.md](../ppu/PPU_OVERVIEW.md) - PPU registers and operation
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Cartridge memory mapping

---

**Related Documents**:
- [BUS_CONFLICTS.md](BUS_CONFLICTS.md) - Bus conflict behavior
- [CPU_6502.md](../cpu/CPU_6502.md) - CPU instruction set
- [PPU_OVERVIEW.md](../ppu/PPU_OVERVIEW.md) - PPU architecture
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper implementations
