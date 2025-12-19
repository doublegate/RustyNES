# [Milestone 3] Sprint 3.3: Triangle & Noise Channels

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 3.1 (APU Core) must be complete

---

## Overview

Implement the triangle wave channel (bass line) and noise channel (percussion/effects). The triangle channel produces a fixed-volume triangle wave with a linear counter, while the noise channel generates pseudo-random noise using a Linear Feedback Shift Register (LFSR).

---

## Acceptance Criteria

- [ ] Triangle channel with 32-step sequencer
- [ ] Linear counter (triangle-specific timing)
- [ ] Noise channel with 15-bit LFSR
- [ ] Two noise modes (long and short period)
- [ ] Length counter integration for both channels
- [ ] Timer logic for both channels
- [ ] Zero unsafe code
- [ ] Comprehensive unit tests

---

## Tasks

### 3.3.1 Triangle Channel Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the TriangleChannel struct with 32-step sequence and linear counter.

**Files:**

- `crates/rustynes-apu/src/triangle.rs` - Triangle channel implementation

**Subtasks:**

- [ ] Define TriangleChannel struct
- [ ] 32-step triangle sequence
- [ ] Linear counter (separate from length counter)
- [ ] Timer (11-bit period)
- [ ] Control flag (halt length counter and reload linear counter)
- [ ] Sequence position tracking

**Implementation:**

```rust
use crate::length_counter::LengthCounter;

pub struct TriangleChannel {
    // Sequencer
    sequence_position: u8,     // 0-31 position in triangle wave

    // Linear counter
    linear_counter: u8,        // 7-bit counter
    linear_reload: u8,         // Reload value
    control_flag: bool,        // Halt length and reload linear
    reload_flag: bool,         // Reload linear counter flag

    // Length counter
    length_counter: LengthCounter,

    // Timer
    timer: u16,                // 11-bit period
    timer_counter: u16,        // Current timer value

    // State
    enabled: bool,
}

impl TriangleChannel {
    pub fn new() -> Self {
        Self {
            sequence_position: 0,
            linear_counter: 0,
            linear_reload: 0,
            control_flag: false,
            reload_flag: false,
            length_counter: LengthCounter::new(),
            timer: 0,
            timer_counter: 0,
            enabled: false,
        }
    }
}

// Triangle wave sequence (32 steps)
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10,  9,  8,  7,  6,  5,  4,  3,  2,  1,  0,
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15,
];
```

---

### 3.3.2 Triangle Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1.5 hours

**Description:**
Implement the 3-register interface for the triangle channel ($4008, $400A, $400B).

**Files:**

- `crates/rustynes-apu/src/triangle.rs` - Register handlers

**Subtasks:**

- [ ] Register $4008: Control flag and linear counter reload
- [ ] Register $400A: Timer low 8 bits
- [ ] Register $400B: Length counter load and timer high 3 bits
- [ ] Note: $4009 is unused

**Implementation:**

```rust
impl TriangleChannel {
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4008: CRRR RRRR
                self.control_flag = (value & 0x80) != 0;
                self.linear_reload = value & 0x7F;

                // Control flag also sets length counter halt
                self.length_counter.set_halt(self.control_flag);
            }
            2 => {
                // $400A: TTTT TTTT
                self.timer = (self.timer & 0xFF00) | (value as u16);
            }
            3 => {
                // $400B: LLLL LTTT
                self.timer = (self.timer & 0x00FF) | (((value & 0x07) as u16) << 8);

                // Load length counter
                let length_index = (value >> 3) & 0x1F;
                if self.enabled {
                    self.length_counter.load(length_index);
                }

                // Set linear counter reload flag
                self.reload_flag = true;
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter.set_enabled(false);
        }
    }
}
```

---

### 3.3.3 Linear Counter Logic

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the triangle-specific linear counter (clocked on quarter frames).

**Files:**

- `crates/rustynes-apu/src/triangle.rs` - Linear counter methods

**Subtasks:**

- [ ] Reload flag handling
- [ ] Control flag behavior
- [ ] Clock on quarter frame
- [ ] Silencing when linear counter reaches 0

**Implementation:**

```rust
impl TriangleChannel {
    pub fn clock_linear_counter(&mut self) {
        if self.reload_flag {
            self.linear_counter = self.linear_reload;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.reload_flag = false;
        }
    }

    pub fn is_active(&self) -> bool {
        self.enabled
            && self.length_counter.is_active()
            && self.linear_counter > 0
    }
}
```

---

### 3.3.4 Triangle Timer and Sequencer

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the timer and 32-step triangle sequencer.

**Files:**

- `crates/rustynes-apu/src/triangle.rs` - Timer and sequencer

**Subtasks:**

- [ ] Timer countdown (clocked every CPU cycle, not APU)
- [ ] Sequence advance on timer overflow
- [ ] Ultrasonic silencing (timer < 2)
- [ ] Output from sequence table

**Implementation:**

