# APU Channel Details

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Common Components](#common-components)
- [Pulse Channels](#pulse-channels)
- [Triangle Channel](#triangle-channel)
- [Noise Channel](#noise-channel)
- [DMC Channel](#dmc-channel)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

This document provides detailed specifications for each of the NES APU's 5 audio channels. Each channel combines several common components (timers, envelopes, length counters) with channel-specific features.

**Channel Summary:**

| Channel | Waveform | Volume Ctrl | Length Ctr | Envelope | Sweep | Special |
|---------|----------|-------------|------------|----------|-------|---------|
| **Pulse 1** | Square | Yes (4-bit) | Yes | Yes | Yes | Duty cycle |
| **Pulse 2** | Square | Yes (4-bit) | Yes | Yes | Yes | Duty cycle |
| **Triangle** | Triangle | No | Yes | No | No | Linear counter |
| **Noise** | LFSR | Yes (4-bit) | Yes | Yes | No | Mode select |
| **DMC** | 1-bit Delta | Yes (7-bit) | No | No | No | DMA, IRQ |

---

## Common Components

### Timer

All channels use a **timer** to control output frequency:

```rust
pub struct Timer {
    period: u16,      // Reload value
    counter: u16,     // Current count
}

impl Timer {
    pub fn clock(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period;
            true // Timer expired
        } else {
            self.counter -= 1;
            false
        }
    }
}
```

**Frequency Calculation:**
```
CPU Clock = 1.789773 MHz

Pulse/Noise: frequency = CPU_CLOCK / (16 × (period + 1))
Triangle:    frequency = CPU_CLOCK / (32 × (period + 1))
DMC:         frequency = CPU_CLOCK / (16 × rate_table[index])
```

### Envelope

Pulse and Noise channels use an **envelope generator** for volume control:

```rust
pub struct Envelope {
    start_flag: bool,
    divider: u8,
    decay_counter: u8,
    loop_flag: bool,
    constant_volume: bool,
    volume: u8,      // Constant volume or envelope period
}

impl Envelope {
    pub fn clock(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.decay_counter = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;

            if self.decay_counter > 0 {
                self.decay_counter -= 1;
            } else if self.loop_flag {
                self.decay_counter = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay_counter
        }
    }
}
```

### Length Counter

All channels except DMC use a **length counter** for note duration:

```rust
pub struct LengthCounter {
    counter: u8,
    halt: bool,    // Halt flag (from channel register)
}

impl LengthCounter {
    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    pub fn load(&mut self, index: u8) {
        self.counter = LENGTH_TABLE[index as usize];
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}
```

**Length Counter Table:**
```rust
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20,  2, 40,  4, 80,  6,  // 0x00-0x07
    160,  8, 60, 10, 14, 12, 26, 14,  // 0x08-0x0F
    12,  16, 24, 18, 48, 20, 96, 22,  // 0x10-0x17
    192, 24, 72, 26, 16, 28, 32, 30,  // 0x18-0x1F
];
```

---

## Pulse Channels

### Overview

Two identical pulse (square wave) channels with **duty cycle** control and **frequency sweep**.

**Registers:**
```
$4000/$4004: DDLC VVVV - Duty, loop, constant volume, volume/envelope
$4001/$4005: EPPP NSSS - Sweep enable, period, negate, shift
$4002/$4006: TTTT TTTT - Timer low 8 bits
$4003/$4007: LLLL LTTT - Length counter load, timer high 3 bits
```

### Duty Cycle

Pulse channels output a **square wave** with selectable duty cycle:

```rust
const DUTY_CYCLES: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],  // 12.5% duty
    [0, 1, 1, 0, 0, 0, 0, 0],  // 25% duty
    [0, 1, 1, 1, 1, 0, 0, 0],  // 50% duty
    [1, 0, 0, 1, 1, 1, 1, 1],  // 75% duty (inverted 25%)
];

pub struct PulseChannel {
    duty_mode: u8,        // 0-3
    sequence_pos: u8,     // 0-7
    // ... other components
}

impl PulseChannel {
    pub fn clock_sequencer(&mut self) {
        self.sequence_pos = (self.sequence_pos + 1) % 8;
    }

    pub fn output(&self) -> u8 {
        if !self.is_enabled() {
            return 0;
        }

        let duty_output = DUTY_CYCLES[self.duty_mode as usize][self.sequence_pos as usize];

        if duty_output == 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}
```

### Sweep Unit

The **sweep unit** provides automatic pitch bending:

```rust
pub struct Sweep {
    enabled: bool,
    period: u8,        // Sweep period (0-7)
    negate: bool,      // Sweep direction (up/down)
    shift: u8,         // Shift amount (0-7)
    reload_flag: bool,
    divider: u8,
}

impl Sweep {
    pub fn clock(&mut self, timer: &mut Timer, channel: u8) {
        if self.divider == 0 && self.enabled && !self.is_muting(timer) {
            let change = timer.period >> self.shift;

            if self.negate {
                timer.period -= change;

                // Pulse 1 uses one's complement (subtract 1 more)
                if channel == 1 {
                    timer.period -= 1;
                }
            } else {
                timer.period += change;
            }
        }

        if self.divider == 0 || self.reload_flag {
            self.divider = self.period;
            self.reload_flag = false;
        } else {
            self.divider -= 1;
        }
    }

    fn is_muting(&self, timer: &Timer) -> bool {
        let change = timer.period >> self.shift;
        let target = if self.negate {
            timer.period - change - if /* channel 1 */ true { 1 } else { 0 }
        } else {
            timer.period + change
        };

        timer.period < 8 || target > 0x7FF
    }
}
```

**Muting Conditions:**
- Timer period < 8 (frequency too high)
- Target period > $7FF (frequency too low)

### Complete Pulse Implementation

```rust
pub struct PulseChannel {
    // Registers
    duty_mode: u8,
    length_halt: bool,
    constant_volume: bool,
    envelope_period: u8,

    // Components
    timer: Timer,
    envelope: Envelope,
    length_counter: LengthCounter,
    sweep: Sweep,

    // Sequencer
    sequence_pos: u8,

    // Enable
    enabled: bool,
}

impl PulseChannel {
    pub fn clock_timer(&mut self) {
        if self.timer.clock() {
            self.clock_sequencer();
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
        self.sweep.clock(&mut self.timer, /* channel num */);
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        if self.sweep.is_muting(&self.timer) {
            return 0;
        }

        let duty_bit = DUTY_CYCLES[self.duty_mode as usize][self.sequence_pos as usize];

        if duty_bit == 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}
```

---

## Triangle Channel

### Overview

Triangle wave generator with **linear counter** (instead of envelope) and **no volume control**.

**Registers:**
```
$4008: CRRR RRRR - Control flag, linear counter reload
$4009: (unused)
$400A: TTTT TTTT - Timer low 8 bits
$400B: LLLL LTTT - Length counter load, timer high 3 bits
```

### Triangle Waveform

The triangle channel outputs a **32-step sequence**:

```rust
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10,  9,  8,  7,  6,  5,  4,  3,  2,  1,  0,
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15,
];

pub struct TriangleChannel {
    sequence_pos: u8,  // 0-31
    // ... other components
}

impl TriangleChannel {
    pub fn clock_sequencer(&mut self) {
        self.sequence_pos = (self.sequence_pos + 1) % 32;
    }

    pub fn output(&self) -> u8 {
        if !self.is_enabled() {
            return 0;
        }

        TRIANGLE_SEQUENCE[self.sequence_pos as usize]
    }
}
```

### Linear Counter

The triangle channel uses a **linear counter** instead of an envelope:

```rust
pub struct LinearCounter {
    reload_value: u8,
    counter: u8,
    reload_flag: bool,
    control_flag: bool,  // Halt length counter
}

impl LinearCounter {
    pub fn clock(&mut self) {
        if self.reload_flag {
            self.counter = self.reload_value;
        } else if self.counter > 0 {
            self.counter -= 1;
        }

        if !self.control_flag {
            self.reload_flag = false;
        }
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}
```

### Complete Triangle Implementation

```rust
pub struct TriangleChannel {
    // Registers
    control_flag: bool,
    linear_reload: u8,

    // Components
    timer: Timer,
    length_counter: LengthCounter,
    linear_counter: LinearCounter,

    // Sequencer
    sequence_pos: u8,

    // Enable
    enabled: bool,
}

impl TriangleChannel {
    pub fn clock_timer(&mut self) {
        // Triangle only clocks if both counters are non-zero
        if self.length_counter.is_active() && self.linear_counter.is_active() {
            if self.timer.clock() {
                self.clock_sequencer();
            }
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.linear_counter.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        if !self.linear_counter.is_active() {
            return 0;
        }

        // Ultrasonic frequencies output constant 7.5
        if self.timer.period < 2 {
            return 7;
        }

        TRIANGLE_SEQUENCE[self.sequence_pos as usize]
    }
}
```

---

## Noise Channel

### Overview

Pseudo-random noise generator using a **Linear Feedback Shift Register (LFSR)**.

**Registers:**
```
$400C: --LC VVVV - Loop envelope, constant volume, volume/envelope
$400D: (unused)
$400E: L--- PPPP - Loop noise, period
$400F: LLLL L--- - Length counter load
```

### LFSR (Linear Feedback Shift Register)

```rust
pub struct NoiseChannel {
    shift_register: u16,  // 15-bit LFSR
    mode_flag: bool,      // false: long (15-bit), true: short (6-bit)
    // ... other components
}

impl NoiseChannel {
    pub fn clock_lfsr(&mut self) {
        let feedback_bit = if self.mode_flag {
            // Short mode: feedback from bits 0 and 6
            (self.shift_register & 0x01) ^ ((self.shift_register >> 6) & 0x01)
        } else {
            // Long mode: feedback from bits 0 and 1
            (self.shift_register & 0x01) ^ ((self.shift_register >> 1) & 0x01)
        };

        self.shift_register >>= 1;
        self.shift_register |= feedback_bit << 14;
    }

    pub fn output(&self) -> u8 {
        if !self.is_enabled() {
            return 0;
        }

        // Output is zero if bit 0 of shift register is set
        if (self.shift_register & 0x01) != 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}
```

### Noise Period Table

```rust
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160,
    202, 254, 380, 508, 762, 1016, 2034, 4068,
];
```

### Complete Noise Implementation

```rust
pub struct NoiseChannel {
    // Registers
    mode_flag: bool,
    length_halt: bool,

    // Components
    timer: Timer,
    envelope: Envelope,
    length_counter: LengthCounter,

    // LFSR
    shift_register: u16,

    // Enable
    enabled: bool,
}

impl NoiseChannel {
    pub fn write_period(&mut self, value: u8) {
        self.mode_flag = (value & 0x80) != 0;
        let period_index = value & 0x0F;
        self.timer.period = NOISE_PERIOD_TABLE[period_index as usize];
    }

    pub fn clock_timer(&mut self) {
        if self.timer.clock() {
            self.clock_lfsr();
        }
    }

    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        if (self.shift_register & 0x01) != 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}
```

---

## DMC Channel

### Overview

1-bit **Delta Modulation Channel** for sample playback via DMA.

**Registers:**
```
$4010: IL-- RRRR - IRQ enable, loop, frequency/rate
$4011: -DDD DDDD - Direct load (7-bit DAC)
$4012: AAAA AAAA - Sample address = $C000 + (A × $40)
$4013: LLLL LLLL - Sample length = (L × $10) + 1
```

### Rate Table

```rust
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106,  84,  72,  54,
];
```

**Frequency Calculation:**
```
frequency = CPU_CLOCK / rate_table[index]

Examples:
  Index 0: 1.789773 MHz / 428 = 4.18 kHz
  Index 15: 1.789773 MHz / 54 = 33.14 kHz
```

### Sample Buffer

```rust
pub struct DmcChannel {
    // Registers
    irq_enabled: bool,
    loop_flag: bool,
    rate_index: u8,

    // Sample state
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,

    // Output
    output_level: u8,      // 7-bit (0-127)
    sample_buffer: u8,
    sample_buffer_empty: bool,

    // Shift register
    shift_register: u8,
    bits_remaining: u8,
    silence_flag: bool,

    // Timer
    timer: Timer,

    // IRQ
    irq_flag: bool,
}
```

### DMA Read

```rust
impl DmcChannel {
    pub fn read_sample(&mut self, bus: &mut Bus) {
        if self.sample_buffer_empty && self.bytes_remaining > 0 {
            // Perform DMA read (stalls CPU for 4 cycles)
            self.sample_buffer = bus.read(self.current_address);
            self.sample_buffer_empty = false;

            // Increment address
            if self.current_address == 0xFFFF {
                self.current_address = 0x8000;
            } else {
                self.current_address += 1;
            }

            // Decrement bytes remaining
            self.bytes_remaining -= 1;

            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    // Restart sample
                    self.current_address = self.sample_address;
                    self.bytes_remaining = self.sample_length;
                } else if self.irq_enabled {
                    self.irq_flag = true;
                }
            }
        }
    }
}
```

### Output Unit

```rust
impl DmcChannel {
    pub fn clock_output_unit(&mut self) {
        if !self.silence_flag {
            let bit = (self.shift_register & 0x01) != 0;

            if bit && self.output_level <= 125 {
                self.output_level += 2;
            } else if !bit && self.output_level >= 2 {
                self.output_level -= 2;
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;

        if self.bits_remaining == 0 {
            self.bits_remaining = 8;

            if self.sample_buffer_empty {
                self.silence_flag = true;
            } else {
                self.silence_flag = false;
                self.shift_register = self.sample_buffer;
                self.sample_buffer_empty = true;
            }
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }
}
```

---

## Implementation Guide

### Channel Update Loop

```rust
pub fn step_apu(&mut self, cpu_cycles: u8) {
    for _ in 0..cpu_cycles {
        // Clock timers at CPU rate
        self.pulse1.clock_timer();
        self.pulse2.clock_timer();

        // Triangle clocks at half rate (every other CPU cycle)
        if (self.cycles & 1) == 0 {
            self.triangle.clock_timer();
        }

        self.noise.clock_timer();
        self.dmc.clock_timer();

        self.cycles += 1;
    }
}
```

---

## Test ROM Validation

### APU Test ROMs

1. **apu_test**
   - Tests all channels
   - Validates register writes

2. **blargg_apu**
   - Comprehensive channel tests
   - Timing validation

3. **dmc_tests**
   - DMC DMA timing
   - Sample playback

---

**Next:** [APU Timing](APU_TIMING.md) | [Back to APU Overview](APU_OVERVIEW.md)
