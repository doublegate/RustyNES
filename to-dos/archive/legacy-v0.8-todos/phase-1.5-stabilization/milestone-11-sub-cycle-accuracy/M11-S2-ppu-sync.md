# M11-S2: PPU Synchronization Integration

**Sprint:** S2 (PPU Synchronization)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 1 week (5-10 hours)
**Status:** PLANNED
**Priority:** HIGH - Required for VBlank timing accuracy
**Depends On:** S1 (CPU Refactor)

---

## Overview

Integrate PPU stepping into the `on_cpu_cycle()` callback to achieve sub-cycle accurate VBlank flag timing.

---

## Goal

Pass the currently-ignored VBlank timing tests:
- `ppu_02-vbl_set_time` - Requires +/-2 cycle accuracy
- `ppu_03-vbl_clear_time` - Requires +/-2 cycle accuracy

---

## Current State

### PPU Infrastructure (GOOD)

The PPU already has dot-level stepping capability:

```rust
// crates/rustynes-ppu/src/ppu.rs
pub fn step_with_chr(&mut self, chr_access: impl Fn(u16) -> u8) -> StepResult {
    // Advances one dot (pixel clock)
    // 341 dots per scanline, 262 scanlines per frame
}

// crates/rustynes-ppu/src/timing.rs
pub fn is_vblank_set_dot(&self) -> bool {
    self.scanline == 241 && self.dot == 1
}

pub fn is_vblank_clear_dot(&self) -> bool {
    self.scanline == 261 && self.dot == 1
}
```

### Console Integration (PARTIAL)

```rust
// Current: PPU stepped before CPU instruction
pub fn tick(&mut self) -> TickResult {
    for _ in 0..3 {
        self.ppu.step_with_chr(/* ... */);
    }
    let cycles = self.cpu.step(&mut self.bus);
    // PPU catches up after instruction
}
```

### Problem

PPU is stepped before the **instruction**, not before each **memory access**. When CPU reads $2002 mid-instruction, PPU state is stale.

---

## Tasks

### S2.1: Implement on_cpu_cycle() in Bus

**Effort:** 3 hours
**Files:** `crates/rustynes-core/src/bus.rs`

```rust
impl CpuBus for Bus {
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3 times (3:1 PPU:CPU ratio)
        for _ in 0..3 {
            let result = self.ppu.step_with_chr(|addr| {
                self.mapper.read_chr(addr)
            });

            // Handle NMI from PPU
            if result.nmi {
                self.nmi_pending = true;
            }

            // Handle frame completion
            if result.frame_complete {
                self.frame_ready = true;
            }
        }

        // Step APU once (moved from S3, but include here)
        self.apu.clock();

        // Clock mapper
        self.mapper.clock(1);
    }

    fn read(&mut self, addr: u16) -> u8 {
        // Note: on_cpu_cycle() already called by CPU
        self.internal_read(addr)
    }

    fn write(&mut self, addr: u16, val: u8) {
        // Note: on_cpu_cycle() already called by CPU
        self.internal_write(addr, val)
    }
}
```

- [ ] Add `on_cpu_cycle()` implementation
- [ ] Integrate PPU stepping (3x per cycle)
- [ ] Handle NMI flag propagation
- [ ] Handle frame completion flag

---

### S2.2: VBlank Flag Cycle Accuracy

**Effort:** 2 hours
**Files:** `crates/rustynes-ppu/src/ppu.rs`

Verify VBlank flag is set/cleared at exact cycle:

```text
VBlank Set:
  Scanline 241, Dot 1 - Set VBlank flag ($2002 bit 7)
  NMI generated if enabled ($2000 bit 7)

VBlank Clear:
  Scanline 261, Dot 1 - Clear VBlank flag
  Also clear sprite 0 hit, sprite overflow flags
```

```rust
// Ensure timing.rs checks are used correctly
fn step_internal(&mut self) {
    if self.timing.is_vblank_set_dot() {
        self.status.set_vblank(true);
        if self.ctrl.nmi_enabled() {
            self.nmi_pending = true;
        }
    }

    if self.timing.is_vblank_clear_dot() {
        self.status.set_vblank(false);
        self.status.set_sprite0_hit(false);
        self.status.set_sprite_overflow(false);
    }
}
```

- [ ] Verify VBlank set timing (241, 1)
- [ ] Verify VBlank clear timing (261, 1)
- [ ] Verify flag clear on $2002 read
- [ ] Add cycle-level timing tests

---

### S2.3: $2002 Read Race Condition

**Effort:** 2 hours
**Files:** `crates/rustynes-ppu/src/ppu.rs`

Handle the $2002 read race condition at VBlank boundary:

```text
If CPU reads $2002 at exactly the cycle VBlank is set:
- NMI is suppressed for that frame
- VBlank flag returns as clear (race condition)

This is tested by ppu_02-vbl_set_time and ppu_03-vbl_clear_time
```

```rust
pub fn read_status(&mut self) -> u8 {
    let status = self.status.bits();

    // Clear VBlank flag on read (before returning)
    self.status.set_vblank(false);

    // Clear write latch
    self.write_latch = false;

    // Return status with old VBlank value
    status
}
```

