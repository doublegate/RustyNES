# M11-S4: Bus and DMA Integration

**Sprint:** S4 (Bus and DMA)
**Milestone:** M11 (Sub-Cycle Accuracy)
**Duration:** 2 weeks (15-20 hours)
**Status:** PLANNED
**Priority:** HIGH - Correct DMA timing
**Depends On:** S1 (CPU Refactor), S2 (PPU Sync)

---

## Overview

Implement the `on_cpu_cycle()` callback in the Bus, integrate OAM DMA cycle stealing with the new cycle-by-cycle framework, handle DMC DMA conflicts, implement proper open bus behavior, and remove instruction-level stepping from Console.

---

## Dependencies

### Required Before Starting
- **S1 (CPU Refactor)** - `on_cpu_cycle()` trait method must exist
- **S2 (PPU Sync)** - PPU stepping integration should be defined

### Parallel Work
- **S3 (APU)** - Can be integrated concurrently

### Blocks
- **S5 (Mappers)** - Mapper clocking depends on Bus integration
- **S6 (Testing)** - DMA timing tests require this sprint

---

## Current Implementation

### Bus Structure

```rust
// crates/rustynes-core/src/bus.rs
pub struct Bus {
    ram: [u8; 0x800],
    prg_ram: [u8; 0x2000],
    pub ppu: Ppu,
    pub apu: Apu,
    pub mapper: Box<dyn Mapper>,
    pub controller1: Controller,
    pub controller2: Controller,

    // OAM DMA state
    dma_page: u8,
    dma_addr: u8,
    dma_data: u8,
    dma_dummy_cycles: u8,
    dma_transfer: bool,
    dma_write: bool,
    cpu_cycles: u64,  // For odd/even tracking
}
```

### CpuBus Trait (Current)

```rust
impl CpuBus for Bus {
    fn read(&mut self, addr: u16) -> u8 { /* ... */ }
    fn write(&mut self, addr: u16, val: u8) { /* ... */ }
    fn peek(&self, addr: u16) -> u8 { /* ... */ }
    // No on_cpu_cycle() currently
}
```

### Console Stepping (Current)

```rust
// crates/rustynes-core/src/console.rs
pub fn tick(&mut self) -> (bool, bool) {
    // Step PPU BEFORE CPU (partial fix)
    for _ in 0..3 {
        let (fc, nmi) = self.bus.step_ppu();
        // ...
    }

    // DMA handling outside cycle framework
    if self.bus.dma_active() {
        let dma_done = self.bus.tick_dma();
        // ...
    }

    // CPU tick
    let instruction_complete = self.cpu.tick(&mut self.bus);
    // ...
}
```

---

## Required Changes

### Task S4.1: Implement on_cpu_cycle() in Bus

**Priority:** P0
**Effort:** 4 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`
- `crates/rustynes-cpu/src/lib.rs`

#### Subtasks
- [ ] Add `on_cpu_cycle()` to CpuBus trait (if not done in S1)
- [ ] Implement `on_cpu_cycle()` in Bus
- [ ] Step PPU 3 times per call
- [ ] Step APU 1 time per call
- [ ] Clock mapper per cycle
- [ ] Track NMI/IRQ state changes

#### Implementation Notes

```rust
impl CpuBus for Bus {
    fn on_cpu_cycle(&mut self) {
        // Increment CPU cycle counter (for DMA timing)
        self.cpu_cycles += 1;

        // Step PPU 3 times (3:1 PPU:CPU ratio)
        for _ in 0..3 {
            let result = self.ppu.step_with_chr(|addr| {
                self.mapper.read_chr(addr)
            });

            // Collect NMI status
            if result.nmi {
                self.nmi_pending = true;
            }

            // Track frame completion
            if result.frame_complete {
                self.frame_ready = true;
            }
        }

        // Step APU once per CPU cycle
        self.apu.step();

        // Clock mapper (for cycle-based timers like MMC3)
        self.mapper.clock(1);
    }

    fn poll_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;  // Clear on read (edge-triggered)
        pending
    }

    fn poll_irq(&mut self) -> bool {
        // Level-triggered: returns current state
        self.mapper.irq_pending() || self.apu.irq_pending()
    }
}
```

---

### Task S4.2: Open Bus Behavior Implementation

**Priority:** P1
**Effort:** 3 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`

#### Subtasks
- [ ] Add `last_bus_value: u8` field to Bus
- [ ] Update `last_bus_value` on every read
- [ ] Return `last_bus_value` for unmapped addresses
- [ ] Handle partial open bus for PPU registers
- [ ] Test open bus behavior with known test ROMs

#### Implementation Notes

