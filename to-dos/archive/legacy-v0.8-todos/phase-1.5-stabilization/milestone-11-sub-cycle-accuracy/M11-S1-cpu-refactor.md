# M11-S1: CPU Cycle-by-Cycle Refactor

**Sprint:** S1 (CPU Refactor)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 3-4 weeks (40-60 hours)
**Status:** PLANNED
**Priority:** CRITICAL - Foundation for all other work

---

## Overview

Refactor the CPU from atomic instruction execution to cycle-by-cycle execution where each memory access triggers an `on_cpu_cycle()` callback to synchronize PPU and APU.

---

## Current State Analysis

### Atomic Execution Problem

```rust
// Current: Entire instruction executes without PPU/APU callbacks
pub fn step(&mut self, bus: &mut impl CpuBus) -> u8 {
    let opcode = bus.read(self.pc);  // No callback
    self.pc = self.pc.wrapping_add(1);
    let cycles = self.execute_opcode(opcode, bus);  // Multiple reads/writes, no callbacks
    cycles
}
```

### Required Solution

```rust
// Target: Each memory access triggers callback
pub fn step(&mut self, bus: &mut impl CpuBus) -> u8 {
    let opcode = self.read_cycle(bus, self.pc);  // on_cpu_cycle() called
    self.pc = self.pc.wrapping_add(1);
    self.execute_opcode(opcode, bus)  // All reads/writes use read_cycle/write_cycle
}

#[inline]
fn read_cycle(&mut self, bus: &mut impl CpuBus, addr: u16) -> u8 {
    bus.on_cpu_cycle();  // PPU steps 3x, APU steps 1x
    bus.read(addr)
}
```

---

## Tasks

### S1.1: CpuBus Trait Enhancement

**Effort:** 1 hour
**Files:** `crates/rustynes-cpu/src/lib.rs`

Add `on_cpu_cycle()` method to CpuBus trait:

```rust
pub trait CpuBus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    fn on_cpu_cycle(&mut self);  // NEW
    fn poll_nmi(&mut self) -> bool;
    fn poll_irq(&mut self) -> bool;
}
```

- [ ] Add method signature to trait
- [ ] Update default implementation (if any)
- [ ] Document timing semantics

---

### S1.2: CPU Cycle Methods

**Effort:** 2 hours
**Files:** `crates/rustynes-cpu/src/cpu.rs`

Create cycle-aware read/write methods:

```rust
impl Cpu {
    /// Read with cycle callback - use for all instruction memory accesses
    #[inline]
    pub fn read_cycle(&mut self, bus: &mut impl CpuBus, addr: u16) -> u8 {
        bus.on_cpu_cycle();
        bus.read(addr)
    }

    /// Write with cycle callback - use for all instruction memory accesses
    #[inline]
    pub fn write_cycle(&mut self, bus: &mut impl CpuBus, addr: u16, val: u8) {
        bus.on_cpu_cycle();
        bus.write(addr, val)
    }

    /// Dummy cycle (e.g., page boundary, taken branch)
    #[inline]
    pub fn dummy_cycle(&mut self, bus: &mut impl CpuBus) {
        bus.on_cpu_cycle();
    }
}
```

- [ ] Add `read_cycle()` method
- [ ] Add `write_cycle()` method
- [ ] Add `dummy_cycle()` method
- [ ] Add documentation

---

### S1.3: Official Opcode Refactoring (151 opcodes)

**Effort:** 20-25 hours
**Files:** `crates/rustynes-cpu/src/opcodes.rs`

Refactor each opcode to use cycle methods. Example:

**Before:**
```rust
fn lda_absolute(&mut self, bus: &mut impl CpuBus) -> u8 {
    let lo = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);
    let hi = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);
    let addr = u16::from_le_bytes([lo, hi]);
    self.a = bus.read(addr);
    self.update_nz_flags(self.a);
    4
}
```

