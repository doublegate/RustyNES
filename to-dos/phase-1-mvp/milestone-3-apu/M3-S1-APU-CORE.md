# [Milestone 3] Sprint 3.1: APU Core & Frame Counter

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 2025
**Duration:** ~1 week (actual)
**Assignee:** Claude Code / Developer

---

## Overview

Establish the APU foundation with register interface, frame counter, and shared components (envelope, length counter, sweep). This sprint creates the infrastructure that all audio channels will build upon.

---

## Acceptance Criteria

- [x] APU register map ($4000-$4017) implemented
- [x] Frame counter with 4-step and 5-step modes
- [x] Frame IRQ generation (4-step mode only)
- [x] Envelope generator (shared by pulse/noise)
- [x] Length counter (shared by all channels)
- [x] Sweep unit (pulse channels only)
- [x] Zero unsafe code
- [x] Comprehensive unit tests

---

## Tasks

### 3.1.1 Create APU Crate Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Set up the rustynes-apu crate with initial file structure and dependencies.

**Files:**

- `crates/rustynes-apu/Cargo.toml` - Crate manifest
- `crates/rustynes-apu/src/lib.rs` - Public API
- `crates/rustynes-apu/src/apu.rs` - Main APU struct

**Subtasks:**

- [ ] Create Cargo.toml with dependencies
  - [ ] Add `bitflags = "2.4"` for register flags
  - [ ] Add `log = "0.4"` for logging
  - [ ] Add optional `blip_buf` for band-limited synthesis
- [ ] Set up lib.rs with public exports
- [ ] Create initial module structure
- [ ] Add documentation and README

**Implementation:**

```rust
// Cargo.toml
[package]
name = "rustynes-apu"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[dependencies]
bitflags = "2.4"
log = "0.4"

[dev-dependencies]
```

---

### 3.1.2 APU Register Map

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement memory-mapped register interface for all APU channels ($4000-$4017).

**Files:**

- `crates/rustynes-apu/src/apu.rs` - Register handlers
- `crates/rustynes-apu/src/registers.rs` - Register definitions

**Subtasks:**

- [ ] Define register address constants
- [ ] Implement read_register($4015 - Status)
- [ ] Implement write_register dispatcher
- [ ] Pulse 1 registers ($4000-$4003)
- [ ] Pulse 2 registers ($4004-$4007)
- [ ] Triangle registers ($4008-$400B)
- [ ] Noise registers ($400C-$400F)
- [ ] DMC registers ($4010-$4013)
- [ ] Status register ($4015)
- [ ] Frame counter register ($4017)

**Implementation:**

```rust
pub struct Apu {
    pub cycles: u64,

    // Channels (will be added in later sprints)
    // pulse1: PulseChannel,
    // pulse2: PulseChannel,
    // triangle: TriangleChannel,
    // noise: NoiseChannel,
    // dmc: DmcChannel,

    frame_counter: FrameCounter,
}

impl Apu {
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => self.read_status(),
            _ => 0, // Write-only registers return open bus
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // Pulse 1
            0x4000 => {} // Duty, envelope, volume
            0x4001 => {} // Sweep
            0x4002 => {} // Timer low
            0x4003 => {} // Length counter, timer high

            // Pulse 2
            0x4004 => {} // Duty, envelope, volume
            0x4005 => {} // Sweep
            0x4006 => {} // Timer low
            0x4007 => {} // Length counter, timer high

            // Triangle
            0x4008 => {} // Linear counter
            0x400A => {} // Timer low
            0x400B => {} // Length counter, timer high

            // Noise
            0x400C => {} // Envelope, volume
            0x400E => {} // Period
            0x400F => {} // Length counter

            // DMC
            0x4010 => {} // Flags, rate
            0x4011 => {} // Direct load
            0x4012 => {} // Sample address
            0x4013 => {} // Sample length

            // Status
            0x4015 => self.write_status(value),

            // Frame counter
            0x4017 => self.frame_counter.write_control(value),

            _ => {}
        }
    }
}
```

---

### 3.1.3 Frame Counter Implementation

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 4 hours

**Description:**
Implement the frame counter that sequences envelope, length counter, and sweep updates.

**Files:**

- `crates/rustynes-apu/src/frame_counter.rs` - Frame counter logic

**Subtasks:**

- [ ] Frame counter struct with mode flag
- [ ] IRQ inhibit flag
- [ ] 4-step mode sequencing (7457, 14913, 22371, 29829 cycles)
- [ ] 5-step mode sequencing (7457, 14913, 22371, 37281 cycles)
- [ ] Quarter frame actions (envelopes + linear counter)
- [ ] Half frame actions (length counters + sweep)
- [ ] IRQ generation (4-step mode only)
- [ ] Write to $4017 handling (immediate effects)

