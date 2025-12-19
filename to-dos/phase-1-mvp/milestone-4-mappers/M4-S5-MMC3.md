# [Milestone 4] Sprint 4.5: Mapper 4 (MMC3)

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~2 weeks
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 4.4 (UxROM/CNROM) must be complete

---

## Overview

Implement Mapper 4 (MMC3/TxROM), the most complex and widely-used NES mapper. MMC3 is found in over 700 games including Super Mario Bros. 2/3, Mega Man 3-6, all Kirby games, and most late-era NES titles. Features:

- Switchable PRG-ROM banks (8KB and 16KB)
- Switchable CHR-ROM banks (1KB and 2KB)
- Scanline counter with IRQ generation
- Configurable PRG/CHR banking modes
- Optional PRG-RAM with write protection
- Mirroring control (horizontal/vertical)

MMC3 is essential for achieving high game compatibility and validates the entire mapper framework.

---

## Acceptance Criteria

- [ ] Eight bank selection registers (R0-R7)
- [ ] Two configuration registers (Bank Select, Mirroring)
- [ ] PRG banking modes (two 8KB banks + 16KB fixed)
- [ ] CHR banking modes (2KB + 1KB banks)
- [ ] Scanline counter with configurable IRQ
- [ ] IRQ enable/disable/acknowledge
- [ ] Mirroring control (horizontal/vertical)
- [ ] PRG-RAM protection (write enable/disable)
- [ ] Clock on PPU A12 rising edge (0 → 1)
- [ ] Zero unsafe code
- [ ] Unit tests for all features
- [ ] Integration tests with real MMC3 games

---

## Tasks

### 4.5.1 MMC3 Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the Mapper4 struct with all registers, banking state, and IRQ logic.

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - MMC3 implementation

**Subtasks:**

- [ ] Define Mapper4 struct
- [ ] Bank select register (target register + mode bits)
- [ ] Eight bank registers (R0-R7)
- [ ] Mirroring register
- [ ] PRG-RAM protect register
- [ ] IRQ latch, counter, enabled, pending
- [ ] PRG/CHR ROM storage
- [ ] PRG-RAM storage

**Implementation:**

```rust
use crate::mapper::{Mapper, Mirroring};

pub struct Mapper4 {
    // ROM data
    prg_rom: Vec<u8>,
    chr_memory: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_is_ram: bool,

    // Bank select register ($8000-$9FFE, even)
    bank_select: u8,       // Bits 0-2: Bank register to update, Bit 6: PRG mode, Bit 7: CHR mode

    // Bank registers (R0-R7)
    bank_registers: [u8; 8],

    // Mirroring ($A000-$BFFE, even)
    mirroring: Mirroring,

    // PRG-RAM protect ($A001-$BFFF, odd)
    prg_ram_write_enabled: bool,
    prg_ram_chip_enabled: bool,

    // IRQ
    irq_latch: u8,         // IRQ latch value ($C000-$DFFE, even)
    irq_counter: u8,       // Current counter value
    irq_enabled: bool,     // IRQ enabled ($E001-$FFFF, odd)
    irq_pending: bool,     // IRQ triggered
    irq_reload: bool,      // Reload counter next clock

    // PPU A12 tracking (for IRQ counter)
    last_ppu_a12: bool,
}

impl Mapper4 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_memory = if chr_is_ram {
            vec![0; 0x2000]
        } else {
            chr_rom
        };

        Self {
            prg_rom,
            chr_memory,
            prg_ram: vec![0; 0x2000], // 8KB PRG-RAM
            chr_is_ram,
            bank_select: 0,
            bank_registers: [0; 8],
            mirroring,
            prg_ram_write_enabled: true,
            prg_ram_chip_enabled: true,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            irq_reload: false,
            last_ppu_a12: false,
        }
    }
}
```

---

### 4.5.2 Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement the CPU-accessible register interface for bank selection, mirroring, and IRQ control.

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - Register writes

**Subtasks:**

