# M11-S5: Mapper Timing Accuracy

**Sprint:** S5 (Mapper Accuracy)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 2 weeks (15-20 hours)
**Status:** PLANNED
**Priority:** MEDIUM - Correct mapper IRQ timing
**Depends On:** S1 (CPU Refactor), S2 (PPU Sync)

---

## Overview

Integrate mapper clocking into the `on_cpu_cycle()` callback to achieve cycle-accurate mapper timing, particularly for scanline counters (MMC3), A12 edge detection, and mapper-specific IRQ timing.

---

## Dependencies

### Required Before Starting
- **S1 (CPU Refactor)** - Cycle-by-cycle execution framework
- **S2 (PPU Sync)** - PPU dot-accurate stepping for A12 detection

### Parallel Work
- **S3 (APU)** - Independent work
- **S4 (Bus/DMA)** - `on_cpu_cycle()` must include mapper.clock()

### Blocks
- **S6 (Testing)** - Mapper IRQ timing tests

---

## Current Implementation

### Mapper Trait

```rust
// crates/rustynes-mappers/src/lib.rs
pub trait Mapper: Send {
    fn read_prg(&self, addr: u16) -> u8;
    fn write_prg(&mut self, addr: u16, val: u8);
    fn read_chr(&self, addr: u16) -> u8;
    fn write_chr(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring;

    fn irq_pending(&self) -> bool { false }
    fn clear_irq(&mut self) {}
    fn clock(&mut self, _cycles: u8) {}
    fn ppu_a12_edge(&mut self) {}
}
```

### MMC3 IRQ Counter (Current)

```rust
// crates/rustynes-mappers/src/mmc3.rs
fn clock_irq_counter(&mut self) {
    if self.irq_counter == 0 || self.irq_reload {
        self.irq_counter = self.irq_latch;
        self.irq_reload = false;
    } else {
        self.irq_counter -= 1;
    }

    if self.irq_counter == 0 && self.irq_enable {
        self.irq_pending = true;
    }
}

fn ppu_a12_edge(&mut self) {
    self.clock_irq_counter();
}
```

### Console Mapper Clocking (Current)

```rust
// crates/rustynes-core/src/console.rs
// Clock mapper after instruction (coarse)
self.bus.clock_mapper(cpu_cycles);
```

---

## Required Changes

### Task S5.1: Mapper Clocking in on_cpu_cycle()

**Priority:** P0
**Effort:** 2 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`

#### Subtasks
- [ ] Add `self.mapper.clock(1)` to `on_cpu_cycle()`
- [ ] Ensure mapper is clocked once per CPU cycle
- [ ] Verify with cycle-based mappers (VRC)

#### Implementation Notes

```rust
impl CpuBus for Bus {
    fn on_cpu_cycle(&mut self) {
        // Step PPU 3x
        for _ in 0..3 {
            let result = self.ppu.step_with_chr(|addr| {
                self.mapper.read_chr(addr)
            });
            // ... handle NMI/frame
        }

        // Step APU 1x
        self.apu.step();

        // Clock mapper (for cycle-based timing)
        self.mapper.clock(1);
    }
}
```

---

### Task S5.2: MMC3 A12 Rising Edge Detection (Per PPU Dot)

**Priority:** P0
**Effort:** 5 hours
**Files:**
- `crates/rustynes-mappers/src/mmc3.rs`
- `crates/rustynes-ppu/src/ppu.rs`
- `crates/rustynes-core/src/bus.rs`

#### Subtasks
- [ ] Track A12 state across PPU fetches
- [ ] Detect rising edge (low-to-high transition)
- [ ] Clock MMC3 counter on rising edge
- [ ] Implement 8-12 PPU cycle filter for edge detection
- [ ] Test with SMB3, Mega Man 3

#### Implementation Notes

MMC3 scanline counter uses PPU address line A12:

```text
A12 Rising Edge Detection:
- A12 is bit 12 of PPU address ($0xxx or $1xxx)
- Rising edge = A12 goes from 0 to 1
- Typically happens when PPU fetches from $1xxx after $0xxx

MMC3 Timing:
- Counter decrements on A12 rising edge
- 8-12 PPU cycle filter to prevent spurious edges
- IRQ fires when counter transitions to 0 (after decrement)
```

```rust
// crates/rustynes-mappers/src/mmc3.rs
pub struct Mmc3 {
    // ... existing fields ...

    /// Previous A12 state for edge detection
    last_a12: bool,

    /// Cycle counter for A12 filter (8-12 PPU cycles)
    a12_filter_cycles: u8,
}

