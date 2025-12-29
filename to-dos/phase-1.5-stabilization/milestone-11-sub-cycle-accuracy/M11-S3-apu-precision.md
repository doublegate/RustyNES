# M11-S3: APU Precision Integration

**Sprint:** S3 (APU Precision)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 1-2 weeks (10-15 hours)
**Status:** PLANNED
**Priority:** MEDIUM - Correct audio timing
**Depends On:** S1 (CPU Refactor)

---

## Overview

Integrate APU stepping into the `on_cpu_cycle()` callback to achieve cycle-accurate audio timing, including precise frame counter events, length counter clocking, sweep unit timing, and DMC DMA cycle stealing.

---

## Dependencies

### Required Before Starting
- **S1 (CPU Refactor)** - `on_cpu_cycle()` callback must be implemented in CpuBus trait
- S2 can run in parallel as APU integration is independent of PPU sync

### Blocks
- **S4 (Bus/DMA)** - DMC DMA conflicts depend on APU cycle-accurate integration
- **S6 (Testing)** - APU timing tests require this sprint

---

## Current Implementation

### Frame Counter (GOOD)

The frame counter already has correct cycle timing:

```rust
// crates/rustynes-apu/src/frame_counter.rs
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

### DMC Channel (PARTIAL)

DMC has DMA cycle stealing implemented but needs integration with `on_cpu_cycle()`:

```rust
// crates/rustynes-apu/src/dmc.rs
pub fn clock_timer<F>(&mut self, mut read_memory: F) -> u8
where
    F: FnMut(u16) -> u8,
{
    let mut dma_cycles = 0;

    // Refill buffer if empty and bytes remaining
    if self.sample_buffer_empty && self.bytes_remaining > 0 {
        dma_cycles = self.fetch_sample(&mut read_memory);  // Returns 3 cycles
    }
    // ...
    dma_cycles
}
```

### Console Integration (COARSE)

```rust
// Current: APU stepped after instruction
for _ in 0..cpu_cycles {
    self.bus.apu.step();
}
```

---

## Required Changes

### Task S3.1: APU Stepping in on_cpu_cycle()

**Priority:** P0
**Effort:** 2 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`

#### Subtasks
- [ ] Add `self.apu.step()` call to `on_cpu_cycle()` implementation
- [ ] Ensure APU is stepped exactly once per CPU cycle
- [ ] Verify frame counter receives correct cycle count

#### Implementation Notes

```rust
impl CpuBus for Bus {
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3 times (from S2)
        for _ in 0..3 {
            let result = self.ppu.step_with_chr(|addr| {
                self.mapper.read_chr(addr)
            });
            // ... handle NMI
        }

        // Step APU once per CPU cycle
        self.apu.step();  // NEW

        // Clock mapper
        self.mapper.clock(1);
    }
}
```

---

### Task S3.2: Frame Counter Cycle Accuracy Verification

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `crates/rustynes-apu/src/frame_counter.rs`
- `crates/rustynes-apu/src/apu.rs`

#### Subtasks
- [ ] Verify 4-step mode cycle values: 7458, 14914, 22373, 29830-29832
- [ ] Verify 5-step mode cycle values: 7458, 14914, 22372, 37282
- [ ] Verify IRQ flag timing at cycles 29830, 29831, 29832
- [ ] Test $4017 write reset behavior
- [ ] Verify immediate half-frame on 5-step mode switch

#### Implementation Notes

The frame counter timing is already implemented correctly. This task verifies integration:

```text
4-Step Mode Sequence:
  Cycle 7458:  Quarter frame (envelopes + linear counter)
  Cycle 14914: Half frame (envelopes, linear, length, sweep)
  Cycle 22373: Quarter frame
  Cycle 29830: Half frame + IRQ (if enabled)
  Cycle 29831: IRQ
  Cycle 29832: IRQ + reset to 0

5-Step Mode Sequence:
  Cycle 7458:  Quarter frame
  Cycle 14914: Half frame
  Cycle 22372: Quarter frame
  Cycle 37282: Half frame + reset to 0
  (No IRQ in 5-step mode)
```

