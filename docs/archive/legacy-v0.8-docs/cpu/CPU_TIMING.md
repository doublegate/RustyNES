# CPU Timing and Cycle-Accurate Execution

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Clock Specifications](#clock-specifications)
- [Instruction Cycle Breakdown](#instruction-cycle-breakdown)
- [Page Crossing Penalties](#page-crossing-penalties)
- [Dummy Reads and Writes](#dummy-reads-and-writes)
- [DMA Timing](#dma-timing)
- [Interrupt Timing](#interrupt-timing)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The Ricoh 2A03 CPU executes instructions with **cycle-accurate timing** that must be emulated precisely for correct NES behavior. Many games rely on exact cycle counts for:

- **Sprite multiplexing** - Changing PPU registers mid-frame
- **Raster effects** - Split-screen scrolling via mapper IRQs
- **Audio synchronization** - DMC sample timing
- **Input polling** - Controller reads synchronized with VBlank

This document provides complete timing specifications for implementing a cycle-accurate 6502 core.

---

## Clock Specifications

### Master Clock Hierarchy

The NES operates on a master clock that divides down to component clocks:

```
Master Clock (NTSC): 21.477272 MHz
├─ CPU Clock: ÷12 = 1.789773 MHz (~559 ns/cycle)
├─ PPU Clock: ÷4  = 5.369318 MHz (~186 ns/dot)
└─ APU Clock: Same as CPU (1.789773 MHz)

Ratio: 3 PPU dots per 1 CPU cycle (exact, no drift)
```

**PAL Variant:**

```
Master Clock (PAL): 26.601712 MHz
├─ CPU Clock: ÷16 = 1.662607 MHz (~601 ns/cycle)
├─ PPU Clock: ÷5  = 5.320342 MHz (~188 ns/dot)

Ratio: 3.2 PPU dots per CPU cycle (requires fractional tracking)
```

### Frame Timing (NTSC)

```
CPU cycles per frame:   29,780.5 cycles
  - Even frames:        29,780 cycles
  - Odd frames:         29,781 cycles (extra cycle from PPU dot skip)

PPU dots per frame:     89,341 dots (odd frames)
                        89,342 dots (even frames)

Frame rate:             60.0988 Hz (not exactly 60 Hz)
```

---

## Instruction Cycle Breakdown

### Basic Cycle Costs

Instructions take 2-7 cycles depending on addressing mode and operation:

| Instruction Type | Base Cycles | Examples |
|------------------|-------------|----------|
| **Implied** | 2 | `NOP`, `CLC`, `DEX` |
| **Immediate** | 2 | `LDA #$42` |
| **Zero Page** | 3 | `LDA $80` |
| **Zero Page,X/Y** | 4 | `LDA $80,X` |
| **Absolute** | 4 | `LDA $4020` |
| **Absolute,X/Y** | 4-5 | `LDA $4020,X` (+1 if page crossed) |
| **Indirect,X** | 6 | `LDA ($80,X)` |
| **Indirect,Y** | 5-6 | `LDA ($80),Y` (+1 if page crossed) |
| **RMW** | 5-7 | `INC $80` (read, modify, write back) |
| **Stack** | 3-4 | `PHA`, `PLA` |
| **Branches** | 2-4 | `BNE label` (+1 if taken, +2 if page crossed) |
| **Jumps** | 3-6 | `JMP $8000`, `JSR $8000` |
| **Interrupts** | 7 | `BRK`, `NMI`, `IRQ` |

### Cycle-by-Cycle Execution

Each instruction follows a predictable pattern of memory operations. Here's a detailed breakdown for common patterns:

#### Example 1: LDA Absolute (4 cycles)

```
Cycle 1: Fetch opcode ($AD) from PC, increment PC
Cycle 2: Fetch low byte of address from PC, increment PC
Cycle 3: Fetch high byte of address from PC, increment PC
Cycle 4: Read value from effective address, store in A
```

**Implementation:**

```rust
fn lda_absolute(&mut self, bus: &mut Bus) -> u8 {
    let lo = self.read(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let hi = self.read(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let addr = u16::from_le_bytes([lo, hi]);
    let value = self.read(bus, addr);

    self.a = value;
    self.set_zn_flags(value);

    4 // Base cycles
}
```

#### Example 2: LDA Absolute,X with Page Crossing (5 cycles)

```
Cycle 1: Fetch opcode ($BD) from PC, increment PC
Cycle 2: Fetch low byte (BAL) from PC, increment PC
Cycle 3: Fetch high byte (BAH) from PC, increment PC
Cycle 4: Read from BAH:(BAL + X) [may be wrong page, dummy read]
Cycle 5: Read from BAH+1:(BAL + X) [correct page if crossed]
```

**Critical Detail:** Cycle 4 occurs even if no page crossing happens. The CPU speculatively reads from the incorrect address, then either:

- Uses that value (no page crossing)
- Discards it and reads again with corrected high byte (page crossing occurred)

**Implementation:**

```rust
fn lda_absolute_x(&mut self, bus: &mut Bus) -> u8 {
    let lo = self.read(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let hi = self.read(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let base_addr = u16::from_le_bytes([lo, hi]);
    let indexed_addr = base_addr.wrapping_add(self.x as u16);

    // Dummy read from potentially incorrect address
    let dummy_addr = (base_addr & 0xFF00) | ((base_addr + self.x as u16) & 0x00FF);
    let _ = self.read(bus, dummy_addr);

    let mut cycles = 4;

    // Check for page crossing
    if (base_addr & 0xFF00) != (indexed_addr & 0xFF00) {
        cycles += 1; // Extra cycle for correct read
    }

    let value = self.read(bus, indexed_addr);
    self.a = value;
    self.set_zn_flags(value);

    cycles
}
```

#### Example 3: INC Zero Page (5 cycles, Read-Modify-Write)

```
Cycle 1: Fetch opcode ($E6) from PC, increment PC
Cycle 2: Fetch address from PC, increment PC
Cycle 3: Read value from address
Cycle 4: Write old value back to address (dummy write)
Cycle 5: Write incremented value to address
```

**Critical Detail:** RMW instructions always write the original value back before writing the modified value. This is observable behavior that some games exploit.

**Implementation:**

```rust
fn inc_zero_page(&mut self, bus: &mut Bus) -> u8 {
    let addr = self.read(bus, self.pc) as u16;
    self.pc = self.pc.wrapping_add(1);

    let value = self.read(bus, addr);

    // Dummy write (critical for hardware accuracy)
    self.write(bus, addr, value);

    let result = value.wrapping_add(1);
    self.write(bus, addr, result);

    self.set_zn_flags(result);

    5 // Always 5 cycles
}
```

---

## Page Crossing Penalties

### What is a Page Crossing?

A **page** is a 256-byte block of memory aligned on a 256-byte boundary (addresses $xx00-$xxFF). A page crossing occurs when:

```
Base Address:     $20F0
Index (X):        $20
Indexed Address:  $2110  ← High byte changed ($20 → $21)
```

The low byte wrapped around ($F0 + $20 = $110, carry into high byte).

### Instructions Affected

Only certain addressing modes incur page crossing penalties:

| Addressing Mode | Instructions Affected | Penalty |
|-----------------|----------------------|---------|
| **Absolute,X** | `LDA`, `LDY`, `EOR`, `AND`, `ORA`, `ADC`, `SBC`, `CMP` | +1 cycle |
| **Absolute,Y** | `LDA`, `LDX`, `EOR`, `AND`, `ORA`, `ADC`, `SBC`, `CMP` | +1 cycle |
| **(Indirect),Y** | `LDA`, `EOR`, `AND`, `ORA`, `ADC`, `SBC`, `CMP` | +1 cycle |
| **Branches Taken** | `BCC`, `BCS`, `BEQ`, `BNE`, `BMI`, `BPL`, `BVC`, `BVS` | +1 cycle (branch), +2 total if page crossed |

**Important:** Write instructions like `STA`, `STX`, `STY` do NOT benefit from page boundary optimization. They always take the same number of cycles regardless of page crossing because they must perform the write.

### Page Crossing Detection

```rust
fn crosses_page_boundary(base: u16, indexed: u16) -> bool {
    (base & 0xFF00) != (indexed & 0xFF00)
}
```

**Example:**

```rust
let base_addr = 0x20F0;
let index = 0x20;
let indexed_addr = base_addr.wrapping_add(index as u16); // 0x2110

if crosses_page_boundary(base_addr, indexed_addr) {
    // Add extra cycle
}
```

---

## Dummy Reads and Writes

### Dummy Reads

The 6502 always performs a predictable sequence of memory operations, including reads that don't affect processor state. These are NOT optimizations that can be skipped - they are observable hardware behavior.

#### Why Dummy Reads Matter

1. **PPU Register Side Effects**: Reading `$2002` (PPU status) clears the VBlank flag. A dummy read can trigger this.
2. **Mapper IRQ Counters**: Some mappers (MMC3) clock their scanline counters on PPU address line changes, which occur during reads.
3. **Controller Shift Registers**: Reading `$4016`/`$4017` advances the controller shift register state.

#### Common Dummy Read Patterns

**Absolute,X/Y with Page Crossing:**

```
Address $20F0,X where X = $20:
  - Dummy read from $20:($F0 + $20) = $2010 [wrong page]
  - Real read from $21:10 [correct page]
```

**Indirect Indexed (Indirect),Y:**

```
($80),Y where ($80) = $2000 and Y = $10:
  - Read pointer low byte from $80
  - Read pointer high byte from $81
  - Dummy read from $20:10 [base + Y, potentially wrong page]
  - Real read from $2010 [correct address]
```

### Dummy Writes

RMW (Read-Modify-Write) instructions always write the original value back before writing the modified value:

```
INC $4014:
  Cycle 3: Read $4014 → $05
  Cycle 4: Write $05 back to $4014 (dummy write)
  Cycle 5: Write $06 to $4014
```

**Critical for:**

- **OAM DMA Trigger**: Writing to `$4014` triggers DMA even during the dummy write phase of an RMW instruction.
- **Mapper State Machines**: Some mappers track write sequences and may be triggered by dummy writes.

**Implementation:**

```rust
// Always write original value back for RMW
fn inc(&mut self, bus: &mut Bus, addr: u16) {
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write
    let result = value.wrapping_add(1);
    self.write(bus, addr, result); // Real write
    self.set_zn_flags(result);
}
```

---

## DMA Timing

### OAM DMA (Sprite DMA)

Writing any value to `$4014` initiates a 256-byte transfer from CPU memory to PPU OAM (sprite memory):

```
Write to $4014 = $02:
  → Copies $0200-$02FF to PPU OAM $00-$FF
  → CPU is suspended for 513 or 514 cycles
```

#### Cycle Breakdown

```
Cycle 1-2:   Dummy reads (wait for write cycle to finish)
             - 1 cycle if on an odd CPU cycle
             - 2 cycles if on an even CPU cycle

Cycle 3-514: 512 cycles for 256 reads + 256 writes
             - Read from $02xx
             - Write to OAM
             - Repeat 256 times
```

**Total:** 513 cycles (odd alignment) or 514 cycles (even alignment)

#### Implementation

```rust
pub fn trigger_oam_dma(&mut self, bus: &mut Bus, page: u8) {
    // Align to odd CPU cycle (add 1 if on even cycle)
    if self.cycles % 2 == 0 {
        self.cycles += 1;
    }

    // Dummy wait cycle
    self.cycles += 1;

    // Transfer 256 bytes
    let base = (page as u16) << 8;
    for i in 0..256 {
        let value = bus.read(base + i);
        self.oam_data[i as usize] = value;
        self.cycles += 2; // 1 read + 1 write
    }
}
```

**Important:** During OAM DMA:

- CPU cannot execute instructions
- DMC DMA can still occur (and will steal additional cycles)
- PPU continues rendering normally

### DMC DMA (Audio Sample DMA)

The DMC audio channel can read samples from CPU memory, stealing cycles from the CPU:

```
DMC Sample Read:
  - Stalls CPU for 4 cycles
  - Can interrupt OAM DMA (adding 2-4 cycles to total)
  - Can corrupt controller reads if poorly timed
```

#### DMC/OAM DMA Conflict

If DMC DMA occurs during OAM DMA, the timing becomes complex:

```
Best case:  +2 cycles (DMC aligns perfectly)
Worst case: +4 cycles (DMC causes alignment issues)
```

**Implementation Note:** Most emulators simplify this to always adding 4 cycles for DMC reads.

---

## Interrupt Timing

### Interrupt Priority

```
RESET > NMI > IRQ
```

**Priority Rules:**

1. RESET always takes precedence
2. NMI can interrupt an IRQ handler
3. IRQ is blocked by the I (interrupt disable) flag
4. BRK behaves like IRQ but sets the B flag

### NMI Timing (7 cycles)

NMI is edge-triggered on the falling edge of the NMI line (PPU VBlank flag set):

```
Cycle 1: Current instruction completes
Cycle 2: Dummy read (internal operation)
Cycle 3: Push PCH to stack, decrement S
Cycle 4: Push PCL to stack, decrement S
Cycle 5: Push P (status) to stack, decrement S
Cycle 6: Fetch NMI vector low byte from $FFFA
Cycle 7: Fetch NMI vector high byte from $FFFB, jump to handler
```

**Critical Timing Points:**

- NMI triggered at dot 1 of scanline 241 (start of VBlank)
- Takes 7 cycles to reach handler
- Current instruction completes before NMI servicing begins
- Reading `$2002` during cycle 1 of scanline 241 suppresses NMI (race condition)

**Implementation:**

```rust
fn service_nmi(&mut self, bus: &mut Bus) {
    self.cycles += 2; // Internal operations

    self.push_u16(bus, self.pc);
    self.push(bus, self.p & !0x10); // Clear B flag
    self.p |= 0x04; // Set I flag

    let lo = bus.read(0xFFFA);
    let hi = bus.read(0xFFFB);
    self.pc = u16::from_le_bytes([lo, hi]);

    self.cycles += 5;
}
```

### IRQ Timing (7 cycles)

IRQ is level-triggered and blocked by the I flag:

```
Same cycle breakdown as NMI, but:
  - Vector at $FFFE/$FFFF
  - Can be blocked by I flag
  - Checked at the end of each instruction
```

**IRQ Polling Point:**

```rust
fn check_irq(&self) -> bool {
    self.irq_line && (self.p & 0x04 == 0)
}
```

### BRK Timing (7 cycles)

BRK is a software interrupt:

```
Same as IRQ, but:
  - B flag is SET in pushed status byte
  - PC pushed is PC+2 (skips padding byte)
```

---

## Implementation Guide

### Cycle Tracking Strategy

**Option 1: Instruction-Level Tracking**

Execute entire instruction, return total cycles:

```rust
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    if self.nmi_pending {
        return self.service_nmi(bus);
    }

    let opcode = self.read(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let base_cycles = CYCLE_TABLE[opcode as usize];
    let extra_cycles = self.execute(opcode, bus);

    base_cycles + extra_cycles
}
```

**Option 2: Sub-Cycle Tracking**

Track individual memory operations (more accurate for mid-instruction events):

```rust
pub fn tick(&mut self, bus: &mut Bus) -> bool {
    self.cycle_count += 1;

    match self.instruction_state {
        InstructionState::Fetch => { /* ... */ }
        InstructionState::Decode => { /* ... */ }
        InstructionState::Execute(cycle) => { /* ... */ }
    }

    self.instruction_state == InstructionState::Complete
}
```

### Cycle Table

Pre-compute base cycle costs for all 256 opcodes:

```rust
const CYCLE_TABLE: [u8; 256] = [
    // 0x00    0x01    0x02    0x03    0x04    0x05    0x06    0x07
    /*0x00*/ 7,      6,      0,      8,      3,      3,      5,      5,
    /*0x08*/ 3,      2,      2,      2,      4,      4,      6,      6,
    // ... continue for all 256 opcodes
];
```

### Page Crossing Detection

```rust
fn add_page_crossing_penalty(&self, base: u16, indexed: u16) -> u8 {
    if (base & 0xFF00) != (indexed & 0xFF00) {
        1
    } else {
        0
    }
}
```

### Branch Timing

```rust
fn branch(&mut self, bus: &mut Bus, condition: bool) -> u8 {
    let offset = self.read(bus, self.pc) as i8;
    self.pc = self.pc.wrapping_add(1);

    if !condition {
        return 2; // Branch not taken
    }

    let old_pc = self.pc;
    let new_pc = self.pc.wrapping_add(offset as u16);
    self.pc = new_pc;

    let mut cycles = 3; // Branch taken

    // Add cycle for page crossing
    if (old_pc & 0xFF00) != (new_pc & 0xFF00) {
        cycles += 1;
    }

    cycles
}
```

---

## Test ROM Validation

### Timing Test ROMs

1. **nestest.nes**
   - Validates basic instruction timing
   - Checks cycle-accurate execution
   - Golden log comparison

2. **blargg's cpu_timing_test**
   - Tests page crossing penalties
   - Validates branch timing
   - Checks DMA timing

3. **cpu_dummy_reads**
   - Validates dummy read behavior
   - Tests PPU register side effects
   - Checks mapper interactions

4. **cpu_dummy_writes**
   - Validates RMW dummy writes
   - Tests write-triggered side effects

5. **oam_dma_timing**
   - Tests OAM DMA cycle counts
   - Validates alignment behavior
   - Checks DMC DMA conflicts

### Cycle Count Verification

```rust
#[test]
fn test_instruction_timing() {
    let mut cpu = Cpu::new();
    let mut bus = MockBus::new();

    // LDA Absolute (4 cycles)
    bus.write(0x8000, 0xAD); // LDA opcode
    bus.write(0x8001, 0x00); // Low byte
    bus.write(0x8002, 0x40); // High byte
    bus.write(0x4000, 0x42); // Value to load

    cpu.pc = 0x8000;
    let cycles = cpu.step(&mut bus);

    assert_eq!(cycles, 4);
    assert_eq!(cpu.a, 0x42);
}
```

---

## References

- [NesDev Wiki - CPU](https://www.nesdev.org/wiki/CPU)
- [6502 Timing Reference](https://www.nesdev.org/6502_cpu.txt)
- [Visual 6502 Simulator](http://visual6502.org/)
- [Cycle-by-Cycle Breakdown](https://www.nesdev.org/wiki/CPU_instruction_set)
- [DMA Timing Details](https://www.nesdev.org/wiki/PPU_OAM#DMA)

---

**Next:** [CPU Unofficial Opcodes](CPU_UNOFFICIAL_OPCODES.md) | [Back to CPU Overview](CPU_6502.md)