- [ ] $8000-$9FFE (even): Bank select register
- [ ] $8001-$9FFF (odd): Bank data register
- [ ] $A000-$BFFE (even): Mirroring register
- [ ] $A001-$BFFF (odd): PRG-RAM protect register
- [ ] $C000-$DFFE (even): IRQ latch
- [ ] $C001-$DFFF (odd): IRQ reload
- [ ] $E000-$FFFE (even): IRQ disable
- [ ] $E001-$FFFF (odd): IRQ enable

**Implementation:**

```rust
impl Mapper for Mapper4 {
    fn write_prg(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM
                if self.prg_ram_chip_enabled && self.prg_ram_write_enabled {
                    self.prg_ram[(addr - 0x6000) as usize] = val;
                }
            }
            0x8000..=0x9FFF => {
                if (addr & 0x01) == 0 {
                    // $8000-$9FFE (even): Bank select
                    self.bank_select = val;
                } else {
                    // $8001-$9FFF (odd): Bank data
                    let register = (self.bank_select & 0x07) as usize;
                    self.bank_registers[register] = val;
                }
            }
            0xA000..=0xBFFF => {
                if (addr & 0x01) == 0 {
                    // $A000-$BFFE (even): Mirroring
                    self.mirroring = if (val & 0x01) == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                } else {
                    // $A001-$BFFF (odd): PRG-RAM protect
                    self.prg_ram_write_enabled = (val & 0x40) != 0;
                    self.prg_ram_chip_enabled = (val & 0x80) != 0;
                }
            }
            0xC000..=0xDFFF => {
                if (addr & 0x01) == 0 {
                    // $C000-$DFFE (even): IRQ latch
                    self.irq_latch = val;
                } else {
                    // $C001-$DFFF (odd): IRQ reload
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if (addr & 0x01) == 0 {
                    // $E000-$FFFE (even): IRQ disable
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    // $E001-$FFFF (odd): IRQ enable
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }
}
```

---

### 4.5.3 PRG Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement PRG-ROM banking with two modes (swappable 8KB banks at different locations).

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - PRG read logic

**Subtasks:**

- [ ] Mode 0: R6 at $8000, R7 at $A000, second-last at $C000, last at $E000
- [ ] Mode 1: Second-last at $8000, R7 at $A000, R6 at $C000, last at $E000
- [ ] Calculate bank numbers considering PRG-ROM size

**Implementation:**

```rust
impl Mapper for Mapper4 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM
                if self.prg_ram_chip_enabled {
                    self.prg_ram[(addr - 0x6000) as usize]
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                let offset = (addr & 0x1FFF) as usize; // 8KB bank offset
                let prg_addr = (bank * 0x2000 + offset) % self.prg_rom.len();
                self.prg_rom[prg_addr]
            }
            _ => 0,
        }
    }
}

impl Mapper4 {
    fn get_prg_bank(&self, addr: u16) -> usize {
        let num_banks = self.prg_rom.len() / 0x2000; // Number of 8KB banks
        let prg_mode = (self.bank_select & 0x40) != 0;

        let bank = match addr {
            0x8000..=0x9FFF => {
                if prg_mode {
                    num_banks - 2 // Second-last bank
                } else {
                    self.bank_registers[6] as usize
                }
            }
            0xA000..=0xBFFF => self.bank_registers[7] as usize,
            0xC000..=0xDFFF => {
                if prg_mode {
                    self.bank_registers[6] as usize
                } else {
                    num_banks - 2 // Second-last bank
                }
            }
            0xE000..=0xFFFF => num_banks - 1, // Last bank (always fixed)
            _ => 0,
        };

        bank % num_banks
    }
}
```

---

### 4.5.4 CHR Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement CHR-ROM banking with two modes (2KB + 1KB banks at different locations).

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - CHR read/write logic

**Subtasks:**

- [ ] Mode 0: R0/R1 (2KB each) at $0000/$0800, R2/R3/R4/R5 (1KB each) at $1000-$1FFF
- [ ] Mode 1: R0/R1 (2KB each) at $1000/$1800, R2/R3/R4/R5 (1KB each) at $0000-$0FFF
- [ ] Handle CHR-RAM writes