---

### Task S3.3: Length Counter Clocking Precision

**Priority:** P1
**Effort:** 1 hour
**Files:**
- `crates/rustynes-apu/src/length_counter.rs`
- `crates/rustynes-apu/src/apu.rs`

#### Subtasks
- [ ] Verify length counters clock on HalfFrame only
- [ ] Verify halt flag prevents clocking
- [ ] Verify channel silencing when counter reaches 0
- [ ] Test reload behavior during halt

#### Implementation Notes

Length counters are clocked at half-frame rate (every 14914 cycles in 4-step mode):

```rust
// Half frame clocks length counters for all channels
fn clock_half_frame(&mut self) {
    self.pulse1.clock_length_counter();
    self.pulse2.clock_length_counter();
    self.triangle.clock_length_counter();
    self.noise.clock_length_counter();
    // Also clock sweep units
}
```

---

### Task S3.4: Sweep Unit Timing Precision

**Priority:** P1
**Effort:** 1 hour
**Files:**
- `crates/rustynes-apu/src/sweep.rs`
- `crates/rustynes-apu/src/pulse.rs`

#### Subtasks
- [ ] Verify sweep units clock on HalfFrame
- [ ] Verify period calculation and muting behavior
- [ ] Verify pulse 1 vs pulse 2 negate mode difference
- [ ] Test sweep reload behavior

#### Implementation Notes

Sweep units modify pulse channel period:

```text
Sweep Unit Operation:
- Clocked at half-frame rate
- Period = current_period >> shift_amount
- Pulse 1: new_period = current - period (ones' complement)
- Pulse 2: new_period = current - period - 1 (two's complement)
- Channel muted if target period < 8 or > $7FF
```

---

### Task S3.5: DMC DMA Cycle Stealing

**Priority:** P0
**Effort:** 5 hours
**Files:**
- `crates/rustynes-apu/src/dmc.rs`
- `crates/rustynes-core/src/bus.rs`
- `crates/rustynes-cpu/src/cpu.rs`

#### Subtasks
- [ ] Implement DMC DMA stall detection in `on_cpu_cycle()`
- [ ] Handle 1-4 cycle CPU stall based on CPU state
- [ ] Integrate DMC sample fetching with CPU cycle stealing
- [ ] Verify DMC does not conflict with OAM DMA
- [ ] Test DMC timing with sample playback

#### Implementation Notes

DMC DMA is more complex than OAM DMA:

```rust
// DMC DMA stall cycles depend on CPU state:
// - 1 cycle: CPU not reading (write cycle)
// - 2 cycles: CPU reading (most common)
// - 3 cycles: CPU in get cycle (read dummy byte)
// - 4 cycles: CPU in halt cycle (special case)

// Typical implementation:
fn check_dmc_dma(&mut self) -> bool {
    if self.apu.dmc_needs_sample() {
        let stall_cycles = match self.cpu_state {
            CpuState::Write => 1,
            CpuState::Read => 2,
            CpuState::GetCycle => 3,
            CpuState::Halt => 4,
        };
        self.apu.dmc_fetch_sample(|addr| self.read_for_dma(addr));
        return true;
    }
    false
}
```

**Reference:** NESdev Wiki - DMC: "The DMA reader halts the CPU for 4 cycles to read from PRG memory."

---

### Task S3.6: $4015 Status Register Timing

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `crates/rustynes-apu/src/apu.rs`

#### Subtasks
- [ ] Verify $4015 read returns correct channel status at exact cycle
- [ ] Verify DMC IRQ flag cleared on $4015 read
- [ ] Verify frame counter IRQ flag cleared on $4015 read
- [ ] Test race conditions with IRQ flag read/clear

#### Implementation Notes

