# CPU Unofficial Opcodes (Illegal Instructions)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Why Unofficial Opcodes Exist](#why-unofficial-opcodes-exist)
- [Games Using Unofficial Opcodes](#games-using-unofficial-opcodes)
- [Instruction Categories](#instruction-categories)
- [Complete Opcode Reference](#complete-opcode-reference)
- [Stability and Variants](#stability-and-variants)
- [Implementation Strategy](#implementation-strategy)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The MOS 6502 processor has **256 possible opcodes** (8-bit opcode space), but MOS Technology officially documented only **151 instructions** (56 unique mnemonics with multiple addressing modes). The remaining **105 opcodes** are **unofficial** or **illegal** instructions that were never documented but perform predictable operations due to the 6502's microcode architecture.

**Key Facts:**
- Unofficial opcodes are deterministic and reliable on genuine hardware
- Several commercial NES games use them (intentionally or via compiler bugs)
- Emulators must implement them for 100% compatibility
- Some opcodes are unstable and produce unpredictable results

**Terminology:**
- **Unofficial**: Not documented by MOS Technology
- **Illegal**: Alternative term for unofficial
- **Undocumented**: Another synonym
- **Stable**: Produces consistent, predictable results
- **Unstable**: May produce unpredictable behavior across different 6502 revisions

---

## Why Unofficial Opcodes Exist

### 6502 Microcode Architecture

The 6502 uses a **130-entry decode ROM** that maps opcodes to microcode sequences. Each opcode activates certain ROM lines through combinational logic. Unofficial opcodes result from:

1. **Partial Decode Logic**: The 6502 doesn't fully decode all 8 bits of the opcode
2. **Microcode Combination**: Unofficial opcodes activate multiple ROM lines simultaneously
3. **Predictable Behavior**: The resulting operation is the logical combination of activated microcode

**Example:**
```
Official: LDA #imm (0xA9) - Load Accumulator
Official: LDX #imm (0xA2) - Load X Register
Unofficial: LAX #imm (0xAB) - Load both A and X (combination)
```

### Instruction Component Breakdown

The 6502 instruction set can be decomposed into components:

| Component | Function |
|-----------|----------|
| **Addressing Mode** | How to fetch operand |
| **ALU Operation** | What calculation to perform |
| **Register Selection** | Which register to use |
| **Read/Write** | Memory operation type |

Unofficial opcodes typically combine components in ways the official instruction set doesn't:

- **Combo Instructions**: Perform two operations in sequence (e.g., `LAX` = `LDA` + `TAX`)
- **RMW + ALU**: Read-Modify-Write combined with ALU operation (e.g., `DCP` = `DEC` + `CMP`)
- **Weird Stores**: Store result of register AND operations (e.g., `SAX` = `A & X`)

---

## Games Using Unofficial Opcodes

### Confirmed Commercial Titles

These licensed NES games are known to use unofficial opcodes:

| Game | Region | Opcodes Used | Purpose |
|------|--------|--------------|---------|
| **Beauty and the Beast** | USA | `LAX`, `SAX` | Load/store optimizations |
| **Disney's Aladdin** | USA | `SLO`, `RLA` | Graphics rendering |
| **Dynowarz** | USA | `LAX` | Sprite management |
| **F-1 Sensation** | Japan | `LAX` | Unknown |
| **Gremlins 2** | Unknown | `SLO` | Graphics effects |
| **Infiltrator** | USA | `LAX` | Data loading |
| **Joe & Mac** | USA | `SLO` | Graphics rendering |
| **Ninja Jajamaru-kun** | Japan | `LAX` | Sprite handling |
| **Puzznic** | USA | `LAX`, `SAX` | Puzzle logic |
| **Rainbow Islands** | Europe | `DCP` | Sprite evaluation |
| **R.C. Pro-Am** | USA | `LAX` | Unknown |
| **Super Cars** | Europe | `DCP` | Sprite rendering |
| **The Big Nose's American Adventure** | Unlicensed | Multiple | Various |

**Important:** These games will NOT run correctly on emulators that treat unofficial opcodes as NOPs or halt execution.

---

## Instruction Categories

Unofficial opcodes fall into several functional categories:

### 1. Combined Load Operations

Load a value into multiple registers simultaneously:

- **LAX** - Load Accumulator and X (A = X = memory)
- **LXA** - Load X and A (with AND #$EE on some CPUs)

### 2. Combined Store Operations

Store the result of register AND operations:

- **SAX** - Store A AND X (memory = A & X)
- **SHA** - Store A AND X AND (H+1)
- **SHX** - Store X AND (H+1)
- **SHY** - Store Y AND (H+1)

### 3. Arithmetic Combinations

Perform RMW (Read-Modify-Write) and then compare/ALU operation:

- **DCP** - Decrement memory, then Compare with A (DEC + CMP)
- **ISC** - Increment memory, then Subtract with Carry (INC + SBC)

### 4. Shift + Logic Combinations

Perform shift operation and then logical operation:

- **SLO** - Shift Left, then OR with A (ASL + ORA)
- **RLA** - Rotate Left, then AND with A (ROL + AND)
- **SRE** - Shift Right, then XOR with A (LSR + EOR)
- **RRA** - Rotate Right, then Add with Carry (ROR + ADC)

### 5. No-Operation Variants

Opcodes that read memory but perform no operation:

- **NOP** (unofficial variants) - 27 different opcodes
- **DOP** - Double-byte NOP (reads immediate byte)
- **TOP** - Triple-byte NOP (reads absolute address)

### 6. Unstable/Highly Unpredictable

These opcodes may behave differently across 6502 variants:

- **ANE** (XAA) - A = (A | magic) & X & immediate
- **LXA** (LAX immediate) - A = X = (A | magic) & immediate
- **TAS** - Store A & X & (H+1), set S = A & X
- **LAS** - A = X = S = memory & S
- **SHY**, **SHX**, **SHA** - Store operations with unstable high byte

### 7. Halting Instructions

These opcodes lock up the CPU (should be avoided):

- **JAM** (KIL, HLT) - Freeze the CPU until hardware reset
  - Opcodes: `0x02`, `0x12`, `0x22`, `0x32`, `0x42`, `0x52`, `0x62`, `0x72`, `0x92`, `0xB2`, `0xD2`, `0xF2`

---

## Complete Opcode Reference

### Stable Unofficial Opcodes

#### LAX - Load A and X

**Opcode Variants:**
- `0xA7` - Zero Page (3 cycles)
- `0xB7` - Zero Page,Y (4 cycles)
- `0xAF` - Absolute (4 cycles)
- `0xBF` - Absolute,Y (4 cycles, +1 if page crossed)
- `0xA3` - (Indirect,X) (6 cycles)
- `0xB3` - (Indirect),Y (5 cycles, +1 if page crossed)

**Operation:**
```
A = X = memory
N = bit 7 of value
Z = (value == 0)
```

**Use Case:** Load the same value into both A and X with a single instruction (saves 1 byte and 2 cycles vs. `LDA` + `TAX`).

**Implementation:**
```rust
fn lax(&mut self, bus: &mut Bus, addr: u16) {
    let value = self.read(bus, addr);
    self.a = value;
    self.x = value;
    self.set_zn_flags(value);
}
```

#### SAX - Store A AND X

**Opcode Variants:**
- `0x87` - Zero Page (3 cycles)
- `0x97` - Zero Page,Y (4 cycles)
- `0x8F` - Absolute (4 cycles)
- `0x83` - (Indirect,X) (6 cycles)

**Operation:**
```
memory = A & X
(no flags affected)
```

**Use Case:** Store bitwise AND of A and X in a single operation.

**Implementation:**
```rust
fn sax(&mut self, bus: &mut Bus, addr: u16) {
    let value = self.a & self.x;
    self.write(bus, addr, value);
}
```

#### DCP - Decrement and Compare

**Opcode Variants:**
- `0xC7` - Zero Page (5 cycles)
- `0xD7` - Zero Page,X (6 cycles)
- `0xCF` - Absolute (6 cycles)
- `0xDF` - Absolute,X (7 cycles)
- `0xDB` - Absolute,Y (7 cycles)
- `0xC3` - (Indirect,X) (8 cycles)
- `0xD3` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = memory - 1
CMP A, memory
(sets N, Z, C flags based on comparison)
```

**Use Case:** Decrement a counter and immediately compare it with A (common loop pattern).

**Implementation:**
```rust
fn dcp(&mut self, bus: &mut Bus, addr: u16) {
    // Read-Modify-Write cycle
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let result = value.wrapping_sub(1);
    self.write(bus, addr, result);

    // Compare A with decremented value
    let cmp_result = self.a.wrapping_sub(result);
    self.set_carry(self.a >= result);
    self.set_zn_flags(cmp_result);
}
```

#### ISC - Increment and Subtract with Carry

**Opcode Variants:**
- `0xE7` - Zero Page (5 cycles)
- `0xF7` - Zero Page,X (6 cycles)
- `0xEF` - Absolute (6 cycles)
- `0xFF` - Absolute,X (7 cycles)
- `0xFB` - Absolute,Y (7 cycles)
- `0xE3` - (Indirect,X) (8 cycles)
- `0xF3` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = memory + 1
A = A - memory - (1 - C)
(sets N, V, Z, C flags)
```

**Use Case:** Increment a value and subtract it from A in one instruction.

**Implementation:**
```rust
fn isc(&mut self, bus: &mut Bus, addr: u16) {
    // Increment
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let incremented = value.wrapping_add(1);
    self.write(bus, addr, incremented);

    // SBC
    self.sbc_impl(incremented);
}
```

#### SLO - Shift Left and OR

**Opcode Variants:**
- `0x07` - Zero Page (5 cycles)
- `0x17` - Zero Page,X (6 cycles)
- `0x0F` - Absolute (6 cycles)
- `0x1F` - Absolute,X (7 cycles)
- `0x1B` - Absolute,Y (7 cycles)
- `0x03` - (Indirect,X) (8 cycles)
- `0x13` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = memory << 1
A = A | memory
C = bit 7 of original value
N, Z = result flags
```

**Implementation:**
```rust
fn slo(&mut self, bus: &mut Bus, addr: u16) {
    // ASL
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let shifted = value << 1;
    self.set_carry((value & 0x80) != 0);
    self.write(bus, addr, shifted);

    // ORA
    self.a |= shifted;
    self.set_zn_flags(self.a);
}
```

#### RLA - Rotate Left and AND

**Opcode Variants:**
- `0x27` - Zero Page (5 cycles)
- `0x37` - Zero Page,X (6 cycles)
- `0x2F` - Absolute (6 cycles)
- `0x3F` - Absolute,X (7 cycles)
- `0x3B` - Absolute,Y (7 cycles)
- `0x23` - (Indirect,X) (8 cycles)
- `0x33` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = (memory << 1) | C
A = A & memory
C = bit 7 of original value
N, Z = result flags
```

**Implementation:**
```rust
fn rla(&mut self, bus: &mut Bus, addr: u16) {
    // ROL
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let carry_in = if self.get_carry() { 1 } else { 0 };
    let rotated = (value << 1) | carry_in;
    self.set_carry((value & 0x80) != 0);
    self.write(bus, addr, rotated);

    // AND
    self.a &= rotated;
    self.set_zn_flags(self.a);
}
```

#### SRE - Shift Right and XOR

**Opcode Variants:**
- `0x47` - Zero Page (5 cycles)
- `0x57` - Zero Page,X (6 cycles)
- `0x4F` - Absolute (6 cycles)
- `0x5F` - Absolute,X (7 cycles)
- `0x5B` - Absolute,Y (7 cycles)
- `0x43` - (Indirect,X) (8 cycles)
- `0x53` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = memory >> 1
A = A ^ memory
C = bit 0 of original value
N, Z = result flags
```

**Implementation:**
```rust
fn sre(&mut self, bus: &mut Bus, addr: u16) {
    // LSR
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let shifted = value >> 1;
    self.set_carry((value & 0x01) != 0);
    self.write(bus, addr, shifted);

    // EOR
    self.a ^= shifted;
    self.set_zn_flags(self.a);
}
```

#### RRA - Rotate Right and Add with Carry

**Opcode Variants:**
- `0x67` - Zero Page (5 cycles)
- `0x77` - Zero Page,X (6 cycles)
- `0x6F` - Absolute (6 cycles)
- `0x7F` - Absolute,X (7 cycles)
- `0x7B` - Absolute,Y (7 cycles)
- `0x63` - (Indirect,X) (8 cycles)
- `0x73` - (Indirect),Y (8 cycles)

**Operation:**
```
memory = (memory >> 1) | (C << 7)
A = A + memory + C
C = bit 0 of original value
N, V, Z, C = ADC result flags
```

**Implementation:**
```rust
fn rra(&mut self, bus: &mut Bus, addr: u16) {
    // ROR
    let value = self.read(bus, addr);
    self.write(bus, addr, value); // Dummy write

    let carry_in = if self.get_carry() { 0x80 } else { 0x00 };
    let rotated = (value >> 1) | carry_in;
    self.set_carry((value & 0x01) != 0);
    self.write(bus, addr, rotated);

    // ADC
    self.adc_impl(rotated);
}
```

#### Unofficial NOP Variants

**Single-Byte NOPs:**
- `0x1A`, `0x3A`, `0x5A`, `0x7A`, `0xDA`, `0xFA` - Implied (2 cycles)

**Double-Byte NOPs (DOP/SKB):**
- `0x80`, `0x82`, `0x89`, `0xC2`, `0xE2` - Immediate (2 cycles)
- `0x04`, `0x44`, `0x64` - Zero Page (3 cycles)
- `0x14`, `0x34`, `0x54`, `0x74`, `0xD4`, `0xF4` - Zero Page,X (4 cycles)

**Triple-Byte NOPs (TOP/SKW):**
- `0x0C` - Absolute (4 cycles)
- `0x1C`, `0x3C`, `0x5C`, `0x7C`, `0xDC`, `0xFC` - Absolute,X (4 cycles, +1 if page crossed)

**Implementation:**
```rust
fn nop_read(&mut self, bus: &mut Bus, addr: u16) {
    let _ = self.read(bus, addr); // Dummy read (important!)
}
```

---

### Unstable Unofficial Opcodes

#### ANE (XAA) - Magic AND

**Opcode:** `0x8B` - Immediate (2 cycles)

**Operation:**
```
A = (A | MAGIC) & X & immediate
```

**Issue:** The `MAGIC` constant varies across different 6502 variants:
- Most common: `0xEE` or `0xFF`
- May be `0x00`, `0x11`, or other values
- Depends on manufacturing process and individual CPU

**Recommendation:** Emulate as `A = (A | 0xEE) & X & immediate` but document the instability.

#### LXA (LAX Immediate) - Magic Load

**Opcode:** `0xAB` - Immediate (2 cycles)

**Operation:**
```
A = X = (A | MAGIC) & immediate
```

**Issue:** Same `MAGIC` constant instability as ANE.

**Recommendation:** Emulate as `A = X = (A | 0xEE) & immediate`.

#### SHY, SHX, SHA - Unstable Stores

**Opcodes:**
- `0x9C` - SHY Absolute,X (5 cycles)
- `0x9E` - SHX Absolute,Y (5 cycles)
- `0x9F` - SHA Absolute,Y (5 cycles)
- `0x93` - SHA (Indirect),Y (6 cycles)

**Operation (SHY example):**
```
memory[addr] = Y & (addr_high + 1)
```

**Issue:** If page crossing occurs, the high byte used in the AND may be incorrect, causing the write to fail or write to wrong address.

**Recommendation:** Emulate the AND operation, but note that real hardware behavior varies.

---

## Stability and Variants

### Stability Categories

| Stability | Opcodes | Behavior |
|-----------|---------|----------|
| **Highly Stable** | LAX, SAX, DCP, ISC, SLO, RLA, SRE, RRA, NOP variants | Consistent across all 6502 variants |
| **Mostly Stable** | ANC, ALR, ARR | Consistent on NES, may vary on other 6502 systems |
| **Unstable** | ANE, LXA, SHY, SHX, SHA, TAS, LAS | Varies across manufacturing processes |
| **Highly Unstable** | JAM | Locks up CPU, requires hardware reset |

### Implementation Priority

For NES emulation:

1. **Must Implement (High Priority):**
   - LAX, SAX, DCP, ISC - Used by commercial games
   - SLO, RLA, SRE, RRA - Common optimizations
   - All NOP variants - Used for timing

2. **Should Implement (Medium Priority):**
   - ANC, ALR, ARR - Less common but stable
   - ANE, LXA - Unstable but predictable

3. **Optional (Low Priority):**
   - SHY, SHX, SHA, TAS, LAS - Rarely used, unstable
   - JAM - CPU freeze, mostly used in copy protection

---

## Implementation Strategy

### Opcode Table Expansion

Extend the instruction table to cover all 256 opcodes:

```rust
const OPCODE_TABLE: [Instruction; 256] = [
    // Official opcodes (0x00-0xFF)
    /* 0x00 */ Instruction::BRK,
    /* 0x01 */ Instruction::ORA_INDIRECT_X,
    /* 0x02 */ Instruction::JAM, // Unofficial
    /* 0x03 */ Instruction::SLO_INDIRECT_X, // Unofficial
    // ... continue for all 256 opcodes
];
```

### Execution Handler

```rust
pub fn execute(&mut self, opcode: u8, bus: &mut Bus) -> u8 {
    match opcode {
        // Official instructions
        0xA9 => self.lda_immediate(bus),

        // Unofficial instructions
        0xA7 => self.lax_zero_page(bus),
        0xB7 => self.lax_zero_page_y(bus),
        0xAF => self.lax_absolute(bus),
        0xBF => self.lax_absolute_y(bus),
        0xA3 => self.lax_indirect_x(bus),
        0xB3 => self.lax_indirect_y(bus),

        // JAM instructions - halt CPU
        0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 |
        0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
            self.jammed = true;
            0xFF // Infinite cycles
        }

        _ => unreachable!("Opcode 0x{:02X} not implemented", opcode),
    }
}
```

### Testing Strategy

```rust
#[test]
fn test_lax_zero_page() {
    let mut cpu = Cpu::new();
    let mut bus = MockBus::new();

    bus.write(0x8000, 0xA7); // LAX $42
    bus.write(0x8001, 0x42);
    bus.write(0x0042, 0x55);

    cpu.pc = 0x8000;
    let cycles = cpu.step(&mut bus);

    assert_eq!(cycles, 3);
    assert_eq!(cpu.a, 0x55);
    assert_eq!(cpu.x, 0x55);
    assert_eq!(cpu.get_zero_flag(), false);
    assert_eq!(cpu.get_negative_flag(), false);
}
```

---

## Test ROM Validation

### Recommended Test ROMs

1. **blargg's instr_test-v5**
   - Tests all official and stable unofficial opcodes
   - Validates timing and flag behavior

2. **instr_misc**
   - Tests edge cases for unofficial opcodes
   - Validates RMW dummy write behavior

3. **nestest.nes**
   - Comprehensive test including some unofficial opcodes
   - Golden log comparison

4. **cpu_interrupts_v2**
   - Tests interrupt behavior with unofficial opcodes

### Validation Checklist

- [ ] All stable unofficial opcodes implemented
- [ ] Cycle counts match hardware
- [ ] Flag behavior correct for each instruction
- [ ] RMW instructions perform dummy writes
- [ ] Page crossing penalties applied correctly
- [ ] NOP variants read memory (side effects)
- [ ] JAM instructions halt CPU

---

## References

- [NesDev Wiki - CPU Unofficial Opcodes](https://www.nesdev.org/wiki/CPU_unofficial_opcodes)
- [6502 Undocumented Opcodes](http://www.ffd2.com/fridge/docs/6502-NMOS.extra.opcodes)
- [Visual 6502 Analysis](http://visual6502.org/wiki/6502_Unsupported_Opcodes)
- [Games Using Unofficial Opcodes](https://www.nesdev.org/wiki/CPU_unofficial_opcodes#Games_using_unofficial_opcodes)

---

**Next:** [PPU Overview](../ppu/PPU_OVERVIEW.md) | [Back to CPU Timing](CPU_TIMING.md)