**Implementation:**

```rust
impl Mapper for Mapper4 {
    fn read_chr(&self, addr: u16) -> u8 {
        let bank = self.get_chr_bank(addr);
        let offset = (addr & 0x03FF) as usize; // 1KB bank offset
        let chr_addr = (bank * 0x0400 + offset) % self.chr_memory.len();
        self.chr_memory[chr_addr]
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            let bank = self.get_chr_bank(addr);
            let offset = (addr & 0x03FF) as usize;
            let chr_addr = (bank * 0x0400 + offset) % self.chr_memory.len();
            self.chr_memory[chr_addr] = val;
        }
    }
}

impl Mapper4 {
    fn get_chr_bank(&self, addr: u16) -> usize {
        let num_banks = self.chr_memory.len() / 0x0400; // Number of 1KB banks
        let chr_mode = (self.bank_select & 0x80) != 0;

        let bank = match addr {
            0x0000..=0x03FF => {
                if chr_mode {
                    self.bank_registers[2] as usize
                } else {
                    (self.bank_registers[0] & 0xFE) as usize // 2KB bank (ignore low bit)
                }
            }
            0x0400..=0x07FF => {
                if chr_mode {
                    self.bank_registers[3] as usize
                } else {
                    (self.bank_registers[0] | 0x01) as usize
                }
            }
            0x0800..=0x0BFF => {
                if chr_mode {
                    self.bank_registers[4] as usize
                } else {
                    (self.bank_registers[1] & 0xFE) as usize
                }
            }
            0x0C00..=0x0FFF => {
                if chr_mode {
                    self.bank_registers[5] as usize
                } else {
                    (self.bank_registers[1] | 0x01) as usize
                }
            }
            0x1000..=0x13FF => {
                if chr_mode {
                    (self.bank_registers[0] & 0xFE) as usize
                } else {
                    self.bank_registers[2] as usize
                }
            }
            0x1400..=0x17FF => {
                if chr_mode {
                    (self.bank_registers[0] | 0x01) as usize
                } else {
                    self.bank_registers[3] as usize
                }
            }
            0x1800..=0x1BFF => {
                if chr_mode {
                    (self.bank_registers[1] & 0xFE) as usize
                } else {
                    self.bank_registers[4] as usize
                }
            }
            0x1C00..=0x1FFF => {
                if chr_mode {
                    (self.bank_registers[1] | 0x01) as usize
                } else {
                    self.bank_registers[5] as usize
                }
            }
            _ => 0,
        };

        bank % num_banks
    }
}
```

---

### 4.5.5 Scanline Counter & IRQ

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 4 hours

**Description:**
Implement the scanline counter that generates IRQs for raster effects (status bars, parallax scrolling).

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - IRQ logic

**Subtasks:**

- [ ] Track PPU A12 rising edges (0 → 1)
- [ ] Clock counter on rising edge
- [ ] Reload counter when it reaches 0 or reload flag set
- [ ] Set IRQ pending when counter reaches 0
- [ ] IRQ enable/disable logic
- [ ] IRQ acknowledge (clear pending)

**Implementation:**

```rust
impl Mapper for Mapper4 {
    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock(&mut self, _cycles: u8) {
        // Called by PPU when rendering
        // (This is a simplified interface; actual implementation needs PPU integration)
    }
}

impl Mapper4 {
    /// Clock the IRQ counter on PPU A12 rising edge
    pub fn clock_irq(&mut self, ppu_a12: bool) {
        // Detect rising edge of A12 (background pattern table switch: $0000 → $1000)
        let rising_edge = !self.last_ppu_a12 && ppu_a12;
        self.last_ppu_a12 = ppu_a12;

        if !rising_edge {
            return;
        }

        // Clock the counter
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        // Trigger IRQ when counter reaches 0 and IRQs enabled
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}
```

---

### 4.5.6 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 4 hours

**Description:**
Create comprehensive unit tests for all MMC3 features.

**Files:**

- `crates/rustynes-mappers/src/mapper4.rs` - Test module

**Subtasks:**