impl Mmc3 {
    /// Notify mapper of CHR address access
    pub fn chr_address_changed(&mut self, addr: u16) {
        let a12 = (addr & 0x1000) != 0;

        // Detect rising edge with filter
        if a12 && !self.last_a12 && self.a12_filter_cycles == 0 {
            self.clock_irq_counter();
        }

        self.last_a12 = a12;

        // Reset filter on low A12
        if !a12 {
            self.a12_filter_cycles = 8;  // Minimum filter cycles
        }
    }

    /// Clock the A12 filter (called per PPU cycle)
    pub fn clock_a12_filter(&mut self) {
        if self.a12_filter_cycles > 0 {
            self.a12_filter_cycles -= 1;
        }
    }
}
```

**Integration with PPU:**

```rust
// crates/rustynes-core/src/bus.rs
fn on_cpu_cycle(&mut self) {
    for _ in 0..3 {
        // Get current PPU address for A12 detection
        let ppu_addr = self.ppu.current_address();

        // Notify mapper of CHR access (for A12 edge detection)
        if let Some(addr) = ppu_addr {
            self.mapper.notify_chr_access(addr);
        }

        // Clock A12 filter
        self.mapper.clock_a12_filter();

        // Step PPU
        let result = self.ppu.step_with_chr(|addr| {
            self.mapper.read_chr(addr)
        });
        // ...
    }
}
```

---

### Task S5.3: MMC3 IRQ Timing Edge Cases

**Priority:** P1
**Effort:** 3 hours
**Files:**
- `crates/rustynes-mappers/src/mmc3.rs`

#### Subtasks
- [ ] Handle IRQ counter reload timing
- [ ] Handle counter = 0 and reload = 0 case
- [ ] Handle IRQ acknowledge timing
- [ ] Test with Holy Mapperel MMC3 tests

#### Implementation Notes

MMC3 IRQ edge cases:

```text
Counter Reload Behavior:
- If counter = 0 OR reload flag set: counter = latch
- Otherwise: counter = counter - 1

IRQ Trigger:
- IRQ fires when counter TRANSITIONS to 0 (not when already 0)
- Some revisions: IRQ fires when counter reaches 0 after decrement
- MMC3A: IRQ fires on counter 0 (original behavior)
- MMC3B: IRQ fires on transition (most common)

Reload Flag:
- Set by writing to $C001
- Cleared after reload occurs
```

```rust
fn clock_irq_counter(&mut self) {
    // MMC3B behavior (most common)
    let was_zero = self.irq_counter == 0;

    if self.irq_counter == 0 || self.irq_reload {
        self.irq_counter = self.irq_latch;
        self.irq_reload = false;
    } else {
        self.irq_counter -= 1;
    }

    // Fire IRQ on transition to 0, not when already 0
    if self.irq_counter == 0 && !was_zero && self.irq_enable {
        self.irq_pending = true;
    }
}
```

---

### Task S5.4: MMC5 Scanline Detection Integration

**Priority:** P2
**Effort:** 4 hours
**Files:**
- `crates/rustynes-mappers/src/mmc5.rs` (create if needed)

#### Subtasks
- [ ] Implement MMC5 scanline counter (different from MMC3)
- [ ] Handle in-frame detection
- [ ] Integrate with PPU rendering state
- [ ] Test with Castlevania III

#### Implementation Notes

MMC5 uses a different scanline detection mechanism:

```text
MMC5 Scanline Detection:
- Does NOT use A12 edge detection
- Monitors PPU reads to detect fetches
- Counts 3 consecutive fetches from $2xxx as scanline end
- More complex than MMC3 but more reliable

MMC5 IRQ:
- Compare scanline counter to target value
- IRQ when match occurs
- In-frame flag indicates rendering active
```

---

### Task S5.5: VRC IRQ Timing (Cycle-Based)

**Priority:** P2
**Effort:** 3 hours
**Files:**
- `crates/rustynes-mappers/src/vrc*.rs`

#### Subtasks
- [ ] Implement VRC4/6/7 cycle-based IRQ counter
- [ ] Handle prescaler modes (256 or 341 cycles)
- [ ] Integrate with `clock()` method
- [ ] Test with appropriate games

#### Implementation Notes

VRC mappers use CPU cycle-based IRQ:

```rust
pub struct VrcIrq {
    counter: u8,
    latch: u8,
    prescaler: u16,
    prescaler_mask: u16,  // 0xFF (256 cycles) or 0x155 (341 cycles)
    enabled: bool,
    pending: bool,
    acknowledge_mode: bool,
}