**After:**
```rust
fn lda_absolute(&mut self, bus: &mut impl CpuBus) -> u8 {
    // Cycle 2: Read low byte
    let lo = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Cycle 3: Read high byte
    let hi = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Cycle 4: Read value
    let addr = u16::from_le_bytes([lo, hi]);
    self.a = self.read_cycle(bus, addr);

    self.update_nz_flags(self.a);
    4
}
```

**Opcode Categories:**

| Category | Count | Effort |
|----------|-------|--------|
| Load (LDA, LDX, LDY) | 13 | 2h |
| Store (STA, STX, STY) | 12 | 2h |
| Transfer (TAX, TXA, etc.) | 6 | 0.5h |
| Stack (PHA, PLA, PHP, PLP) | 4 | 0.5h |
| Arithmetic (ADC, SBC) | 16 | 2h |
| Logic (AND, ORA, EOR) | 24 | 3h |
| Compare (CMP, CPX, CPY) | 11 | 1.5h |
| Increment/Decrement | 10 | 1.5h |
| Shifts (ASL, LSR, ROL, ROR) | 20 | 3h |
| Jumps (JMP, JSR, RTS, RTI) | 7 | 2h |
| Branches (BCC, BCS, etc.) | 8 | 2h |
| Flags (SEC, CLC, etc.) | 7 | 0.5h |
| System (BRK, NOP) | 13 | 1h |
| **Total Official** | **151** | **~21h** |

- [ ] Load instructions (13)
- [ ] Store instructions (12)
- [ ] Transfer instructions (6)
- [ ] Stack instructions (4)
- [ ] Arithmetic instructions (16)
- [ ] Logic instructions (24)
- [ ] Compare instructions (11)
- [ ] Increment/Decrement (10)
- [ ] Shift/Rotate (20)
- [ ] Jump/Call/Return (7)
- [ ] Branch instructions (8)
- [ ] Flag instructions (7)
- [ ] System instructions (13)

---

### S1.4: Unofficial Opcode Refactoring (105 opcodes)

**Effort:** 10-15 hours
**Files:** `crates/rustynes-cpu/src/opcodes.rs`

Refactor unofficial opcodes following same pattern:

| Category | Count | Effort |
|----------|-------|--------|
| LAX (LDA+LDX) | 8 | 1h |
| SAX (STA&STX) | 4 | 0.5h |
| DCP (DEC+CMP) | 8 | 1h |
| ISC (INC+SBC) | 8 | 1h |
| SLO (ASL+ORA) | 8 | 1h |
| RLA (ROL+AND) | 8 | 1h |
| SRE (LSR+EOR) | 8 | 1h |
| RRA (ROR+ADC) | 8 | 1h |
| ANC, ALR, ARR, etc. | 45+ | 4-6h |
| **Total Unofficial** | **105** | **~12h** |

- [ ] LAX variants
- [ ] SAX variants
- [ ] DCP variants
- [ ] ISC variants
- [ ] SLO variants
- [ ] RLA variants
- [ ] SRE variants
- [ ] RRA variants
- [ ] Remaining unofficial opcodes

---

### S1.5: Page Boundary Cycle Handling

**Effort:** 3 hours
**Files:** `crates/rustynes-cpu/src/opcodes.rs`, `crates/rustynes-cpu/src/addressing.rs`

Handle +1 cycle for page boundary crossing:

```rust
fn lda_absolute_x(&mut self, bus: &mut impl CpuBus) -> u8 {
    // Cycle 2: Read low byte
    let lo = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Cycle 3: Read high byte
    let hi = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    let base = u16::from_le_bytes([lo, hi]);
    let addr = base.wrapping_add(self.x as u16);

    // Check page crossing
    let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
    if page_crossed {
        // Cycle 4: Dummy read at wrong address
        self.dummy_cycle(bus);
    }

    // Cycle 4/5: Read value
    self.a = self.read_cycle(bus, addr);

    self.update_nz_flags(self.a);
    if page_crossed { 5 } else { 4 }
}
```

