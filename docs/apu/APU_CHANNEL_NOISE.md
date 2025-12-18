# APU Noise Channel Specification (2A03)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete technical reference for NES APU noise channel

---

## Table of Contents

- [Overview](#overview)
- [Channel Architecture](#channel-architecture)
- [Register Interface](#register-interface)
- [LFSR Implementation](#lfsr-implementation)
- [Mode Differences](#mode-differences)
- [Envelope Generator](#envelope-generator)
- [Length Counter](#length-counter)
- [Period and Timing](#period-and-timing)
- [Implementation Guide](#implementation-guide)
- [Common Pitfalls](#common-pitfalls)
- [Testing and Validation](#testing-and-validation)

---

## Overview

The NES APU contains **one noise channel** that generates pseudo-random waveforms using a Linear Feedback Shift Register (LFSR). This channel is primarily used for percussion, explosions, and environmental sound effects.

**Key Characteristics:**
- 15-bit Linear Feedback Shift Register (LFSR)
- Two modes: 32,767-step or 93-step sequences
- Hardware envelope generator (volume control)
- Length counter (automatic note duration)
- 16 selectable period values
- 4-bit output (16 volume levels)
- Typical use: drums, cymbals, explosions, wind/water effects

**Why LFSR?**

True white noise requires expensive random number generation. An LFSR provides **deterministic pseudo-random sequences** with minimal hardware. The two modes offer different timbres: Mode 0 (long sequence) sounds like white noise, Mode 1 (short sequence) produces metallic tones.

---

## Channel Architecture

The noise channel consists of **five interconnected units**:

```
┌───────────────────────────────────────────────────────────┐
│                     Noise Channel                         │
│                                                           │
│  ┌───────┐    ┌──────────┐    ┌──────────┐               │
│  │ Timer │───>│   LFSR   │───>│ Envelope │               │
│  │16 val │    │  15-bit  │    │Generator │               │
│  └───────┘    └──────────┘    └──────────┘               │
│                                     │                     │
│                    ┌──────────────┐ │                     │
│                    │Length Counter│─┘                     │
│                    └──────────────┘                       │
│                                                           │
│                           ▼                               │
│                    4-bit Output (0-15)                    │
└───────────────────────────────────────────────────────────┘
```

**Signal Flow:**
1. **Timer** counts down using selected period from lookup table
2. **LFSR** shifts and generates pseudo-random bit (0 or 1)
3. **Envelope** scales output by volume (0-15)
4. **Length Counter** gates the output

**Gating Logic:**
```
Output = (LFSR bit 0 == 0) × Envelope × (Length > 0)
```

**Key Difference:** LFSR bit 0 determines if sound outputs. When bit 0 = 1, channel is muted regardless of envelope.

---

## Register Interface

### Complete Register Map

| Address | Bits | Description |
|---------|------|-------------|
| **$400C** | --LC VVVV | Loop, Constant volume, Volume/Envelope |
| **$400D** | ---- ---- | Unused (write has no effect) |
| **$400E** | L--- PPPP | Mode flag, Period index |
| **$400F** | LLLL L--- | Length counter load |

### Register $400C - Envelope and Volume

```
--LC VVVV
  || ||||
  || ++++- Volume/Envelope divider period (V)
  |+------ Constant volume flag (C)
  +------- Length counter halt / Envelope loop (L)
```

**Bit Definitions:**
- **L (Loop)**: If 1, length counter is frozen and envelope loops
- **C (Constant)**: If 1, volume = VVVV; if 0, envelope generates volume
- **VVVV (Volume)**: Constant volume value OR envelope divider period

**Identical to Pulse Channel $4000/$4004.**

### Register $400D - Unused

Writing to this register has no effect. Exists for address space symmetry.

### Register $400E - Mode and Period

```
L--- PPPP
|    ||||
|    ++++- Period index (P): 0-15, selects from lookup table
+--------- Mode flag (L): 0 = long sequence, 1 = short sequence
```

**Bit Definitions:**
- **L (Mode)**: Determines LFSR feedback tap
  - 0: XOR bits 0 and 1 (32,767-step sequence)
  - 1: XOR bits 0 and 6 (93 or 31-step sequence)
- **PPPP (Period)**: Index into period lookup table

### Register $400F - Length Counter Load

```
LLLL L---
|||| |
++++-+--- Length counter load (L): Index 0-31
```

**Side Effects of Writing:**
- Length counter is loaded from lookup table
- **Envelope restarts** (sets divider to reload value)

**Note:** Unlike pulse channels, no timer value is stored here (noise uses fixed period table).

---

## LFSR Implementation

### What is an LFSR?

A **Linear Feedback Shift Register** is a shift register with its input bit determined by XOR of specific output bits. It generates **pseudo-random sequences** that repeat after a deterministic period.

```
15-bit LFSR:
┌─────────────────────────────────────────────────────────────┐
│14│13│12│11│10│ 9│ 8│ 7│ 6│ 5│ 4│ 3│ 2│ 1│ 0│
└──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┴──┘
 ▲                          ▲           ▲     │
 │                          │           │     │
 │          Mode 1          │  Mode 0   │     │
 └──────────XOR────────────┘     XOR────┘     │
                                               │
                                               ▼
                                          Output bit
```

### Operation Steps

On each timer clock:

**1. Feedback Calculation:**
```rust
let bit0 = lfsr & 1;
let feedback = if mode == 0 {
    bit0 ^ ((lfsr >> 1) & 1)  // XOR bits 0 and 1
} else {
    bit0 ^ ((lfsr >> 6) & 1)  // XOR bits 0 and 6
};
```

**2. Shift Right:**
```rust
lfsr >>= 1;
```

**3. Insert Feedback:**
```rust
if feedback {
    lfsr |= 0x4000;  // Set bit 14
}
```

### Initialization

On power-up or reset:
```rust
lfsr = 1;
```

**Why 1, not 0?** If LFSR is 0, it remains 0 forever (all XOR outputs are 0). Starting with 1 ensures proper sequence generation.

### Implementation

```rust
pub struct NoiseChannel {
    lfsr: u16,      // 15-bit shift register
    mode: bool,     // false = Mode 0, true = Mode 1
    timer: Timer,
    envelope: Envelope,
    length_counter: LengthCounter,
}

impl NoiseChannel {
    pub fn clock_lfsr(&mut self) {
        let bit0 = self.lfsr & 1;
        let other_bit = if self.mode {
            (self.lfsr >> 6) & 1  // Mode 1: bit 6
        } else {
            (self.lfsr >> 1) & 1  // Mode 0: bit 1
        };

        let feedback = bit0 ^ other_bit;

        self.lfsr >>= 1;

        if feedback == 1 {
            self.lfsr |= 0x4000;  // Set bit 14
        }
    }

    pub fn output(&self) -> u8 {
        if !self.length_counter.is_active() {
            return 0;
        }

        // Channel muted when LFSR bit 0 is set
        if (self.lfsr & 1) == 1 {
            return 0;
        }

        self.envelope.output()
    }
}
```

---

## Mode Differences

### Mode 0: Long Sequence (Mode Flag = 0)

**Feedback:** XOR of bits 0 and 1

**Sequence Length:** 32,767 steps (2^15 - 1)

**Sound Character:** White noise (broadband spectrum)

**Use Cases:**
- White noise effects (wind, static)
- Cymbals and hi-hats
- Explosion sounds
- Waterfall/rain effects

**Frequency Content:** Full spectrum with near-uniform distribution

### Mode 1: Short Sequence (Mode Flag = 1)

**Feedback:** XOR of bits 0 and 6

**Sequence Length:** 93 steps (typical) or 31 steps (rare, depends on initial state)

**Sound Character:** Metallic, periodic tone

**Use Cases:**
- Metallic percussion (snare drums)
- Laser/electric sounds
- Robot/mechanical effects
- Special sound effects

**Frequency Content:** Harmonic-rich tone with strong fundamental

### Sequence Length Analysis

**Mode 0 (bits 0, 1):**
- Maximum-length sequence: 2^15 - 1 = 32,767 steps
- Covers all non-zero 15-bit values exactly once

**Mode 1 (bits 0, 6):**
- Not maximum-length: typically 93 steps
- Can also produce 31-step sequence
- Depends on LFSR seed value

### Timbre Comparison

| Mode | Period $00 | Period $0F | Description |
|------|-----------|-----------|-------------|
| **0** | Hiss (447 kHz) | Low rumble (439 Hz) | White noise |
| **1** | Buzz (447 kHz) | Metallic tone (439 Hz) | Periodic tone |

---

## Envelope Generator

The noise channel envelope is **identical to pulse channels**. See [APU_CHANNEL_PULSE.md](APU_CHANNEL_PULSE.md#envelope-generator) for detailed explanation.

### Quick Reference

**Mode 1: Constant Volume (C=1)**
```
Output = VVVV (0-15)
```

**Mode 2: Envelope Volume (C=0)**
```
Output = Decay Level (15 → 0 over time)
Clock: 240 Hz (NTSC quarter frames)
```

### Implementation

```rust
pub struct Envelope {
    start_flag: bool,
    loop_flag: bool,
    constant_flag: bool,
    reload_value: u8,
    decay_level: u8,
    divider: u8,
}

impl Envelope {
    pub fn clock(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.decay_level = 15;
            self.divider = self.reload_value;
        } else if self.divider == 0 {
            self.divider = self.reload_value;

            if self.decay_level > 0 {
                self.decay_level -= 1;
            } else if self.loop_flag {
                self.decay_level = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if self.constant_flag {
            self.reload_value
        } else {
            self.decay_level
        }
    }
}
```

---

## Length Counter

The noise channel length counter is **identical to pulse/triangle channels**.

### Lookup Table

| Index | Length | Index | Length | Index | Length | Index | Length |
|-------|--------|-------|--------|-------|--------|-------|--------|
| $00 | 10 | $08 | 20 | $10 | 80 | $18 | 160 |
| $01 | 254 | $09 | 40 | $11 | 30 | $19 | 6 |
| $02 | 20 | $0A | 80 | $12 | 160 | $1A | 12 |
| $03 | 2 | $0B | 160 | $13 | 6 | $1B | 24 |
| $04 | 40 | $0C | 60 | $14 | 12 | $1C | 48 |
| $05 | 4 | $0D | 8 | $15 | 24 | $1D | 96 |
| $06 | 80 | $0E | 16 | $16 | 48 | $1E | 192 |
| $07 | 6 | $0F | 32 | $17 | 96 | $1F | 72 |

**Units:** Half frames (120 Hz NTSC), ~8.33ms per count

### Implementation

See [APU_CHANNEL_PULSE.md](APU_CHANNEL_PULSE.md#length-counter) for complete details.

---

## Period and Timing

### Period Lookup Tables

The noise channel timer uses **fixed period values** selected by index (0-15):

**NTSC Periods (CPU cycles):**
```rust
const NOISE_PERIOD_NTSC: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160,
    202, 254, 380, 508, 762, 1016, 2034, 4068,
];
```

**PAL Periods (CPU cycles):**
```rust
const NOISE_PERIOD_PAL: [u16; 16] = [
    4, 8, 14, 30, 60, 88, 118, 148,
    188, 236, 354, 472, 708, 944, 1890, 3778,
];
```

### Frequency Calculation

**NTSC:**
```
f_noise = f_CPU / (period × 2)
       = 1,789,773 / (period × 2)

Example (period index $0F):
  f = 1,789,773 / (4068 × 2) = 220 Hz
```

**Why ×2?** Timer runs at APU clock (CPU/2), so each period count represents 2 CPU cycles.

### Frequency Table

| Index | NTSC Period | NTSC Freq | PAL Period | PAL Freq | Description |
|-------|-------------|-----------|------------|----------|-------------|
| $00 | 4 | 447 kHz | 4 | 415 kHz | Ultrasonic (inaudible) |
| $01 | 8 | 224 kHz | 8 | 208 kHz | Ultrasonic |
| $02 | 16 | 112 kHz | 14 | 119 kHz | Ultrasonic |
| $03 | 32 | 55.9 kHz | 30 | 55.4 kHz | High hiss |
| $04 | 64 | 27.9 kHz | 60 | 27.7 kHz | High hiss |
| $05 | 96 | 18.6 kHz | 88 | 18.9 kHz | High tone |
| $06 | 128 | 14.0 kHz | 118 | 14.1 kHz | Mid-high tone |
| $07 | 160 | 11.2 kHz | 148 | 11.3 kHz | Mid tone |
| $08 | 202 | 8.86 kHz | 188 | 8.85 kHz | Mid tone |
| $09 | 254 | 7.05 kHz | 236 | 7.05 kHz | Snare drum |
| $0A | 380 | 4.71 kHz | 354 | 4.70 kHz | Snare drum |
| $0B | 508 | 3.52 kHz | 472 | 3.52 kHz | Tom drum |
| $0C | 762 | 2.35 kHz | 708 | 2.35 kHz | Tom drum |
| $0D | 1016 | 1.76 kHz | 944 | 1.76 kHz | Bass drum |
| $0E | 2034 | 880 Hz | 1890 | 880 Hz | Low rumble |
| $0F | 4068 | 440 Hz | 3778 | 440 Hz | Deep rumble |

**Typical Usage:**
- $00-$04: Cymbals, hi-hats (very short periods)
- $05-$09: Snare drums
- $0A-$0C: Tom drums
- $0D-$0F: Bass drums, explosions

### Timer Operation

```rust
pub struct NoiseTimer {
    period_table: [u16; 16],
    period_index: u8,  // 0-15
    counter: u16,
}

impl NoiseTimer {
    pub fn clock(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period_table[self.period_index as usize];
            true  // Clock LFSR
        } else {
            self.counter -= 1;
            false
        }
    }
}
```

---

## Implementation Guide

### Complete Noise Channel Structure

```rust
pub struct NoiseChannel {
    // LFSR state
    lfsr: u16,      // 15-bit shift register
    mode: bool,     // false = Mode 0, true = Mode 1

    // Component units
    timer: NoiseTimer,
    envelope: Envelope,
    length_counter: LengthCounter,

    // Enable flag from $4015
    enabled: bool,
}

impl NoiseChannel {
    pub fn new(system: System) -> Self {
        let period_table = match system {
            System::NTSC => NOISE_PERIOD_NTSC,
            System::PAL => NOISE_PERIOD_PAL,
        };

        Self {
            lfsr: 1,  // Initial value
            mode: false,
            timer: NoiseTimer::new(period_table),
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            enabled: false,
        }
    }

    /// Clock the timer (every APU cycle = every 2 CPU cycles)
    pub fn clock_timer(&mut self) {
        if self.timer.clock() {
            self.clock_lfsr();
        }
    }

    /// Clock envelope (quarter frame)
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    /// Clock length counter (half frame)
    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    /// Shift LFSR and calculate feedback
    fn clock_lfsr(&mut self) {
        let bit0 = self.lfsr & 1;
        let other_bit = if self.mode {
            (self.lfsr >> 6) & 1  // Mode 1: bit 6
        } else {
            (self.lfsr >> 1) & 1  // Mode 0: bit 1
        };

        let feedback = bit0 ^ other_bit;

        self.lfsr >>= 1;

        if feedback == 1 {
            self.lfsr |= 0x4000;  // Set bit 14
        }
    }

    /// Get current output sample (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        // LFSR bit 0 = 1 mutes output
        if (self.lfsr & 1) == 1 {
            return 0;
        }

        self.envelope.output()
    }

    /// Write to register $400C
    pub fn write_envelope(&mut self, value: u8) {
        self.envelope.loop_flag = (value & 0x20) != 0;
        self.envelope.constant_flag = (value & 0x10) != 0;
        self.envelope.reload_value = value & 0x0F;
        self.length_counter.halt = (value & 0x20) != 0;
    }

    /// Write to register $400E
    pub fn write_mode_period(&mut self, value: u8) {
        self.mode = (value & 0x80) != 0;
        self.timer.period_index = value & 0x0F;
    }

    /// Write to register $400F
    pub fn write_length(&mut self, value: u8) {
        let length_index = value >> 3;
        self.length_counter.load(length_index);
        self.envelope.restart();
    }
}
```

---

## Common Pitfalls

### 1. LFSR Initialization to Zero

**Problem:** Initializing LFSR to 0 causes it to remain 0 forever.

**Solution:** Always initialize to 1 (or any non-zero value).

```rust
// WRONG: LFSR stuck at 0
let mut lfsr = 0u16;

// CORRECT: Proper initialization
let mut lfsr = 1u16;
```

### 2. Wrong Feedback Bits

**Problem:** Using incorrect bit positions for XOR feedback.

**Solution:** Mode 0 uses bits 0 and 1; Mode 1 uses bits 0 and 6.

```rust
// Mode 0: bits 0 and 1
let feedback = (lfsr & 1) ^ ((lfsr >> 1) & 1);

// Mode 1: bits 0 and 6
let feedback = (lfsr & 1) ^ ((lfsr >> 6) & 1);
```

### 3. Output Polarity

**Problem:** Outputting volume when LFSR bit 0 = 1 (inverted logic).

**Solution:** Bit 0 = 0 allows output; bit 0 = 1 mutes.

```rust
// WRONG: Inverted logic
if (self.lfsr & 1) == 1 {
    return self.envelope.output();
}

// CORRECT: Bit 0 = 0 means output
if (self.lfsr & 1) == 0 {
    return self.envelope.output();
}
```

### 4. Timer Clock Rate

**Problem:** Clocking timer at CPU rate instead of APU rate.

**Solution:** Noise timer runs at APU clock (CPU/2).

```rust
// Clock once per APU cycle (every 2 CPU cycles)
if cpu_cycle % 2 == 1 {
    noise.clock_timer();
}
```

---

## Testing and Validation

### Test ROMs

| ROM | Tests | Pass Criteria |
|-----|-------|---------------|
| **apu_test** | Basic noise functionality | All tests pass |
| **blargg_apu_2005.nes** | Comprehensive APU behavior | Text output "Passed" |
| **04-noise.nes** | Noise channel specifics | Correct LFSR behavior |
| **noise_pitch.nes** | Period accuracy | Frequency verification |

### Manual Testing

**Mode 0 Test (White Noise):**
```
Set: Mode=0, Period=$08, Volume=15
Play for 1 second
Verify: Broadband white noise, no tonal quality
```

**Mode 1 Test (Metallic Tone):**
```
Set: Mode=1, Period=$08, Volume=15
Play for 1 second
Verify: Metallic, periodic tone with clear pitch
```

**Envelope Test:**
```
Set: Mode=0, Period=$0A, Constant=0, Volume=15
Trigger with $400F
Verify: Noise fades from loud to silent over ~1 second
```

**Period Sweep Test:**
```
For period in 0..16:
    Set period, play 500ms
    Verify frequency matches lookup table
```

---

## Related Documentation

- [APU_OVERVIEW.md](APU_OVERVIEW.md) - General APU architecture
- [APU_CHANNEL_PULSE.md](APU_CHANNEL_PULSE.md) - Pulse channel (envelope reference)
- [APU_TIMING.md](APU_TIMING.md) - Frame counter and timing details
- [APU_MIXER.md](APU_MIXER.md) - Audio mixing and output

---

## References

- [NESdev Wiki: APU Noise](https://www.nesdev.org/wiki/APU_Noise)
- [NESdev Wiki: APU Envelope](https://www.nesdev.org/wiki/APU_Envelope)
- [NESdev Wiki: APU Length Counter](https://www.nesdev.org/wiki/APU_Length_Counter)
- [Wikipedia: Linear-feedback shift register](https://en.wikipedia.org/wiki/Linear-feedback_shift_register)
- Blargg APU Test ROMs - Noise channel validation

---

**Document Status:** Complete specification for noise channel implementation with cycle-accurate LFSR behavior.