```rust
impl TriangleChannel {
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;

            // Clock sequencer if channel is active
            if self.is_active() {
                self.sequence_position = (self.sequence_position + 1) % 32;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.is_active() {
            return 0;
        }

        // Silence ultrasonic frequencies (reduces popping)
        if self.timer < 2 {
            return 0;
        }

        TRIANGLE_SEQUENCE[self.sequence_position as usize]
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }
}
```

---

### 3.3.5 Noise Channel Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the NoiseChannel struct with LFSR and envelope.

**Files:**

- `crates/rustynes-apu/src/noise.rs` - Noise channel implementation

**Subtasks:**

- [ ] Define NoiseChannel struct
- [ ] 15-bit LFSR (shift register)
- [ ] Mode flag (short/long period)
- [ ] Envelope integration
- [ ] Length counter integration
- [ ] Timer with period lookup table

**Implementation:**

```rust
use crate::envelope::Envelope;
use crate::length_counter::LengthCounter;

pub struct NoiseChannel {
    // Envelope
    envelope: Envelope,

    // Length counter
    length_counter: LengthCounter,

    // LFSR
    shift_register: u16,       // 15-bit shift register
    mode: bool,                // false = long (15-bit), true = short (6-bit)

    // Timer
    timer_period: u16,         // From lookup table
    timer_counter: u16,        // Current timer value

    // State
    enabled: bool,
}

impl NoiseChannel {
    pub fn new() -> Self {
        Self {
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            shift_register: 1,  // Initial state
            mode: false,
            timer_period: 0,
            timer_counter: 0,
            enabled: false,
        }
    }
}

// Noise period lookup table (NTSC)
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
```

---

### 3.3.6 Noise Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1.5 hours

**Description:**
Implement the 3-register interface for the noise channel ($400C, $400E, $400F).

**Files:**

- `crates/rustynes-apu/src/noise.rs` - Register handlers

**Subtasks:**

- [ ] Register $400C: Envelope and volume
- [ ] Register $400E: Mode and period
- [ ] Register $400F: Length counter load
- [ ] Note: $400D is unused

**Implementation:**

```rust
impl NoiseChannel {
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $400C: --LC VVVV
                let halt = (value & 0x20) != 0;
                self.length_counter.set_halt(halt);
                self.envelope.write_register(value);
            }
            2 => {
                // $400E: L--- PPPP
                self.mode = (value & 0x80) != 0;
                let period_index = value & 0x0F;
                self.timer_period = NOISE_PERIOD_TABLE[period_index as usize];
            }
            3 => {
                // $400F: LLLL L---
                let length_index = (value >> 3) & 0x1F;
                if self.enabled {
                    self.length_counter.load(length_index);
                }

                // Restart envelope
                self.envelope.start();
            }
            _ => {}
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter.set_enabled(false);
        }
    }
}
```

---

### 3.3.7 LFSR Implementation

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the Linear Feedback Shift Register for pseudo-random noise generation.

**Files:**

- `crates/rustynes-apu/src/noise.rs` - LFSR logic

**Subtasks:**

- [ ] Timer countdown
- [ ] LFSR feedback (XOR bits 0 and 1 or bits 0 and 6)
- [ ] Right shift with feedback at bit 14
- [ ] Mode flag determines feedback tap (bit 1 or bit 6)
- [ ] Output from bit 0

**Implementation:**

```rust
impl NoiseChannel {
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.clock_lfsr();
        } else {
            self.timer_counter -= 1;
        }
    }

    fn clock_lfsr(&mut self) {
        // Feedback from bit 0 XOR bit 1 (long) or bit 6 (short)
        let feedback_bit = if self.mode {
            (self.shift_register & 1) ^ ((self.shift_register >> 6) & 1)
        } else {
            (self.shift_register & 1) ^ ((self.shift_register >> 1) & 1)
        };

        // Right shift
        self.shift_register >>= 1;

        // Insert feedback at bit 14
        self.shift_register |= feedback_bit << 14;
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if !self.length_counter.is_active() {
            return 0;
        }

        // Bit 0 of shift register determines output
        if (self.shift_register & 1) == 0 {
            self.envelope.output()
        } else {
            0
        }
    }

    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }
}
```

---

### 3.3.8 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for triangle and noise channels.

**Files:**

- `crates/rustynes-apu/src/triangle.rs` - Triangle tests
- `crates/rustynes-apu/src/noise.rs` - Noise tests

**Subtasks:**

- [ ] Test triangle sequence output
- [ ] Test linear counter behavior
- [ ] Test triangle ultrasonic silencing
- [ ] Test LFSR feedback (both modes)
- [ ] Test noise period table
- [ ] Test noise envelope integration

**Tests:**

