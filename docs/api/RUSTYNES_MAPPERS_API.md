# RustyNES Mappers Crate API Reference

**Crate:** `rustynes-mappers`
**Version:** 0.1.0
**License:** MIT/Apache-2.0

The `rustynes-mappers` crate provides a complete mapper abstraction layer and implementations for NES cartridge memory mapping ICs. Supports 300+ mappers with dynamic registration.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Mapper Trait](#mapper-trait)
- [Cartridge Loading](#cartridge-loading)
- [Mapper Registry](#mapper-registry)
- [Common Mappers](#common-mappers)
- [Banking Utilities](#banking-utilities)
- [IRQ Handling](#irq-handling)
- [Save States](#save-states)
- [Examples](#examples)

---

## Quick Start

```rust
use rustynes_mappers::{create_mapper, Mapper, Cartridge, Mirroring};

fn load_rom(path: &str) -> Box<dyn Mapper> {
    let data = std::fs::read(path).expect("Failed to read ROM");
    let cartridge = Cartridge::from_ines(&data).expect("Invalid iNES file");

    create_mapper(cartridge).expect("Unsupported mapper")
}

fn main() {
    let mut mapper = load_rom("game.nes");

    // Read PRG-ROM
    let value = mapper.read_prg(0x8000);

    // Write to mapper (bank switch)
    mapper.write_prg(0x8000, 0x01);

    // Read CHR (for PPU)
    let tile_byte = mapper.read_chr(0x0000);

    // Get mirroring mode
    let mirroring = mapper.mirroring();
}
```

---

## Mapper Trait

### Core Interface

```rust
/// Cartridge mapper interface
pub trait Mapper: Send + Sync {
    /// Read from PRG address space ($4020-$FFFF)
    fn read_prg(&self, addr: u16) -> u8;

    /// Write to PRG address space
    fn write_prg(&mut self, addr: u16, value: u8);

    /// Read from CHR address space ($0000-$1FFF)
    fn read_chr(&self, addr: u16) -> u8;

    /// Write to CHR address space (CHR-RAM only)
    fn write_chr(&mut self, addr: u16, value: u8);

    /// Get current nametable mirroring mode
    fn mirroring(&self) -> Mirroring;

    /// Check if IRQ is pending
    fn irq_pending(&self) -> bool {
        false
    }

    /// Acknowledge/clear pending IRQ
    fn irq_acknowledge(&mut self) {}

    /// Clock mapper (for CPU cycle counters)
    fn clock_cpu(&mut self) {}

    /// Notify PPU scanline (for scanline counters)
    fn notify_scanline(&mut self) {}

    /// Notify PPU A12 rising edge (for MMC3-style IRQs)
    fn notify_a12_rise(&mut self) {}

    /// Get battery-backed RAM for save
    fn battery_ram(&self) -> Option<&[u8]> {
        None
    }

    /// Load battery-backed RAM
    fn load_battery_ram(&mut self, _data: &[u8]) {}

    /// Get mapper number
    fn mapper_number(&self) -> u16;

    /// Get mapper name
    fn mapper_name(&self) -> &'static str;

    /// Get submapper number (NES 2.0)
    fn submapper(&self) -> u8 {
        0
    }
}
```

### State Serialization

```rust
/// Mapper state for save states
pub trait MapperState: Mapper {
    /// Serialize mapper state
    fn save_state(&self) -> MapperStateData;

    /// Restore mapper state
    fn load_state(&mut self, state: &MapperStateData);
}

/// Serialized mapper state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MapperStateData {
    /// Mapper number
    pub mapper: u16,
    /// Submapper number
    pub submapper: u8,
    /// Mapper-specific register state
    pub registers: Vec<u8>,
    /// PRG-RAM contents (if any)
    pub prg_ram: Option<Vec<u8>>,
    /// CHR-RAM contents (if any)
    pub chr_ram: Option<Vec<u8>>,
}
```

---

## Cartridge Loading

### Cartridge Struct

```rust
/// Parsed cartridge data
#[derive(Debug, Clone)]
pub struct Cartridge {
    /// PRG-ROM data
    pub prg_rom: Vec<u8>,
    /// CHR-ROM data (empty if CHR-RAM)
    pub chr_rom: Vec<u8>,
    /// PRG-RAM size in bytes
    pub prg_ram_size: usize,
    /// CHR-RAM size in bytes (if no CHR-ROM)
    pub chr_ram_size: usize,
    /// Mapper number
    pub mapper: u16,
    /// Submapper (NES 2.0)
    pub submapper: u8,
    /// Initial mirroring
    pub mirroring: Mirroring,
    /// Has battery-backed RAM
    pub has_battery: bool,
    /// Console type
    pub console_type: ConsoleType,
    /// TV system
    pub tv_system: TvSystem,
}
```

### Loading Functions

```rust
impl Cartridge {
    /// Load from iNES format
    pub fn from_ines(data: &[u8]) -> Result<Self, CartridgeError> {
        if data.len() < 16 {
            return Err(CartridgeError::TooSmall);
        }

        // Check magic number
        if &data[0..4] != b"NES\x1A" {
            return Err(CartridgeError::InvalidMagic);
        }

        let prg_size = data[4] as usize * 16384;
        let chr_size = data[5] as usize * 8192;

        let flags6 = data[6];
        let flags7 = data[7];

        let mapper_lo = (flags6 >> 4) & 0x0F;
        let mapper_hi = flags7 & 0xF0;
        let mapper = (mapper_hi | mapper_lo) as u16;

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_battery = flags6 & 0x02 != 0;
        let has_trainer = flags6 & 0x04 != 0;

        let header_size = if has_trainer { 16 + 512 } else { 16 };

        let prg_start = header_size;
        let prg_end = prg_start + prg_size;
        let chr_start = prg_end;
        let chr_end = chr_start + chr_size;

        if data.len() < chr_end {
            return Err(CartridgeError::TooSmall);
        }

        Ok(Self {
            prg_rom: data[prg_start..prg_end].to_vec(),
            chr_rom: data[chr_start..chr_end].to_vec(),
            prg_ram_size: 8192,
            chr_ram_size: if chr_size == 0 { 8192 } else { 0 },
            mapper,
            submapper: 0,
            mirroring,
            has_battery,
            console_type: ConsoleType::Nes,
            tv_system: TvSystem::Ntsc,
        })
    }

    /// Load from NES 2.0 format
    pub fn from_nes20(data: &[u8]) -> Result<Self, CartridgeError> {
        // Extended parsing for NES 2.0...
        let mut cart = Self::from_ines(data)?;

        let flags7 = data[7];
        if (flags7 & 0x0C) == 0x08 {
            // NES 2.0 format
            let mapper_ext = data[8] & 0x0F;
            cart.mapper |= (mapper_ext as u16) << 8;
            cart.submapper = (data[8] >> 4) & 0x0F;

            // PRG-RAM size
            let prg_ram_shift = data[10] & 0x0F;
            if prg_ram_shift > 0 {
                cart.prg_ram_size = 64 << prg_ram_shift;
            }

            // CHR-RAM size
            let chr_ram_shift = data[11] & 0x0F;
            if chr_ram_shift > 0 {
                cart.chr_ram_size = 64 << chr_ram_shift;
            }

            // TV system
            cart.tv_system = match data[12] & 0x03 {
                0 => TvSystem::Ntsc,
                1 => TvSystem::Pal,
                2 => TvSystem::Multi,
                3 => TvSystem::Dendy,
                _ => TvSystem::Ntsc,
            };
        }

        Ok(cart)
    }

    /// Load from UNIF format
    pub fn from_unif(data: &[u8]) -> Result<Self, CartridgeError> {
        // UNIF parsing...
        todo!()
    }
}
```

### Error Types

```rust
use thiserror::Error;

/// Cartridge loading errors
#[derive(Debug, Error)]
pub enum CartridgeError {
    #[error("File too small for valid ROM")]
    TooSmall,

    #[error("Invalid magic number (expected NES\\x1A)")]
    InvalidMagic,

    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),

    #[error("Invalid PRG-ROM size")]
    InvalidPrgSize,

    #[error("Invalid CHR-ROM size")]
    InvalidChrSize,

    #[error("Corrupted ROM data")]
    CorruptedData,
}
```

---

## Mapper Registry

### Dynamic Registration

```rust
use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;

/// Mapper constructor function
type MapperConstructor = fn(Cartridge) -> Box<dyn Mapper>;

/// Global mapper registry
static MAPPER_REGISTRY: Lazy<RwLock<HashMap<u16, MapperConstructor>>> =
    Lazy::new(|| {
        let mut map = HashMap::new();
        register_standard_mappers(&mut map);
        RwLock::new(map)
    });

/// Register a mapper implementation
pub fn register_mapper(number: u16, constructor: MapperConstructor) {
    MAPPER_REGISTRY
        .write()
        .unwrap()
        .insert(number, constructor);
}

/// Create mapper from cartridge
pub fn create_mapper(cartridge: Cartridge) -> Result<Box<dyn Mapper>, CartridgeError> {
    let registry = MAPPER_REGISTRY.read().unwrap();
    let constructor = registry
        .get(&cartridge.mapper)
        .ok_or(CartridgeError::UnsupportedMapper(cartridge.mapper))?;

    Ok(constructor(cartridge))
}

/// Check if mapper is supported
pub fn is_mapper_supported(number: u16) -> bool {
    MAPPER_REGISTRY.read().unwrap().contains_key(&number)
}

/// Get list of supported mappers
pub fn supported_mappers() -> Vec<u16> {
    MAPPER_REGISTRY.read().unwrap().keys().copied().collect()
}
```

### Standard Mapper Registration

```rust
fn register_standard_mappers(map: &mut HashMap<u16, MapperConstructor>) {
    // Nintendo first-party
    map.insert(0, |c| Box::new(Nrom::new(c)));
    map.insert(1, |c| Box::new(Mmc1::new(c)));
    map.insert(2, |c| Box::new(UxRom::new(c)));
    map.insert(3, |c| Box::new(Cnrom::new(c)));
    map.insert(4, |c| Box::new(Mmc3::new(c)));
    map.insert(5, |c| Box::new(Mmc5::new(c)));
    map.insert(7, |c| Box::new(AxRom::new(c)));
    map.insert(9, |c| Box::new(Mmc2::new(c)));
    map.insert(10, |c| Box::new(Mmc4::new(c)));

    // Konami
    map.insert(21, |c| Box::new(Vrc4::new(c, Vrc4Variant::Vrc4a)));
    map.insert(22, |c| Box::new(Vrc2::new(c, Vrc2Variant::Vrc2a)));
    map.insert(23, |c| Box::new(Vrc4::new(c, Vrc4Variant::Vrc4e)));
    map.insert(24, |c| Box::new(Vrc6::new(c, Vrc6Variant::Vrc6a)));
    map.insert(25, |c| Box::new(Vrc4::new(c, Vrc4Variant::Vrc4b)));
    map.insert(26, |c| Box::new(Vrc6::new(c, Vrc6Variant::Vrc6b)));
    map.insert(85, |c| Box::new(Vrc7::new(c)));

    // Namco
    map.insert(19, |c| Box::new(Namco163::new(c)));
    map.insert(210, |c| Box::new(Namco175_340::new(c)));

    // Sunsoft
    map.insert(68, |c| Box::new(Sunsoft4::new(c)));
    map.insert(69, |c| Box::new(Fme7::new(c)));

    // Common unlicensed
    map.insert(11, |c| Box::new(ColorDreams::new(c)));
    map.insert(34, |c| Box::new(BnRom::new(c)));
    map.insert(66, |c| Box::new(GxRom::new(c)));
    map.insert(71, |c| Box::new(Camerica::new(c)));
    map.insert(79, |c| Box::new(Nina003_006::new(c)));

    // Multi-cart
    map.insert(28, |c| Box::new(Action53::new(c)));

    // ... (300+ more mappers)
}
```

---

## Common Mappers

### NROM (Mapper 0)

```rust
/// NROM - No mapper (32KB PRG, 8KB CHR)
pub struct Nrom {
    prg_rom: Vec<u8>,
    chr_mem: Vec<u8>,
    chr_is_ram: bool,
    mirroring: Mirroring,
}

impl Nrom {
    pub fn new(cart: Cartridge) -> Self {
        let chr_is_ram = cart.chr_rom.is_empty();
        let chr_mem = if chr_is_ram {
            vec![0; cart.chr_ram_size]
        } else {
            cart.chr_rom
        };

        Self {
            prg_rom: cart.prg_rom,
            chr_mem,
            chr_is_ram,
            mirroring: cart.mirroring,
        }
    }
}

impl Mapper for Nrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, _addr: u16, _value: u8) {
        // No writable registers
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_mem[addr as usize & 0x1FFF]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        if self.chr_is_ram {
            self.chr_mem[addr as usize & 0x1FFF] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 { 0 }
    fn mapper_name(&self) -> &'static str { "NROM" }
}
```

### MMC1 (Mapper 1)

```rust
/// MMC1 (SxROM) - Serial shift register mapper
pub struct Mmc1 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_mem: Vec<u8>,
    chr_is_ram: bool,

    // Shift register
    shift_register: u8,
    shift_count: u8,

    // Control registers
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,

    // Derived state
    prg_ram_enabled: bool,
    has_battery: bool,
}

impl Mapper for Mmc1 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr < 0x8000 {
            // PRG-RAM write
            if self.prg_ram_enabled && addr >= 0x6000 {
                self.prg_ram[(addr - 0x6000) as usize] = value;
            }
            return;
        }

        // Mapper register write
        if value & 0x80 != 0 {
            // Reset
            self.shift_register = 0;
            self.shift_count = 0;
            self.control |= 0x0C;
        } else {
            // Shift in bit
            self.shift_register |= (value & 0x01) << self.shift_count;
            self.shift_count += 1;

            if self.shift_count == 5 {
                // Load register
                match addr {
                    0x8000..=0x9FFF => self.control = self.shift_register,
                    0xA000..=0xBFFF => self.chr_bank_0 = self.shift_register,
                    0xC000..=0xDFFF => self.chr_bank_1 = self.shift_register,
                    0xE000..=0xFFFF => {
                        self.prg_bank = self.shift_register & 0x0F;
                        self.prg_ram_enabled = self.shift_register & 0x10 == 0;
                    }
                    _ => {}
                }
                self.shift_register = 0;
                self.shift_count = 0;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }

    fn battery_ram(&self) -> Option<&[u8]> {
        if self.has_battery {
            Some(&self.prg_ram)
        } else {
            None
        }
    }

    fn mapper_number(&self) -> u16 { 1 }
    fn mapper_name(&self) -> &'static str { "MMC1" }
}
```

### MMC3 (Mapper 4)

```rust
/// MMC3 (TxROM) - Scanline counter mapper
pub struct Mmc3 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_mem: Vec<u8>,
    chr_is_ram: bool,

    // Bank registers
    bank_select: u8,
    bank_data: [u8; 8],

    // IRQ
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,

    // State
    mirroring: Mirroring,
    prg_ram_protect: u8,
    has_battery: bool,
}

impl Mapper for Mmc3 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x6000 && addr < 0x8000 {
            if (self.prg_ram_protect & 0x80) != 0 && (self.prg_ram_protect & 0x40) == 0 {
                self.prg_ram[(addr - 0x6000) as usize] = value;
            }
            return;
        }

        match addr {
            0x8000..=0x9FFE if addr & 1 == 0 => {
                self.bank_select = value;
            }
            0x8001..=0x9FFF if addr & 1 == 1 => {
                let reg = (self.bank_select & 0x07) as usize;
                self.bank_data[reg] = value;
            }
            0xA000..=0xBFFE if addr & 1 == 0 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0xA001..=0xBFFF if addr & 1 == 1 => {
                self.prg_ram_protect = value;
            }
            0xC000..=0xDFFE if addr & 1 == 0 => {
                self.irq_latch = value;
            }
            0xC001..=0xDFFF if addr & 1 == 1 => {
                self.irq_reload = true;
            }
            0xE000..=0xFFFE if addr & 1 == 0 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xE001..=0xFFFF if addr & 1 == 1 => {
                self.irq_enabled = true;
            }
            _ => {}
        }
    }

    fn notify_scanline(&mut self) {
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

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn mapper_number(&self) -> u16 { 4 }
    fn mapper_name(&self) -> &'static str { "MMC3" }
}
```

---

## Banking Utilities

### Bank Calculation Helpers

```rust
/// PRG bank calculation utilities
pub mod prg_banking {
    /// Calculate 16KB bank offset
    pub fn bank_16k(rom: &[u8], bank: usize) -> usize {
        let bank_count = rom.len() / 0x4000;
        (bank % bank_count) * 0x4000
    }

    /// Calculate 8KB bank offset
    pub fn bank_8k(rom: &[u8], bank: usize) -> usize {
        let bank_count = rom.len() / 0x2000;
        (bank % bank_count) * 0x2000
    }

    /// Calculate 32KB bank offset
    pub fn bank_32k(rom: &[u8], bank: usize) -> usize {
        let bank_count = rom.len() / 0x8000;
        (bank % bank_count) * 0x8000
    }

    /// Get last bank index for given size
    pub fn last_bank(rom: &[u8], bank_size: usize) -> usize {
        (rom.len() / bank_size).saturating_sub(1)
    }
}

/// CHR bank calculation utilities
pub mod chr_banking {
    /// Calculate 1KB bank offset
    pub fn bank_1k(chr: &[u8], bank: usize) -> usize {
        let bank_count = chr.len() / 0x400;
        if bank_count == 0 { return 0; }
        (bank % bank_count) * 0x400
    }

    /// Calculate 2KB bank offset
    pub fn bank_2k(chr: &[u8], bank: usize) -> usize {
        let bank_count = chr.len() / 0x800;
        if bank_count == 0 { return 0; }
        (bank % bank_count) * 0x800
    }

    /// Calculate 4KB bank offset
    pub fn bank_4k(chr: &[u8], bank: usize) -> usize {
        let bank_count = chr.len() / 0x1000;
        if bank_count == 0 { return 0; }
        (bank % bank_count) * 0x1000
    }

    /// Calculate 8KB bank offset
    pub fn bank_8k(chr: &[u8], bank: usize) -> usize {
        let bank_count = chr.len() / 0x2000;
        if bank_count == 0 { return 0; }
        (bank % bank_count) * 0x2000
    }
}
```

---

## IRQ Handling

### IRQ Counter Types

```rust
/// Scanline-based IRQ counter (MMC3-style)
pub struct ScanlineIrqCounter {
    latch: u8,
    counter: u8,
    reload_pending: bool,
    enabled: bool,
    pending: bool,
}

impl ScanlineIrqCounter {
    pub fn new() -> Self {
        Self {
            latch: 0,
            counter: 0,
            reload_pending: false,
            enabled: false,
            pending: false,
        }
    }

    pub fn write_latch(&mut self, value: u8) {
        self.latch = value;
    }

    pub fn write_reload(&mut self) {
        self.reload_pending = true;
    }

    pub fn write_disable(&mut self) {
        self.enabled = false;
        self.pending = false;
    }

    pub fn write_enable(&mut self) {
        self.enabled = true;
    }

    pub fn clock(&mut self) -> bool {
        if self.reload_pending || self.counter == 0 {
            self.counter = self.latch;
            self.reload_pending = false;
        } else {
            self.counter -= 1;
        }

        if self.counter == 0 && self.enabled {
            self.pending = true;
            return true;
        }
        false
    }

    pub fn pending(&self) -> bool { self.pending }
    pub fn acknowledge(&mut self) { self.pending = false; }
}

/// CPU cycle-based IRQ counter (FME-7 style)
pub struct CycleIrqCounter {
    counter: u16,
    enabled: bool,
    pending: bool,
}

impl CycleIrqCounter {
    pub fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
            if self.counter == 0 {
                self.pending = true;
                return true;
            }
        }
        false
    }
}
```

---

## Save States

### Mapper Serialization

```rust
impl<M: Mapper + MapperState> MapperState for M {
    fn save_state(&self) -> MapperStateData {
        MapperStateData {
            mapper: self.mapper_number(),
            submapper: self.submapper(),
            registers: self.serialize_registers(),
            prg_ram: self.prg_ram().map(|r| r.to_vec()),
            chr_ram: self.chr_ram().map(|r| r.to_vec()),
        }
    }

    fn load_state(&mut self, state: &MapperStateData) {
        if state.mapper != self.mapper_number() {
            panic!("Mapper mismatch in save state");
        }

        self.deserialize_registers(&state.registers);

        if let Some(ref prg_ram) = state.prg_ram {
            self.load_prg_ram(prg_ram);
        }
        if let Some(ref chr_ram) = state.chr_ram {
            self.load_chr_ram(chr_ram);
        }
    }
}
```

---

## Examples

### Custom Mapper Implementation

```rust
/// Example: Simple 32KB bank-switched PRG mapper
pub struct SimpleMapper {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    prg_bank: u8,
    mirroring: Mirroring,
}

impl SimpleMapper {
    pub fn new(cart: Cartridge) -> Self {
        Self {
            prg_rom: cart.prg_rom,
            chr_ram: [0; 8192],
            prg_bank: 0,
            mirroring: cart.mirroring,
        }
    }
}

impl Mapper for SimpleMapper {
    fn read_prg(&self, addr: u16) -> u8 {
        if addr >= 0x8000 {
            let bank_offset = (self.prg_bank as usize) * 0x8000;
            let offset = (addr - 0x8000) as usize;
            self.prg_rom[(bank_offset + offset) % self.prg_rom.len()]
        } else {
            0
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.prg_bank = value;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_ram[addr as usize & 0x1FFF]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        self.chr_ram[addr as usize & 0x1FFF] = value;
    }

    fn mirroring(&self) -> Mirroring { self.mirroring }
    fn mapper_number(&self) -> u16 { 999 } // Example
    fn mapper_name(&self) -> &'static str { "SimpleMapper" }
}

// Register custom mapper
fn main() {
    register_mapper(999, |c| Box::new(SimpleMapper::new(c)));
}
```

---

## References

- [NESdev Wiki: Mapper](https://www.nesdev.org/wiki/Mapper)
- [NESdev Wiki: List of Mappers](https://www.nesdev.org/wiki/List_of_mappers)
- [NESCartDB](https://nescartdb.com/) - Cartridge database

---

**Related Documents:**
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md)
- [MAPPER_IMPLEMENTATION_GUIDE.md](../mappers/MAPPER_IMPLEMENTATION_GUIDE.md)
- [INES_FORMAT.md](../formats/INES_FORMAT.md)
- [NES20_FORMAT.md](../formats/NES20_FORMAT.md)
