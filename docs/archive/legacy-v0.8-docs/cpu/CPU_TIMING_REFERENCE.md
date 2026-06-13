# CPU Timing Reference

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete per-instruction cycle timing for all 256 opcodes

---

## Table of Contents

- [Overview](#overview)
- [Timing Fundamentals](#timing-fundamentals)
- [Cycle Count Tables](#cycle-count-tables)
- [Page Crossing Behavior](#page-crossing-behavior)
- [Branch Timing](#branch-timing)
- [Interrupt Timing](#interrupt-timing)
- [DMA Timing](#dma-timing)
- [Read-Modify-Write Timing](#read-modify-write-timing)
- [Dummy Reads and Writes](#dummy-reads-and-writes)
- [Timing Verification](#timing-verification)

---

## Overview

The NES CPU runs at **1.789773 MHz** (NTSC) or **1.662607 MHz** (PAL), where each cycle is approximately 559 ns (NTSC) or 601 ns (PAL). Accurate cycle timing is critical for:

- **Sprite 0 hit detection** - Requires cycle-perfect CPU/PPU synchronization
- **MMC3 IRQ timing** - IRQ counter clocked by PPU A12 rising edge
- **APU frame counter** - Generates IRQs at specific cycle counts
- **DMC sample fetching** - Steals CPU cycles at precise intervals
- **Raster effects** - Mid-frame register writes for parallax scrolling

### Clock Relationship

```
NTSC Master Clock: 21.477272 MHz
PPU Clock = Master ÷ 4 = 5.369318 MHz
CPU Clock = Master ÷ 12 = 1.789773 MHz

1 CPU cycle = 3 PPU dots = ~559 ns

PAL Master Clock: 26.601712 MHz
PPU Clock = Master ÷ 5 = 5.320342 MHz
CPU Clock = Master ÷ 16 = 1.662607 MHz

1 CPU cycle = 3.2 PPU dots = ~601 ns
```

---

## Timing Fundamentals

### Instruction Phases

Every instruction consists of:

1. **Opcode Fetch**: Read instruction byte, increment PC
2. **Operand Fetch**: Read 0-2 operand bytes, increment PC
3. **Address Calculation**: Compute effective address (indexed modes)
4. **Dummy Reads**: Extra reads for page crossing or timing alignment
5. **Operation Execution**: Perform actual operation
6. **Writeback**: Store result to memory (if applicable)

### Minimum Instruction Timing

```
Implied:        2 cycles (opcode fetch + execute)
Immediate:      2 cycles (opcode + operand)
Zero Page:      3 cycles (opcode + address + read/write)
Absolute:       4 cycles (opcode + addr_lo + addr_hi + read/write)
```

### Page Crossing Detection

A page crossing occurs when an indexed address crosses a 256-byte boundary:

```rust
fn page_crossed(base: u16, indexed: u16) -> bool {
    (base & 0xFF00) != (indexed & 0xFF00)
}
```

**Example:**

```
LDA $10FF,X where X=$01
Base:    $10FF (page $10)
Indexed: $1100 (page $11)
Result:  Page crossed, +1 cycle penalty
```

---

## Cycle Count Tables

### Load/Store Instructions

| Opcode | Mnemonic | Addr Mode | Cycles | +Page Cross | Notes |
|--------|----------|-----------|--------|-------------|-------|
| A9 | LDA | Immediate | 2 | - | - |
| A5 | LDA | Zero Page | 3 | - | - |
| B5 | LDA | Zero Page,X | 4 | - | Dummy read at base |
| AD | LDA | Absolute | 4 | - | - |
| BD | LDA | Absolute,X | 4 | +1 | Penalty on page cross |
| B9 | LDA | Absolute,Y | 4 | +1 | Penalty on page cross |
| A1 | LDA | (Indirect,X) | 6 | - | Always 6 |
| B1 | LDA | (Indirect),Y | 5 | +1 | Penalty on page cross |
| A2 | LDX | Immediate | 2 | - | - |
| A6 | LDX | Zero Page | 3 | - | - |
| B6 | LDX | Zero Page,Y | 4 | - | Note: Y not X! |
| AE | LDX | Absolute | 4 | - | - |
| BE | LDX | Absolute,Y | 4 | +1 | Penalty on page cross |
| A0 | LDY | Immediate | 2 | - | - |
| A4 | LDY | Zero Page | 3 | - | - |
| B4 | LDY | Zero Page,X | 4 | - | - |
| AC | LDY | Absolute | 4 | - | - |
| BC | LDY | Absolute,X | 4 | +1 | Penalty on page cross |
| 85 | STA | Zero Page | 3 | - | - |
| 95 | STA | Zero Page,X | 4 | - | Dummy read at base |
| 8D | STA | Absolute | 4 | - | - |
| 9D | STA | Absolute,X | 5 | - | Always 5, no penalty |
| 99 | STA | Absolute,Y | 5 | - | Always 5, no penalty |
| 81 | STA | (Indirect,X) | 6 | - | Always 6 |
| 91 | STA | (Indirect),Y | 6 | - | Always 6, no penalty |
| 86 | STX | Zero Page | 3 | - | - |
| 96 | STX | Zero Page,Y | 4 | - | Note: Y not X! |
| 8E | STX | Absolute | 4 | - | - |
| 84 | STY | Zero Page | 3 | - | - |
| 94 | STY | Zero Page,X | 4 | - | - |
| 8C | STY | Absolute | 4 | - | - |

### Arithmetic/Logic Instructions

| Opcode | Mnemonic | Addr Mode | Cycles | +Page Cross |
|--------|----------|-----------|--------|-------------|
| 69 | ADC | Immediate | 2 | - |
| 65 | ADC | Zero Page | 3 | - |
| 75 | ADC | Zero Page,X | 4 | - |
| 6D | ADC | Absolute | 4 | - |
| 7D | ADC | Absolute,X | 4 | +1 |
| 79 | ADC | Absolute,Y | 4 | +1 |
| 61 | ADC | (Indirect,X) | 6 | - |
| 71 | ADC | (Indirect),Y | 5 | +1 |
| E9 | SBC | Immediate | 2 | - |
| E5 | SBC | Zero Page | 3 | - |
| F5 | SBC | Zero Page,X | 4 | - |
| ED | SBC | Absolute | 4 | - |
| FD | SBC | Absolute,X | 4 | +1 |
| F9 | SBC | Absolute,Y | 4 | +1 |
| E1 | SBC | (Indirect,X) | 6 | - |
| F1 | SBC | (Indirect),Y | 5 | +1 |
| 29 | AND | Immediate | 2 | - |
| 25 | AND | Zero Page | 3 | - |
| 35 | AND | Zero Page,X | 4 | - |
| 2D | AND | Absolute | 4 | - |
| 3D | AND | Absolute,X | 4 | +1 |
| 39 | AND | Absolute,Y | 4 | +1 |
| 21 | AND | (Indirect,X) | 6 | - |
| 31 | AND | (Indirect),Y | 5 | +1 |
| 09 | ORA | Immediate | 2 | - |
| 05 | ORA | Zero Page | 3 | - |
| 15 | ORA | Zero Page,X | 4 | - |
| 0D | ORA | Absolute | 4 | - |
| 1D | ORA | Absolute,X | 4 | +1 |
| 19 | ORA | Absolute,Y | 4 | +1 |
| 01 | ORA | (Indirect,X) | 6 | - |
| 11 | ORA | (Indirect),Y | 5 | +1 |
| 49 | EOR | Immediate | 2 | - |
| 45 | EOR | Zero Page | 3 | - |
| 55 | EOR | Zero Page,X | 4 | - |
| 4D | EOR | Absolute | 4 | - |
| 5D | EOR | Absolute,X | 4 | +1 |
| 59 | EOR | Absolute,Y | 4 | +1 |
| 41 | EOR | (Indirect,X) | 6 | - |
| 51 | EOR | (Indirect),Y | 5 | +1 |

### Increment/Decrement

| Opcode | Mnemonic | Addr Mode | Cycles | Notes |
|--------|----------|-----------|--------|-------|
| E8 | INX | Implied | 2 | - |
| C8 | INY | Implied | 2 | - |
| CA | DEX | Implied | 2 | - |
| 88 | DEY | Implied | 2 | - |
| E6 | INC | Zero Page | 5 | RMW |
| F6 | INC | Zero Page,X | 6 | RMW |
| EE | INC | Absolute | 6 | RMW |
| FE | INC | Absolute,X | 7 | RMW, always 7 |
| C6 | DEC | Zero Page | 5 | RMW |
| D6 | DEC | Zero Page,X | 6 | RMW |
| CE | DEC | Absolute | 6 | RMW |
| DE | DEC | Absolute,X | 7 | RMW, always 7 |

### Shift/Rotate

| Opcode | Mnemonic | Addr Mode | Cycles | Notes |
|--------|----------|-----------|--------|-------|
| 0A | ASL | Accumulator | 2 | - |
| 06 | ASL | Zero Page | 5 | RMW |
| 16 | ASL | Zero Page,X | 6 | RMW |
| 0E | ASL | Absolute | 6 | RMW |
| 1E | ASL | Absolute,X | 7 | RMW, always 7 |
| 4A | LSR | Accumulator | 2 | - |
| 46 | LSR | Zero Page | 5 | RMW |
| 56 | LSR | Zero Page,X | 6 | RMW |
| 4E | LSR | Absolute | 6 | RMW |
| 5E | LSR | Absolute,X | 7 | RMW, always 7 |
| 2A | ROL | Accumulator | 2 | - |
| 26 | ROL | Zero Page | 5 | RMW |
| 36 | ROL | Zero Page,X | 6 | RMW |
| 2E | ROL | Absolute | 6 | RMW |
| 3E | ROL | Absolute,X | 7 | RMW, always 7 |
| 6A | ROR | Accumulator | 2 | - |
| 66 | ROR | Zero Page | 5 | RMW |
| 76 | ROR | Zero Page,X | 6 | RMW |
| 6E | ROR | Absolute | 6 | RMW |
| 7E | ROR | Absolute,X | 7 | RMW, always 7 |

### Compare Instructions

| Opcode | Mnemonic | Addr Mode | Cycles | +Page Cross |
|--------|----------|-----------|--------|-------------|
| C9 | CMP | Immediate | 2 | - |
| C5 | CMP | Zero Page | 3 | - |
| D5 | CMP | Zero Page,X | 4 | - |
| CD | CMP | Absolute | 4 | - |
| DD | CMP | Absolute,X | 4 | +1 |
| D9 | CMP | Absolute,Y | 4 | +1 |
| C1 | CMP | (Indirect,X) | 6 | - |
| D1 | CMP | (Indirect),Y | 5 | +1 |
| E0 | CPX | Immediate | 2 | - |
| E4 | CPX | Zero Page | 3 | - |
| EC | CPX | Absolute | 4 | - |
| C0 | CPY | Immediate | 2 | - |
| C4 | CPY | Zero Page | 3 | - |
| CC | CPY | Absolute | 4 | - |
| 24 | BIT | Zero Page | 3 | - |
| 2C | BIT | Absolute | 4 | - |

### Branch Instructions

| Opcode | Mnemonic | Condition | Base | +Taken | +Page Cross |
|--------|----------|-----------|------|--------|-------------|
| 10 | BPL | N=0 | 2 | +1 | +1 |
| 30 | BMI | N=1 | 2 | +1 | +1 |
| 50 | BVC | V=0 | 2 | +1 | +1 |
| 70 | BVS | V=1 | 2 | +1 | +1 |
| 90 | BCC | C=0 | 2 | +1 | +1 |
| B0 | BCS | C=1 | 2 | +1 | +1 |
| D0 | BNE | Z=0 | 2 | +1 | +1 |
| F0 | BEQ | Z=1 | 2 | +1 | +1 |

### Transfer/Stack Instructions

| Opcode | Mnemonic | Cycles | Notes |
|--------|----------|--------|-------|
| AA | TAX | 2 | - |
| A8 | TAY | 2 | - |
| 8A | TXA | 2 | - |
| 98 | TYA | 2 | - |
| BA | TSX | 2 | - |
| 9A | TXS | 2 | - |
| 48 | PHA | 3 | - |
| 08 | PHP | 3 | - |
| 68 | PLA | 4 | Includes dummy stack read |
| 28 | PLP | 4 | Includes dummy stack read |

### Jump/Subroutine/Interrupt

| Opcode | Mnemonic | Cycles | Notes |
|--------|----------|--------|-------|
| 4C | JMP Absolute | 3 | - |
| 6C | JMP Indirect | 5 | Page boundary bug |
| 20 | JSR | 6 | - |
| 60 | RTS | 6 | - |
| 00 | BRK | 7 | Software interrupt |
| 40 | RTI | 6 | - |

### Flag Instructions

| Opcode | Mnemonic | Cycles |
|--------|----------|--------|
| 18 | CLC | 2 |
| 38 | SEC | 2 |
| 58 | CLI | 2 |
| 78 | SEI | 2 |
| B8 | CLV | 2 |
| D8 | CLD | 2 |
| F8 | SED | 2 |
| EA | NOP | 2 |

---

## Page Crossing Behavior

### Read Operations (Penalty Applied)

When a read instruction crosses a page boundary, an extra cycle is spent reading from the incorrect address before correcting:

```
Cycle 1: Fetch opcode
Cycle 2: Fetch address low
Cycle 3: Fetch address high
Cycle 4: Read from (base_hi << 8) | ((base_lo + index) & 0xFF)  <- Wrong!
Cycle 5: Read from correct address                              <- Correct
```

**Instructions affected:** LDA, LDX, LDY, EOR, AND, ORA, ADC, SBC, CMP, (Indirect),Y

### Write Operations (No Penalty, Always Extra Cycle)

Write operations with indexed addressing **always** take the extra cycle:

```
STA $1000,X  - Always 5 cycles (even if X=0)
STA $1000,Y  - Always 5 cycles
```

This is because the CPU performs a dummy write to the incorrect address.

### Read-Modify-Write (Always Extra Cycle)

RMW operations **always** take the full cycle count regardless of page crossing:

```
INC $1000,X  - Always 7 cycles
ASL $1000,X  - Always 7 cycles
```

---

## Branch Timing

Branches have variable timing based on:

1. **Branch taken?**
2. **Page boundary crossed?**

### Branch Not Taken: 2 Cycles

```
Cycle 1: Fetch opcode BNE
Cycle 2: Fetch offset, check condition (false), discard offset
```

### Branch Taken, Same Page: 3 Cycles

```
Example: BNE $02 (branch forward 2 bytes, same page)

Cycle 1: Fetch opcode BNE
Cycle 2: Fetch offset, check condition (true)
Cycle 3: Fix PC low byte
```

### Branch Taken, Page Crossed: 4 Cycles

```
Example: BNE $7F from $10F0 -> $1171 (crosses page boundary)

Cycle 1: Fetch opcode BNE
Cycle 2: Fetch offset $7F, check condition (true)
Cycle 3: Add offset to PCL (result: $016F, wrong page)
Cycle 4: Fix PCH (result: $1171, correct)
```

### Implementation

```rust
fn branch(&mut self, bus: &Bus, condition: bool) -> u8 {
    let offset = bus.read(self.pc) as i8;
    self.pc = self.pc.wrapping_add(1);

    if !condition {
        return 2; // Not taken
    }

    let old_pc = self.pc;
    let new_pc = self.pc.wrapping_add(offset as u16);
    self.pc = new_pc;

    let page_crossed = (old_pc & 0xFF00) != (new_pc & 0xFF00);
    if page_crossed {
        4 // Taken + page cross
    } else {
        3 // Taken, same page
    }
}
```

---

## Interrupt Timing

### NMI Sequence: 7 Cycles

```
Cycle 1: Internal operation (fetch next opcode, discarded)
Cycle 2: Internal operation
Cycle 3: Push PCH to stack ($0100 + SP), SP--
Cycle 4: Push PCL to stack ($0100 + SP), SP--
Cycle 5: Push P to stack (B=0, U=1), SP--, set I=1
Cycle 6: Fetch NMI vector low from $FFFA
Cycle 7: Fetch NMI vector high from $FFFB, PC = vector
```

### IRQ Sequence: 7 Cycles

Identical to NMI but uses vector $FFFE-$FFFF.

### BRK Sequence: 7 Cycles

```
Cycle 1: Fetch opcode $00 from PC, PC++
Cycle 2: Read next byte (signature), PC++
Cycle 3: Push PCH to stack, SP--
Cycle 4: Push PCL to stack, SP--
Cycle 5: Push P | 0x30 to stack (B=1, U=1), SP--, set I=1
Cycle 6: Fetch IRQ vector low from $FFFE
Cycle 7: Fetch IRQ vector high from $FFFF, PC = vector
```

**Note:** BRK increments PC by 2 before pushing, skipping the signature byte.

### RESET Sequence: 7 Cycles

```
Cycle 1-2: Internal operations
Cycle 3: Decrement SP (no write)
Cycle 4: Decrement SP (no write)
Cycle 5: Decrement SP (no write), set I=1
Cycle 6: Fetch RESET vector low from $FFFC
Cycle 7: Fetch RESET vector high from $FFFD, PC = vector
```

**Note:** SP is decremented 3 times but nothing is written to stack.

---

## DMA Timing

### OAM DMA ($4014)

Writing to $4014 triggers 256-byte DMA to PPU OAM:

**Total Cycles:** 513 or 514 depending on alignment

```
If write occurs on odd CPU cycle:
    Dummy read:  1 cycle  (alignment)
    DMA:       512 cycles (256 reads + 256 writes)
    Total:     513 cycles

If write occurs on even CPU cycle:
    Dummy read:  2 cycles (alignment)
    DMA:       512 cycles
    Total:     514 cycles
```

**Per-byte timing:**

```
Cycle N+0: Read from $XX00
Cycle N+1: Write to $2004
Cycle N+2: Read from $XX01
Cycle N+3: Write to $2004
...
(256 iterations)
```

### DMC DMA (APU Sample Fetch)

When DMC channel fetches a sample byte:

```
Steal 4 CPU cycles:
    Cycle 1: Dummy read
    Cycle 2: Dummy read
    Cycle 3: Dummy read
    Cycle 4: Read sample byte
```

**Interaction with OAM DMA:** If DMC fetch occurs during OAM DMA, OAM DMA is delayed.

---

## Read-Modify-Write Timing

RMW instructions perform a dummy write before the final write:

### INC $80 (Zero Page)

```
Cycle 1: Fetch opcode $E6
Cycle 2: Fetch address $80
Cycle 3: Read value from $0080 (e.g., $42)
Cycle 4: Write old value $42 back to $0080 (dummy write)
Cycle 5: Write new value $43 to $0080, set flags
```

### DEC $1234 (Absolute)

```
Cycle 1: Fetch opcode $CE
Cycle 2: Fetch address low $34
Cycle 3: Fetch address high $12
Cycle 4: Read value from $1234 (e.g., $10)
Cycle 5: Write old value $10 back to $1234 (dummy write)
Cycle 6: Write new value $0F to $1234, set flags
```

### ASL $1000,X (Absolute,X)

```
Cycle 1: Fetch opcode $1E
Cycle 2: Fetch address low $00
Cycle 3: Fetch address high $10, add X
Cycle 4: Read from wrong page (if page crossed)
Cycle 5: Read value from $1000+X
Cycle 6: Write old value back (dummy write)
Cycle 7: Write shifted value, set flags
```

**Bus Conflict Mappers:** Some mappers (NROM, CNROM, UXROM) are affected by this dummy write.

---

## Dummy Reads and Writes

### Dummy Reads

**Purpose:** Maintain cycle alignment and respect hardware timing constraints.

**Locations:**

1. **Zero Page,X/Y addressing** - Read from base address before adding index
2. **Indexed addressing page crossing** - Read from incorrect page
3. **Stack operations** - Dummy read from $0100+SP during increment

### Zero Page,X Example

```
LDA $80,X where X=$05

Cycle 1: Fetch opcode $B5
Cycle 2: Fetch address $80
Cycle 3: Read from $0080 (dummy), add X
Cycle 4: Read from $0085, load into A
```

### PLA Timing

```
Cycle 1: Fetch opcode $68
Cycle 2: Internal operation (increment SP)
Cycle 3: Read from $0100+SP (dummy)
Cycle 4: Read from $0100+(SP+1), load into A, set flags
```

### Dummy Writes

**All RMW operations** write the original value back before writing the new value. This is critical for:

- **Hardware registers** that react to writes
- **Mapper banking** that detects write operations
- **Bus conflict detection** on certain mappers

---

## Timing Verification

### Critical Test ROMs

**nestest.nes** - Golden log comparison:

```
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
C5F5  A2 00     LDX #$00                        A:00 X:00 Y:00 P:24 SP:FD CYC:10
```

Each line shows exact cycle count after instruction execution.

**cpu_timing_test6** - Tests all instructions:

```
01-implied
02-immediate
03-zero_page
04-zp_xy
05-absolute
06-abs_xy
07-ind_x
08-ind_y
09-branches
10-branch_timing
11-special
```

### Cycle Counting Implementation

```rust
pub struct Cpu {
    cycles: u64,  // Global cycle counter
}

impl Cpu {
    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        let cycles = self.execute_instruction(bus);
        self.cycles += cycles as u64;
        cycles
    }

    pub fn get_cycles(&self) -> u64 {
        self.cycles
    }
}
```

### PPU Synchronization

CPU and PPU must maintain exact 3:1 cycle ratio:

```rust
pub fn run_frame(&mut self) {
    while !self.ppu.frame_complete {
        let cpu_cycles = self.cpu.step(&mut self.bus);
        let ppu_cycles = cpu_cycles * 3;

        for _ in 0..ppu_cycles {
            self.ppu.step();
        }
    }
}
```

---

## Related Documentation

- [CPU_6502_SPECIFICATION.md](CPU_6502_SPECIFICATION.md) - Complete opcode reference
- [CPU_6502.md](CPU_6502.md) - High-level CPU overview
- [../ppu/PPU_TIMING.md](../ppu/PPU_TIMING.md) - PPU timing coordination
- [../apu/APU_TIMING.md](../apu/APU_TIMING.md) - APU cycle timing

---

## References

- [NESdev Wiki: CPU](https://www.nesdev.org/wiki/CPU)
- [NESdev Wiki: 6502 Cycle Times](https://www.nesdev.org/wiki/6502_cycle_times)
- [NESdev Wiki: CPU Timing](https://www.nesdev.org/wiki/CPU_timing_test_ROMs)
- [Visual6502](http://visual6502.org/) - Transistor-level timing verification
- nestest.log - Golden cycle log
- blargg cpu_timing_test6 ROM

---

**Document Status:** Complete cycle timing reference for all instructions and edge cases.