```rust
pub struct Bus {
    // ... existing fields ...

    /// Last value on data bus (for open bus behavior)
    last_bus_value: u8,
}

impl CpuBus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => {
                // PPU registers have partial open bus behavior
                self.read_ppu_register(addr)
            }
            0x4000..=0x4014 => self.apu.read_register(addr),
            0x4015 => self.apu.read_register(addr),
            0x4016 => self.controller1.read() | (self.last_bus_value & 0xE0),
            0x4017 => self.controller2.read() | (self.last_bus_value & 0xE0),
            0x4018..=0x401F => self.last_bus_value,  // Open bus
            0x4020..=0x5FFF => self.last_bus_value,  // Open bus (unless mapper)
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.mapper.read_prg(addr),
        };

        self.last_bus_value = value;
        value
    }
}
```

**Open Bus Specifics:**
- Controllers: Bits 0-4 are data, bits 5-7 are open bus
- PPU $2002: Bits 0-4 are open bus (last write to $2000-$2007)
- Unmapped ranges return last bus value

---

### Task S4.3: OAM DMA Cycle-Level Integration

**Priority:** P0
**Effort:** 5 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`
- `crates/rustynes-cpu/src/cpu.rs`

#### Subtasks
- [ ] Restructure DMA to work within `on_cpu_cycle()` framework
- [ ] Handle odd/even CPU cycle alignment (513 vs 514 cycles)
- [ ] Implement read/write alternation during DMA
- [ ] Ensure PPU/APU continue stepping during DMA
- [ ] Verify DMA timing with test ROMs

#### Implementation Notes

OAM DMA timing:
```text
DMA Sequence (starting on even cycle):
  Cycle 0: Dummy cycle (alignment)
  Cycle 1: Read $XX00 from source
  Cycle 2: Write to $2004
  Cycle 3: Read $XX01
  Cycle 4: Write to $2004
  ...
  Cycle 513: Write final byte to $2004

DMA Sequence (starting on odd cycle):
  Cycle 0: Dummy cycle 1 (alignment)
  Cycle 1: Dummy cycle 2 (alignment)
  Cycle 2: Read $XX00
  Cycle 3: Write to $2004
  ...
  Cycle 514: Write final byte to $2004
```

```rust
// DMA state machine
pub enum DmaState {
    Idle,
    DummyCycle { remaining: u8 },
    Read,
    Write,
}

impl Bus {
    /// Execute one DMA cycle
    /// Returns true if DMA is complete
    pub fn tick_dma_cycle(&mut self) -> bool {
        match self.dma_state {
            DmaState::Idle => true,

            DmaState::DummyCycle { remaining } => {
                if remaining > 1 {
                    self.dma_state = DmaState::DummyCycle { remaining: remaining - 1 };
                } else {
                    self.dma_state = DmaState::Read;
                }
                false
            }

            DmaState::Read => {
                let addr = u16::from(self.dma_page) << 8 | u16::from(self.dma_addr);
                self.dma_data = self.read_for_dma(addr);
                self.dma_state = DmaState::Write;
                false
            }

            DmaState::Write => {
                self.ppu.write_oam_data(self.dma_data);
                self.dma_addr = self.dma_addr.wrapping_add(1);

                if self.dma_addr == 0 {
                    self.dma_state = DmaState::Idle;
                    true
                } else {
                    self.dma_state = DmaState::Read;
                    false
                }
            }
        }
    }
}
```

---

### Task S4.4: DMC DMA Conflict Handling

**Priority:** P1
**Effort:** 4 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`
- `crates/rustynes-apu/src/dmc.rs`

#### Subtasks
- [ ] Detect DMC sample buffer empty condition
- [ ] Implement CPU stall for DMC DMA (1-4 cycles)
- [ ] Handle DMC DMA during OAM DMA
- [ ] Handle DMC DMA during instruction execution
- [ ] Verify DMC sample address wrapping ($FFFF -> $8000)

#### Implementation Notes

DMC DMA has priority over OAM DMA and can interrupt instruction execution:

```rust
impl Bus {
    /// Check and execute DMC DMA if needed
    /// Returns number of stall cycles (0 if no DMA)
    fn check_dmc_dma(&mut self, cpu_in_read_cycle: bool) -> u8 {
        if !self.apu.dmc_needs_sample() {
            return 0;
        }

        // Determine stall cycles based on CPU state
        let stall_cycles = if cpu_in_read_cycle { 4 } else { 3 };

        // Perform DMC DMA
        let addr = self.apu.dmc_sample_address();
        let sample = self.read_for_dma(addr);
        self.apu.dmc_load_sample(sample);

        stall_cycles
    }
}
```

**DMC DMA vs OAM DMA:**
- If OAM DMA is in progress and DMC needs sample:
  - DMC DMA executes during OAM DMA read cycle
  - OAM DMA continues after DMC completes
  - Total time = OAM DMA + DMC DMA (no overlap)

---

### Task S4.5: Remove Instruction-Level Stepping from Console

**Priority:** P0
**Effort:** 2 hours
**Files:**
- `crates/rustynes-core/src/console.rs`

