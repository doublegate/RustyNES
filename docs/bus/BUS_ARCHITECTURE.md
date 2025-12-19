# NES Bus Architecture

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete reference for NES memory bus architecture, DMA, and bus conflicts

---

## Table of Contents

- [Overview](#overview)
- [CPU Bus Architecture](#cpu-bus-architecture)
- [PPU Bus Architecture](#ppu-bus-architecture)
- [CPU Memory Map](#cpu-memory-map)
- [PPU Memory Map](#ppu-memory-map)
- [DMA Operations](#dma-operations)
- [Open Bus Behavior](#open-bus-behavior)
- [Bus Conflicts](#bus-conflicts)
- [Implementation Guide](#implementation-guide)
- [Common Pitfalls](#common-pitfalls)

---

## Overview

The NES uses **two independent bus systems**:

1. **CPU Bus** (16-bit address space): Connects 6502 CPU, RAM, PPU registers, APU registers, cartridge
2. **PPU Bus** (14-bit address space): Connects 2C02 PPU, VRAM, pattern tables, cartridge CHR

These buses operate **independently** but are **coordinated** through memory-mapped PPU registers ($2000-$2007) on the CPU bus.

**Key Concepts:**

- Address mirroring (RAM, PPU registers)
- Open bus behavior (unconnected addresses)
- Bus conflicts (simultaneous reads/writes)
- DMA operations (CPU halt for memory transfer)
- Cartridge observation (mappers see all bus activity)

---

## CPU Bus Architecture

### Bus Signals

```
CPU Bus (40-pin 6502):
  A0-A15:  16-bit address bus (64 KB address space)
  D0-D7:   8-bit data bus (bidirectional)
  R/W:     Read/Write signal (1=read, 0=write)
  M2:      Master clock ÷12 (1.789773 MHz)
  /NMI:    Non-maskable interrupt input (from PPU)
  /IRQ:    Maskable interrupt input (from APU/cartridge)
  RDY:     Ready input (for DMA halt)
```

### Bus Timing

**Read Cycle:**

```
Clock 1:  Address output on A0-A15
          R/W = 1
          Data becomes available on D0-D7
Clock 2:  CPU samples data
```

**Write Cycle:**

```
Clock 1:  Address output on A0-A15
          R/W = 0
          Data output on D0-D7
Clock 2:  External devices latch data
```

### Address Decoding

The NES Control Deck uses a **74LS139 decoder (U3)** to divide CPU address space:

| Address Range | Decoded Region | Enable Signal |
|---------------|----------------|---------------|
| $0000-$1FFF | Internal RAM | A15=0, A14-A13=00 |
| $2000-$3FFF | PPU Registers | A15=0, A14-A13=01 |
| $4000-$5FFF | APU/I/O | A15=0, A14-A13=10 |
| $6000-$7FFF | Cartridge RAM | A15=0, A14-A13=11 |
| $8000-$FFFF | Cartridge ROM | A15=1 |

**Cartridge Observation:** Cartridges can **passively monitor** all bus activity (except reads from $4015, which is internal to CPU).

---

## PPU Bus Architecture

### Bus Signals

```
PPU Bus:
  A0-A13:  14-bit address bus (16 KB address space)
  AD0-AD7: 8-bit address/data multiplexed bus
  /RD:     Read enable
  /WR:     Write enable
  ALE:     Address latch enable
```

### Multiplexed Address/Data

The PPU uses **multiplexed address/data pins** (AD0-AD7):

```
Phase 1 (ALE high): AD0-AD7 = A0-A7 (low address byte)
Phase 2 (ALE low):  AD0-AD7 = data
```

**Latching:** External latch (74LS373) captures low address byte when ALE goes low.

### Independent Operation

**Critical:** CPU bus and PPU bus operate **completely independently**. CPU accesses PPU registers ($2000-$2007), which **internally** trigger PPU bus operations.

**Example: VRAM Read**

```
CPU writes $20 to $2006  → PPU latches address high byte
CPU writes $00 to $2006  → PPU latches address low byte
CPU reads $2007          → PPU reads from $2000 (internal bus)
```

---

## CPU Memory Map

### Complete Address Space

```
$0000-$07FF   Internal RAM (2 KB)
$0800-$0FFF   │
$1000-$17FF   │ Mirrors of $0000-$07FF (4× total)
$1800-$1FFF   │

$2000-$2007   PPU Registers
$2008-$3FFF   Mirrors of $2000-$2007 (repeat every 8 bytes)

$4000-$4015   APU Registers
$4016-$4017   Controller Ports
$4018-$401F   APU/IO Test Mode (normally disabled)

$4020-$5FFF   Cartridge Expansion (varies by mapper)
$6000-$7FFF   Cartridge SRAM (8 KB typical, battery-backed)
$8000-$FFFF   Cartridge PRG-ROM (32 KB typical)
```

### RAM Organization

**Zero Page ($0000-$00FF):**

- Fast access (2 cycles vs 4 for absolute addressing)
- Used for variables, pointers, temporary storage

**Stack ($0100-$01FF):**

- Hardware stack (SP register = $01xx)
- Grows downward from $01FF
- Used by JSR, RTS, interrupts, PHA/PLA

**General RAM ($0200-$07FF):**

- $0200-$02FF: OAM buffer (sprite data for DMA)
- $0300-$07FF: Variables, buffers, game data

### RAM Mirroring

Internal RAM mirrors **4 times** across $0000-$1FFF:

```
$0000-$07FF = Physical RAM
$0800-$0FFF = Mirror 1 (same as $0000-$07FF)
$1000-$17FF = Mirror 2
$1800-$1FFF = Mirror 3
```

**Address Calculation:**

```rust
fn map_ram_address(addr: u16) -> u16 {
    addr & 0x07FF  // Mask to 2 KB
}
```

### PPU Register Mirroring

PPU registers ($2000-$2007, 8 bytes) mirror **every 8 bytes** through $3FFF:

```
$2000-$2007 = Physical registers
$2008-$200F = Mirror
$2010-$2017 = Mirror
...
$3FF8-$3FFF = Mirror
```

**Address Calculation:**

```rust
fn map_ppu_register(addr: u16) -> u16 {
    0x2000 | (addr & 0x07)  // Mask to 8 registers
}
```

### Cartridge Space

**SRAM ($6000-$7FFF):**

- Battery-backed save RAM
- 8 KB typical (some mappers support more)
- Read/write access

**PRG-ROM ($8000-$FFFF):**

- Program code and data
- 32 KB typical (banked by mappers)
- Read-only (bus conflict if writing)

### Interrupt Vectors

**Fixed vectors at top of memory:**

| Address | Vector | Description |
|---------|--------|-------------|
| $FFFA-$FFFB | NMI | Non-maskable interrupt (VBlank) |
| $FFFC-$FFFD | RESET | Reset/power-on entry point |
| $FFFE-$FFFF | IRQ/BRK | Maskable interrupt |

**Mapper Consideration:** These vectors must be accessible in all PRG banks, or mapper must fix $FFFA-$FFFF to a known bank.

---

## PPU Memory Map

### Complete Address Space

```
$0000-$0FFF   Pattern Table 0 (CHR, 4 KB)
$1000-$1FFF   Pattern Table 1 (CHR, 4 KB)

$2000-$23FF   Nametable 0
$2400-$27FF   Nametable 1
$2800-$2BFF   Nametable 2
$2C00-$2FFF   Nametable 3

$3000-$3EFF   Mirrors of $2000-$2EFF

$3F00-$3F1F   Palette RAM (32 bytes)
$3F20-$3FFF   Mirrors of $3F00-$3F1F
```

### CHR Memory

**CHR-ROM:** Read-only pattern tables (cartridge)
**CHR-RAM:** Writable pattern tables (on cartridge)

Accessed by PPU during rendering for tile graphics.

### Nametables

**Physical VRAM:** 2 KB in NES (enough for 2 nametables)

**Mirroring Modes:**

- **Horizontal:** NT0=NT1, NT2=NT3 (vertical scrolling)
- **Vertical:** NT0=NT2, NT1=NT3 (horizontal scrolling)
- **Single-screen:** All nametables map to one (fixed screen)
- **Four-screen:** 4 KB VRAM on cartridge (no mirroring)

### Palette RAM

**32 bytes of palette memory:**

```
$3F00-$3F0F: Background palettes (4 palettes × 4 colors)
$3F10-$3F1F: Sprite palettes (4 palettes × 4 colors)
```

**Mirroring Quirk:** $3F10, $3F14, $3F18, $3F1C mirror to $3F00, $3F04, $3F08, $3F0C (backdrop color is shared).

---

## DMA Operations

The NES has **two DMA units**:

1. **OAM DMA:** Copy 256 bytes to sprite memory ($4014)
2. **DMC DMA:** Read sample bytes for audio playback

### CPU Halting Mechanism

DMA halts the CPU using the **RDY pin**:

```
RDY asserted (high):   CPU runs normally
RDY deasserted (low):  CPU halts (repeats last read cycle)
```

**Behavior:** When halted, CPU **repeats the last read cycle** indefinitely, making no forward progress.

### Get/Put Cycles

The 2A03 alternates between **get** (read) and **put** (write) cycles:

```
APU Cycle:  │    1    │    2    │    3    │    4    │
CPU Cycle:  │ 1  │ 2  │ 3  │ 4  │ 5  │ 6  │ 7  │ 8  │
Phase:      │get│put │get│put │get│put │get│put │
```

**At Power-On:** Random whether first CPU cycle is get or put.

### OAM DMA ($4014)

**Trigger:** Write page number to $4014

```rust
cpu.write(0x4014, 0x02);  // Copy $0200-$02FF to OAM
```

**Process:**

```
1. Halt CPU on next cycle
2. Alignment cycle (if next cycle is put)
3. Get/put 256 times:
   - Get: Read from $page00 + offset
   - Put: Write to $2004 (PPU OAM)
```

**Total Cycles:**

- Even CPU cycle at write: 513 cycles (no alignment)
- Odd CPU cycle at write: 514 cycles (1 alignment)

### OAM DMA Timing Diagram

```
Cycle   Action
─────   ──────
   0    CPU writes $4014
   1    Halt cycle (CPU stopped)
   2    Alignment cycle (if needed)
   3    Get byte 0 from $page00
   4    Put byte 0 to $2004
   5    Get byte 1 from $page01
   6    Put byte 1 to $2004
...
 512    Get byte 255 from $pageFF
 513    Put byte 255 to $2004
 514    CPU resumes
```

### DMC DMA

**Trigger:** DMC audio channel sample buffer empty

**Process:**

```
1. Halt CPU
2. Dummy cycle
3. Optional alignment cycle
4. Read sample byte from memory
5. Resume CPU
```

**Total Cycles:** 3-4 cycles typical

**Address Range:** $8000-$FFFF (with wrap from $FFFF → $8000)

### DMA Conflicts

**OAM DMA + DMC DMA:**

```
If DMC sample fetch occurs during OAM DMA:
  - DMC gets priority
  - Adds 1-3 cycles to total time (typical: 2)
```

**Total Time:** 515-516 cycles (instead of 513-514)

### Repeated Read Side Effects

**Critical Issue:** While CPU is halted, it **repeats the read cycle**, which can trigger side effects:

**Affected Registers:**

- $4016/$4017 (Controllers): Extra clock pulses
- $2002 (PPU Status): Clears VBlank flag multiple times
- $2007 (PPU Data): Increments address

**Hardware Difference:**

- **2A03 (NTSC):** Repeated reads are externally visible
- **2A07 (PAL):** Reportedly fixes this issue

**Workaround:** Avoid halting CPU while reading these registers.

---

## Open Bus Behavior

### CPU Open Bus

**Definition:** Reading from unmapped memory returns the **last value on the data bus**.

**Mechanism:**

```rust
let last_value = cpu.last_bus_value;  // Decays over time

match address {
    0x0000..=0x1FFF => ram[address & 0x7FF],
    0x5000..=0x5FFF => last_value,  // Unmapped → open bus
    _ => // ...
}
```

**Typical Values:**

- Last instruction's operand high byte
- Last read data value
- 0xFF (if bus has decayed)

### PPU Open Bus

The PPU has **two separate buses** with different open bus behavior:

**I/O Bus (CPU side):**

```
Reading $2002: Bits 7-5 = status, bits 4-0 = open bus
Reading $2004: Bits 7-0 = OAM data (or open bus during rendering)
Reading $2007: Bits 7-0 = buffered VRAM data
```

**Video Bus (PPU side):**

```
Reading from unmapped VRAM: Returns low byte of address
Example: Read from $1234 (unmapped) → returns $34
```

### Open Bus Decay

Open bus values **decay over time** (capacitive discharge):

```
Immediate:  Returns last value accurately
~1ms:       Value begins to decay
~10ms:      Typically decayed to $00 or $FF
```

**Games rarely rely on decay** - most check open bus immediately after setting it.

---

## Bus Conflicts

### What is a Bus Conflict?

A **bus conflict** occurs when the CPU writes to ROM while ROM is outputting a different value:

```
CPU wants to write: $42
ROM is outputting:  $FF
Result on bus:      $42 AND $FF = $42 (in this case, works)

CPU wants to write: $FF
ROM is outputting:  $42
Result on bus:      $FF AND $FF = $42 (WRONG!)
```

**Hardware Behavior:** Data bus lines are **wired-AND**, so 0 bits "win" over 1 bits.

### Conflict-Prone Scenarios

**Mapper Register Writes:**

```rust
// WRONG: Writing to ROM without ensuring data matches
cpu.write(0x8000, 0x01);  // Select bank 1

// CORRECT: Write to address containing desired value
let bank = 0x01;
cpu.write(0x8000 | (bank as u16), bank);  // ROM contains $01 at $8001
```

**Common Mappers with Conflicts:**

- **Mapper 0 (NROM):** No writable registers (no conflict)
- **Mapper 2 (UxROM):** Conflicts if write value ≠ ROM value
- **Mapper 3 (CNROM):** Conflicts if write value ≠ ROM value
- **Mapper 7 (AxROM):** Conflicts if write value ≠ ROM value

### Avoiding Bus Conflicts

**Strategy 1: Match ROM Data**

```rust
// Ensure ROM at write address contains the value being written
rom[0x8000] = 0x00;
rom[0x8001] = 0x01;
rom[0x8002] = 0x02;
// ...

cpu.write(0x8000 | (bank as u16), bank);
```

**Strategy 2: Use Writable RAM**

```rust
// Some mappers provide writable registers at $6000-$7FFF
cpu.write(0x6000, bank);  // No conflict (RAM, not ROM)
```

**Strategy 3: Ignore Conflicts**

```rust
// If ROM data matches write data, conflict is harmless
// Games often write same value to multiple addresses
```

---

## Implementation Guide

### CPU Bus Structure

```rust
pub struct CpuBus {
    ram: [u8; 0x800],        // 2 KB internal RAM
    ppu: Ppu,                 // PPU (for register access)
    apu: Apu,                 // APU (for register access)
    cartridge: Box<dyn Mapper>,
    controllers: [Controller; 2],

    last_bus_value: u8,       // For open bus behavior
}

impl CpuBus {
    pub fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            0x0000..=0x1FFF => {
                // Internal RAM (mirrored)
                self.ram[(addr & 0x07FF) as usize]
            }
            0x2000..=0x3FFF => {
                // PPU registers (mirrored every 8 bytes)
                self.ppu.read_register(0x2000 | (addr & 0x07))
            }
            0x4000..=0x4015 => {
                // APU registers
                self.apu.read_register(addr)
            }
            0x4016 => {
                // Controller 1
                self.controllers[0].read()
            }
            0x4017 => {
                // Controller 2
                self.controllers[1].read()
            }
            0x4018..=0x401F => {
                // APU test mode (disabled)
                self.last_bus_value  // Open bus
            }
            0x4020..=0xFFFF => {
                // Cartridge space
                self.cartridge.read_prg(addr)
            }
        };

        self.last_bus_value = value;
        value
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.last_bus_value = value;

        match addr {
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize] = value;
            }
            0x2000..=0x3FFF => {
                self.ppu.write_register(0x2000 | (addr & 0x07), value);
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                self.apu.write_register(addr, value);
            }
            0x4014 => {
                // OAM DMA
                self.oam_dma(value);
            }
            0x4016 => {
                self.controllers[0].write(value);
                self.controllers[1].write(value);
            }
            0x4020..=0xFFFF => {
                self.cartridge.write_prg(addr, value);
            }
            _ => {}
        }
    }

    fn oam_dma(&mut self, page: u8) {
        let base_addr = (page as u16) << 8;

        for offset in 0..256 {
            let addr = base_addr | offset;
            let value = self.read(addr);
            self.ppu.write_register(0x2004, value);
        }

        // Note: Actual implementation should stall CPU for 513-514 cycles
    }
}
```

### DMA Cycle Counting

```rust
pub struct Cpu {
    // ... other fields

    dma_cycles_remaining: u16,
    dmc_dma_pending: bool,
}

impl Cpu {
    pub fn trigger_oam_dma(&mut self, page: u8) {
        // Check alignment (even/odd cycle)
        let alignment = if self.cycles % 2 == 0 { 0 } else { 1 };

        // 1 halt + alignment + 256×2 get/put
        self.dma_cycles_remaining = 1 + alignment + 512;

        // Store page for later processing
        self.oam_dma_page = page;
    }

    pub fn step(&mut self, bus: &mut CpuBus) -> u8 {
        // Handle DMA
        if self.dma_cycles_remaining > 0 {
            self.dma_cycles_remaining -= 1;

            // Process OAM DMA after initial cycles
            if self.dma_cycles_remaining % 2 == 0 {
                let offset = (512 - self.dma_cycles_remaining) / 2;
                let addr = ((self.oam_dma_page as u16) << 8) | offset;
                let value = bus.read(addr);
                bus.ppu.write_register(0x2004, value);
            }

            return 1;  // 1 cycle consumed
        }

        // Normal instruction execution
        // ...
    }
}
```

---

## Common Pitfalls

### 1. Incorrect RAM Mirroring

**Problem:** Not masking RAM addresses, causing out-of-bounds access.

**Solution:** Always mask to 2 KB.

```rust
// WRONG
let value = self.ram[addr as usize];

// CORRECT
let value = self.ram[(addr & 0x07FF) as usize];
```

### 2. PPU Register Mirroring

**Problem:** Not recognizing that $2000-$2007 mirrors through $3FFF.

**Solution:** Mask to 8 bytes.

```rust
// CORRECT
let register = 0x2000 | (addr & 0x07);
```

### 3. Ignoring DMA Stall Cycles

**Problem:** Not accounting for CPU stall during OAM DMA.

**Solution:** Add 513-514 cycles to timing.

```rust
cpu_cycles += 513;  // Or 514 if odd-aligned
```

### 4. Bus Conflicts

**Problem:** Writing to ROM without matching data values.

**Solution:** Ensure ROM contains written values at write addresses.

### 5. Open Bus on $2002 Read

**Problem:** Expecting specific values from open bus bits.

**Solution:** Mask to relevant bits.

```rust
let status = ppu.read(0x2002);
let vblank = (status & 0x80) != 0;  // Only bit 7 is valid
```

---

## Testing and Validation

### Test ROMs

| ROM | Tests | Pass Criteria |
|-----|-------|---------------|
| **cpu_dummy_reads** | DMA repeated read behavior | Correct side effects |
| **oam_dma_start** | OAM DMA timing | 513 or 514 cycles |
| **oam_dma_timing** | DMA cycle alignment | Precise timing |
| **dmc_dma_during_read** | DMC DMA conflicts | Proper handling |

---

## Related Documentation

- [MEMORY_MAP.md](MEMORY_MAP.md) - Detailed memory layout
- [CPU_6502_SPECIFICATION.md](../cpu/CPU_6502_SPECIFICATION.md) - CPU instruction reference
- [PPU_2C02_SPECIFICATION.md](../ppu/PPU_2C02_SPECIFICATION.md) - PPU details
- [APU_CHANNEL_DMC.md](../apu/APU_CHANNEL_DMC.md) - DMC DMA specifics

---

## References

- [NESdev Wiki: CPU Memory Map](https://www.nesdev.org/wiki/CPU_memory_map)
- [NESdev Wiki: PPU Memory Map](https://www.nesdev.org/wiki/PPU_memory_map)
- [NESdev Wiki: DMA](https://www.nesdev.org/wiki/DMA)
- [NESdev Wiki: Open Bus Behavior](https://www.nesdev.org/wiki/Open_bus_behavior)

---

**Document Status:** Complete specification for bus architecture with DMA timing and conflict handling.
