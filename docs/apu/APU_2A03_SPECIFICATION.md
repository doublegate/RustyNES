# APU 2A03 Complete Specification

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete 2A03 APU channel specifications and frame sequencer

---

## Table of Contents

- [Overview](#overview)
- [Register Map](#register-map)
- [Frame Sequencer](#frame-sequencer)
- [Channel Specifications](#channel-specifications)
- [Length Counter](#length-counter)
- [Envelope Generator](#envelope-generator)
- [Sweep Unit](#sweep-unit)
- [Mixer](#mixer)
- [Implementation Guide](#implementation-guide)

---

## Overview

The **2A03** (NTSC) and **2A07** (PAL) are the NES CPUs with integrated Audio Processing Unit (APU). The APU generates 5 channels of audio:

- **Pulse 1**: Square wave with sweep
- **Pulse 2**: Square wave with sweep
- **Triangle**: Triangle wave
- **Noise**: Pseudo-random noise
- **DMC**: Delta Modulation Channel (sample playback)

### Clock Rates

```
NTSC (2A03): 1.789773 MHz
PAL (2A07):  1.662607 MHz

APU Frame Counter Rates:
  4-step: 60 Hz (NTSC), 50 Hz (PAL)
  5-step: 48 Hz (NTSC), 40 Hz (PAL)
```

---

## Register Map

| Address | Channel | Register | Description |
|---------|---------|----------|-------------|
| **$4000** | Pulse 1 | DDLC VVVV | Duty, loop, constant, volume/envelope |
| **$4001** | Pulse 1 | EPPP NSSS | Sweep enable, period, negate, shift |
| **$4002** | Pulse 1 | TTTT TTTT | Timer low 8 bits |
| **$4003** | Pulse 1 | LLLL LTTT | Length counter load, timer high 3 bits |
| **$4004** | Pulse 2 | DDLC VVVV | Duty, loop, constant, volume/envelope |
| **$4005** | Pulse 2 | EPPP NSSS | Sweep enable, period, negate, shift |
| **$4006** | Pulse 2 | TTTT TTTT | Timer low 8 bits |
| **$4007** | Pulse 2 | LLLL LTTT | Length counter load, timer high 3 bits |
| **$4008** | Triangle | CRRR RRRR | Control, linear counter load |
| **$400A** | Triangle | TTTT TTTT | Timer low 8 bits |
| **$400B** | Triangle | LLLL LTTT | Length counter load, timer high 3 bits |
| **$400C** | Noise | --LC VVVV | Loop, constant, volume/envelope |
| **$400E** | Noise | L--- PPPP | Loop noise, period |
| **$400F** | Noise | LLLL L--- | Length counter load |
| **$4010** | DMC | IL-- RRRR | IRQ enable, loop, rate |
| **$4011** | DMC | -DDD DDDD | Direct load (7-bit) |
| **$4012** | DMC | AAAA AAAA | Sample address = $C000 + A×64 |
| **$4013** | DMC | LLLL LLLL | Sample length = L×16 + 1 |
| **$4015** | Status | ---D NT21 | Enable DMC, noise, triangle, pulse2, pulse1 |
| **$4017** | Frame | MI-- ---- | Mode, IRQ inhibit |

### $4015 - Status Register

**Write:**
```
7  bit  0
---- ----
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
7  bit  0
---- ----
IF-D NT21
|| | ||||
|| | |||+- Pulse 1 length counter > 0
|| | ||+-- Pulse 2 length counter > 0
|| | |+--- Triangle length counter > 0
|| | +---- Noise length counter > 0
|| +------ DMC bytes remaining > 0
|+-------- Frame interrupt flag
+--------- DMC interrupt flag
```

---

## Frame Sequencer

The frame sequencer clocks the length counters and envelopes at regular intervals.

### 4-Step Mode (Default)

```
Mode 0:   60 Hz frame rate
Sequence: 4 steps per frame

Step   Cycles  Actions
----   ------  -------
1      7457    Clock envelope
2      14913   Clock envelope, length counter
3      22371   Clock envelope
4      29829   Clock envelope, length counter, set IRQ flag (if enabled)
0      29830   (Wrap to step 1)
```

### 5-Step Mode

```
Mode 1:   48 Hz frame rate
Sequence: 5 steps per frame

Step   Cycles  Actions
----   ------  -------
1      7457    Clock envelope, length counter
2      14913   Clock envelope
3      22371   Clock envelope, length counter
4      29829   Nothing
5      37281   Clock envelope, length counter
0      37282   (Wrap to step 1)
```

### $4017 - Frame Counter

```
7  bit  0
---- ----
MI-- ----
||
|+------- IRQ inhibit flag (0: IRQ enabled, 1: IRQ disabled)
+-------- Mode (0: 4-step, 1: 5-step)
```

**Write behavior:**
- If bit 7 set: 5-step mode
- If bit 6 set: Disable frame IRQ
- Write immediately clocks all units if bit 7 is set

---

## Channel Specifications

### Pulse Channels (1 & 2)

**Components:**
- Timer (11-bit, clocked at APU rate)
- Duty cycle generator (8-step sequencer)
- Length counter
- Envelope generator
- Sweep unit

**Output:** 4-bit volume (0-15)

#### Duty Cycles

```
Duty 0: 12.5%  01000000
Duty 1: 25%    01100000
Duty 2: 50%    01111000
Duty 3: 75%    01111110
```

#### Pulse Formula

```rust
if length_counter > 0 && !sweep_muted() {
    let sequencer_output = DUTY_TABLE[duty][sequencer_pos];
    if sequencer_output == 1 {
        envelope_volume()
    } else {
        0
    }
} else {
    0
}
```

### Triangle Channel

**Components:**
- Timer (11-bit)
- Linear counter (7-bit, reload value)
- Length counter
- 32-step sequencer (triangle wave)

**Output:** 4-bit volume (0-15, not controllable by software)

#### Triangle Sequence

```
32-step sequence:
15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
 0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
```

#### Triangle Formula

```rust
if length_counter > 0 && linear_counter > 0 {
    TRIANGLE_SEQUENCE[sequencer_pos]
} else {
    0
}
```

### Noise Channel

**Components:**
- Timer (4-bit period index)
- 15-bit Linear Feedback Shift Register (LFSR)
- Length counter
- Envelope generator

**Output:** 4-bit volume (0-15)

#### LFSR Modes

```
Mode 0 (normal):    Feedback from bits 0 and 1
Mode 1 (short):     Feedback from bits 0 and 6
```

#### Noise Period Table (NTSC)

```
Period  Cycles
------  ------
0       4
1       8
2       16
3       32
4       64
5       96
6       128
7       160
8       202
9       254
10      380
11      508
12      762
13      1016
14      2034
15      4068
```

### DMC Channel

**Components:**
- Timer (4-bit period index)
- Memory reader (fetches samples from $8000-$FFFF)
- Output unit (7-bit counter)
- Sample buffer

**Output:** 7-bit PCM (0-127)

#### DMC Rate Table (NTSC)

```
Rate  Period
----  ------
0     428
1     380
2     340
3     320
4     286
5     254
6     226
7     214
8     190
9     160
10    142
11    128
12    106
13    84
14    72
15    54
```

---

## Length Counter

Shared by all channels except DMC. When enabled, automatically silences channel after a duration.

### Length Counter Table

```
Index  Length    Index  Length
-----  ------    -----  ------
$00    10        $10    254
$01    254       $11    2
$02    20        $12    4
$03    2         $13    2
$04    40        $14    8
$05    4         $15    2
$06    80        $16    16
$07    6         $17    2
$08    160       $18    32
$09    8         $19    2
$0A    60        $1A    64
$0B    10        $1B    2
$0C    14        $1C    128
$0D    12        $1D    2
$0E    26        $1E    48
$0F    14        $1F    4
```

### Length Counter Behavior

```rust
// On quarter frame
if !halt && length > 0 {
    length -= 1;
}

// On register write ($4003, $4007, $400B, $400F)
if enabled {
    length = LENGTH_TABLE[value >> 3];
}
```

---

## Envelope Generator

Used by pulse and noise channels to generate volume envelopes.

### Envelope Operation

```rust
struct Envelope {
    start_flag: bool,
    divider: u8,
    decay_level: u8,
    loop_flag: bool,
    constant_volume: bool,
    volume: u8,
}

fn clock_envelope(&mut self) {
    if self.start_flag {
        self.start_flag = false;
        self.decay_level = 15;
        self.divider = self.volume;
    } else {
        if self.divider == 0 {
            self.divider = self.volume;

            if self.decay_level > 0 {
                self.decay_level -= 1;
            } else if self.loop_flag {
                self.decay_level = 15;
            }
        } else {
            self.divider -= 1;
        }
    }
}

fn get_volume(&self) -> u8 {
    if self.constant_volume {
        self.volume
    } else {
        self.decay_level
    }
}
```

---

## Sweep Unit

Used by pulse channels to automatically adjust frequency.

### Sweep Formula

```rust
fn clock_sweep(&mut self) {
    if self.sweep_divider == 0 && self.sweep_enabled && !self.is_muted() {
        let delta = self.timer >> self.sweep_shift;
        if self.sweep_negate {
            self.timer -= delta;
            if self.channel == 1 {
                self.timer -= 1;  // Pulse 1 uses one's complement
            }
        } else {
            self.timer += delta;
        }
    }

    if self.sweep_divider == 0 || self.sweep_reload {
        self.sweep_divider = self.sweep_period;
        self.sweep_reload = false;
    } else {
        self.sweep_divider -= 1;
    }
}

fn is_muted(&self) -> bool {
    self.timer < 8 || (!self.sweep_negate && self.timer + (self.timer >> self.sweep_shift) > 0x7FF)
}
```

---

## Mixer

All 5 channels are mixed together with non-linear mixing:

### Mixer Formula

```
pulse_out = 95.88 / ((8128 / (pulse1 + pulse2)) + 100)

tnd_out = 159.79 / (1 / (triangle/8227 + noise/12241 + dmc/22638) + 100)

output = pulse_out + tnd_out
```

### Lookup Table Optimization

```rust
// Precompute lookup tables
const PULSE_TABLE: [f32; 31] = /* ... */;
const TND_TABLE: [f32; 203] = /* ... */;

fn mix(&self) -> f32 {
    let pulse_index = self.pulse1_output + self.pulse2_output;
    let tnd_index = 3 * self.triangle_output + 2 * self.noise_output + self.dmc_output;

    PULSE_TABLE[pulse_index as usize] + TND_TABLE[tnd_index as usize]
}
```

---

## Implementation Guide

### Complete APU Structure

```rust
pub struct Apu {
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    frame_counter: FrameCounter,
    cycles: u64,
}

impl Apu {
    pub fn step(&mut self) {
        // Clock frame counter
        self.frame_counter.step();

        // Clock all channels
        self.pulse1.step();
        self.pulse2.step();
        self.triangle.step_timer();
        self.noise.step();
        self.dmc.step();

        self.cycles += 1;
    }

    pub fn get_output(&self) -> f32 {
        let pulse1 = self.pulse1.output();
        let pulse2 = self.pulse2.output();
        let triangle = self.triangle.output();
        let noise = self.noise.output();
        let dmc = self.dmc.output();

        mix_channels(pulse1, pulse2, triangle, noise, dmc)
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write_register(addr & 3, value),
            0x4004..=0x4007 => self.pulse2.write_register(addr & 3, value),
            0x4008..=0x400B => self.triangle.write_register(addr & 3, value),
            0x400C..=0x400F => self.noise.write_register(addr & 3, value),
            0x4010..=0x4013 => self.dmc.write_register(addr & 3, value),
            0x4015 => self.write_status(value),
            0x4017 => self.frame_counter.write_control(value),
            _ => {}
        }
    }
}
```

---

## Related Documentation

- [APU_CHANNEL_PULSE.md](APU_CHANNEL_PULSE.md) - Pulse channel deep-dive
- [APU_CHANNEL_TRIANGLE.md](APU_CHANNEL_TRIANGLE.md) - Triangle channel
- [APU_CHANNEL_NOISE.md](APU_CHANNEL_NOISE.md) - Noise channel
- [APU_CHANNEL_DMC.md](APU_CHANNEL_DMC.md) - DMC channel
- [APU_TIMING.md](APU_TIMING.md) - APU cycle timing

---

## References

- [NESdev Wiki: APU](https://www.nesdev.org/wiki/APU)
- [NESdev Wiki: APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [NESdev Wiki: APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
- blargg's apu_test ROM suite
- NesDev APU reference

---

**Document Status:** Complete 2A03 APU specification with all channels and frame sequencer.