impl VrcIrq {
    pub fn clock(&mut self, cycles: u8) {
        if !self.enabled {
            return;
        }

        self.prescaler += u16::from(cycles);

        while self.prescaler > self.prescaler_mask {
            self.prescaler -= self.prescaler_mask + 1;

            if self.counter == 0xFF {
                self.counter = self.latch;
                self.pending = true;
            } else {
                self.counter += 1;
            }
        }
    }
}
```

---

### Task S5.6: Mapper Register Write Timing

**Priority:** P2
**Effort:** 2 hours
**Files:**
- `crates/rustynes-mappers/src/*.rs`

#### Subtasks
- [ ] Verify register writes take effect at correct cycle
- [ ] Handle bank switch timing (some mappers have delay)
- [ ] Test mid-instruction bank switching
- [ ] Document timing for each mapper

#### Implementation Notes

Most mapper register writes take effect immediately, but some edge cases exist:

```text
Immediate Effect:
- NROM, UxROM, CNROM: Bank switch immediate
- MMC1: Shift register immediate, bank switch on 5th write
- MMC3: Bank select/data immediate

Delayed Effect:
- Some bootleg mappers have 1-cycle delay
- FDS disk access has timing constraints
```

---

### Task S5.7: Bus Conflict Implementation (Per Mapper)

**Priority:** P2
**Effort:** 2 hours
**Files:**
- `crates/rustynes-mappers/src/lib.rs`
- `crates/rustynes-mappers/src/*.rs`

#### Subtasks
- [ ] Add `has_bus_conflicts()` trait method
- [ ] Implement bus conflict for BNROM (34), GNROM (66)
- [ ] Implement bus conflict for Color Dreams (11)
- [ ] Implement bus conflict for Action 52 (228)

#### Implementation Notes

```rust
pub trait Mapper: Send {
    // ... existing methods ...

    /// Returns true if mapper has bus conflicts on PRG writes
    fn has_bus_conflicts(&self) -> bool { false }
}

// In Bus::write()
fn write(&mut self, addr: u16, value: u8) {
    match addr {
        0x8000..=0xFFFF => {
            if self.mapper.has_bus_conflicts() {
                let rom_value = self.mapper.read_prg(addr);
                let effective = value & rom_value;
                self.mapper.write_prg(addr, effective);
            } else {
                self.mapper.write_prg(addr, value);
            }
        }
        // ...
    }
}
```

---

## Testing Requirements

### Unit Tests
- [ ] MMC3 A12 edge detection per PPU dot
- [ ] MMC3 counter reload behavior
- [ ] MMC3 IRQ fire timing (transition to 0)
- [ ] VRC prescaler cycle accuracy
- [ ] Bus conflict AND behavior

### Integration Tests
- [ ] Holy Mapperel MMC3 tests
- [ ] Super Mario Bros. 3 status bar
- [ ] Mega Man 3-6 split screens
- [ ] Kirby's Adventure raster effects
- [ ] Ninja Gaiden scrolling

---

## Validation Criteria

| Criterion | Target | Current |
|-----------|--------|---------|
| MMC3 IRQ counter | Per A12 edge | Per instruction |
| MMC3 A12 filter | 8-12 PPU cycles | Not implemented |
| VRC IRQ prescaler | Cycle-accurate | Not tested |
| Mapper clock() | Per CPU cycle | Per instruction |
| Bus conflicts | Correct mappers | Not implemented |

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| MMC3 timing breaks games | High | Medium | Test with known problem games |
| A12 filter too strict/loose | Medium | Medium | Reference Mesen2 implementation |
| MMC5 complexity | Medium | High | Focus on MMC3 first |
| Performance regression | Low | Medium | Profile mapper hot paths |

---

## References

### Internal Documentation
- [Mapper Overview](../../../docs/mappers/MAPPER_OVERVIEW.md)
- [MMC3 Specification](../../../docs/mappers/MAPPER_004_MMC3.md)

### External Resources
- [NESdev Wiki - MMC3](https://www.nesdev.org/wiki/MMC3)
- [NESdev Wiki - MMC5](https://www.nesdev.org/wiki/MMC5)
- [NESdev Wiki - VRC6](https://www.nesdev.org/wiki/VRC6)

### Reference Implementations
- **Mesen2** - C++, gold standard mapper accuracy
- **puNES** - C++, 461+ mapper implementations

### Test ROMs
- Holy Mapperel MMC3 tests
- Blargg mapper tests

---

## Acceptance Criteria

- [ ] Mappers clocked at correct points in `on_cpu_cycle()`
- [ ] MMC3 A12 edge detection per PPU dot with filter
- [ ] MMC3 IRQ fires on counter transition (not already 0)
- [ ] All Holy Mapperel MMC3 tests pass
- [ ] SMB3/Mega Man 3 status bars work correctly
- [ ] Game-specific timing tests pass
- [ ] Zero regressions on existing mapper tests

---

**Status:** PLANNED
**Created:** 2025-12-28
