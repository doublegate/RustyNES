# [Milestone 1] Sprint 1: CPU Core Structure

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 2025
**Assignee:** Claude Code / Developer

---

## Overview

Implemented the foundational CPU structure including all registers, status flags, bus interface, and basic power-on/reset behavior. This sprint establishes the skeleton for all CPU operations.

---

## Acceptance Criteria

- [x] CPU struct with all 6502 registers
- [x] Status flags using bitflags
- [x] Bus trait for memory access
- [x] Power-on initialization
- [x] RESET sequence implementation
- [x] Zero unsafe code
- [x] Unit tests for basic operations

---

## Tasks

### Task 1: Create CPU Structure

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Create the main CPU struct with all registers matching 6502 hardware.

**Files:**

- `crates/rustynes-cpu/src/cpu.rs` - Main CPU structure
- `crates/rustynes-cpu/src/lib.rs` - Public exports

**Subtasks:**

- [x] Define CPU struct with registers (A, X, Y, PC, SP, P)
- [x] Add cycle counter
- [x] Add stall mechanism for DMA
- [x] Add interrupt pending flags
- [x] Add jam flag for halt opcodes
- [x] Implement Debug trait

**Implementation:**

```rust
pub struct Cpu {
    pub a: u8,              // Accumulator
    pub x: u8,              // X index
    pub y: u8,              // Y index
    pub pc: u16,            // Program counter
    pub sp: u8,             // Stack pointer
    pub status: StatusFlags, // Status register
    pub cycles: u64,        // Total cycles
    pub stall: u8,          // DMA stall cycles
    nmi_pending: bool,      // NMI flag
    irq_pending: bool,      // IRQ flag
    pub jammed: bool,       // CPU halted
}
```

---

### Task 2: Implement Status Flags

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 1 hour
- **Actual:** ~1 hour

**Description:**
Implement processor status flags using bitflags crate for type-safe manipulation.

**Files:**

- `crates/rustynes-cpu/src/status.rs` - Status flags definition

**Subtasks:**

- [x] Define StatusFlags using bitflags
- [x] Implement all 6 flags (C, Z, I, D, V, N)
- [x] Add U flag (always set)
- [x] Add B flag (BRK instruction)
- [x] Implement from_bits methods
- [x] Unit tests for flag operations

**Implementation:**

```rust
bitflags! {
    pub struct StatusFlags: u8 {
        const CARRY            = 0b0000_0001;
        const ZERO             = 0b0000_0010;
        const INTERRUPT_DISABLE= 0b0000_0100;
        const DECIMAL          = 0b0000_1000; // Unused on NES
        const BREAK            = 0b0001_0000; // B flag
        const UNUSED           = 0b0010_0000; // Always set
        const OVERFLOW         = 0b0100_0000;
        const NEGATIVE         = 0b1000_0000;
    }
}
```

---

### Task 3: Define Bus Trait

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 1 hour
- **Actual:** ~1 hour

**Description:**
Create abstraction for memory access allowing flexible memory system implementations.

**Files:**

- `crates/rustynes-cpu/src/bus.rs` - Bus trait definition

**Subtasks:**

- [x] Define Bus trait with read/write methods
- [x] Add read_u16 helper for 16-bit reads
- [x] Document trait requirements
- [x] Provide example implementation

**Implementation:**

```rust
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}
```

---

### Task 4: Power-On State

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 1 hour
- **Actual:** ~0.5 hours

**Description:**
Implement CPU power-on initialization matching hardware behavior.

**Files:**

- `crates/rustynes-cpu/src/cpu.rs` - new() method

**Subtasks:**

- [x] Set registers to power-on state
- [x] SP = $FD (after RESET pulls 3 bytes)
- [x] P = $24 (I and U flags set)
- [x] PC = 0 (will be set by RESET)
- [x] A, X, Y = 0 (undefined, but we use 0)

**Implementation:**

```rust
pub fn new() -> Self {
    Self {
        a: 0,
        x: 0,
        y: 0,
        pc: 0,
        sp: 0xFD,
        status: StatusFlags::from_bits_truncate(0x24),
        cycles: 0,
        stall: 0,
        nmi_pending: false,
        irq_pending: false,
        jammed: false,
    }
}
```

---

### Task 5: RESET Sequence

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 2 hours
- **Actual:** ~1.5 hours

**Description:**
Implement RESET interrupt sequence matching hardware timing and behavior.

**Files:**

- `crates/rustynes-cpu/src/cpu.rs` - reset() method

**Subtasks:**

- [x] Decrement SP by 3 (no stack writes)
- [x] Set I flag (disable interrupts)
- [x] Load PC from RESET vector ($FFFC-$FFFD)
- [x] Takes 7 cycles
- [x] Clear pending interrupts
- [x] Clear jammed flag

**Implementation:**

```rust
pub fn reset(&mut self, bus: &mut impl Bus) {
    self.sp = self.sp.wrapping_sub(3);
    self.status.insert(StatusFlags::INTERRUPT_DISABLE);
    self.pc = bus.read_u16(0xFFFC);
    self.cycles += 7;
    self.nmi_pending = false;
    self.irq_pending = false;
    self.jammed = false;
}
```

---

### Task 6: Unit Tests

- **Status:** ✅ Done
- **Priority:** Medium
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Create unit tests for power-on state and RESET behavior.

**Files:**

- `crates/rustynes-cpu/src/lib.rs` - Test module

**Subtasks:**

- [x] Test power-on register values
- [x] Test RESET vector loading
- [x] Test RESET flag changes
- [x] Test RESET cycle count
- [x] Create TestBus helper

**Tests Created:**

- Basic CPU initialization
- RESET sequence validation
- Status flag operations

---

## Dependencies

**Required:**

- Rust 1.75+ toolchain
- bitflags = "2.4" crate
- thiserror = "1.0" crate
- log = "0.4" crate

**Blocks:**

- Sprint 2: Opcode Implementation (needs CPU structure)
- Sprint 3: Addressing Modes (needs register access)

---

## Related Documentation

- [CPU 6502 Specification](../../../docs/cpu/CPU_6502_SPECIFICATION.md)
- [CPU Timing Reference](../../../docs/cpu/CPU_TIMING_REFERENCE.md)

---

## Commits

- `506a810` - feat(cpu): implement complete cycle-accurate 6502 CPU emulation
- `f977a97` - feat(cpu): implement complete cycle-accurate 6502 CPU emulation

---

## Retrospective

### What Went Well

- Clean, idiomatic Rust code
- bitflags made status flag handling elegant
- Bus trait provides good abstraction

### What Could Be Improved

- Could have added more documentation upfront
- Property-based testing for flag operations

### Lessons Learned

- Strong typing catches bugs early
- Trait-based design enables flexibility
- Following hardware spec exactly simplifies implementation
