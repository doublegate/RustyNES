# [Milestone 5] Sprint 5.3: Console Coordinator

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer

---

## Overview

Implement the master Console struct that coordinates execution of all emulation subsystems (CPU, PPU, APU) with accurate clock synchronization. This sprint creates the heart of the emulator that orchestrates component interaction.

### Goals

- Console struct integrating CPU, PPU, APU, Bus
- Master clock synchronization (21.477272 MHz NTSC)
- Component stepping with correct frequency ratios
- Frame execution loop (step_frame method)
- NMI/IRQ delivery to CPU
- Power-on and reset sequences
- Cycle-accurate timing
- Zero unsafe code

---

## Acceptance Criteria

- [ ] Console struct created with all subsystems
- [ ] Master clock runs at 21.477272 MHz
- [ ] CPU steps every 12 master cycles
- [ ] PPU steps every 4 master cycles (3 dots per CPU cycle)
- [ ] APU steps every 12 master cycles
- [ ] Frame execution completes in 29,780 CPU cycles
- [ ] NMI delivered from PPU to CPU correctly
- [ ] IRQ delivered from APU/Mapper to CPU correctly
- [ ] Power-on state matches hardware
- [ ] Reset sequence accurate
- [ ] Comprehensive unit tests
- [ ] Zero unsafe code

---

## Tasks

### Task 1: Define Console Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Define the main Console struct that owns all emulation subsystems.

**Files:**

- `crates/rustynes-core/src/console.rs` - Console struct definition
- `crates/rustynes-core/src/lib.rs` - Public API exports

**Subtasks:**

- [ ] Define Console struct with CPU, Bus, clocks
- [ ] Add master clock counter (u64)
- [ ] Add frame counter (u64)
- [ ] Add configuration options
- [ ] Implement Debug trait
- [ ] Add getters for subsystems

**Implementation:**

```rust
use rustynes_cpu::Cpu;
use rustynes_mappers::Mapper;
use crate::bus::NesBus;

/// Main NES console coordinating all emulation subsystems
pub struct Console {
    /// 6502 CPU
    cpu: Cpu,

    /// System bus connecting all components
    bus: NesBus,

    /// Master clock counter (21.477272 MHz NTSC)
    master_clock: u64,

    /// Frame counter
    frame_count: u64,

    /// Console region (NTSC/PAL)
    region: Region,

    /// Cycles since last frame
    frame_cycles: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// NTSC (North America, Japan)
    /// Master: 21.477272 MHz
    /// CPU: 1.789773 MHz
    /// PPU: 5.369318 MHz
    /// Frame: 29,780 CPU cycles
    Ntsc,

    /// PAL (Europe, Australia)
    /// Master: 26.601712 MHz
    /// CPU: 1.662607 MHz
    /// PPU: 5.320214 MHz
    /// Frame: 33,247 CPU cycles
    Pal,
}

impl Region {
    pub fn master_clock_hz(&self) -> u64 {
        match self {
            Region::Ntsc => 21_477_272,
            Region::Pal => 26_601_712,
        }
    }

    pub fn cpu_divisor(&self) -> u64 {
        12 // Both NTSC and PAL
    }

    pub fn ppu_divisor(&self) -> u64 {
        4 // Both NTSC and PAL
    }

    pub fn frame_cpu_cycles(&self) -> u32 {
        match self {
            Region::Ntsc => 29_780,
            Region::Pal => 33_247,
        }
    }
}
```

---

### Task 2: Console Initialization

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement Console creation and initialization.

**Files:**

- `crates/rustynes-core/src/console.rs` - Console::new method

**Subtasks:**

- [ ] Implement Console::new taking ROM and config
- [ ] Create mapper from ROM
- [ ] Initialize PPU with mirroring mode
- [ ] Initialize APU
- [ ] Create Bus with all components
- [ ] Initialize CPU
- [ ] Run power-on sequence
- [ ] Set initial clocks to 0

**Implementation:**