**Implementation:**

```rust
pub struct FrameCounter {
    mode: u8,              // 0 = 4-step, 1 = 5-step
    irq_inhibit: bool,     // I flag in $4017
    cycle_count: u64,      // Cycles since last reset
    irq_flag: bool,        // Frame IRQ flag
}

impl FrameCounter {
    pub fn new() -> Self {
        Self {
            mode: 0,
            irq_inhibit: false,
            cycle_count: 0,
            irq_flag: false,
        }
    }

    pub fn write_control(&mut self, value: u8) {
        self.mode = (value >> 7) & 1;
        self.irq_inhibit = (value & 0x40) != 0;

        // Writing to $4017 resets the frame counter
        self.cycle_count = 0;

        if self.irq_inhibit {
            self.irq_flag = false;
        }

        // If 5-step mode, clock immediately
        if self.mode == 1 {
            // Clock half frame immediately
        }
    }

    pub fn clock(&mut self) -> FrameAction {
        self.cycle_count += 1;

        match self.mode {
            0 => self.clock_4step(),
            1 => self.clock_5step(),
            _ => FrameAction::None,
        }
    }

    fn clock_4step(&mut self) -> FrameAction {
        match self.cycle_count {
            7457 => FrameAction::QuarterFrame,
            14913 => FrameAction::HalfFrame,
            22371 => FrameAction::QuarterFrame,
            29829 => {
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
                FrameAction::HalfFrame
            }
            29830 | 29831 => {
                // Additional IRQ flag sets
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
                if self.cycle_count == 29831 {
                    self.cycle_count = 0;
                }
                FrameAction::None
            }
            _ => FrameAction::None,
        }
    }

    fn clock_5step(&mut self) -> FrameAction {
        match self.cycle_count {
            7457 => FrameAction::QuarterFrame,
            14913 => FrameAction::HalfFrame,
            22371 => FrameAction::QuarterFrame,
            37281 => {
                self.cycle_count = 0;
                FrameAction::HalfFrame
            }
            _ => FrameAction::None,
        }
    }

    pub fn irq_pending(&self) -> bool {
        self.irq_flag && !self.irq_inhibit
    }
}

pub enum FrameAction {
    None,
    QuarterFrame,  // Clock envelopes and linear counter
    HalfFrame,     // Clock envelopes, linear, length, and sweep
}
```

---

### 3.1.4 Envelope Generator

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the envelope generator used by pulse and noise channels.

**Files:**

- `crates/rustynes-apu/src/envelope.rs` - Envelope logic

**Subtasks:**

- [ ] Envelope struct with divider and decay counter
- [ ] Start flag and loop flag
- [ ] Constant volume vs. envelope mode
- [ ] Divider clocking (240 Hz rate)
- [ ] Decay level counter (0-15)
- [ ] Loop behavior

**Implementation:**

```rust
pub struct Envelope {
    start_flag: bool,
    loop_flag: bool,
    constant_volume: bool,
    volume: u8,          // V bits (0-15)

    divider: u8,         // Reload value = V
    decay_level: u8,     // Current envelope level (0-15)
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            start_flag: false,
            loop_flag: false,
            constant_volume: false,
            volume: 0,
            divider: 0,
            decay_level: 0,
        }
    }

    pub fn write_register(&mut self, value: u8) {
        self.loop_flag = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.volume = value & 0x0F;
    }

    pub fn start(&mut self) {
        self.start_flag = true;
    }

    pub fn clock(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.decay_level = 15;
            self.divider = self.volume;
        } else {
            if self.divider == 0 {
                self.divider = self.volume;

                if self.decay_level == 0 {
                    if self.loop_flag {
                        self.decay_level = 15;
                    }
                } else {
                    self.decay_level -= 1;
                }
            } else {
                self.divider -= 1;
            }
        }
    }

    pub fn output(&self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay_level
        }
    }
}
```

---

### 3.1.5 Length Counter

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the length counter used by all channels except DMC.

**Files:**

- `crates/rustynes-apu/src/length_counter.rs` - Length counter logic

**Subtasks:**

- [ ] Length counter struct
- [ ] Lookup table (32 values)
- [ ] Halt flag (from channel)
- [ ] Clock behavior (decrement if not halted)
- [ ] Load from register write
- [ ] Channel silencing when counter reaches 0

**Implementation:**