- [ ] Verify read-clear behavior
- [ ] Test race condition at boundary
- [ ] Document edge case behavior

---

### S2.4: Sprite 0 Hit Timing

**Effort:** 1 hour
**Files:** `crates/rustynes-ppu/src/ppu.rs`

Verify sprite 0 hit is detected at correct dot:

```text
Sprite 0 Hit:
- Set when non-transparent sprite 0 pixel overlaps non-transparent BG
- Earliest: dot 1 of the scanline where sprite 0 is visible
- Not set on dot 255 (last visible dot)
- Not set if rendering disabled
```

- [ ] Verify sprite 0 hit dot timing
- [ ] Verify dot 255 exclusion
- [ ] Verify rendering enable check

---

### S2.5: NMI Timing Verification

**Effort:** 1 hour
**Files:** `crates/rustynes-ppu/src/ppu.rs`, `crates/rustynes-core/src/bus.rs`

Verify NMI is generated at correct cycle:

```text
NMI Generation:
- Scanline 241, Dot 1 (same as VBlank set)
- Only if NMI enable bit ($2000 bit 7) is set
- If NMI enable set after VBlank flag, NMI still triggers
```

- [ ] Verify NMI timing matches VBlank set
- [ ] Test NMI enable during VBlank
- [ ] Verify NMI suppression on $2002 read

---

### S2.6: Remove Instruction-Level Stepping

**Effort:** 1 hour
**Files:** `crates/rustynes-core/src/console.rs`

Remove redundant PPU stepping from Console::tick():

```rust
// BEFORE: Console steps PPU before instruction
pub fn tick(&mut self) -> TickResult {
    for _ in 0..3 {
        self.ppu.step_with_chr(/* ... */);  // REMOVE
    }
    let cycles = self.cpu.step(&mut self.bus);
    // ... catch-up stepping also removed
}

// AFTER: PPU stepping happens in on_cpu_cycle()
pub fn tick(&mut self) -> TickResult {
    let cycles = self.cpu.step(&mut self.bus);
    // All PPU stepping now in on_cpu_cycle()
}
```

- [ ] Remove pre-instruction PPU stepping
- [ ] Remove post-instruction PPU catch-up
- [ ] Verify frame timing unchanged

---

## Test Validation

### Critical Tests

| Test | Current | Target |
|------|---------|--------|
| `ppu_02-vbl_set_time` | IGNORED | PASS |
| `ppu_03-vbl_clear_time` | IGNORED | PASS |

### Test Requirements

```text
ppu_02-vbl_set_time:
- VBlank flag must be set at cycle N (exact timing)
- Reading $2002 at cycle N-1: VBlank clear
- Reading $2002 at cycle N: VBlank set
- Reading $2002 at cycle N+1: VBlank set (and cleared by read)

ppu_03-vbl_clear_time:
- VBlank flag must be cleared at cycle M (exact timing)
- Similar +/-2 cycle accuracy requirement
```

### Unit Tests to Add

```rust
#[test]
fn test_vblank_set_cycle_accuracy() {
    let mut ppu = Ppu::new();
    // Advance to scanline 240, dot 340
    for _ in 0..(241 * 341 - 1) {
        ppu.step();
    }
    assert!(!ppu.status().vblank());

    // Step to scanline 241, dot 1
    ppu.step();  // dot 341 -> 0
    ppu.step();  // dot 0 -> 1
    assert!(ppu.status().vblank());
}

#[test]
fn test_vblank_clear_cycle_accuracy() {
    let mut ppu = Ppu::new();
    ppu.set_vblank(true);

    // Advance to scanline 261, dot 0
    for _ in 0..(261 * 341) {
        ppu.step();
    }
    assert!(ppu.status().vblank());

    // Step to dot 1
    ppu.step();
    assert!(!ppu.status().vblank());
}
```

- [ ] Add VBlank set cycle test
- [ ] Add VBlank clear cycle test
- [ ] Add $2002 read race test
- [ ] Add NMI timing test

---

## Acceptance Criteria

- [ ] PPU stepped exactly 3x per CPU cycle
- [ ] $2002 reads return correct VBlank state for exact PPU dot
- [ ] `ppu_02-vbl_set_time` test PASSES
- [ ] `ppu_03-vbl_clear_time` test PASSES
- [ ] All existing PPU tests still pass
- [ ] NMI timing accurate

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Timing off by 1 dot | Verify against NESdev wiki timing diagrams |
| Race condition bugs | Add comprehensive edge case tests |
| NMI suppression issues | Reference Mesen2 implementation |

---

## Dependencies

- **Requires:** S1 (CPU Refactor) - `on_cpu_cycle()` must be called
- **Blocks:** S4 (DMA) - DMA needs cycle-accurate PPU

---

## References

- [NESdev Wiki - PPU Frame Timing](https://www.nesdev.org/wiki/PPU_frame_timing)
- [NESdev Wiki - PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)

---

**Status:** PLANNED
**Created:** 2025-12-28