```rust
use rustynes_mappers::{Rom, create_mapper};
use rustynes_ppu::Ppu;
use rustynes_apu::Apu;

impl Console {
    /// Create new console with ROM
    pub fn new(rom: Rom) -> Result<Self, ConsoleError> {
        // Create mapper from ROM
        let mapper = create_mapper(rom)?;
        let mirroring = mapper.mirroring();

        // Initialize subsystems
        let ppu = Ppu::new(mirroring);
        let apu = Apu::new();
        let bus = NesBus::new(ppu, apu, mapper);

        // Create CPU
        let mut cpu = Cpu::new();

        // Create console
        let mut console = Self {
            cpu,
            bus,
            master_clock: 0,
            frame_count: 0,
            region: Region::Ntsc,
            frame_cycles: 0,
        };

        // Power-on sequence
        console.power_on();

        Ok(console)
    }

    /// Power-on initialization (cold boot)
    fn power_on(&mut self) {
        // CPU power-on state
        self.cpu.reset(&mut self.bus);

        // PPU power-on state
        self.bus.ppu.reset();

        // APU power-on state
        self.bus.apu.reset();

        // Reset clocks
        self.master_clock = 0;
        self.frame_count = 0;
        self.frame_cycles = 0;
    }

    /// Reset console (warm reset via RESET button)
    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
        self.bus.ppu.reset();
        self.bus.apu.reset();
        self.frame_cycles = 0;
        // Note: master_clock and frame_count preserved
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    #[error("Mapper error: {0}")]
    Mapper(#[from] rustynes_mappers::MapperError),

    #[error("ROM error: {0}")]
    Rom(#[from] rustynes_mappers::RomError),
}
```

---

### Task 3: Single Instruction Stepping

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement single CPU instruction execution with correct component synchronization.

**Files:**

- `crates/rustynes-core/src/console.rs` - Console::step method

**Subtasks:**

- [ ] Implement step method executing one CPU instruction
- [ ] Run PPU for 3 dots per CPU cycle
- [ ] Run APU for each CPU cycle
- [ ] Handle NMI from PPU
- [ ] Handle IRQ from APU/Mapper
- [ ] Handle DMA stalling
- [ ] Update master clock
- [ ] Return cycle count

**Implementation:**

```rust
impl Console {
    /// Execute one CPU instruction
    /// Returns the number of CPU cycles consumed
    pub fn step(&mut self) -> u8 {
        // Check for DMA
        if self.bus.dma_pending() {
            let dma_cycles = self.handle_dma();
            self.sync_components(dma_cycles);
            return dma_cycles;
        }

        // Check for DMC DMA
        if let Some(dmc_addr) = self.bus.poll_dmc_dma() {
            let sample = self.bus.fetch_dmc_sample(dmc_addr);
            self.bus.apu.load_dmc_sample(sample);
            // DMC DMA takes 4 cycles
            self.sync_components(4);
            return 4;
        }

        // Check for interrupts
        if self.bus.poll_nmi() {
            let cycles = self.cpu.trigger_nmi(&mut self.bus);
            self.sync_components(cycles);
            return cycles;
        }

        if self.bus.poll_irq() && !self.cpu.interrupt_disabled() {
            let cycles = self.cpu.trigger_irq(&mut self.bus);
            self.sync_components(cycles);
            return cycles;
        }

        // Execute one CPU instruction
        let cycles = self.cpu.step(&mut self.bus);

        // Run other components for same duration
        self.sync_components(cycles);

        cycles
    }

    /// Synchronize PPU and APU for given CPU cycles
    fn sync_components(&mut self, cpu_cycles: u8) {
        for _ in 0..cpu_cycles {
            // PPU runs 3 dots per CPU cycle
            for _ in 0..3 {
                self.bus.ppu.tick();
                self.master_clock += 4; // PPU dot = 4 master cycles
            }

            // APU runs once per CPU cycle
            self.bus.apu.tick();
        }

        self.frame_cycles += cpu_cycles as u32;
    }

    /// Handle OAM DMA (513/514 cycles)
    fn handle_dma(&mut self) -> u8 {
        let mut cycles = 0;

        // Dummy read (alignment wait)
        if self.master_clock % 2 == 1 {
            cycles += 1;
            self.sync_components(1);
        }

        // Transfer 256 bytes
        self.bus.execute_oam_dma();
        cycles += 256 * 2; // Read + write = 2 cycles per byte

        cycles
    }
}
```

