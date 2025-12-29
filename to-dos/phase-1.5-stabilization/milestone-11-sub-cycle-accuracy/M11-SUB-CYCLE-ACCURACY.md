# Milestone 11: Sub-Cycle Accuracy Implementation

**Milestone:** M11 (Sub-Cycle Accuracy)
**Phase:** 1.5 (Stabilization & Accuracy) -> Phase 2 Bridge
**Duration:** ~8-12 weeks (Estimated 100-150 hours)
**Status:** PLANNED
**Version Target:** v1.0.0
**Priority:** Critical - Architectural Foundation
**Progress:** 0%

---

## Table of Contents

- [Executive Summary](#executive-summary)
- [Audit Report](#audit-report)
- [Architecture Recommendations](#architecture-recommendations)
- [Sprint Breakdown](#sprint-breakdown)
- [Test Validation Targets](#test-validation-targets)
- [Effort Estimates](#effort-estimates)
- [Risk Assessment](#risk-assessment)
- [Dependencies](#dependencies)
- [References](#references)

---

## Executive Summary

### Goal

Achieve **100% sub-cycle level hardware emulation accuracy**, surpassing even Mesen2, by implementing cycle-by-cycle CPU execution with per-memory-access PPU/APU synchronization.

### Current State

RustyNES v0.8.4 achieves:
- 100% Blargg pass rate (90/90 tests)
- 517+ unit tests passing
- Dot-level PPU accuracy
- Cycle-level APU frame counter timing

### Gap Analysis

Two critical timing tests remain **ignored** due to architectural limitations:
- `ppu_02-vbl_set_time` - Requires +/-2 cycle VBlank flag precision
- `ppu_03-vbl_clear_time` - Requires +/-2 cycle VBlank clear precision

### Root Cause

**CPU instructions execute atomically** rather than cycle-by-cycle. PPU is stepped before each CPU instruction, not before **each memory access** within an instruction.

### Solution

Implement Pinky's `on_cpu_cycle()` callback pattern:
- PPU stepped 3 times before each CPU memory access
- APU stepped once before each CPU memory access
- DMA integrated into cycle-by-cycle execution

---

## Audit Report

### Component Status Overview

| Component | Current State | Sub-Cycle Ready | Effort |
|-----------|---------------|-----------------|--------|
| **CPU** | Atomic instruction execution | NO | High (40-60h) |
| **PPU** | Dot-level stepping | YES | Low (5-10h) |
| **APU** | Frame counter accurate | PARTIAL | Medium (10-15h) |
| **Bus** | DMA cycle tracking | PARTIAL | Medium (15-20h) |
| **Console** | Pre-steps PPU before CPU | PARTIAL | Medium (10-15h) |
| **Mappers** | Clocked after instruction | NO | Medium (15-20h) |

---

### CPU Audit (`crates/rustynes-cpu/`)

#### Current Implementation

```rust
// Current: Atomic instruction execution
pub fn step(&mut self, bus: &mut impl CpuBus) -> u8 {
    let opcode = bus.read(self.pc);
    self.pc = self.pc.wrapping_add(1);
    let cycles = self.execute_opcode(opcode, bus);
    cycles
}
```

#### Gap: No Per-Memory-Access Callbacks

The CPU performs all memory reads/writes within `execute_opcode()` without notifying PPU/APU. This means:
- A 7-cycle instruction reads $2002 on cycle 3
- PPU state reflects cycle 0, not cycle 3
- VBlank flag read may be incorrect by several cycles

#### Required Change

```rust
// Required: Bus trait with on_cpu_cycle() callback
pub trait CpuBus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    fn on_cpu_cycle(&mut self);  // NEW: Called before each memory access
}

// CPU implementation
fn read_and_tick(&mut self, bus: &mut impl CpuBus, addr: u16) -> u8 {
    bus.on_cpu_cycle();  // Step PPU 3x, APU 1x
    bus.read(addr)
}
```

#### CpuState Enum (Existing Infrastructure)

```rust
pub enum CpuState {
    ExecutingInstruction,
    HandleInterrupt,
    OamDmaTransfer { addr: u16, cycle: u16 },
    DmcDmaStall { stall_cycles: u8 },
}
```

This state machine exists but isn't utilized for cycle-by-cycle execution.

#### Files Requiring Changes

- `crates/rustynes-cpu/src/cpu.rs` - Core execution logic
- `crates/rustynes-cpu/src/lib.rs` - CpuBus trait
- `crates/rustynes-cpu/src/opcodes.rs` - All 256 opcode handlers

---

### PPU Audit (`crates/rustynes-ppu/`)

#### Current Implementation - GOOD

```rust
// PPU has dot-level stepping capability
pub fn step_with_chr(&mut self, chr_access: impl Fn(u16) -> u8) -> StepResult {
    // Advances one dot (pixel clock)
    // 341 dots per scanline, 262 scanlines per frame
}
```

#### Timing Module - GOOD

```rust
// timing.rs - Well-implemented state machine
pub struct Timing {
    scanline: u16,  // 0-261
    dot: u16,       // 0-340
    frame: u64,
}

// Key timing checks already implemented:
fn is_vblank_set_dot(&self) -> bool {
    self.scanline == 241 && self.dot == 1
}

fn is_vblank_clear_dot(&self) -> bool {
    self.scanline == 261 && self.dot == 1
}
```

#### Required Changes - MINIMAL

PPU infrastructure is ready. Only integration changes needed:
- Expose `step_with_chr()` for callback-based invocation
- Ensure VBlank flag reads return cycle-accurate state

---

### APU Audit (`crates/rustynes-apu/`)

#### Current Implementation - GOOD

```rust
// Frame counter with correct cycle values
fn clock_4step(&mut self) -> FrameAction {
    match self.cycle_count {
        7458 | 22373 => FrameAction::QuarterFrame,
        14914 => FrameAction::HalfFrame,
        29830 => { /* IRQ + HalfFrame */ }
        29831 | 29832 => { /* IRQ flags */ }
        _ => FrameAction::None,
    }
}

fn clock_5step(&mut self) -> FrameAction {
    match self.cycle_count {
        7458 | 22372 => FrameAction::QuarterFrame,
        14914 => FrameAction::HalfFrame,
        37282 => { /* HalfFrame + reset */ }
        _ => FrameAction::None,
    }
}
```

#### Required Changes

- Integrate APU stepping into `on_cpu_cycle()` callback
- DMC DMA cycle stealing needs precise integration
- Ensure $4015 status register reads are cycle-accurate

---

### Bus Audit (`crates/rustynes-core/src/bus.rs`)

#### Current Implementation

```rust
impl Bus {
    pub fn read(&mut self, addr: u16, ...) -> u8 {
        match addr {
            0x0000..=0x1FFF => /* RAM */,
            0x2000..=0x3FFF => /* PPU registers */,
            0x4000..=0x401F => /* APU/IO */,
            // ...
        }
    }
}
```

#### DMA Tracking - PARTIAL

```rust
// OAM DMA cycle tracking exists
pub fn start_oam_dma(&mut self, high_byte: u8) {
    self.dma_pending = true;
    self.dma_page = high_byte;
    self.dma_addr = 0;
    // 513-514 cycles based on CPU parity
}
```

#### Required Changes

- Implement `CpuBus::on_cpu_cycle()` method
- Integrate PPU stepping (3x per cycle)
- Integrate APU stepping (1x per cycle)
- Handle DMA within cycle-by-cycle framework

---

### Console Audit (`crates/rustynes-core/src/console.rs`)

#### Current Implementation

```rust
pub fn tick(&mut self) -> TickResult {
    // Current: Pre-step PPU before CPU instruction
    for _ in 0..3 {
        self.ppu.step_with_chr(/* ... */);
    }

    // Then execute CPU
    let cycles = self.cpu.step(&mut self.bus);

    // Catch up PPU/APU
    for _ in 1..cycles {
        for _ in 0..3 {
            self.ppu.step_with_chr(/* ... */);
        }
        self.apu.clock();
    }
}
```

#### Gap Analysis

PPU is stepped before the **instruction**, not before each **memory access** within the instruction. For a 7-cycle instruction:

```text
Current (Wrong):
  PPU step 3x → CPU reads opcode → CPU reads addr → CPU reads data → CPU writes result
                ^-- PPU state frozen for all memory accesses

Required (Correct):
  PPU step 3x → CPU reads opcode
  PPU step 3x → CPU reads addr
  PPU step 3x → CPU reads data
  PPU step 3x → CPU writes result
                ^-- PPU state updated before each access
```

---

### Reference Implementation: Pinky

#### Key Pattern (Rust)

```rust
// From ref-proj/pinky/nes/src/virtual_nes.rs
impl<C: Context> mos6502::Context for Orphan<C> {
    fn peek(&mut self, address: u16) -> u8 {
        self.as_mut().on_cpu_cycle();  // CRITICAL: Before EACH memory access
        dma::Interface::execute(self, address);
        Private::peek_memory(self.as_mut(), address)
    }

    fn poke(&mut self, address: u16, value: u8) {
        self.as_mut().on_cpu_cycle();  // CRITICAL: Before EACH memory access
        Private::poke_memory(self.as_mut(), address, value);
    }
}

fn on_cpu_cycle(&mut self) {
    self.state_mut().cpu_cycle.wrapping_inc();

    // Step APU once per CPU cycle
    virtual_apu::Interface::execute(self.newtype_mut());

    // Step PPU three times per CPU cycle (3:1 ratio)
    for _ in 0..3 {
        rp2c02::Interface::execute(self.newtype_mut());
    }

    Context::on_cycle(self);
}
```

#### Why This Works

1. **Memory Access = Callback**: Every CPU peek/poke triggers `on_cpu_cycle()`
2. **PPU Always Current**: PPU state is updated before each memory read
3. **$2002 Reads Accurate**: When CPU reads VBlank flag, PPU is at correct cycle
4. **Atomic Instruction Impossible**: No multi-cycle gaps in PPU/APU stepping

---

## Architecture Recommendations

### Recommended Architecture

```rust
// 1. Enhanced CpuBus Trait
pub trait CpuBus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    fn on_cpu_cycle(&mut self);  // NEW
    fn poll_nmi(&mut self) -> bool;
    fn poll_irq(&mut self) -> bool;
}

// 2. Bus Implementation
impl CpuBus for Bus {
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3 times (3:1 PPU:CPU ratio)
        for _ in 0..3 {
            self.ppu.step_with_chr(|addr| self.mapper.read_chr(addr));
        }

        // Step APU once
        self.apu.clock();

        // Clock mapper for IRQ timing
        self.mapper.clock(1);
    }

    fn read(&mut self, addr: u16) -> u8 {
        // Note: on_cpu_cycle() is NOT called here
        // It's called by CPU before invoking read()
        self.internal_read(addr)
    }
}

// 3. CPU Read/Write with Cycle Callback
impl Cpu {
    #[inline]
    fn read_cycle(&mut self, bus: &mut impl CpuBus, addr: u16) -> u8 {
        bus.on_cpu_cycle();  // PPU/APU step BEFORE read
        bus.read(addr)
    }

    #[inline]
    fn write_cycle(&mut self, bus: &mut impl CpuBus, addr: u16, val: u8) {
        bus.on_cpu_cycle();  // PPU/APU step BEFORE write
        bus.write(addr, val)
    }

    // Dummy cycle (e.g., page boundary penalty)
    #[inline]
    fn dummy_cycle(&mut self, bus: &mut impl CpuBus) {
        bus.on_cpu_cycle();
    }
}

// 4. Opcode Handler Example
fn lda_absolute(&mut self, bus: &mut impl CpuBus) -> u8 {
    // Cycle 1: Read opcode (already done in step())
    // Cycle 2: Read low byte of address
    let lo = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Cycle 3: Read high byte of address
    let hi = self.read_cycle(bus, self.pc);
    self.pc = self.pc.wrapping_add(1);

    // Cycle 4: Read value from address
    let addr = u16::from_le_bytes([lo, hi]);
    self.a = self.read_cycle(bus, addr);

    self.update_nz_flags(self.a);
    4  // Total cycles
}
```

### Alternative: Coroutine-Based (Future)

```rust
// Using Rust's async/generators (nightly feature)
async fn execute_instruction(&mut self, bus: &mut impl CpuBus) {
    let opcode = self.read_and_yield(bus, self.pc).await;
    // Each .await yields control, allowing PPU/APU step
}
```

This is more elegant but requires nightly Rust. Recommend explicit cycle callbacks for stable Rust compatibility.

---

## Sprint Breakdown

### Sprint 1: CPU Cycle-by-Cycle Refactor (40-60 hours)

**Duration:** 3-4 weeks
**Priority:** CRITICAL - Foundation for all other work

#### Tasks

- [ ] **S1.1** Add `on_cpu_cycle()` to CpuBus trait
- [ ] **S1.2** Create `read_cycle()` and `write_cycle()` CPU methods
- [ ] **S1.3** Refactor all 151 official opcodes to use cycle methods
- [ ] **S1.4** Refactor all 105 unofficial opcodes to use cycle methods
- [ ] **S1.5** Handle page boundary penalty cycles correctly
- [ ] **S1.6** Handle branch taken/not-taken cycle differences
- [ ] **S1.7** Verify all instruction cycle counts match NESdev wiki
- [ ] **S1.8** Update unit tests for new bus interface

#### Acceptance Criteria

- All 256 opcodes use `read_cycle()`/`write_cycle()`
- Each memory access triggers one `on_cpu_cycle()` call
- Cycle counts match NESdev timing exactly
- nestest.nes still passes 100%
- Zero performance regression >10%

#### Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| `cpu/src/lib.rs` | Add `on_cpu_cycle()` to trait | 1h |
| `cpu/src/cpu.rs` | Add cycle methods, refactor execution | 8h |
| `cpu/src/opcodes.rs` | Refactor all 256 handlers | 30-40h |
| `cpu/src/addressing.rs` | Refactor addressing mode helpers | 5h |
| `cpu/tests/*.rs` | Update tests for new interface | 3h |

---

### Sprint 2: PPU Synchronization Integration (5-10 hours)

**Duration:** 1 week
**Priority:** HIGH - Required for VBlank timing accuracy

#### Tasks

- [ ] **S2.1** Implement `on_cpu_cycle()` in Bus to step PPU 3x
- [ ] **S2.2** Ensure VBlank flag ($2002 bit 7) is cycle-accurate
- [ ] **S2.3** Ensure VBlank flag clear on read is cycle-accurate
- [ ] **S2.4** Verify sprite 0 hit timing
- [ ] **S2.5** Verify NMI timing (scanline 241, dot 1)
- [ ] **S2.6** Test mid-scanline register writes

#### Acceptance Criteria

- PPU stepped exactly 3x per CPU cycle
- $2002 reads return correct VBlank state for exact PPU dot
- `ppu_02-vbl_set_time` test PASSES (currently ignored)
- `ppu_03-vbl_clear_time` test PASSES (currently ignored)

#### Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| `core/src/bus.rs` | Add `on_cpu_cycle()` implementation | 3h |
| `ppu/src/ppu.rs` | Verify step_with_chr() integration | 2h |
| `core/src/console.rs` | Remove instruction-level PPU stepping | 2h |
| `core/tests/*.rs` | Add VBlank timing tests | 2h |

---

### Sprint 3: APU Precision Integration (10-15 hours)

**Duration:** 1-2 weeks
**Priority:** MEDIUM - Correct audio timing

#### Tasks

- [ ] **S3.1** Step APU once per CPU cycle in `on_cpu_cycle()`
- [ ] **S3.2** Verify frame counter cycle accuracy
- [ ] **S3.3** Implement DMC DMA cycle stealing
- [ ] **S3.4** Verify $4015 status register timing
- [ ] **S3.5** Test APU IRQ timing
- [ ] **S3.6** Verify length counter halt timing

#### Acceptance Criteria

- APU stepped exactly 1x per CPU cycle
- Frame counter triggers at exact cycle counts
- DMC DMA properly steals CPU cycles
- All blargg APU tests still pass

#### Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| `core/src/bus.rs` | APU stepping in `on_cpu_cycle()` | 2h |
| `apu/src/lib.rs` | Verify clock() is cycle-accurate | 2h |
| `apu/src/dmc.rs` | DMC DMA cycle stealing | 5h |
| `core/tests/*.rs` | APU timing tests | 3h |

---

### Sprint 4: Bus and DMA Integration (15-20 hours)

**Duration:** 2 weeks
**Priority:** HIGH - Correct DMA timing

#### Tasks

- [ ] **S4.1** Integrate OAM DMA into cycle-by-cycle framework
- [ ] **S4.2** Handle DMA alignment (odd/even CPU cycle)
- [ ] **S4.3** Implement DMC DMA stall cycles
- [ ] **S4.4** Handle DMA during instruction execution
- [ ] **S4.5** Verify open bus behavior
- [ ] **S4.6** Test bus conflict scenarios

#### Acceptance Criteria

- OAM DMA executes exactly 513/514 cycles
- DMA properly interleaves with CPU cycles
- DMC DMA stall cycles accurate
- Open bus returns correct values

#### Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| `core/src/bus.rs` | DMA cycle integration | 8h |
| `cpu/src/cpu.rs` | DMA state handling | 5h |
| `core/src/console.rs` | DMA orchestration | 4h |
| `core/tests/*.rs` | DMA timing tests | 3h |

---

### Sprint 5: Mapper Accuracy (15-20 hours)

**Duration:** 2 weeks
**Priority:** MEDIUM - Correct mapper IRQ timing

#### Tasks

- [ ] **S5.1** Clock mappers in `on_cpu_cycle()` callback
- [ ] **S5.2** MMC3 A12 rising edge detection per PPU dot
- [ ] **S5.3** MMC5 scanline counter integration
- [ ] **S5.4** VRC IRQ timing verification
- [ ] **S5.5** Verify mapper register write timing
- [ ] **S5.6** Test mapper-specific games

#### Acceptance Criteria

- Mappers clocked at correct points in execution
- MMC3 IRQ counter accurate (A12 transitions)
- All Holy Mapperel tests pass
- Game-specific timing tests pass

#### Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| `mappers/src/mmc3.rs` | A12 per-dot detection | 5h |
| `mappers/src/mmc5.rs` | Scanline counter | 4h |
| `mappers/src/lib.rs` | clock() interface | 2h |
| `core/src/bus.rs` | Mapper clock integration | 2h |
| `core/tests/*.rs` | Mapper timing tests | 5h |

---

## Test Validation Targets

### Critical Tests (Currently Failing/Ignored)

| Test | Status | Requirement | Sprint |
|------|--------|-------------|--------|
| `ppu_02-vbl_set_time` | IGNORED | +/-2 cycle VBlank set | S2 |
| `ppu_03-vbl_clear_time` | IGNORED | +/-2 cycle VBlank clear | S2 |

### Regression Tests (Must Continue Passing)

| Category | Count | Status |
|----------|-------|--------|
| nestest CPU | 1 | PASS |
| Blargg CPU timing | 11 | PASS |
| Blargg PPU tests | 49 | PASS (2 ignored) |
| Blargg APU tests | 30 | PASS |
| Unit tests | 517+ | PASS |

### New Test Targets

| Test | Target | Sprint |
|------|--------|--------|
| `ppu_02-vbl_set_time` | PASS | S2 |
| `ppu_03-vbl_clear_time` | PASS | S2 |
| DMC DMA timing | PASS | S3/S4 |
| MMC3 IRQ timing | PASS | S5 |

---

## Effort Estimates

### Summary

| Sprint | Hours | Risk | Dependencies |
|--------|-------|------|--------------|
| S1: CPU Refactor | 40-60h | HIGH | None |
| S2: PPU Sync | 5-10h | LOW | S1 |
| S3: APU Precision | 10-15h | MEDIUM | S1 |
| S4: Bus/DMA | 15-20h | MEDIUM | S1, S2 |
| S5: Mappers | 15-20h | MEDIUM | S1, S2 |
| **Total** | **85-125h** | | |

### Confidence Level

- **Conservative estimate:** 125 hours (worst case)
- **Expected estimate:** 100 hours (likely)
- **Optimistic estimate:** 85 hours (best case)

### Resource Requirements

- 1 developer, 8-12 weeks at 10h/week
- OR 2 developers, 4-6 weeks at 10h/week each
- Reference materials: Pinky source, NESdev wiki, Mesen2 source

---

## Risk Assessment

### High Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Performance regression | High | Medium | Profile continuously, optimize hot paths |
| Opcode refactor errors | High | Medium | Comprehensive test coverage, golden log |
| Breaking existing tests | High | Low | Run full test suite after each change |

### Medium Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| DMA timing complexity | Medium | Medium | Reference Pinky's DMA implementation |
| Mapper edge cases | Medium | High | Focus on common mappers first (0-4) |
| API breaking changes | Medium | High | Version bump, clear migration docs |

### Low Risk

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| PPU integration issues | Low | Low | PPU already has dot-level stepping |
| APU timing issues | Low | Low | Frame counter already accurate |

---

## Dependencies

### Blockers

- None - This is architectural work that doesn't depend on other milestones

### Required Before

- Phase 2 advanced features (TAS, netplay, achievements) - These require cycle-accurate state

### Blocks

- Sub-instruction debugging
- Cycle-accurate save states
- TAS recording at instruction boundaries

### External Dependencies

- Pinky source code (for reference)
- NESdev wiki (timing documentation)
- Mesen2 source (for verification)

---

## References

### Internal Documentation

- [CPU Timing Reference](../../../docs/cpu/CPU_TIMING_REFERENCE.md)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)
- [APU Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)
- [M7-S1 CPU Accuracy](../milestone-7-accuracy/M7-S1-cpu-accuracy.md)

### Reference Implementations

- **Pinky** (`ref-proj/pinky/`) - Rust, `on_cpu_cycle()` pattern
- **Mesen2** - C++, gold standard accuracy
- **TetaNES** - Rust, alternative architecture

### External Resources

- [NesDev Wiki - CPU](https://www.nesdev.org/wiki/CPU)
- [NesDev Wiki - PPU](https://www.nesdev.org/wiki/PPU)
- [NesDev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [NesDev Wiki - VBlank](https://www.nesdev.org/wiki/PPU_frame_timing)

### Key Code References

#### Pinky on_cpu_cycle Pattern

```rust
// ref-proj/pinky/nes/src/virtual_nes.rs:631-639
fn on_cpu_cycle(&mut self) {
    self.state_mut().cpu_cycle.wrapping_inc();
    virtual_apu::Interface::execute(self.newtype_mut());
    for _ in 0..3 {
        rp2c02::Interface::execute(self.newtype_mut());
    }
    Context::on_cycle(self);
}
```

#### Pinky Memory Access Pattern

```rust
// ref-proj/pinky/nes/src/virtual_nes.rs:220-229
impl<C: Context> mos6502::Context for Orphan<C> {
    fn peek(&mut self, address: u16) -> u8 {
        self.as_mut().on_cpu_cycle();  // BEFORE each read
        dma::Interface::execute(self, address);
        Private::peek_memory(self.as_mut(), address)
    }

    fn poke(&mut self, address: u16, value: u8) {
        self.as_mut().on_cpu_cycle();  // BEFORE each write
        Private::poke_memory(self.as_mut(), address, value);
    }
}
```

---

## Success Criteria

### Milestone Complete When

1. [x] CPU refactored to cycle-by-cycle execution
2. [ ] PPU stepped before each CPU memory access
3. [ ] APU stepped before each CPU memory access
4. [ ] `ppu_02-vbl_set_time` test PASSES
5. [ ] `ppu_03-vbl_clear_time` test PASSES
6. [ ] All 90 Blargg tests still pass (0 regressions)
7. [ ] All 517+ unit tests still pass (0 regressions)
8. [ ] Performance within 20% of current (acceptable tradeoff for accuracy)
9. [ ] OAM DMA timing verified
10. [ ] DMC DMA cycle stealing implemented

### Version Targets

- **Start:** v0.8.4
- **Complete:** v1.0.0

---

## Appendix A: Detailed Opcode Refactoring

### Official Opcodes by Addressing Mode

| Mode | Opcodes | Cycles | Memory Accesses |
|------|---------|--------|-----------------|
| Immediate | 11 | 2 | 2 (opcode, operand) |
| Zero Page | 22 | 3 | 3 |
| Zero Page,X/Y | 16 | 4 | 4 |
| Absolute | 23 | 4 | 4 |
| Absolute,X/Y | 15 | 4-5 | 4-5 (page cross) |
| Indirect | 1 (JMP) | 5 | 5 |
| Indexed Indirect | 8 | 6 | 6 |
| Indirect Indexed | 8 | 5-6 | 5-6 (page cross) |
| Implied | 25 | 2 | 1-2 |
| Accumulator | 4 | 2 | 1 |
| Relative | 8 | 2-4 | 2-4 (branch) |

### Unofficial Opcodes

105 unofficial opcodes also require refactoring, following same patterns.

---

## Appendix B: Testing Strategy

### Phase 1: Unit Tests

- Verify each opcode handler calls `on_cpu_cycle()` correct times
- Mock bus to count callback invocations
- Compare cycle counts with NESdev wiki

### Phase 2: Integration Tests

- Run nestest.nes with callback verification
- Verify PPU state at specific cycle points
- Test VBlank flag timing edge cases

### Phase 3: Timing Tests

- `ppu_02-vbl_set_time` - Must pass
- `ppu_03-vbl_clear_time` - Must pass
- Additional timing tests from TASVideos suite

### Phase 4: Game Tests

- Super Mario Bros. - Critical timing game
- Battletoads - Known difficult timing
- Top 50 games validation

---

**Status:** PLANNED
**Created:** 2025-12-28
**Author:** Claude Code Audit
**Next Review:** Upon Sprint 1 kickoff
