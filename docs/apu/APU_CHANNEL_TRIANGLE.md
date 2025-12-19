# APU Triangle Channel Specification (2A03)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete technical reference for NES APU triangle wave channel

---

## Table of Contents

- [Overview](#overview)
- [Channel Architecture](#channel-architecture)
- [Register Interface](#register-interface)
- [Triangle Wave Generation](#triangle-wave-generation)
- [Linear Counter](#linear-counter)
- [Length Counter](#length-counter)
- [Timer and Frequency](#timer-and-frequency)
- [Silencing Techniques](#silencing-techniques)
- [Implementation Guide](#implementation-guide)
- [Common Pitfalls](#common-pitfalls)
- [Testing and Validation](#testing-and-validation)

---

## Overview

The NES APU contains **one triangle wave channel** that produces a pseudo-triangle waveform using a 32-step sequencer. Unlike pulse channels, the triangle channel has **no volume control** - it outputs at a fixed amplitude or remains silent.

**Key Characteristics:**

- 32-step triangle wave sequencer
- No volume control (always maximum or silent)
- Linear counter (alternative to envelope)
- Length counter (note duration)
- 11-bit timer (27.3 Hz - 55.9 kHz range)
- Higher frequency range than pulse channels
- Typical use: bass lines, low-frequency effects

**Why No Volume Control?**

The triangle channel was designed for bass lines, where volume dynamics are less critical. The hardware simplification allowed space for other features (DMC channel). Games needing volume control must use software techniques (rapidly toggling enable bit or using ultrasonic frequencies).

---

## Channel Architecture

The triangle channel consists of **four interconnected units**:

```
┌─────────────────────────────────────────────────────────┐
│                  Triangle Channel                       │
│                                                         │
│  ┌───────┐    ┌──────────────┐                          │
│  │ Timer │───>│  Sequencer   │                          │
│  │11-bit │    │  32-step     │                          │
│  └───────┘    └──────────────┘                          │
│                                                         │
│  ┌───────────────┐    ┌──────────────┐                  │
│  │Linear Counter │───>│Length Counter│                  │
│  └───────────────┘    └──────────────┘                  │
│                                                         │
│                             ▼                           │
│                      4-bit Output (0-15)                │
└─────────────────────────────────────────────────────────┘
```

**Signal Flow:**

1. **Timer** counts down, clocking the sequencer
2. **Sequencer** outputs triangle waveform (0-15)
3. **Linear Counter** and **Length Counter** gate the output

**Gating Logic:**

```
Output = Sequencer × (Linear > 0) × (Length > 0)
```

**Critical Difference from Pulse:**

- No envelope generator
- Linear counter provides precise timing control
- No sweep unit
- Fixed amplitude (15)

---

## Register Interface

### Complete Register Map

| Address | Bits | Description |
|---------|------|-------------|
| **$4008** | CRRR RRRR | Control flag, Linear counter reload value |
| **$4009** | ---- ---- | Unused (write has no effect) |
| **$400A** | TTTT TTTT | Timer low 8 bits |
| **$400B** | LLLL LTTT | Length counter load, Timer high 3 bits |

### Register $4008 - Control and Linear Counter

```
CRRR RRRR
|||| ||||
|+++-++++- Linear counter reload value (R)
+--------- Control flag / Length counter halt (C)
```

**Bit Definitions:**

- **C (Control)**: If 1, length counter is halted and linear counter reload flag is set
- **RRRRRRR (Reload)**: 7-bit value loaded into linear counter (0-127)

**Timing:** Linear counter reload value controls note duration independently of length counter, providing **higher accuracy** than length counter alone.

### Register $4009 - Unused

Writing to this register has no effect. It exists for address space symmetry with pulse channels.

### Register $400A - Timer Low

```
TTTT TTTT
|||| ||||
++++-++++- Timer low 8 bits (T)
```

Forms bits 0-7 of the 11-bit timer period.

### Register $400B - Length Counter and Timer High

```
LLLL LTTT
|||| ||||
|||| |+++- Timer high 3 bits (T)
++++-+---- Length counter load (L)
```

**Side Effects of Writing:**

- Timer bits 8-10 are set
- Length counter is loaded from lookup table (if enabled in $4015)
- **Linear counter reload flag is set**

**Important:** Unlike pulse channels, writing $400B does NOT reset sequencer phase, avoiding phase-reset clicks.

---

## Triangle Wave Generation

### 32-Step Sequencer

The triangle channel uses a **32-step sequencer** that cycles through values to create a pseudo-triangle waveform:

```
Step:   0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
Value: 15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0

Step:  16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
Value:  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
```

**Waveform Visualization:**

```
15 ▄
14 ▐▀▄
13 ▐  ▀▄
12 ▐    ▀▄
11 ▐      ▀▄
10 ▐        ▀▄
 9 ▐          ▀▄
 8 ▐            ▀▄
 7 ▐              ▀▄
 6 ▐                ▀▄
 5 ▐                  ▀▄
 4 ▐                    ▀▄
 3 ▐                      ▀▄
 2 ▐                        ▀▄
 1 ▐                          ▀▄
 0 ▐                            ▀▄▀▄▀▄▀▄▀▄▀▄▀▄▀▄
```

**Sequence Pattern:**

- Steps 0-15: Descending (15 → 0)
- Steps 16-31: Ascending (0 → 15)

### Sequencer Behavior

**Clock Rate:** Timer clocks sequencer at **CPU clock rate** (not CPU/2 like pulse channels)

**Sequencer Advancement:**

```
if linear_counter > 0 AND length_counter > 0:
    sequence_step = (sequence_step + 1) % 32
```

**Output:** Current step value (0-15), no envelope scaling

### Implementation

```rust
pub struct TriangleSequencer {
    sequence_step: u8,  // 0-31
}

const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

impl TriangleSequencer {
    pub fn clock(&mut self) {
        self.sequence_step = (self.sequence_step + 1) % 32;
    }

    pub fn output(&self) -> u8 {
        TRIANGLE_SEQUENCE[self.sequence_step as usize]
    }
}
```

---

## Linear Counter

The linear counter is a **7-bit counter** (0-127) providing more precise timing control than the length counter's 5-bit resolution.

### Purpose

The linear counter allows:

- Fine-grained note duration control
- Automatic silencing without CPU intervention
- Higher timing accuracy than length counter

**Comparison:**

- **Length Counter**: 5-bit (32 values), units of ~8.33ms (half frames)
- **Linear Counter**: 7-bit (128 values), units of ~4.17ms (quarter frames)

### Operation

**Clock Source:** Frame counter (quarter frames) - 240 Hz NTSC

**State Machine:**

```
State: counter (0-127), reload_flag (bool)

On quarter frame:
    if reload_flag:
        counter = reload_value
    else if counter > 0:
        counter -= 1

    if control_flag == 0:
        reload_flag = false
```

**Key Behavior:**

- Reload flag is set when writing $400B
- Reload flag is set continuously when control flag = 1
- Counter only reloads when reload flag is set
- Counter decrements when reload flag is clear and counter > 0

### Control Flag Modes

**Mode 1: Control Flag = 0 (Normal)**

```
Write $400B → reload_flag = true
Next quarter frame → counter = reload_value, reload_flag = false
Following frames → counter decrements to 0
```

**Mode 2: Control Flag = 1 (Hold)**

```
reload_flag = true continuously
Counter stays at reload_value (never decrements)
Length counter is halted
```

### Implementation

```rust
pub struct LinearCounter {
    reload_value: u8,  // 0-127
    counter: u8,       // 0-127
    reload_flag: bool,
    control_flag: bool,
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

    pub fn set_reload_flag(&mut self) {
        self.reload_flag = true;
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}
```

---

## Length Counter

The triangle channel's length counter works **identically** to pulse channels, using the same lookup table.

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

**Units:** Half frames (120 Hz NTSC)

### Halt Behavior

The **control flag** (C bit in $4008) halts the length counter:

- **C=0**: Counter decrements normally
- **C=1**: Counter is frozen at current value

### Interaction with Linear Counter

**Both counters must be non-zero** for the sequencer to advance:

```rust
pub fn clock_timer(&mut self) {
    if self.timer.clock() {
        // Only clock sequencer if both counters active
        if self.linear_counter.is_active() && self.length_counter.is_active() {
            self.sequencer.clock();
        }
    }
}
```

---

## Timer and Frequency

### Timer Operation

The triangle timer operates at **CPU clock rate** (not divided by 2 like pulse channels):

```
Timer = HHHLLLLLLLL (bits 10-0)

Each CPU cycle:
    if timer == 0:
        timer = reload_value
        clock_sequencer()
    else:
        timer -= 1
```

**Sequencer Clock Period:** 32 × (timer + 1) CPU cycles

### Frequency Calculation

```
f_triangle = f_CPU / (32 × (timer + 1))

NTSC: f_CPU = 1.789773 MHz
PAL:  f_CPU = 1.662607 MHz
```

**Frequency Range (NTSC):**

- Minimum: 27.3 Hz (timer = $7FF, 2048 + 1 = 2049)
- Maximum: 55.9 kHz (timer = $000, 0 + 1 = 1)

**Note:** Unlike pulse channels, triangle does NOT mute at low timer values. Timer = 0 produces 55.9 kHz (ultrasonic).

### Frequency Comparison

| Timer | Triangle Freq | Pulse Freq | Ratio |
|-------|---------------|------------|-------|
| $000 | 55.9 kHz | Muted | N/A |
| $008 | 13.9 kHz | 12.4 kHz | 1.12× |
| $100 | 6.98 kHz | 6.21 kHz | 1.12× |
| $200 | 3.48 kHz | 3.09 kHz | 1.13× |
| $400 | 1.74 kHz | 1.54 kHz | 1.13× |
| $7FF | 27.3 Hz | 54.6 Hz | 0.50× |

**Key Insight:** Triangle channel's timer runs twice as fast (no ÷2), compensated by 32-step sequence (vs 8-step for pulse).

### Period-to-Note Conversion

```rust
fn frequency_to_timer(freq_hz: f32) -> u16 {
    let cpu_clock = 1_789_773.0;
    ((cpu_clock / (32.0 * freq_hz)) - 1.0) as u16
}

// Example: C2 (65.41 Hz - bass note)
let timer = frequency_to_timer(65.41);  // ~852
```

---

## Silencing Techniques

Since the triangle channel lacks volume control, games use four techniques to silence it:

### Method 1: Disable via $4015

**Most Common Method:**

```rust
// Silence triangle channel
apu.write(0x4015, 0b11111011);  // Clear bit 2

// Re-enable
apu.write(0x4015, 0b11111111);  // Set bit 2
```

**Effect:** Sets length counter to 0 immediately.

**Pros:** Instant silence, clean
**Cons:** Requires re-initialization ($400B write) to restart

### Method 2: Set Control Flag

```rust
// Hold linear counter at reload value
apu.write(0x4008, 0x80);  // Control = 1, Reload = 0
```

**Effect:** Linear counter stays at reload value but length counter still active.

**Pros:** Easy restart (just write $400B)
**Cons:** Doesn't fully silence (length counter must also be 0)

### Method 3: Ultrasonic Frequency

```rust
// Timer = 0 or 1 produces >28 kHz (inaudible)
apu.write(0x400A, 0x00);
apu.write(0x400B, 0x00);
```

**Effect:** Channel plays at ultrasonic frequency, filtered out by TV/audio hardware.

**Pros:** Channel remains "active" for fast restart
**Cons:** May produce noise on sensitive audio equipment

### Method 4: Linear/Length Counter Timing

```rust
// Use short counter values for automatic stop
apu.write(0x4008, 0x01);  // Linear counter = 1 (4.17ms)
apu.write(0x400B, 0x08);  // Length counter = $00 = 10 half-frames (83.3ms)
```

**Effect:** Channel automatically silences after specified duration.

**Pros:** No CPU intervention required
**Cons:** Not instant, requires timing calculation

### Comparison

| Method | Silence Time | CPU Overhead | Restart Cost | Use Case |
|--------|-------------|--------------|--------------|----------|
| **$4015** | Instant | 1 write | High (re-init) | Stop playback |
| **Control Flag** | ~4ms (quarter frame) | 1 write | Low ($400B) | Pause/resume |
| **Ultrasonic** | Instant | 2 writes | None | Fast toggle |
| **Counters** | Variable | 2 writes | None | Auto-stop notes |

---

## Implementation Guide

### Complete Triangle Channel Structure

```rust
pub struct TriangleChannel {
    // Component units
    timer: Timer,
    sequencer: TriangleSequencer,
    linear_counter: LinearCounter,
    length_counter: LengthCounter,

    // Enable flag from $4015
    enabled: bool,
}

impl TriangleChannel {
    /// Clock the timer (every CPU cycle)
    pub fn clock_timer(&mut self) {
        // Timer runs at CPU clock rate (no ÷2)
        if self.timer.clock() {
            // Sequencer only advances if both counters active
            if self.linear_counter.is_active() && self.length_counter.is_active() {
                self.sequencer.clock();
            }
        }
    }

    /// Clock linear counter (quarter frame)
    pub fn clock_quarter_frame(&mut self) {
        self.linear_counter.clock();
    }

    /// Clock length counter (half frame)
    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
    }

    /// Get current output sample (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if !self.linear_counter.is_active() || !self.length_counter.is_active() {
            return 0;
        }

        self.sequencer.output()
    }

    /// Write to register $4008
    pub fn write_linear_counter(&mut self, value: u8) {
        self.linear_counter.control_flag = (value & 0x80) != 0;
        self.linear_counter.reload_value = value & 0x7F;
        self.length_counter.halt = (value & 0x80) != 0;
    }

    /// Write to register $400A
    pub fn write_timer_low(&mut self, value: u8) {
        self.timer.period = (self.timer.period & 0xFF00) | (value as u16);
    }

    /// Write to register $400B
    pub fn write_length_timer_high(&mut self, value: u8) {
        self.timer.period = (self.timer.period & 0x00FF) | (((value & 0x07) as u16) << 8);

        let length_index = value >> 3;
        self.length_counter.load(length_index);

        // Set linear counter reload flag
        self.linear_counter.set_reload_flag();
    }
}
```

### Timer Implementation

```rust
pub struct Timer {
    period: u16,   // 11-bit reload value
    counter: u16,  // Current count
}

impl Timer {
    /// Clock the timer, returns true if wrapped
    pub fn clock(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period;
            true
        } else {
            self.counter -= 1;
            false
        }
    }
}
```

---

## Common Pitfalls

### 1. Timer Clock Rate Confusion

**Problem:** Using APU clock (CPU/2) instead of CPU clock for triangle timer.

**Solution:** Triangle timer runs at CPU speed, no division.

```rust
// WRONG: APU clock rate
if apu_cycle % 2 == 0 {
    triangle.clock_timer();
}

// CORRECT: CPU clock rate
triangle.clock_timer();  // Every CPU cycle
```

### 2. Sequencer Phase Reset

**Problem:** Resetting sequencer phase when writing $400B (like pulse channels).

**Solution:** Triangle does NOT reset phase, allowing glitch-free frequency changes.

```rust
pub fn write_length_timer_high(&mut self, value: u8) {
    // Update timer
    self.timer.period = (self.timer.period & 0x00FF) | (((value & 0x07) as u16) << 8);

    // Load length counter
    self.length_counter.load(value >> 3);

    // Set reload flag
    self.linear_counter.set_reload_flag();

    // DO NOT reset sequencer phase!
    // self.sequencer.sequence_step = 0;  // WRONG
}
```

### 3. Linear Counter Reload Logic

**Problem:** Not understanding control flag's effect on reload flag.

**Solution:** Control flag determines if reload flag clears after reload.

```rust
pub fn clock(&mut self) {
    if self.reload_flag {
        self.counter = self.reload_value;
    } else if self.counter > 0 {
        self.counter -= 1;
    }

    // Reload flag only clears if control flag is clear
    if !self.control_flag {
        self.reload_flag = false;
    }
}
```

### 4. Counter Gating

**Problem:** Allowing sequencer to advance when either counter is zero.

**Solution:** BOTH counters must be non-zero.

```rust
// WRONG: OR logic
if self.linear_counter.is_active() || self.length_counter.is_active() {
    self.sequencer.clock();
}

// CORRECT: AND logic
if self.linear_counter.is_active() && self.length_counter.is_active() {
    self.sequencer.clock();
}
```

---

## Testing and Validation

### Test ROMs

| ROM | Tests | Pass Criteria |
|-----|-------|---------------|
| **apu_test** | Basic triangle functionality | All tests pass |
| **blargg_apu_2005.nes** | Linear counter behavior | Text output "Passed" |
| **03-triangle.nes** | Triangle channel specifics | Correct timing |
| **test_tri_linear_ctr.nes** | Linear counter edge cases | All tests pass |

### Manual Testing

**Linear Counter Test:**

```
Set: Control=0, Reload=127
Write $400B
Verify: Channel plays for ~529ms (127 × 4.17ms)
```

**Length Counter Test:**

```
Set: Control=0, Reload=0
Write $400B with length=$1F (72 half-frames)
Verify: Channel plays for 600ms (72 × 8.33ms)
```

**Frequency Range Test:**

```
Timer=$000: 55.9 kHz (ultrasonic, inaudible)
Timer=$100: 6.98 kHz (high pitch)
Timer=$400: 1.74 kHz (mid pitch)
Timer=$7FF: 27.3 Hz (low bass)
```

**Counter Interaction Test:**

```
Set: Control=0, Linear=10, Length=$00 (10)
Write $400B
Verify: Channel stops after ~41.7ms (10 quarter frames)
```

---

## Related Documentation

- [APU_OVERVIEW.md](APU_OVERVIEW.md) - General APU architecture
- [APU_TIMING.md](APU_TIMING.md) - Frame counter and timing details
- [APU_CHANNEL_PULSE.md](APU_CHANNEL_PULSE.md) - Pulse channel specification
- [APU_CHANNEL_NOISE.md](APU_CHANNEL_NOISE.md) - Noise channel specification
- [APU_MIXER.md](APU_MIXER.md) - Audio mixing and output

---

## References

- [NESdev Wiki: APU Triangle](https://www.nesdev.org/wiki/APU_Triangle)
- [NESdev Wiki: APU Length Counter](https://www.nesdev.org/wiki/APU_Length_Counter)
- Blargg APU Test ROMs - Linear counter validation
- Visual 2A03 - Hardware simulation

---

**Document Status:** Complete specification for triangle channel implementation with cycle-accurate behavior.