---

### Task 4: Frame Execution

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement frame execution loop that runs until PPU completes one frame.

**Files:**

- `crates/rustynes-core/src/console.rs` - Console::step_frame method

**Subtasks:**

- [ ] Implement step_frame method
- [ ] Run until PPU frame complete
- [ ] Track frame cycles
- [ ] Increment frame counter
- [ ] Handle timing variations (29,780 or 29,781 cycles)
- [ ] Add frame completion check

**Implementation:**

```rust
impl Console {
    /// Execute until one frame is complete
    /// NTSC: 29,780 or 29,781 CPU cycles (odd frames skip one PPU dot)
    /// PAL: 33,247 CPU cycles
    pub fn step_frame(&mut self) {
        // Reset frame cycle counter
        self.frame_cycles = 0;

        // Run until frame complete
        let target_cycles = self.region.frame_cpu_cycles();

        loop {
            // Step one instruction
            let cycles = self.step();

            // Check if frame complete
            if self.bus.ppu.frame_complete() {
                self.frame_count += 1;
                break;
            }

            // Safety check: prevent infinite loops
            if self.frame_cycles > target_cycles + 100 {
                log::warn!(
                    "Frame exceeded expected cycles: {} > {}",
                    self.frame_cycles,
                    target_cycles
                );
                break;
            }
        }
    }

    /// Execute for a specific number of CPU cycles
    pub fn step_cycles(&mut self, target_cycles: u64) {
        let mut executed = 0;

        while executed < target_cycles {
            let cycles = self.step();
            executed += cycles as u64;
        }
    }

    /// Get current frame number
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Get total CPU cycles executed
    pub fn total_cycles(&self) -> u64 {
        self.cpu.cycles()
    }
}
```

---

### Task 5: Output Access

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 1 hour

**Description:**
Provide access to framebuffer and audio samples.

**Files:**

- `crates/rustynes-core/src/console.rs` - Output accessors

**Subtasks:**

- [ ] Add framebuffer accessor
- [ ] Add audio buffer accessor (consumes samples)
- [ ] Add frame dimensions constants
- [ ] Document output formats

**Implementation:**

```rust
impl Console {
    /// Get reference to current framebuffer (256×240 RGB888)
    pub fn framebuffer(&self) -> &[u8; 256 * 240 * 3] {
        self.bus.ppu.framebuffer()
    }

    /// Get audio samples and clear buffer
    /// Returns samples in f32 format (-1.0 to 1.0) at 48kHz
    pub fn audio_buffer(&mut self) -> Vec<f32> {
        self.bus.apu.take_samples()
    }

    /// Get frame dimensions
    pub const SCREEN_WIDTH: usize = 256;
    pub const SCREEN_HEIGHT: usize = 240;
    pub const SCREEN_PIXELS: usize = Self::SCREEN_WIDTH * Self::SCREEN_HEIGHT;
}
```

---

### Task 6: Input API

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 1 hour

**Description:**
Provide API for setting controller button states.

**Files:**

- `crates/rustynes-core/src/console.rs` - Input methods

**Subtasks:**

- [ ] Add set_button method
- [ ] Add set_controller_state method
- [ ] Export Controller and Button enums

**Implementation:**

```rust
use crate::input::{Controller, Button};

impl Console {
    /// Set individual button state
    pub fn set_button(
        &mut self,
        controller: Controller,
        button: Button,
        pressed: bool,
    ) {
        match controller {
            Controller::Controller1 => {
                self.bus.controller1.set_button(button, pressed);
            }
            Controller::Controller2 => {
                self.bus.controller2.set_button(button, pressed);
            }
        }
    }

    /// Set entire controller state (8-bit button mask)
    pub fn set_controller_state(
        &mut self,
        controller: Controller,
        state: u8,
    ) {
        match controller {
            Controller::Controller1 => {
                self.bus.controller1.set_state(state);
            }
            Controller::Controller2 => {
                self.bus.controller2.set_state(state);
            }
        }
    }
}
```