```rust
#[cfg(test)]
mod triangle_tests {
    use super::*;

    #[test]
    fn test_triangle_sequence() {
        let triangle = TriangleChannel::new();

        // Verify sequence matches expected triangle wave
        for i in 0..32 {
            assert_eq!(TRIANGLE_SEQUENCE[i],
                if i < 16 { 15 - i as u8 } else { (i - 16) as u8 });
        }
    }

    #[test]
    fn test_linear_counter() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0); // Non-zero length

        // Set linear counter reload value
        triangle.write_register(0, 0x7F); // Control flag + reload = 127

        // Write to $400B sets reload flag
        triangle.write_register(3, 0x00);
        assert!(triangle.reload_flag);

        // Clock linear counter
        triangle.clock_linear_counter();
        assert_eq!(triangle.linear_counter, 127);
        assert!(triangle.reload_flag); // Still set due to control flag

        // Clear control flag
        triangle.write_register(0, 0x00);
        triangle.clock_linear_counter();
        assert!(!triangle.reload_flag);
        assert_eq!(triangle.linear_counter, 126);
    }

    #[test]
    fn test_ultrasonic_silencing() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);
        triangle.linear_counter = 10;

        // Ultrasonic frequency (timer < 2) should silence
        triangle.timer = 1;
        assert_eq!(triangle.output(), 0);

        // Normal frequency should produce output
        triangle.timer = 100;
        assert!(triangle.output() > 0);
    }
}

#[cfg(test)]
mod noise_tests {
    use super::*;

    #[test]
    fn test_lfsr_long_mode() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0b000000000000001;
        noise.mode = false; // Long mode

        let initial = noise.shift_register;
        noise.clock_lfsr();

        // Should have shifted right and inserted feedback at bit 14
        assert_ne!(noise.shift_register, initial);
        assert_eq!(noise.shift_register >> 15, 0); // Bit 15 should always be 0
    }

    #[test]
    fn test_lfsr_short_mode() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0b000000000000001;
        noise.mode = true; // Short mode

        noise.clock_lfsr();

        // Feedback uses bit 6 instead of bit 1
        // Should produce different sequence than long mode
    }

    #[test]
    fn test_noise_period_table() {
        assert_eq!(NOISE_PERIOD_TABLE[0], 4);
        assert_eq!(NOISE_PERIOD_TABLE[15], 4068);
    }

    #[test]
    fn test_noise_output() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);
        noise.length_counter.load(0);
        noise.envelope.write_register(0x1F); // Constant volume 15

        // Bit 0 = 0 should output envelope volume
        noise.shift_register = 0b000000000000000;
        assert_eq!(noise.output(), 15);

        // Bit 0 = 1 should output 0
        noise.shift_register = 0b000000000000001;
        assert_eq!(noise.output(), 0);
    }
}
```

---

## Dependencies

**Required:**

- Sprint 3.1 complete (envelope, length counter, frame counter)

**Blocks:**

- Sprint 3.5: Audio output and mixing (needs all channel outputs)

---

## Related Documentation

- [APU Triangle Channel](../../docs/apu/APU_CHANNEL_TRIANGLE.md)
- [APU Noise Channel](../../docs/apu/APU_CHANNEL_NOISE.md)
- [APU 2A03 Specification](../../docs/apu/APU_2A03_SPECIFICATION.md)
- [NESdev Wiki - APU Triangle](https://www.nesdev.org/wiki/APU_Triangle)
- [NESdev Wiki - APU Noise](https://www.nesdev.org/wiki/APU_Noise)

---

## Technical Notes

### Triangle Linear Counter vs Length Counter

The triangle channel has BOTH a linear counter and a length counter:
- **Linear counter**: Clocked every quarter frame, reloads based on control flag
- **Length counter**: Clocked every half frame, standard behavior

Both must be non-zero for the triangle to produce sound.

### Ultrasonic Frequencies

Triangle timer values < 2 produce ultrasonic frequencies (> 50 kHz) that can cause audio artifacts. Most emulators silence these to reduce popping.

### LFSR Modes

- **Long mode** (15-bit): Uses bits 0 and 1 for feedback, produces long pseudo-random sequence
- **Short mode** (6-bit): Uses bits 0 and 6 for feedback, produces short repeating pattern (metallic sound)

### Noise Period Table

The noise period table values are specific to NTSC. PAL systems use different values.

---

## Test Requirements

- [ ] Unit tests for triangle 32-step sequence
- [ ] Unit tests for linear counter behavior
- [ ] Unit tests for ultrasonic silencing
- [ ] Unit tests for LFSR feedback (both modes)
- [ ] Unit tests for noise period table
- [ ] Unit tests for noise envelope integration
- [ ] Integration test: Triangle with linear counter and length counter
- [ ] Integration test: Noise with LFSR and envelope

---

## Performance Targets

- Triangle timer: <10 ns per cycle
- LFSR clock: <15 ns per cycle
- Output calculation: <20 ns
- Memory: <100 bytes per channel

---

## Success Criteria

- [ ] Triangle channel produces correct waveform
- [ ] Linear counter works independently from length counter
- [ ] Ultrasonic frequencies are silenced
- [ ] Noise LFSR generates pseudo-random patterns
- [ ] Both noise modes work correctly
- [ ] Envelope controls noise volume
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Previous Sprint:** [Sprint 3.2: Pulse Channels](M3-S2-PULSE-CHANNELS.md)
**Next Sprint:** [Sprint 3.4: DMC Channel](M3-S4-DMC.md)