```rust
pub struct LengthCounter {
    counter: u8,
    halt: bool,
}

impl LengthCounter {
    pub fn new() -> Self {
        Self {
            counter: 0,
            halt: false,
        }
    }

    pub fn load(&mut self, index: u8) {
        self.counter = LENGTH_TABLE[index as usize];
    }

    pub fn set_halt(&mut self, halt: bool) {
        self.halt = halt;
    }

    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.counter = 0;
        }
    }
}

// Length counter lookup table
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20,  2, 40,  4, 80,  6,  // 0-7
    160,  8, 60, 10, 14, 12, 26, 14,  // 8-15
    12,  16, 24, 18, 48, 20, 96, 22,  // 16-23
    192, 24, 72, 26, 16, 28, 32, 30,  // 24-31
];
```

---

### 3.1.6 Sweep Unit

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Implement the sweep unit used by pulse channels for frequency modulation.

**Files:**

- `crates/rustynes-apu/src/sweep.rs` - Sweep logic

**Subtasks:**

- [ ] Sweep struct with divider and shift
- [ ] Enable flag and negate flag
- [ ] Reload flag
- [ ] Target period calculation
- [ ] One's complement (Pulse 1) vs two's complement (Pulse 2)
- [ ] Muting conditions (period < 8 or target > $7FF)
- [ ] Clock behavior

**Implementation:**

```rust
pub struct Sweep {
    enabled: bool,
    negate: bool,
    shift: u8,
    period: u8,
    divider: u8,
    reload_flag: bool,
    channel: u8,     // 0 = Pulse 1, 1 = Pulse 2 (for complement mode)
}

impl Sweep {
    pub fn new(channel: u8) -> Self {
        Self {
            enabled: false,
            negate: false,
            shift: 0,
            period: 0,
            divider: 0,
            reload_flag: false,
            channel,
        }
    }

    pub fn write_register(&mut self, value: u8) {
        self.enabled = (value & 0x80) != 0;
        self.period = (value >> 4) & 0x07;
        self.negate = (value & 0x08) != 0;
        self.shift = value & 0x07;
        self.reload_flag = true;
    }

    pub fn clock(&mut self, timer: &mut u16) {
        if self.divider == 0 && self.enabled && self.shift != 0 && !self.is_muted(*timer) {
            *timer = self.target_period(*timer);
        }

        if self.divider == 0 || self.reload_flag {
            self.divider = self.period;
            self.reload_flag = false;
        } else {
            self.divider -= 1;
        }
    }

    fn target_period(&self, timer: u16) -> u16 {
        let change = timer >> self.shift;

        if self.negate {
            // One's complement for Pulse 1, two's complement for Pulse 2
            if self.channel == 0 {
                timer.wrapping_sub(change).wrapping_sub(1)
            } else {
                timer.wrapping_sub(change)
            }
        } else {
            timer.wrapping_add(change)
        }
    }

    pub fn is_muted(&self, timer: u16) -> bool {
        timer < 8 || self.target_period(timer) > 0x7FF
    }
}
```

---

### 3.1.7 Status Register ($4015)

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the status register for enabling/disabling channels and reading status.

**Files:**

- `crates/rustynes-apu/src/apu.rs` - Status register handlers

**Subtasks:**

- [ ] Read status (channel length counters, DMC active, IRQ flags)
- [ ] Write status (enable/disable channels)
- [ ] Clear DMC IRQ on read
- [ ] Set length counters to 0 when disabled
- [ ] Restart DMC if enabled with 0 bytes remaining

**Implementation:**

```rust
impl Apu {
    fn read_status(&mut self) -> u8 {
        let mut status = 0;

        // Bit 0-3: Channel length counter status
        // if self.pulse1.length_counter.is_active() { status |= 0x01; }
        // if self.pulse2.length_counter.is_active() { status |= 0x02; }
        // if self.triangle.length_counter.is_active() { status |= 0x04; }
        // if self.noise.length_counter.is_active() { status |= 0x08; }

        // Bit 4: DMC active
        // if self.dmc.bytes_remaining > 0 { status |= 0x10; }

        // Bit 6: Frame IRQ
        if self.frame_counter.irq_flag { status |= 0x40; }

        // Bit 7: DMC IRQ
        // if self.dmc.irq_flag { status |= 0x80; }

        // Reading $4015 clears the frame IRQ flag
        self.frame_counter.irq_flag = false;

        status
    }

    fn write_status(&mut self, value: u8) {
        // Enable/disable channels
        // self.pulse1.set_enabled((value & 0x01) != 0);
        // self.pulse2.set_enabled((value & 0x02) != 0);
        // self.triangle.set_enabled((value & 0x04) != 0);
        // self.noise.set_enabled((value & 0x08) != 0);
        // self.dmc.set_enabled((value & 0x10) != 0);
    }
}
```