---

### Task 7: Timing Utilities

- **Status:** ⏳ Pending
- **Priority:** Low
- **Estimated:** 1 hour

**Description:**
Add timing and performance measurement utilities.

**Files:**

- `crates/rustynes-core/src/console.rs` - Timing utilities

**Subtasks:**

- [ ] Add FPS calculation
- [ ] Add cycle count tracking
- [ ] Add frame time estimation
- [ ] Add performance metrics

**Implementation:**

```rust
use std::time::{Duration, Instant};

impl Console {
    /// Get expected frame time for region
    pub fn frame_time(&self) -> Duration {
        match self.region {
            Region::Ntsc => Duration::from_nanos(16_666_666), // 60 Hz
            Region::Pal => Duration::from_nanos(20_000_000),  // 50 Hz
        }
    }

    /// Get expected frame rate for region
    pub fn frame_rate(&self) -> f64 {
        match self.region {
            Region::Ntsc => 60.0988,
            Region::Pal => 50.0070,
        }
    }

    /// Get CPU frequency for region
    pub fn cpu_frequency(&self) -> f64 {
        match self.region {
            Region::Ntsc => 1_789_772.5,
            Region::Pal => 1_662_607.0,
        }
    }

    /// Get master clock frequency for region
    pub fn master_frequency(&self) -> u64 {
        self.region.master_clock_hz()
    }
}
```

---

### Task 8: Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive tests for console coordination.

**Files:**

- `crates/rustynes-core/src/console.rs` - Test module

**Subtasks:**