- [ ] Absolute,X addressing mode
- [ ] Absolute,Y addressing mode
- [ ] Indirect,Y addressing mode
- [ ] Unit tests for page crossing

---

### S1.6: Branch Cycle Handling

**Effort:** 2 hours
**Files:** `crates/rustynes-cpu/src/opcodes.rs`

Handle +1/+2 cycles for branches:

```rust
fn bcc(&mut self, bus: &mut impl CpuBus) -> u8 {
    // Cycle 2: Read offset
    let offset = self.read_cycle(bus, self.pc) as i8;
    self.pc = self.pc.wrapping_add(1);

    if !self.get_flag(Flags::CARRY) {
        // Branch taken: +1 cycle
        self.dummy_cycle(bus);

        let old_pc = self.pc;
        self.pc = self.pc.wrapping_add(offset as u16);

        // Page crossing: +1 more cycle
        if (old_pc & 0xFF00) != (self.pc & 0xFF00) {
            self.dummy_cycle(bus);
            return 4;
        }
        return 3;
    }

    2  // Branch not taken
}
```

- [ ] BCC, BCS, BEQ, BNE
- [ ] BMI, BPL, BVC, BVS
- [ ] Unit tests for branch timing

---

### S1.7: Cycle Count Verification

**Effort:** 2 hours
**Files:** Test files

Verify all opcodes match NESdev timing:

| Instruction | NESdev Cycles | Verified |
|-------------|---------------|----------|
| LDA immediate | 2 | [ ] |
| LDA zeropage | 3 | [ ] |
| LDA absolute | 4 | [ ] |
| LDA absolute,X | 4-5 | [ ] |
| ... | ... | ... |

- [ ] Create cycle count verification tests
- [ ] Compare against NESdev wiki table
- [ ] Document any discrepancies

---

### S1.8: Unit Test Updates

**Effort:** 3 hours
**Files:** `crates/rustynes-cpu/tests/*.rs`

Update tests for new bus interface:

```rust
struct MockBus {
    memory: [u8; 65536],
    cycle_count: usize,
}

impl CpuBus for MockBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.memory[addr as usize]
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.memory[addr as usize] = val;
    }

    fn on_cpu_cycle(&mut self) {
        self.cycle_count += 1;
    }
    // ...
}

#[test]
fn test_lda_absolute_cycles() {
    let mut cpu = Cpu::new();
    let mut bus = MockBus::new();

    // LDA $1234
    bus.write(0x0000, 0xAD);  // LDA absolute
    bus.write(0x0001, 0x34);
    bus.write(0x0002, 0x12);
    bus.write(0x1234, 0x42);

    let cycles = cpu.step(&mut bus);

    assert_eq!(cycles, 4);
    assert_eq!(bus.cycle_count, 4);  // Verify callbacks
    assert_eq!(cpu.a, 0x42);
}
```

- [ ] Create MockBus with cycle counting
- [ ] Update existing tests
- [ ] Add cycle count assertions
- [ ] Run full test suite

---

## Acceptance Criteria

- [ ] All 256 opcodes use `read_cycle()`/`write_cycle()`
- [ ] Each memory access triggers exactly one `on_cpu_cycle()` call
- [ ] Cycle counts match NESdev timing exactly
- [ ] nestest.nes still passes 100%
- [ ] All existing unit tests pass
- [ ] Performance regression < 10%

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Off-by-one cycle errors | Verify each opcode against NESdev wiki |
| Performance regression | Profile before/after, optimize hot paths |
| Breaking existing tests | Run full test suite after each opcode category |
| Missing edge cases | Reference Pinky and Mesen2 implementations |

---

## Dependencies

- **Requires:** None (foundation sprint)
- **Blocks:** S2 (PPU Sync), S3 (APU), S4 (DMA), S5 (Mappers)

---

**Status:** PLANNED
**Created:** 2025-12-28