---

### 3.1.8 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for frame counter, envelope, length counter, and sweep.

**Files:**

- `crates/rustynes-apu/src/lib.rs` - Test module

**Subtasks:**

- [ ] Test frame counter 4-step sequencing
- [ ] Test frame counter 5-step sequencing
- [ ] Test frame IRQ generation
- [ ] Test envelope decay
- [ ] Test envelope loop
- [ ] Test length counter loading
- [ ] Test length counter halt
- [ ] Test sweep target calculation
- [ ] Test sweep muting

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_counter_4step() {
        let mut fc = FrameCounter::new();
        fc.mode = 0;

        // Clock to first quarter frame
        for _ in 0..7457 {
            fc.clock();
        }
        assert_eq!(fc.cycle_count, 7457);

        // Test full sequence
        fc.clock();
        assert_eq!(fc.cycle_count, 7458);
    }

    #[test]
    fn test_envelope_constant_volume() {
        let mut env = Envelope::new();
        env.write_register(0x1F); // Constant volume, V=15

        assert_eq!(env.output(), 15);
        env.clock();
        assert_eq!(env.output(), 15); // Should not change
    }

    #[test]
    fn test_envelope_decay() {
        let mut env = Envelope::new();
        env.write_register(0x0F); // Envelope mode, V=15
        env.start();

        // First clock after start sets decay to 15
        env.clock();
        assert_eq!(env.output(), 15);

        // Clock divider V+1 times to decrement
        for _ in 0..16 {
            env.clock();
        }
        assert_eq!(env.output(), 14);
    }

    #[test]
    fn test_length_counter_load() {
        let mut lc = LengthCounter::new();
        lc.load(0); // Load value 10

        assert_eq!(lc.counter, 10);
        assert!(lc.is_active());
    }

    #[test]
    fn test_length_counter_clock() {
        let mut lc = LengthCounter::new();
        lc.load(0); // Load value 10

        lc.clock();
        assert_eq!(lc.counter, 9);

        // Clock to 0
        for _ in 0..9 {
            lc.clock();
        }
        assert_eq!(lc.counter, 0);
        assert!(!lc.is_active());
    }

    #[test]
    fn test_sweep_target_period() {
        let sweep = Sweep::new(0); // Pulse 1
        // Test calculations
    }
}
```

---

## Dependencies

**Required:**

- Rust 1.75+ toolchain
- bitflags = "2.4"
- log = "0.4"

**Blocks:**

- Sprint 3.2: Pulse Channels (needs envelope, length counter, sweep)
- Sprint 3.3: Triangle & Noise (needs frame counter, length counter)

---

## Related Documentation

- [APU Overview](../../../docs/apu/APU_OVERVIEW.md)
- [APU 2A03 Specification](../../../docs/apu/APU_2A03_SPECIFICATION.md)
- [APU Frame Counter](../../../docs/apu/APU_FRAME_COUNTER.md)
- [NESdev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [NESdev Wiki - APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)

---

## Technical Notes

### Frame Counter Timing

The frame counter operates on CPU cycles, not APU-specific clocks. It's critical to maintain accurate cycle counting synchronized with the CPU.

### Write to $4017 Side Effects

Writing to $4017 has immediate effects:
- Resets the cycle counter to 0
- If 5-step mode, immediately clocks half frame
- Clears IRQ flag if IRQ inhibit is set

### Envelope Loop Flag

The envelope loop flag is the same as the length counter halt flag for pulse and noise channels.

---

## Test Requirements

- [ ] Unit tests for frame counter sequencing (4-step and 5-step)
- [ ] Unit tests for envelope generator (constant volume and decay)
- [ ] Unit tests for length counter (load, clock, halt)
- [ ] Unit tests for sweep unit (target period, muting)
- [ ] Integration test: Frame counter + envelope coordination
- [ ] Integration test: Status register read/write behavior

---

## Performance Targets

- Frame counter: <10 ns per clock
- Envelope/length/sweep: <20 ns per clock
- Memory: <1 KB for all shared components

---

## Success Criteria

- [ ] All unit tests pass
- [ ] Frame counter sequences correctly
- [ ] Envelope generator produces correct output
- [ ] Length counter silences channels appropriately
- [ ] Sweep unit calculates target periods correctly
- [ ] Status register reads/writes work correctly
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Next Sprint:** [Sprint 3.2: Pulse Channels](M3-S2-PULSE-CHANNELS.md)