- [ ] Test console creation
- [ ] Test single instruction stepping
- [ ] Test frame execution
- [ ] Test NMI delivery
- [ ] Test IRQ delivery
- [ ] Test DMA handling
- [ ] Test clock synchronization
- [ ] Test reset sequence

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rom() -> Rom {
        // Create minimal NROM (Mapper 0) ROM
        let mut rom_data = vec![0u8; 16 + 16384 + 8192]; // Header + 16KB PRG + 8KB CHR
        rom_data[0..4].copy_from_slice(b"NES\x1A");
        rom_data[4] = 1; // 1 x 16KB PRG-ROM
        rom_data[5] = 1; // 1 x 8KB CHR-ROM
        Rom::from_bytes(&rom_data).unwrap()
    }

    #[test]
    fn test_console_creation() {
        let rom = create_test_rom();
        let console = Console::new(rom).unwrap();

        assert_eq!(console.frame_count(), 0);
        assert_eq!(console.master_clock, 0);
    }

    #[test]
    fn test_single_step() {
        let rom = create_test_rom();
        let mut console = Console::new(rom).unwrap();

        // Step one instruction
        let cycles = console.step();

        // Should execute at least 2 cycles (fastest instruction)
        assert!(cycles >= 2);

        // Master clock should advance by cycles * 12
        assert_eq!(console.master_clock, (cycles as u64) * 12);
    }

    #[test]
    fn test_frame_execution() {
        let rom = create_test_rom();
        let mut console = Console::new(rom).unwrap();

        // Execute one frame
        console.step_frame();

        // Frame count should increment
        assert_eq!(console.frame_count(), 1);

        // Should execute approximately 29,780 CPU cycles (NTSC)
        assert!(console.frame_cycles >= 29_770);
        assert!(console.frame_cycles <= 29_790);
    }

    #[test]
    fn test_multiple_frames() {
        let rom = create_test_rom();
        let mut console = Console::new(rom).unwrap();

        // Execute 10 frames
        for _ in 0..10 {
            console.step_frame();
        }

        assert_eq!(console.frame_count(), 10);
    }

    #[test]
    fn test_reset() {
        let rom = create_test_rom();
        let mut console = Console::new(rom).unwrap();

        // Run for a while
        console.step_frame();
        let initial_cycles = console.total_cycles();

        // Reset
        console.reset();

        // Cycle count should be preserved (warm reset)
        assert_eq!(console.total_cycles(), initial_cycles);

        // Frame count should be preserved
        assert_eq!(console.frame_count(), 1);
    }

    #[test]
    fn test_timing_constants() {
        let console = Console::new(create_test_rom()).unwrap();

        assert_eq!(console.region, Region::Ntsc);
        assert_eq!(console.cpu_frequency(), 1_789_772.5);
        assert_eq!(console.frame_rate(), 60.0988);
    }
}
```

---

## Dependencies

**Required:**

- rustynes-cpu (CPU subsystem)
- rustynes-ppu (PPU subsystem)
- rustynes-apu (APU subsystem)
- rustynes-mappers (Mapper trait and implementations)
- Sprint 5.2: Bus & Memory Routing (needs NesBus)
- log = "0.4" (logging)
- thiserror = "1.0" (error handling)

**Blocks:**

- Sprint 5.4: ROM Loading (needs Console)
- Sprint 5.5: Save States (needs Console)
- Sprint 5.6: Input Handling (needs Console)
- Milestone 6: Desktop GUI (needs Console API)

---

## Related Documentation

- [Core API](../../../docs/api/CORE_API.md)
- [CPU Timing Reference](../../../docs/cpu/CPU_TIMING_REFERENCE.md)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)
- [Master Clock Timing](../../../ARCHITECTURE.md#timing-model)
- [NES Hardware Overview](../../../ARCHITECTURE.md#hardware-overview)

---

## Technical Notes

### Clock Frequencies

**NTSC:**

- Master: 21.477272 MHz
- CPU: 1.789773 MHz (master ÷ 12)
- PPU: 5.369318 MHz (master ÷ 4)
- Frame: 29,780 CPU cycles (29,781 on odd frames)

**PAL:**

- Master: 26.601712 MHz
- CPU: 1.662607 MHz (master ÷ 12 × 0.923)
- PPU: 5.320214 MHz (master ÷ 5)
- Frame: 33,247 CPU cycles

### Component Synchronization

For every CPU cycle:

- PPU advances 3 dots (3 PPU cycles)
- APU advances 1 cycle
- Master clock advances 12 ticks

### Interrupt Handling

**NMI (Non-Maskable Interrupt):**

- Triggered by PPU at start of VBlank (scanline 241, dot 1)
- Cannot be disabled
- Takes 7 CPU cycles to vector
- Vectors to address at $FFFA-$FFFB

**IRQ (Interrupt Request):**

- Triggered by APU frame counter or Mapper (MMC3 scanline counter)
- Can be disabled via CPU status register I flag
- Takes 7 CPU cycles to vector
- Vectors to address at $FFFE-$FFFF

### Frame Timing Variations

**NTSC Odd Frame:**

- Skips PPU dot (341, 0) on odd frames when rendering enabled
- Results in 29,781 CPU cycles instead of 29,780

**NTSC Even Frame:**

- Normal 262 scanlines × 341 dots
- Results in 29,780 CPU cycles

---

## Performance Targets

- **Frame time**: 16.67 ms (60 FPS NTSC)
- **Step overhead**: <100 ns per CPU instruction
- **Memory**: <10 KB for Console struct
- **Throughput**: 100+ FPS on modern CPUs

---

## Success Criteria

- [ ] Console struct created and compiles
- [ ] Console::new successfully creates emulator
- [ ] step method executes one CPU instruction
- [ ] step_frame executes one complete frame
- [ ] PPU runs 3 dots per CPU cycle
- [ ] APU runs once per CPU cycle
- [ ] NMI delivered correctly at VBlank
- [ ] IRQ delivered correctly from APU/Mapper
- [ ] DMA stalls CPU for correct cycle count
- [ ] Frame cycles match expected (29,780±1 for NTSC)
- [ ] Framebuffer accessor works
- [ ] Audio buffer accessor works
- [ ] Input API functional
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Next Sprint:** [Sprint 5.4: ROM Loading](M5-S4-rom-loading.md)