$4015 status register format:
```text
Read $4015:
  D7: DMC IRQ flag
  D6: Frame counter IRQ flag
  D5: (unused)
  D4: DMC active (bytes remaining > 0)
  D3: Noise length counter > 0
  D2: Triangle length counter > 0
  D1: Pulse 2 length counter > 0
  D0: Pulse 1 length counter > 0

Side effects:
  - Clears frame counter IRQ flag
  - Does NOT clear DMC IRQ flag (that's separate)
```

---

### Task S3.7: IRQ Timing Precision

**Priority:** P1
**Effort:** 2 hours
**Files:**
- `crates/rustynes-apu/src/frame_counter.rs`
- `crates/rustynes-apu/src/dmc.rs`
- `crates/rustynes-core/src/bus.rs`

#### Subtasks
- [ ] Verify frame counter IRQ asserted at exact cycles (29830, 29831, 29832)
- [ ] Verify DMC IRQ asserted when sample completes (if enabled)
- [ ] Verify IRQ inhibit flag ($4017 bit 6) behavior
- [ ] Test IRQ line polling in `on_cpu_cycle()` callback

#### Implementation Notes

IRQ is level-triggered (not edge-triggered):

```rust
// IRQ polling should happen after each CPU cycle
fn on_cpu_cycle(&mut self) {
    // ... PPU and APU stepping ...

    // Update IRQ line state
    let irq = self.apu.irq_pending() || self.mapper.irq_pending();
    self.cpu_irq_line = irq;
}

// Frame counter IRQ timing:
// - Set at cycles 29830, 29831, 29832 (4-step mode only)
// - Inhibited if $4017 bit 6 is set
// - Cleared by reading $4015 or writing $4017 with bit 6 set
```

---

## Testing Requirements

### Unit Tests

- [ ] Frame counter quarter/half frame timing verification
- [ ] Length counter clocking at half-frame rate
- [ ] Sweep unit clocking at half-frame rate
- [ ] DMC DMA stall cycle count verification
- [ ] $4015 status register read/write timing
- [ ] IRQ flag set/clear timing

### Integration Tests

- [ ] Blargg APU tests (apu_test, apu_timing)
- [ ] DMC timing test ROMs
- [ ] Frame counter IRQ timing tests

---

## Validation Criteria

| Criterion | Target | Current |
|-----------|--------|---------|
| blargg_apu_basics | PASS | PASS |
| blargg_apu_timing | PASS | UNKNOWN |
| DMC DMA stall timing | Accurate | Partial |
| Frame counter IRQ | Exact cycle | Approximate |
| $4015 read timing | Cycle-accurate | Approximate |

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| DMC timing complexity | High | Medium | Reference Mesen2/Pinky implementations |
| IRQ race conditions | Medium | Medium | Careful edge case testing |
| Audio quality regression | Medium | Low | A/B testing with known good ROMs |
| Performance impact | Low | Medium | Profile APU hot paths |

---

## References

### Internal Documentation
- [APU Specification](../../../docs/apu/APU_2A03_SPECIFICATION.md)
- [Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)
- [DMC Channel](../../../docs/apu/APU_CHANNEL_DMC.md)

### External Resources
- [NESdev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [NESdev Wiki - APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [NESdev Wiki - APU DMC](https://www.nesdev.org/wiki/APU_DMC)

### Reference Implementations
- **Pinky** (`ref-proj/pinky/nes/src/apu/`) - Rust, cycle-accurate APU
- **Mesen2** - C++, gold standard accuracy

---

## Acceptance Criteria

- [ ] APU stepped exactly 1x per CPU cycle in `on_cpu_cycle()`
- [ ] Frame counter triggers at exact cycle counts
- [ ] DMC DMA properly steals 1-4 CPU cycles based on CPU state
- [ ] $4015 reads are cycle-accurate
- [ ] All existing Blargg APU tests still pass
- [ ] IRQ timing matches hardware behavior

---

**Status:** PLANNED
**Created:** 2025-12-28
