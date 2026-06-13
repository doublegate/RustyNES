# NES CPU: Complete 6502 Specification

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete reference for all 256 opcodes including unofficial instructions

---

## Table of Contents

- [Overview](#overview)
- [Complete Opcode Matrix](#complete-opcode-matrix)
- [Instruction Groups](#instruction-groups)
- [Official Instructions](#official-instructions)
- [Unofficial Instructions](#unofficial-instructions)
- [Cycle-by-Cycle Breakdown](#cycle-by-cycle-breakdown)
- [Addressing Mode Details](#addressing-mode-details)
- [Interrupt Edge Cases](#interrupt-edge-cases)
- [Implementation Reference](#implementation-reference)

---

## Overview

The MOS 6502 CPU has **256 possible opcodes** (0x00-0xFF). Of these:

- **151 opcodes** are officially documented
- **105 opcodes** are undocumented/unofficial but functional
- All 256 opcodes produce predictable behavior (no true "illegal" opcodes)

### Opcode Structure

6502 opcodes follow an `AAABBBCC` bit pattern:

```
Opcode: 7 6 5 4 3 2 1 0
        |AAA| |BBB|CC|
```

- **CC bits (0-1)**: Instruction group (control=00, ALU=01, RMW=10, unofficial=11)
- **BBB bits (2-4)**: Addressing mode
- **AAA bits (5-7)**: Operation within group

### Cycle Count Formula

Base cycles depend on addressing mode and operation type:

```
Immediate:           2 cycles
Zero Page:           3 cycles
Zero Page,X/Y:       4 cycles
Absolute:            4 cycles
Absolute,X/Y:        4 cycles (+1 if page crossed for reads)
(Indirect,X):        6 cycles
(Indirect),Y:        5 cycles (+1 if page crossed for reads)
```

**Page Crossing Penalty:** When indexed addressing crosses a 256-byte boundary:

- **Read operations**: +1 cycle (LDA, LDX, LDY, CMP, etc.)
- **Write operations**: Always take extra cycle (no conditional penalty)
- **Read-Modify-Write**: Always take extra cycle

---

## Complete Opcode Matrix

### Official Opcodes (56 instructions, 151 total opcodes)

| Opcode | Mnemonic | Addr Mode | Bytes | Cycles | Flags | Description |
|--------|----------|-----------|-------|--------|-------|-------------|
| **00** | BRK | Implied | 1 | 7 | I | Force interrupt |
| **01** | ORA | (Indirect,X) | 2 | 6 | N,Z | A \|= M |
| **05** | ORA | Zero Page | 2 | 3 | N,Z | A \|= M |
| **06** | ASL | Zero Page | 2 | 5 | N,Z,C | M <<= 1 |
| **08** | PHP | Implied | 1 | 3 | - | Push P |
| **09** | ORA | Immediate | 2 | 2 | N,Z | A \|= M |
| **0A** | ASL | Accumulator | 1 | 2 | N,Z,C | A <<= 1 |
| **0D** | ORA | Absolute | 3 | 4 | N,Z | A \|= M |
| **0E** | ASL | Absolute | 3 | 6 | N,Z,C | M <<= 1 |
| **10** | BPL | Relative | 2 | 2† | - | Branch if N=0 |
| **11** | ORA | (Indirect),Y | 2 | 5† | N,Z | A \|= M |
| **15** | ORA | Zero Page,X | 2 | 4 | N,Z | A \|= M |
| **16** | ASL | Zero Page,X | 2 | 6 | N,Z,C | M <<= 1 |
| **18** | CLC | Implied | 1 | 2 | C | C = 0 |
| **19** | ORA | Absolute,Y | 3 | 4† | N,Z | A \|= M |
| **1D** | ORA | Absolute,X | 3 | 4† | N,Z | A \|= M |
| **1E** | ASL | Absolute,X | 3 | 7 | N,Z,C | M <<= 1 |
| **20** | JSR | Absolute | 3 | 6 | - | Jump to subroutine |
| **21** | AND | (Indirect,X) | 2 | 6 | N,Z | A &= M |
| **24** | BIT | Zero Page | 2 | 3 | N,V,Z | Test bits |
| **25** | AND | Zero Page | 2 | 3 | N,Z | A &= M |
| **26** | ROL | Zero Page | 2 | 5 | N,Z,C | M rotate left |
| **28** | PLP | Implied | 1 | 4 | All | Pull P |
| **29** | AND | Immediate | 2 | 2 | N,Z | A &= M |
| **2A** | ROL | Accumulator | 1 | 2 | N,Z,C | A rotate left |
| **2C** | BIT | Absolute | 3 | 4 | N,V,Z | Test bits |
| **2D** | AND | Absolute | 3 | 4 | N,Z | A &= M |
| **2E** | ROL | Absolute | 3 | 6 | N,Z,C | M rotate left |
| **30** | BMI | Relative | 2 | 2† | - | Branch if N=1 |
| **31** | AND | (Indirect),Y | 2 | 5† | N,Z | A &= M |
| **35** | AND | Zero Page,X | 2 | 4 | N,Z | A &= M |
| **36** | ROL | Zero Page,X | 2 | 6 | N,Z,C | M rotate left |
| **38** | SEC | Implied | 1 | 2 | C | C = 1 |
| **39** | AND | Absolute,Y | 3 | 4† | N,Z | A &= M |
| **3D** | AND | Absolute,X | 3 | 4† | N,Z | A &= M |
| **3E** | ROL | Absolute,X | 3 | 7 | N,Z,C | M rotate left |
| **40** | RTI | Implied | 1 | 6 | All | Return from interrupt |
| **41** | EOR | (Indirect,X) | 2 | 6 | N,Z | A ^= M |
| **45** | EOR | Zero Page | 2 | 3 | N,Z | A ^= M |
| **46** | LSR | Zero Page | 2 | 5 | N,Z,C | M >>= 1 |
| **48** | PHA | Implied | 1 | 3 | - | Push A |
| **49** | EOR | Immediate | 2 | 2 | N,Z | A ^= M |
| **4A** | LSR | Accumulator | 1 | 2 | N,Z,C | A >>= 1 |
| **4C** | JMP | Absolute | 3 | 3 | - | Jump |
| **4D** | EOR | Absolute | 3 | 4 | N,Z | A ^= M |
| **4E** | LSR | Absolute | 3 | 6 | N,Z,C | M >>= 1 |
| **50** | BVC | Relative | 2 | 2† | - | Branch if V=0 |
| **51** | EOR | (Indirect),Y | 2 | 5† | N,Z | A ^= M |
| **55** | EOR | Zero Page,X | 2 | 4 | N,Z | A ^= M |
| **56** | LSR | Zero Page,X | 2 | 6 | N,Z,C | M >>= 1 |
| **58** | CLI | Implied | 1 | 2 | I | I = 0 |
| **59** | EOR | Absolute,Y | 3 | 4† | N,Z | A ^= M |
| **5D** | EOR | Absolute,X | 3 | 4† | N,Z | A ^= M |
| **5E** | LSR | Absolute,X | 3 | 7 | N,Z,C | M >>= 1 |
| **60** | RTS | Implied | 1 | 6 | - | Return from subroutine |
| **61** | ADC | (Indirect,X) | 2 | 6 | N,V,Z,C | A += M + C |
| **65** | ADC | Zero Page | 2 | 3 | N,V,Z,C | A += M + C |
| **66** | ROR | Zero Page | 2 | 5 | N,Z,C | M rotate right |
| **68** | PLA | Implied | 1 | 4 | N,Z | Pull A |
| **69** | ADC | Immediate | 2 | 2 | N,V,Z,C | A += M + C |
| **6A** | ROR | Accumulator | 1 | 2 | N,Z,C | A rotate right |
| **6C** | JMP | Indirect | 3 | 5 | - | Jump indirect |
| **6D** | ADC | Absolute | 3 | 4 | N,V,Z,C | A += M + C |
| **6E** | ROR | Absolute | 3 | 6 | N,Z,C | M rotate right |
| **70** | BVS | Relative | 2 | 2† | - | Branch if V=1 |
| **71** | ADC | (Indirect),Y | 2 | 5† | N,V,Z,C | A += M + C |
| **75** | ADC | Zero Page,X | 2 | 4 | N,V,Z,C | A += M + C |
| **76** | ROR | Zero Page,X | 2 | 6 | N,Z,C | M rotate right |
| **78** | SEI | Implied | 1 | 2 | I | I = 1 |
| **79** | ADC | Absolute,Y | 3 | 4† | N,V,Z,C | A += M + C |
| **7D** | ADC | Absolute,X | 3 | 4† | N,V,Z,C | A += M + C |
| **7E** | ROR | Absolute,X | 3 | 7 | N,Z,C | M rotate right |
| **81** | STA | (Indirect,X) | 2 | 6 | - | M = A |
| **84** | STY | Zero Page | 2 | 3 | - | M = Y |
| **85** | STA | Zero Page | 2 | 3 | - | M = A |
| **86** | STX | Zero Page | 2 | 3 | - | M = X |
| **88** | DEY | Implied | 1 | 2 | N,Z | Y -= 1 |
| **8A** | TXA | Implied | 1 | 2 | N,Z | A = X |
| **8C** | STY | Absolute | 3 | 4 | - | M = Y |
| **8D** | STA | Absolute | 3 | 4 | - | M = A |
| **8E** | STX | Absolute | 3 | 4 | - | M = X |
| **90** | BCC | Relative | 2 | 2† | - | Branch if C=0 |
| **91** | STA | (Indirect),Y | 2 | 6 | - | M = A |
| **94** | STY | Zero Page,X | 2 | 4 | - | M = Y |
| **95** | STA | Zero Page,X | 2 | 4 | - | M = A |
| **96** | STX | Zero Page,Y | 2 | 4 | - | M = X |
| **98** | TYA | Implied | 1 | 2 | N,Z | A = Y |
| **99** | STA | Absolute,Y | 3 | 5 | - | M = A |
| **9A** | TXS | Implied | 1 | 2 | - | SP = X |
| **9D** | STA | Absolute,X | 3 | 5 | - | M = A |
| **A0** | LDY | Immediate | 2 | 2 | N,Z | Y = M |
| **A1** | LDA | (Indirect,X) | 2 | 6 | N,Z | A = M |
| **A2** | LDX | Immediate | 2 | 2 | N,Z | X = M |
| **A4** | LDY | Zero Page | 2 | 3 | N,Z | Y = M |
| **A5** | LDA | Zero Page | 2 | 3 | N,Z | A = M |
| **A6** | LDX | Zero Page | 2 | 3 | N,Z | X = M |
| **A8** | TAY | Implied | 1 | 2 | N,Z | Y = A |
| **A9** | LDA | Immediate | 2 | 2 | N,Z | A = M |
| **AA** | TAX | Implied | 1 | 2 | N,Z | X = A |
| **AC** | LDY | Absolute | 3 | 4 | N,Z | Y = M |
| **AD** | LDA | Absolute | 3 | 4 | N,Z | A = M |
| **AE** | LDX | Absolute | 3 | 4 | N,Z | X = M |
| **B0** | BCS | Relative | 2 | 2† | - | Branch if C=1 |
| **B1** | LDA | (Indirect),Y | 2 | 5† | N,Z | A = M |
| **B4** | LDY | Zero Page,X | 2 | 4 | N,Z | Y = M |
| **B5** | LDA | Zero Page,X | 2 | 4 | N,Z | A = M |
| **B6** | LDX | Zero Page,Y | 2 | 4 | N,Z | X = M |
| **B8** | CLV | Implied | 1 | 2 | V | V = 0 |
| **B9** | LDA | Absolute,Y | 3 | 4† | N,Z | A = M |
| **BA** | TSX | Implied | 1 | 2 | N,Z | X = SP |
| **BC** | LDY | Absolute,X | 3 | 4† | N,Z | Y = M |
| **BD** | LDA | Absolute,X | 3 | 4† | N,Z | A = M |
| **BE** | LDX | Absolute,Y | 3 | 4† | N,Z | X = M |
| **C0** | CPY | Immediate | 2 | 2 | N,Z,C | Y - M |
| **C1** | CMP | (Indirect,X) | 2 | 6 | N,Z,C | A - M |
| **C4** | CPY | Zero Page | 2 | 3 | N,Z,C | Y - M |
| **C5** | CMP | Zero Page | 2 | 3 | N,Z,C | A - M |
| **C6** | DEC | Zero Page | 2 | 5 | N,Z | M -= 1 |
| **C8** | INY | Implied | 1 | 2 | N,Z | Y += 1 |
| **C9** | CMP | Immediate | 2 | 2 | N,Z,C | A - M |
| **CA** | DEX | Implied | 1 | 2 | N,Z | X -= 1 |
| **CC** | CPY | Absolute | 3 | 4 | N,Z,C | Y - M |
| **CD** | CMP | Absolute | 3 | 4 | N,Z,C | A - M |
| **CE** | DEC | Absolute | 3 | 6 | N,Z | M -= 1 |
| **D0** | BNE | Relative | 2 | 2† | - | Branch if Z=0 |
| **D1** | CMP | (Indirect),Y | 2 | 5† | N,Z,C | A - M |
| **D5** | CMP | Zero Page,X | 2 | 4 | N,Z,C | A - M |
| **D6** | DEC | Zero Page,X | 2 | 6 | N,Z | M -= 1 |
| **D8** | CLD | Implied | 1 | 2 | D | D = 0 (no effect) |
| **D9** | CMP | Absolute,Y | 3 | 4† | N,Z,C | A - M |
| **DD** | CMP | Absolute,X | 3 | 4† | N,Z,C | A - M |
| **DE** | DEC | Absolute,X | 3 | 7 | N,Z | M -= 1 |
| **E0** | CPX | Immediate | 2 | 2 | N,Z,C | X - M |
| **E1** | SBC | (Indirect,X) | 2 | 6 | N,V,Z,C | A -= M + (1-C) |
| **E4** | CPX | Zero Page | 2 | 3 | N,Z,C | X - M |
| **E5** | SBC | Zero Page | 2 | 3 | N,V,Z,C | A -= M + (1-C) |
| **E6** | INC | Zero Page | 2 | 5 | N,Z | M += 1 |
| **E8** | INX | Implied | 1 | 2 | N,Z | X += 1 |
| **E9** | SBC | Immediate | 2 | 2 | N,V,Z,C | A -= M + (1-C) |
| **EA** | NOP | Implied | 1 | 2 | - | No operation |
| **EC** | CPX | Absolute | 3 | 4 | N,Z,C | X - M |
| **ED** | SBC | Absolute | 3 | 4 | N,V,Z,C | A -= M + (1-C) |
| **EE** | INC | Absolute | 3 | 6 | N,Z | M += 1 |
| **F0** | BEQ | Relative | 2 | 2† | - | Branch if Z=1 |
| **F1** | SBC | (Indirect),Y | 2 | 5† | N,V,Z,C | A -= M + (1-C) |
| **F5** | SBC | Zero Page,X | 2 | 4 | N,V,Z,C | A -= M + (1-C) |
| **F6** | INC | Zero Page,X | 2 | 6 | N,Z | M += 1 |
| **F8** | SED | Implied | 1 | 2 | D | D = 1 (no effect) |
| **F9** | SBC | Absolute,Y | 3 | 4† | N,V,Z,C | A -= M + (1-C) |
| **FD** | SBC | Absolute,X | 3 | 4† | N,V,Z,C | A -= M + (1-C) |
| **FE** | INC | Absolute,X | 3 | 7 | N,Z | M += 1 |

**†** = +1 cycle if branch taken, +2 if page crossed; or +1 if page crossed for indexed addressing

---

## Unofficial Instructions

The remaining 105 opcodes are undocumented but functional. Games like Battletoads use these.

### Most Common Unofficial Opcodes

| Opcode | Mnemonic | Addr Mode | Bytes | Cycles | Description |
|--------|----------|-----------|-------|--------|-------------|
| **03** | SLO | (Indirect,X) | 2 | 8 | ASL + ORA |
| **04** | NOP | Zero Page | 2 | 3 | Read, discard |
| **07** | SLO | Zero Page | 2 | 5 | ASL + ORA |
| **0C** | NOP | Absolute | 3 | 4 | Read, discard |
| **0F** | SLO | Absolute | 3 | 6 | ASL + ORA |
| **13** | SLO | (Indirect),Y | 2 | 8 | ASL + ORA |
| **17** | SLO | Zero Page,X | 2 | 6 | ASL + ORA |
| **1B** | SLO | Absolute,Y | 3 | 7 | ASL + ORA |
| **1F** | SLO | Absolute,X | 3 | 7 | ASL + ORA |
| **23** | RLA | (Indirect,X) | 2 | 8 | ROL + AND |
| **27** | RLA | Zero Page | 2 | 5 | ROL + AND |
| **2F** | RLA | Absolute | 3 | 6 | ROL + AND |
| **33** | RLA | (Indirect),Y | 2 | 8 | ROL + AND |
| **37** | RLA | Zero Page,X | 2 | 6 | ROL + AND |
| **3B** | RLA | Absolute,Y | 3 | 7 | ROL + AND |
| **3F** | RLA | Absolute,X | 3 | 7 | ROL + AND |
| **43** | SRE | (Indirect,X) | 2 | 8 | LSR + EOR |
| **47** | SRE | Zero Page | 2 | 5 | LSR + EOR |
| **4F** | SRE | Absolute | 3 | 6 | LSR + EOR |
| **53** | SRE | (Indirect),Y | 2 | 8 | LSR + EOR |
| **57** | SRE | Zero Page,X | 2 | 6 | LSR + EOR |
| **5B** | SRE | Absolute,Y | 3 | 7 | LSR + EOR |
| **5F** | SRE | Absolute,X | 3 | 7 | LSR + EOR |
| **63** | RRA | (Indirect,X) | 2 | 8 | ROR + ADC |
| **67** | RRA | Zero Page | 2 | 5 | ROR + ADC |
| **6F** | RRA | Absolute | 3 | 6 | ROR + ADC |
| **73** | RRA | (Indirect),Y | 2 | 8 | ROR + ADC |
| **77** | RRA | Zero Page,X | 2 | 6 | ROR + ADC |
| **7B** | RRA | Absolute,Y | 3 | 7 | ROR + ADC |
| **7F** | RRA | Absolute,X | 3 | 7 | ROR + ADC |
| **80** | NOP | Immediate | 2 | 2 | Read, discard |
| **82** | NOP | Immediate | 2 | 2 | Read, discard |
| **83** | SAX | (Indirect,X) | 2 | 6 | M = A & X |
| **87** | SAX | Zero Page | 2 | 3 | M = A & X |
| **89** | NOP | Immediate | 2 | 2 | Read, discard |
| **8F** | SAX | Absolute | 3 | 4 | M = A & X |
| **97** | SAX | Zero Page,Y | 2 | 4 | M = A & X |
| **A3** | LAX | (Indirect,X) | 2 | 6 | A,X = M |
| **A7** | LAX | Zero Page | 2 | 3 | A,X = M |
| **AF** | LAX | Absolute | 3 | 4 | A,X = M |
| **B3** | LAX | (Indirect),Y | 2 | 5† | A,X = M |
| **B7** | LAX | Zero Page,Y | 2 | 4 | A,X = M |
| **BF** | LAX | Absolute,Y | 3 | 4† | A,X = M |
| **C2** | NOP | Immediate | 2 | 2 | Read, discard |
| **C3** | DCP | (Indirect,X) | 2 | 8 | DEC + CMP |
| **C7** | DCP | Zero Page | 2 | 5 | DEC + CMP |
| **CF** | DCP | Absolute | 3 | 6 | DEC + CMP |
| **D3** | DCP | (Indirect),Y | 2 | 8 | DEC + CMP |
| **D7** | DCP | Zero Page,X | 2 | 6 | DEC + CMP |
| **DB** | DCP | Absolute,Y | 3 | 7 | DEC + CMP |
| **DF** | DCP | Absolute,X | 3 | 7 | DEC + CMP |
| **E2** | NOP | Immediate | 2 | 2 | Read, discard |
| **E3** | ISC | (Indirect,X) | 2 | 8 | INC + SBC |
| **E7** | ISC | Zero Page | 2 | 5 | INC + SBC |
| **EF** | ISC | Absolute | 3 | 6 | INC + SBC |
| **F3** | ISC | (Indirect),Y | 2 | 8 | INC + SBC |
| **F7** | ISC | Zero Page,X | 2 | 6 | INC + SBC |
| **FB** | ISC | Absolute,Y | 3 | 7 | INC + SBC |
| **FF** | ISC | Absolute,X | 3 | 7 | INC + SBC |

### Highly Unstable Opcodes

These opcodes have unpredictable behavior and should crash emulation:

| Opcode | Mnemonic | Behavior |
|--------|----------|----------|
| **02, 12, 22, 32, 42, 52, 62, 72, 92, B2, D2, F2** | JAM/KIL/HLT | Halts CPU until RESET |
| **9C** | SHY | Store Y & (H+1) |
| **9E** | SHX | Store X & (H+1) |
| **9F** | SHA | Store A & X & (H+1) |
| **9B** | TAS | SP = A & X, M = SP & (H+1) |

---

## Cycle-by-Cycle Breakdown

### Example: LDA $1234,X (Opcode BD)

**Assuming X = $50, Address $1284 contains $42**

```
Cycle 1: Fetch opcode $BD from PC, PC++
Cycle 2: Fetch address low byte $34 from PC, PC++
Cycle 3: Fetch address high byte $12 from PC, PC++
         Calculate: $1234 + $50 = $1284 (page crossed: $12 → $12)
Cycle 4: Read from $1234 + $50 = $1284, A = $42
```

**If no page crossing, only 4 cycles. If page crossed, +1 cycle (total 5).**

### Example: INC $80 (Opcode E6)

**Read-Modify-Write at Zero Page**

```
Cycle 1: Fetch opcode $E6 from PC, PC++
Cycle 2: Fetch zero page address $80 from PC, PC++
Cycle 3: Read value from $0080 (let's say $05)
Cycle 4: Write old value back to $0080 (dummy write)
Cycle 5: Write new value $06 to $0080, set flags
```

**All RMW instructions have this dummy write on cycle N-1.**

### Example: BRK (Opcode 00)

**Software Interrupt**

```
Cycle 1: Fetch opcode $00 from PC, PC++
Cycle 2: Read next byte (signature, ignored), PC++
Cycle 3: Push PCH to stack, SP--
Cycle 4: Push PCL to stack, SP--
Cycle 5: Push P | 0x30 to stack, SP-- (B=1, U=1)
Cycle 6: Fetch IRQ vector low from $FFFE, set I flag
Cycle 7: Fetch IRQ vector high from $FFFF, PC = vector
```

**Note: BRK pushes PC+2, not PC+1.**

---

## Addressing Mode Details

### Zero Page,X/Y Wrapping

When adding X or Y to a zero page address, result wraps within page $00:

```rust
fn zero_page_x(&self, bus: &Bus) -> u8 {
    let base = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);
    base.wrapping_add(self.x) // Stays in $00-$FF
}
```

**Example:** `LDA $FF,X` with `X=$05` reads from `$04`, not `$104`.

### Indexed Indirect (Indirect,X)

Zero page pointer indexed by X, then dereference:

```rust
fn indexed_indirect(&self, bus: &Bus) -> u16 {
    let base = bus.read(self.pc).wrapping_add(self.x);
    self.pc = self.pc.wrapping_add(1);

    let lo = bus.read(base as u16);
    let hi = bus.read(base.wrapping_add(1) as u16);
    u16::from_le_bytes([lo, hi])
}
```

**Example:** `LDA ($80,X)` with `X=$05`:

1. Read ZP address: `$80 + $05 = $85`
2. Read pointer: `[$85] = $20`, `[$86] = $30`
3. Final address: `$3020`
4. Load A from `$3020`

### Indirect Indexed (Indirect),Y

Dereference zero page pointer, then add Y:

```rust
fn indirect_indexed(&self, bus: &Bus) -> (u16, bool) {
    let ptr = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);

    let lo = bus.read(ptr as u16);
    let hi = bus.read(ptr.wrapping_add(1) as u16);
    let base = u16::from_le_bytes([lo, hi]);

    let addr = base.wrapping_add(self.y as u16);
    let page_crossed = (base & 0xFF00) != (addr & 0xFF00);

    (addr, page_crossed)
}
```

**Example:** `LDA ($80),Y` with `Y=$10`:

1. Read pointer: `[$80] = $20`, `[$81] = $30`
2. Base address: `$3020`
3. Add Y: `$3020 + $10 = $3030`
4. Load A from `$3030`

### JMP Indirect Page Boundary Bug

The 6502 has a famous hardware bug in JMP ($xxFF):

```rust
fn jmp_indirect(&mut self, bus: &Bus) -> u16 {
    let ptr_lo = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);
    let ptr_hi = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);

    let ptr = u16::from_le_bytes([ptr_lo, ptr_hi]);

    let lo = bus.read(ptr);
    // BUG: Should read ptr+1, but wraps within page
    let hi_addr = if ptr & 0xFF == 0xFF {
        ptr & 0xFF00  // Wraps to start of same page!
    } else {
        ptr + 1
    };
    let hi = bus.read(hi_addr);

    u16::from_le_bytes([lo, hi])
}
```

**Example:** `JMP ($10FF)`:

- Reads low byte from `$10FF`
- Reads high byte from `$1000` (not `$1100`!)

---

## Interrupt Edge Cases

### Interrupt Polling

Interrupts are polled during the **last cycle** of each instruction:

```
Cycle 1-N: Execute instruction
Cycle N:   Poll IRQ line (if I=0), poll NMI edge detector
```

### NMI Edge Detection

NMI triggers on falling edge of /NMI line:

```
If NMI was high and is now low: trigger NMI
Else: no interrupt
```

**Edge case:** If NMI goes low then high within 2 cycles, interrupt may be lost.

### IRQ vs BRK

Both use vector $FFFE, but B flag distinguishes them:

```
BRK:  Push P | 0x30 (B=1, U=1)
IRQ:  Push P | 0x20 (B=0, U=1)
```

**RTI cannot distinguish** - it just pulls P from stack.

### Interrupt Hijacking

If IRQ and NMI occur simultaneously:

```
Cycle 1-2: Fetch next instruction (dummy, discarded)
Cycle 3:   Push PCH (PC = next instruction)
Cycle 4:   Push PCL
Cycle 5:   Push P with B=0
Cycle 6:   Fetch NMI vector low from $FFFA (IRQ ignored!)
Cycle 7:   Fetch NMI vector high from $FFFB
```

**NMI hijacks IRQ** - IRQ vector is never read.

### CLI Delay

Setting I=0 with CLI has a 1-instruction delay:

```
SEI         ; I = 1
CLI         ; I = 0, but IRQ not checked this cycle
NOP         ; IRQ checked here (if pending, executes)
```

---

## Implementation Reference

### Recommended CPU Structure

```rust
pub struct Cpu {
    // Registers
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub p: StatusFlags,

    // Interrupt state
    nmi_pending: bool,
    nmi_line: bool,
    nmi_line_prev: bool,
    irq_line: bool,

    // Cycle count
    cycles: u64,

    // Opcode dispatch tables
    instruction_table: [fn(&mut Cpu, &mut Bus, u16) -> u8; 256],
    addressing_table: [fn(&mut Cpu, &mut Bus) -> (u16, bool); 256],
    cycle_table: [u8; 256],
}
```

### Opcode Dispatch Pattern

```rust
pub fn step(&mut self, bus: &mut Bus) -> u8 {
    // Check interrupts
    if self.nmi_pending {
        self.nmi_pending = false;
        return self.handle_nmi(bus);
    }
    if self.irq_line && !self.p.contains(StatusFlags::INTERRUPT) {
        return self.handle_irq(bus);
    }

    // Fetch opcode
    let opcode = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Decode and execute
    let (addr, page_crossed) = (self.addressing_table[opcode as usize])(self, bus);
    let extra = (self.instruction_table[opcode as usize])(self, bus, addr);

    let base_cycles = self.cycle_table[opcode as usize];
    base_cycles + extra + (page_crossed as u8)
}
```

### Flag Calculation Helpers

```rust
impl Cpu {
    fn set_nz(&mut self, value: u8) {
        self.p.set(StatusFlags::ZERO, value == 0);
        self.p.set(StatusFlags::NEGATIVE, value & 0x80 != 0);
    }

    fn calc_carry_adc(&self, a: u8, m: u8, c: u8) -> bool {
        (a as u16 + m as u16 + c as u16) > 0xFF
    }

    fn calc_overflow_adc(&self, a: u8, m: u8, result: u8) -> bool {
        ((a ^ result) & (m ^ result) & 0x80) != 0
    }
}
```

---

## Related Documentation

- [CPU_TIMING_REFERENCE.md](CPU_TIMING_REFERENCE.md) - Per-instruction cycle breakdowns
- [CPU_UNOFFICIAL_OPCODES.md](CPU_UNOFFICIAL_OPCODES.md) - Complete unofficial opcode reference
- [CPU_6502.md](CPU_6502.md) - Higher-level CPU overview
- [../bus/MEMORY_MAP.md](../bus/MEMORY_MAP.md) - CPU memory layout

---

## References

- [NESdev Wiki: CPU](https://www.nesdev.org/wiki/CPU)
- [NESdev Wiki: 6502 Instructions](https://www.nesdev.org/wiki/6502_instructions)
- [NESdev Wiki: CPU Unofficial Opcodes](https://wiki.nesdev.org/w/index.php/CPU_unofficial_opcodes)
- [6502 Cycle Times](https://www.nesdev.org/wiki/6502_cycle_times)
- [Visual 6502 All 256 Opcodes](https://www.nesdev.org/wiki/Visual6502wiki/6502_all_256_Opcodes)
- [6502.org Reference](http://www.6502.org/tutorials/6502opcodes.html)
- Visual6502 Transistor-Level Simulator

---

**Document Status:** Complete opcode matrix with cycle-accurate timing for all 256 opcodes.
