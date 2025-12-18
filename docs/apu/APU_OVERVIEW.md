# APU Overview (2A03 Audio Processing Unit)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Introduction](#introduction)
- [APU Specifications](#apu-specifications)
- [Audio Channels](#audio-channels)
- [Register Interface](#register-interface)
- [Frame Counter](#frame-counter)
- [Mixing and Output](#mixing-and-output)
- [DMC DMA](#dmc-dma)
- [Implementation Overview](#implementation-overview)
- [Common Pitfalls](#common-pitfalls)

---

## Introduction

The **Ricoh 2A03** (NTSC) and **2A07** (PAL) are the CPUs used in the NES, which **integrate** a modified 6502 core with an Audio Processing Unit (APU). The APU synthesizes audio in real-time using **5 independent channels**:

1. **Pulse 1** - Square wave with sweep
2. **Pulse 2** - Square wave with sweep
3. **Triangle** - Triangle wave (no volume control)
4. **Noise** - Pseudo-random noise
5. **DMC** - Delta Modulation Channel (sample playback)

**Key Characteristics:**
- Hardware synthesis (no PCM sample memory)
- Real-time mixing of all channels
- Frame counter for envelope/sweep updates
- DMC channel can read samples from CPU memory (DMA)
- IRQ generation capability

---

## APU Specifications

### Clock and Timing

```
NTSC (2A03):
  CPU/APU Clock:  1.789773 MHz (master ÷ 12)
  Frame Rate:     ~240 Hz (CPU cycles ÷ 7457)

PAL (2A07):
  CPU/APU Clock:  1.662607 MHz (master ÷ 16)
  Frame Rate:     ~200 Hz (CPU cycles ÷ 8313)
```

### Output Specifications

```
Sample Rate:      Variable (typically 48 kHz for emulators)
Bit Depth:        4-bit per channel (hardware)
                  16-bit mixed (emulator output)
Dynamic Range:    ~15-bit (after mixing)
```

### Channel Capabilities

| Channel | Type | Frequency Range | Volume Control | Sweep | Duty Cycle |
|---------|------|-----------------|----------------|-------|------------|
| **Pulse 1** | Square | 54.6 Hz - 12.4 kHz | 4-bit (16 levels) | Yes | 4 settings |
| **Pulse 2** | Square | 54.6 Hz - 12.4 kHz | 4-bit (16 levels) | Yes | 4 settings |
| **Triangle** | Triangle | 27.3 Hz - 55.9 kHz | None (on/off) | No | N/A |
| **Noise** | LFSR | 29.3 Hz - 447 kHz | 4-bit (16 levels) | No | N/A |
| **DMC** | 1-bit Delta | 4.2 kHz - 33.1 kHz | 7-bit (128 levels) | No | N/A |

---

## Audio Channels

### 1. Pulse Channels (2×)

Square wave generators with **duty cycle control** and **frequency sweep**:

**Features:**
- 4 duty cycles: 12.5%, 25%, 50%, 75%
- Hardware envelope generator (volume fade)
- Frequency sweep (pitch bend)
- Length counter (note duration)

**Typical Uses:**
- Melody (Pulse 1)
- Harmony (Pulse 2)
- Sound effects

### 2. Triangle Channel

Triangle wave generator with **no volume control**:

**Features:**
- Fixed volume (always maximum or silent)
- Linear counter (alternative to length counter)
- Higher frequency range than pulse channels

**Typical Uses:**
- Bass lines
- Low-frequency sound effects
- Percussion (combined with noise)

### 3. Noise Channel

Pseudo-random noise generator using **Linear Feedback Shift Register (LFSR)**:

**Features:**
- Two modes: 15-bit (long period) and 6-bit (short period)
- Hardware envelope
- Length counter

**Typical Uses:**
- Percussion (drums, cymbals)
- Explosions
- Wind/water effects

### 4. DMC Channel (Delta Modulation)

1-bit delta-encoded sample playback from CPU memory:

**Features:**
- 16 sample rates (4.2 kHz - 33.1 kHz)
- Reads samples via DMA (steals CPU cycles)
- 7-bit output level
- Loop support

**Typical Uses:**
- Drum samples
- Voice clips
- Sound effects

---

## Register Interface

The APU is controlled via **24 memory-mapped registers** at CPU addresses `$4000-$4017`.

### Complete Register Map

| Address | Channel | Register | Description |
|---------|---------|----------|-------------|
| **$4000** | Pulse 1 | DDLC VVVV | Duty, loop envelope, constant volume, volume/envelope |
| **$4001** | Pulse 1 | EPPP NSSS | Sweep enable, period, negate, shift |
| **$4002** | Pulse 1 | TTTT TTTT | Timer low 8 bits |
| **$4003** | Pulse 1 | LLLL LTTT | Length counter load, timer high 3 bits |
| **$4004** | Pulse 2 | DDLC VVVV | Duty, loop envelope, constant volume, volume/envelope |
| **$4005** | Pulse 2 | EPPP NSSS | Sweep enable, period, negate, shift |
| **$4006** | Pulse 2 | TTTT TTTT | Timer low 8 bits |
| **$4007** | Pulse 2 | LLLL LTTT | Length counter load, timer high 3 bits |
| **$4008** | Triangle | CRRR RRRR | Control flag, linear counter reload |
| **$4009** | Triangle | ---- ---- | Unused |
| **$400A** | Triangle | TTTT TTTT | Timer low 8 bits |
| **$400B** | Triangle | LLLL LTTT | Length counter load, timer high 3 bits |
| **$400C** | Noise | --LC VVVV | Loop envelope, constant volume, volume/envelope |
| **$400D** | Noise | ---- ---- | Unused |
| **$400E** | Noise | L--- PPPP | Loop noise, period |
| **$400F** | Noise | LLLL L--- | Length counter load |
| **$4010** | DMC | IL-- RRRR | IRQ enable, loop, frequency/rate |
| **$4011** | DMC | -DDD DDDD | Direct load (7-bit DAC) |
| **$4012** | DMC | AAAA AAAA | Sample address = $C000 + (A × $40) |
| **$4013** | DMC | LLLL LLLL | Sample length = (L × $10) + 1 |
| **$4014** | - | - | OAM DMA (writes to $2004) |
| **$4015** | - | ---D NT21 | Status (read) / Enable (write) |
| **$4016** | - | - | Controller 1 + Expansion |
| **$4017** | - | MI-- ----  | Frame counter mode, IRQ inhibit |

### Key Registers

#### $4015 - Status/Enable

**Write:**
```
---D NT21
   | ||||
   | |||+- Enable Pulse 1
   | ||+-- Enable Pulse 2
   | |+--- Enable Triangle
   | +---- Enable Noise
   +------ Enable DMC
```

**Read:**
```
IF-D NT21
|||| ||||
|||| |||+- Pulse 1 length counter > 0
|||| ||+-- Pulse 2 length counter > 0
|||| |+--- Triangle length counter > 0
|||| +---- Noise length counter > 0
|||+------ DMC active (bytes remaining > 0)
||+------- (unused)
|+-------- Frame interrupt flag
+--------- DMC interrupt flag
```

**Important:** Writing to $4015:
- Clears DMC interrupt flag
- If a channel is disabled, its length counter is set to 0
- If DMC is enabled while bytes remaining = 0, restarts sample

#### $4017 - Frame Counter

```
MI-- ----
||
|+-------- IRQ inhibit (0: enable, 1: disable)
+--------- Mode (0: 4-step, 1: 5-step)
```

**Modes:**
- **4-step mode**: 4 quarter frames, generates IRQ
- **5-step mode**: 5 quarter frames, no IRQ

---

## Frame Counter

The **frame counter** divides time into **frames** and **quarter-frames** for timing envelope, sweep, and length counter updates.

### 4-Step Mode (Mode 0)

```
Step   CPU Cycles   Action
-------------------------------
0      7457         Clock envelopes & linear counter
1      14913        Clock envelopes, linear, length, & sweep
2      22371        Clock envelopes & linear counter
3      29829        Clock envelopes, linear, length, & sweep
                    Set IRQ flag
4      29830        Set IRQ flag
       29831        Set IRQ flag (total 3 CPU cycles)
```

**Frame Time:** 29,829 CPU cycles (~16.67 ms @ 1.789773 MHz)

### 5-Step Mode (Mode 1)

```
Step   CPU Cycles   Action
-------------------------------
0      7457         Clock envelopes & linear counter
1      14913        Clock envelopes, linear, length, & sweep
2      22371        Clock envelopes & linear counter
3      29829        (nothing)
4      37281        Clock envelopes, linear, length, & sweep
```

**Frame Time:** 37,281 CPU cycles (~20.83 ms @ 1.789773 MHz)

**Note:** 5-step mode does NOT generate IRQ.

### Frame Counter Implementation

```rust
pub struct FrameCounter {
    mode: u8,           // 0 = 4-step, 1 = 5-step
    irq_inhibit: bool,
    cycle_count: u64,
    step: u8,
}

impl FrameCounter {
    pub fn clock(&mut self) {
        self.cycle_count += 1;

        let (quarter_frame, half_frame) = match self.mode {
            0 => {
                // 4-step mode
                match self.cycle_count {
                    7457 => (true, false),
                    14913 => (true, true),
                    22371 => (true, false),
                    29829 => {
                        self.cycle_count = 0;
                        (true, true)
                    }
                    _ => (false, false),
                }
            }
            1 => {
                // 5-step mode
                match self.cycle_count {
                    7457 => (true, false),
                    14913 => (true, true),
                    22371 => (true, false),
                    37281 => {
                        self.cycle_count = 0;
                        (true, true)
                    }
                    _ => (false, false),
                }
            }
            _ => unreachable!(),
        };

        if quarter_frame {
            // Clock envelopes and linear counter
        }

        if half_frame {
            // Clock length counters and sweep units
        }
    }
}
```

---

## Mixing and Output

### Channel Outputs

Each channel produces a digital output value:

| Channel | Output Range | Notes |
|---------|--------------|-------|
| Pulse 1 | 0-15 | 4-bit envelope × duty cycle |
| Pulse 2 | 0-15 | 4-bit envelope × duty cycle |
| Triangle | 0-15 | 4-bit sequencer (triangle wave) |
| Noise | 0-15 | 4-bit envelope × LFSR output |
| DMC | 0-127 | 7-bit delta counter |

### Mixing Formula

The NES uses **non-linear mixing** to combine channels:

```
pulse_out = 95.88 / ((8128 / (pulse1 + pulse2)) + 100)

tnd_out = 159.79 / ((1 / (triangle/8227 + noise/12241 + dmc/22638)) + 100)

output = pulse_out + tnd_out
```

**Simplified Integer Implementation:**
```rust
fn mix_channels(&self) -> f32 {
    // Pulse channels
    let pulse_sum = self.pulse1.output() + self.pulse2.output();
    let pulse_out = if pulse_sum == 0 {
        0.0
    } else {
        95.88 / ((8128.0 / pulse_sum as f32) + 100.0)
    };

    // TND channels
    let triangle = self.triangle.output() as f32 / 8227.0;
    let noise = self.noise.output() as f32 / 12241.0;
    let dmc = self.dmc.output() as f32 / 22638.0;

    let tnd_out = if (triangle + noise + dmc) == 0.0 {
        0.0
    } else {
        159.79 / ((1.0 / (triangle + noise + dmc)) + 100.0)
    };

    pulse_out + tnd_out
}
```

### Lookup Table Optimization

Pre-compute mixing tables for performance:

```rust
const PULSE_TABLE: [f32; 31] = [/* ... */];
const TND_TABLE: [f32; 203] = [/* ... */];

fn mix_channels_fast(&self) -> f32 {
    let pulse_index = self.pulse1.output() + self.pulse2.output();
    let tnd_index = 3 * self.triangle.output()
                  + 2 * self.noise.output()
                  + self.dmc.output();

    PULSE_TABLE[pulse_index as usize] + TND_TABLE[tnd_index as usize]
}
```

---

## DMC DMA

The DMC channel reads samples from **CPU memory** via **Direct Memory Access (DMA)**, which **stalls the CPU** for cycles.

### Sample Playback

```
1. DMC timer expires
2. Read byte from sample address
3. Increment address
4. Decrement bytes remaining
5. If bytes remaining == 0:
   - If loop enabled: restart sample
   - If IRQ enabled: trigger IRQ
```

### DMA Timing

Each DMC sample read steals **4 CPU cycles**:

```
Cycle 1: DMC requests DMA
Cycle 2: CPU finishes current instruction
Cycle 3: Dummy read cycle
Cycle 4: Read sample byte from memory
```

**Conflict with OAM DMA:**
If DMC DMA occurs during OAM DMA, the total stall can be:
- Best case: +2 cycles
- Worst case: +4 cycles

### Sample Address Calculation

```
Sample Address = $C000 + (register_value × $40)

Examples:
  $00 → $C000
  $01 → $C040
  $FF → $FFC0
```

### Sample Length Calculation

```
Sample Length = (register_value × $10) + 1

Examples:
  $00 → 1 byte
  $01 → 17 bytes
  $FF → 4081 bytes
```

---

## Implementation Overview

### Core APU Structure

```rust
pub struct Apu {
    // Channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    // Frame counter
    frame_counter: FrameCounter,

    // Cycle tracking
    cycles: u64,

    // IRQ flags
    frame_irq: bool,
    dmc_irq: bool,
}
```

### Step Function

```rust
pub fn step(&mut self, cpu_cycles: u8) {
    for _ in 0..cpu_cycles {
        self.cycles += 1;

        // Clock frame counter
        self.frame_counter.clock(&mut self.pulse1, &mut self.pulse2,
                                  &mut self.triangle, &mut self.noise);

        // Clock channels
        self.pulse1.clock();
        self.pulse2.clock();
        self.triangle.clock();
        self.noise.clock();
        self.dmc.clock();

        // Generate sample if needed
        if self.should_generate_sample() {
            let sample = self.mix_channels();
            self.output_buffer.push(sample);
        }
    }
}
```

---

## Common Pitfalls

### 1. Frame Counter Timing

The frame counter operates on CPU cycles, not APU-specific clocks:

```rust
// WRONG: Separate APU clock
if self.apu_cycle % 7457 == 0 { /* clock envelope */ }

// CORRECT: Use CPU cycle count
if self.cpu_cycles == 7457 { /* clock envelope */ }
```

### 2. Channel Enable State

Disabling a channel via $4015 sets its length counter to 0, but doesn't immediately silence it:

```rust
fn write_status(&mut self, value: u8) {
    if (value & 0x01) == 0 {
        self.pulse1.length_counter = 0; // CORRECT
        // Don't zero out other state!
    }
}
```

### 3. DMC IRQ vs. Frame IRQ

Two separate IRQ sources that must be handled independently:

```rust
pub fn irq_pending(&self) -> bool {
    (!self.frame_counter.irq_inhibit && self.frame_irq) || self.dmc_irq
}
```

### 4. Triangle Linear Counter

Triangle uses **linear counter** instead of envelope, with different behavior:

```rust
// Triangle has no envelope - linear counter only
if self.triangle.control_flag {
    self.triangle.linear_counter = self.triangle.reload_value;
} else if self.triangle.linear_counter > 0 {
    self.triangle.linear_counter -= 1;
}
```

### 5. Sweep Unit Muting

Sweep units can **mute** the pulse channel if the target period is out of range:

```rust
fn pulse_is_muted(&self) -> bool {
    self.timer < 8 || self.target_period() > 0x7FF
}
```

---

## References

- [NesDev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [APU Pulse](https://www.nesdev.org/wiki/APU_Pulse)
- [APU Triangle](https://www.nesdev.org/wiki/APU_Triangle)
- [APU Noise](https://www.nesdev.org/wiki/APU_Noise)
- [APU DMC](https://www.nesdev.org/wiki/APU_DMC)
- [APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)

---

**Next:** [APU Channels](APU_CHANNELS.md) | [APU Timing](APU_TIMING.md)