- [ ] Test bank selection register
- [ ] Test PRG banking modes (0 and 1)
- [ ] Test CHR banking modes (0 and 1)
- [ ] Test mirroring control
- [ ] Test PRG-RAM protection
- [ ] Test IRQ counter and latch
- [ ] Test IRQ enable/disable

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prg_banking_mode_0() {
        let mut prg_rom = vec![0; 0x40000]; // 256KB = 32 banks
        for i in 0..32 {
            prg_rom[i * 0x2000] = i as u8;
        }
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Mode 0: R6 at $8000, R7 at $A000
        mapper.bank_select = 0x00; // Mode 0
        mapper.bank_registers[6] = 5;
        mapper.bank_registers[7] = 10;

        assert_eq!(mapper.read_prg(0x8000), 5);  // R6
        assert_eq!(mapper.read_prg(0xA000), 10); // R7
        assert_eq!(mapper.read_prg(0xC000), 30); // Second-last
        assert_eq!(mapper.read_prg(0xE000), 31); // Last
    }

    #[test]
    fn test_prg_banking_mode_1() {
        let mut prg_rom = vec![0; 0x40000]; // 256KB = 32 banks
        for i in 0..32 {
            prg_rom[i * 0x2000] = i as u8;
        }
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Mode 1: Second-last at $8000, R7 at $A000, R6 at $C000
        mapper.bank_select = 0x40; // Mode 1
        mapper.bank_registers[6] = 5;
        mapper.bank_registers[7] = 10;

        assert_eq!(mapper.read_prg(0x8000), 30); // Second-last
        assert_eq!(mapper.read_prg(0xA000), 10); // R7
        assert_eq!(mapper.read_prg(0xC000), 5);  // R6
        assert_eq!(mapper.read_prg(0xE000), 31); // Last
    }

    #[test]
    fn test_chr_banking_mode_0() {
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x8000]; // 32KB = 32 banks
        for i in 0..32 {
            chr_rom[i * 0x0400] = i as u8;
        }
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Mode 0: R0/R1 (2KB) at $0000/$0800, R2-R5 (1KB) at $1000-$1FFF
        mapper.bank_select = 0x00;
        mapper.bank_registers[0] = 4;  // 2KB bank at $0000
        mapper.bank_registers[1] = 8;  // 2KB bank at $0800
        mapper.bank_registers[2] = 12;
        mapper.bank_registers[3] = 13;
        mapper.bank_registers[4] = 14;
        mapper.bank_registers[5] = 15;

        assert_eq!(mapper.read_chr(0x0000), 4);  // R0 (2KB)
        assert_eq!(mapper.read_chr(0x0800), 8);  // R1 (2KB)
        assert_eq!(mapper.read_chr(0x1000), 12); // R2
        assert_eq!(mapper.read_chr(0x1400), 13); // R3
        assert_eq!(mapper.read_chr(0x1800), 14); // R4
        assert_eq!(mapper.read_chr(0x1C00), 15); // R5
    }

    #[test]
    fn test_chr_banking_mode_1() {
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x8000];
        for i in 0..32 {
            chr_rom[i * 0x0400] = i as u8;
        }
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Mode 1: R2-R5 at $0000-$0FFF, R0/R1 at $1000/$1800
        mapper.bank_select = 0x80;
        mapper.bank_registers[0] = 4;
        mapper.bank_registers[1] = 8;
        mapper.bank_registers[2] = 12;
        mapper.bank_registers[3] = 13;
        mapper.bank_registers[4] = 14;
        mapper.bank_registers[5] = 15;

        assert_eq!(mapper.read_chr(0x0000), 12); // R2
        assert_eq!(mapper.read_chr(0x0400), 13); // R3
        assert_eq!(mapper.read_chr(0x0800), 14); // R4
        assert_eq!(mapper.read_chr(0x0C00), 15); // R5
        assert_eq!(mapper.read_chr(0x1000), 4);  // R0 (2KB)
        assert_eq!(mapper.read_chr(0x1800), 8);  // R1 (2KB)
    }

    #[test]
    fn test_mirroring_control() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Write 0 = vertical
        mapper.write_prg(0xA000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        // Write 1 = horizontal
        mapper.write_prg(0xA000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn test_prg_ram_protection() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Initially writable
        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        // Disable writes (bit 6 = 0)
        mapper.write_prg(0xA001, 0x00);
        mapper.write_prg(0x6000, 0xFF);
        assert_eq!(mapper.read_prg(0x6000), 0x42); // Not changed

        // Disable chip (bit 7 = 0)
        mapper.write_prg(0xA001, 0x40); // Write enabled but chip disabled
        assert_eq!(mapper.read_prg(0x6000), 0); // Open bus
    }

    #[test]
    fn test_irq_counter() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set IRQ latch to 3
        mapper.write_prg(0xC000, 3);

        // Reload counter
        mapper.write_prg(0xC001, 0);
        assert_eq!(mapper.irq_counter, 0);
        assert!(mapper.irq_reload);

        // Enable IRQ
        mapper.write_prg(0xE001, 0);

        // Clock counter (via A12 rising edges)
        mapper.clock_irq(false);
        mapper.clock_irq(true); // Rising edge: reload
        assert_eq!(mapper.irq_counter, 3);
        assert!(!mapper.irq_pending);

        // Clock 3 more times
        for _ in 0..3 {
            mapper.clock_irq(false);
            mapper.clock_irq(true);
        }

        // Counter should be 0 and IRQ pending
        assert_eq!(mapper.irq_counter, 0);
        assert!(mapper.irq_pending);
    }

    #[test]
    fn test_irq_disable() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper4::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set up IRQ
        mapper.write_prg(0xC000, 0); // Latch = 0
        mapper.write_prg(0xC001, 0); // Reload
        mapper.write_prg(0xE001, 0); // Enable

        // Clock to trigger IRQ
        mapper.clock_irq(false);
        mapper.clock_irq(true);
        assert!(mapper.irq_pending);

        // Disable IRQ
        mapper.write_prg(0xE000, 0);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending); // Cleared
    }
}
```

---

## Dependencies

**Required:**

- Sprint 4.4 complete (UxROM/CNROM implementation)

**Blocks:**

- Milestone 5: Full game compatibility testing

---

## Related Documentation

- [Mapper Overview](../../../docs/mappers/MAPPER_OVERVIEW.md)
- [MMC3 Specification](../../../docs/mappers/MAPPER_MMC3.md)
- [NESdev Wiki - MMC3](https://www.nesdev.org/wiki/MMC3)
- [MMC3 IRQ Timing](https://www.nesdev.org/wiki/MMC3_IRQ_timing)

---

## Technical Notes

### Bank Register Map

| Register | PRG Mode 0 | PRG Mode 1 | CHR Mode 0 | CHR Mode 1 |
|----------|-----------|-----------|-----------|-----------|
| R0 | - | - | $0000-$07FF (2KB) | $1000-$17FF (2KB) |
| R1 | - | - | $0800-$0FFF (2KB) | $1800-$1FFF (2KB) |
| R2 | - | - | $1000-$13FF (1KB) | $0000-$03FF (1KB) |
| R3 | - | - | $1400-$17FF (1KB) | $0400-$07FF (1KB) |
| R4 | - | - | $1800-$1BFF (1KB) | $0800-$0BFF (1KB) |
| R5 | - | - | $1C00-$1FFF (1KB) | $0C00-$0FFF (1KB) |
| R6 | $8000-$9FFF | $C000-$DFFF | - | - |
| R7 | $A000-$BFFF | $A000-$BFFF | - | - |

### PRG Banking

**Mode 0:**
```
$8000: R6 (8KB)
$A000: R7 (8KB)
$C000: Second-last 8KB bank (fixed)
$E000: Last 8KB bank (fixed)
```

**Mode 1:**
```
$8000: Second-last 8KB bank (fixed)
$A000: R7 (8KB)
$C000: R6 (8KB)
$E000: Last 8KB bank (fixed)
```

### CHR Banking

**Mode 0:** Two 2KB banks at $0000-$0FFF, four 1KB banks at $1000-$1FFF

**Mode 1:** Four 1KB banks at $0000-$0FFF, two 2KB banks at $1000-$1FFF

### IRQ Counter

- Clocked on PPU A12 rising edge (0 → 1)
- A12 = 1 when accessing $1000-$1FFF (background pattern table 1)
- Games trigger this by setting up background scrolling
- Counter reloads to latch value when it reaches 0
- IRQ triggered when counter transitions to 0 (if enabled)

### Scanline Detection

The MMC3 IRQ counter is designed to count scanlines:
- PPU fetches background tiles from alternating pattern tables
- Each scanline causes multiple A12 toggles
- Counter decrements once per scanline (with proper filtering)
- Used for split-screen effects (status bars), parallax scrolling

### MMC3 Variants

- **TLSROM**: 512KB PRG-ROM
- **TKSROM**: Battery-backed 8KB PRG-RAM
- **TQROM**: 8KB CHR-RAM + 8KB CHR-ROM
- **HKROM**: 512KB PRG-ROM + battery RAM

---

## Test Requirements

- [ ] Unit tests for PRG banking modes
- [ ] Unit tests for CHR banking modes
- [ ] Unit tests for mirroring control
- [ ] Unit tests for PRG-RAM protection
- [ ] Unit tests for IRQ counter
- [ ] Unit tests for IRQ latch reload
- [ ] Unit tests for IRQ enable/disable
- [ ] Integration test with Super Mario Bros. 3
- [ ] Integration test with Mega Man 3-6
- [ ] Integration test with Kirby's Adventure

---

## Performance Targets

- Bank calculation: <15 ns
- Register write: <20 ns
- IRQ clock: <30 ns
- PRG read: <10 ns
- CHR read: <10 ns
- Memory: <200KB overhead

---

## Success Criteria

- [ ] All 8 bank registers work
- [ ] Both PRG banking modes function
- [ ] Both CHR banking modes function
- [ ] Mirroring control works
- [ ] PRG-RAM protection works
- [ ] IRQ counter clocks correctly
- [ ] IRQ triggers on scanlines
- [ ] IRQ enable/disable works
- [ ] All unit tests pass
- [ ] Integration tests pass (SMB3, Mega Man, Kirby)
- [ ] Zero unsafe code
- [ ] Documentation complete

---

## Known MMC3 Games

| Game | PRG Size | CHR Size | Special Features |
|------|----------|----------|------------------|
| Super Mario Bros. 3 | 256KB | 128KB | IRQ for status bar |
| Mega Man 3 | 256KB | 128KB | IRQ for stage select |
| Mega Man 4 | 256KB | 256KB | IRQ for stage select |
| Mega Man 5 | 512KB | 256KB | IRQ for stage select |
| Mega Man 6 | 512KB | 256KB | IRQ for stage select |
| Kirby's Adventure | 512KB | 256KB | Extensive IRQ usage |
| Super Mario Bros. 2 | 128KB | 128KB | - |
| Final Fantasy III | 512KB | 128KB | Battery PRG-RAM |
| StarTropics | 256KB | 128KB | IRQ effects |

---

## Implementation Challenges

### IRQ Timing

- MMC3 IRQ timing is complex and poorly documented
- Counter clocks on A12 rising edges, not scanlines directly
- Games may trigger spurious IRQs due to sprite fetches
- Emulators need careful A12 edge detection

### A12 Edge Detection

- Must track PPU address bus
- Filter rapid toggles (within 1-2 cycles)
- Some accuracy tests require cycle-perfect timing

### Split-Screen Effects

- Super Mario Bros. 3 uses IRQ for status bar
- Kirby's Adventure uses IRQ extensively for parallax
- Requires precise IRQ timing for correct rendering

---

**Previous Sprint:** [Sprint 4.4: Mappers 2 & 3 (UxROM, CNROM)](M4-S4-UXROM-CNROM.md)
**Next Sprint:** [Milestone 5: Full Game Compatibility](../milestone-5-compatibility/M5-OVERVIEW.md)