#### Subtasks
- [ ] Remove PPU catch-up stepping from `step()` method
- [ ] Remove APU catch-up stepping from `step()` method
- [ ] Update `tick()` to rely on `on_cpu_cycle()` for all stepping
- [ ] Verify frame timing is preserved
- [ ] Update documentation comments

#### Implementation Notes

```rust
// BEFORE: Console handles PPU/APU stepping
pub fn tick(&mut self) -> (bool, bool) {
    for _ in 0..3 {
        let (fc, nmi) = self.bus.step_ppu();  // REMOVE
        // ...
    }
    let instruction_complete = self.cpu.tick(&mut self.bus);
    self.bus.apu.step();  // REMOVE
    // ...
}

// AFTER: All stepping happens in on_cpu_cycle()
pub fn tick(&mut self) -> (bool, bool) {
    // Handle DMA if active
    if self.bus.dma_active() {
        self.bus.on_cpu_cycle();  // PPU/APU still step during DMA
        let dma_done = self.bus.tick_dma_cycle();
        return (false, self.bus.frame_ready());
    }

    // Execute one CPU cycle (on_cpu_cycle called before each memory access)
    let instruction_complete = self.cpu.tick(&mut self.bus);

    // Check for frame completion
    let frame_complete = self.bus.frame_ready();

    // Poll interrupts
    if self.bus.poll_nmi() {
        self.cpu.trigger_nmi();
    }
    let irq = self.bus.poll_irq();
    self.cpu.set_irq(irq);

    (instruction_complete, frame_complete)
}
```

---

### Task S4.6: Bus Conflict Handling

**Priority:** P2
**Effort:** 2 hours
**Files:**
- `crates/rustynes-core/src/bus.rs`
- `crates/rustynes-mappers/src/*.rs`

#### Subtasks
- [ ] Document bus conflict behavior for each mapper
- [ ] Implement bus conflict for mappers that require it (BNROM, GNROM)
- [ ] Test with games that depend on bus conflict behavior
- [ ] Add mapper flag for bus conflict support

#### Implementation Notes

Bus conflicts occur when CPU writes to ROM space:

```rust
// Mappers with bus conflicts (AND with ROM data):
// BNROM (34), GNROM (66), Action 52 (228)

impl Mapper for BnromMapper {
    fn write_prg(&mut self, addr: u16, value: u8) {
        // Bus conflict: written value AND'd with ROM content
        let rom_value = self.prg_rom[(addr & self.prg_mask) as usize];
        let effective_value = value & rom_value;
        self.prg_bank = (effective_value & 0x03) as usize;
    }
}
```

---

## Testing Requirements

### Unit Tests
- [ ] `on_cpu_cycle()` called correct number of times per instruction
- [ ] Open bus returns last read value
- [ ] OAM DMA takes exactly 513/514 cycles
- [ ] DMC DMA stall timing (1-4 cycles)
- [ ] DMA/instruction interleaving

### Integration Tests
- [ ] Blargg OAM DMA timing tests
- [ ] Games using mid-frame DMA (Battletoads)
- [ ] DMC sample playback timing

---

## Validation Criteria

| Criterion | Target | Current |
|-----------|--------|---------|
| OAM DMA cycle count | 513/514 exact | 512-514 |
| DMC DMA stall | 1-4 cycles | 3 fixed |
| Open bus behavior | Correct | Not implemented |
| PPU stepping during DMA | Yes | Partial |
| APU stepping during DMA | Yes | Partial |

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| DMA timing breaks games | High | Medium | Extensive game testing |
| Open bus edge cases | Medium | High | Reference accurate emulators |
| Performance regression | Medium | Low | Profile DMA paths |
| Interrupt timing issues | High | Medium | Careful NMI/IRQ integration |

---

## References

### Internal Documentation
- [Bus Memory Map](../../../docs/bus/BUS_MEMORY_MAP.md)
- [OAM DMA](../../../docs/bus/OAM_DMA.md)

### External Resources
- [NESdev Wiki - DMA](https://www.nesdev.org/wiki/PPU_registers#OAMDMA)
- [NESdev Wiki - CPU Memory Map](https://www.nesdev.org/wiki/CPU_memory_map)
- [NESdev Wiki - Open Bus](https://www.nesdev.org/wiki/Open_bus_behavior)

### Reference Implementations
- **Pinky** (`ref-proj/pinky/nes/src/`) - DMA integration with `on_cpu_cycle()`
- **Mesen2** - C++, accurate DMA timing

---

## Acceptance Criteria

- [ ] `on_cpu_cycle()` correctly steps PPU 3x and APU 1x
- [ ] OAM DMA executes exactly 513/514 cycles based on CPU parity
- [ ] DMA properly interleaves with PPU/APU stepping
- [ ] DMC DMA stall cycles accurate (1-4 based on CPU state)
- [ ] Open bus returns correct values
- [ ] Console no longer does catch-up stepping
- [ ] All existing tests pass (zero regressions)

---

**Status:** PLANNED
**Created:** 2025-12-28
