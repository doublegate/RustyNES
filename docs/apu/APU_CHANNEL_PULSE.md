# APU Pulse Channel Specification (2A03)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete technical reference for NES APU pulse (square wave) channels

---

## Table of Contents

- [Overview](#overview)
- [Channel Architecture](#channel-architecture)
- [Register Interface](#register-interface)
- [Duty Cycle Sequencer](#duty-cycle-sequencer)
- [Envelope Generator](#envelope-generator)
- [Sweep Unit](#sweep-unit)
- [Length Counter](#length-counter)
- [Timer and Frequency](#timer-and-frequency)
- [Silencing Conditions](#silencing-conditions)
- [Implementation Guide](#implementation-guide)
- [Common Pitfalls](#common-pitfalls)
- [Testing and Validation](#testing-and-validation)

---

## Overview

The NES APU contains **two independent pulse (square wave) channels** capable of generating variable-duty-cycle waveforms. These channels are the primary melodic voices in NES audio, used for lead melodies, harmonies, and sound effects.

**Key Characteristics:**
- 4 selectable duty cycles (12.5%, 25%, 50%, 75%)
- Hardware envelope generator (volume control)
- Frequency sweep unit (pitch bending)
- Length counter (automatic note duration)
- 11-bit timer (54.6 Hz - 12.4 kHz range)
- 4-bit output (16 volume levels)

**Differences Between Pulse 1 and Pulse 2:**
- Pulse 1: Sweep unit uses ones' complement for negation
- Pulse 2: Sweep unit uses two's complement for negation
- Otherwise identical in functionality

---

## Channel Architecture

Each pulse channel consists of **five interconnected units**:

```
┌─────────────────────────────────────────────────────────────┐
│                     Pulse Channel                           │
│                                                             │
│  ┌───────┐    ┌─────────┐    ┌──────────┐    ┌─────────┐   │
│  │ Timer │───>│Sequencer│───>│ Envelope │───>│  Sweep  │   │
│  │11-bit │    │ 8-step  │    │Generator │    │  Unit   │   │
│  └───────┘    └─────────┘    └──────────┘    └─────────┘   │
│                                                      │       │
│                                ┌──────────────┐     │       │
│                                │Length Counter│─────┘       │
│                                └──────────────┘             │
│                                                             │
│                                     ▼                       │
│                              4-bit Output (0-15)            │
└─────────────────────────────────────────────────────────────┘
```

**Signal Flow:**
1. **Timer** counts down, clocking the sequencer
2. **Sequencer** outputs duty cycle waveform (0 or 1)
3. **Envelope** scales output by volume (0-15)
4. **Sweep** modifies timer period (frequency)
5. **Length Counter** gates the output

**Gating Logic:**
```
Output = Sequencer × Envelope × (Length > 0) × (Timer >= 8) × !SweepMute
```

---

## Register Interface

### Complete Register Map

| Address | Channel | Bits | Description |
|---------|---------|------|-------------|
| **$4000** | Pulse 1 | DDLC VVVV | Duty, Loop, Constant, Volume/Envelope |
| **$4001** | Pulse 1 | EPPP NSSS | Sweep Enable, Period, Negate, Shift |
| **$4002** | Pulse 1 | TTTT TTTT | Timer low 8 bits |
| **$4003** | Pulse 1 | LLLL LTTT | Length load, Timer high 3 bits |
| **$4004** | Pulse 2 | DDLC VVVV | Duty, Loop, Constant, Volume/Envelope |
| **$4005** | Pulse 2 | EPPP NSSS | Sweep Enable, Period, Negate, Shift |
| **$4006** | Pulse 2 | TTTT TTTT | Timer low 8 bits |
| **$4007** | Pulse 2 | LLLL LTTT | Length load, Timer high 3 bits |

### Register $4000/$4004 - Duty, Envelope, Volume

```
DDLC VVVV
|||| ||||
|||| ++++- Volume/Envelope divider period (V)
|||+------ Constant volume flag (C)
||+------- Length counter halt / Envelope loop (L)
++-------- Duty cycle (D)
```

**Bit Definitions:**
- **DD (Duty)**: Selects waveform (0=12.5%, 1=25%, 2=50%, 3=25% inverted)
- **L (Loop)**: If 1, length counter is frozen and envelope loops
- **C (Constant)**: If 1, volume = VVVV; if 0, envelope generates volume
- **VVVV (Volume)**: Constant volume value OR envelope divider period

### Register $4001/$4005 - Sweep Unit

```
EPPP NSSS
|||| ||||
|||| |+++- Shift count (S)
|||| +---- Negate flag (N): 1 = decrease frequency
|||+------ Sweep period (P)
||+------- (unused)
|+-------- Sweep enable (E)
```

**Bit Definitions:**
- **E (Enable)**: 0 = sweep disabled, 1 = sweep active
- **PPP (Period)**: Divider period (0-7), clocked by frame counter
- **N (Negate)**: Direction of sweep (0 = increase, 1 = decrease)
- **SSS (Shift)**: Right shift amount for period adjustment (0-7)

### Register $4002/$4006 - Timer Low

```
TTTT TTTT
|||| ||||
++++-++++- Timer low 8 bits (T)
```

Forms bits 0-7 of the 11-bit timer period.

### Register $4003/$4007 - Length Counter and Timer High

```
LLLL LTTT
|||| ||||
|||| |+++- Timer high 3 bits (T)
++++-+---- Length counter load (L)
```

**Side Effects of Writing:**
- Timer bits 8-10 are set
- Length counter is loaded from lookup table
- **Envelope restarts** (sets divider to reload value)
- **Sequencer phase resets** to step 0 (causes click if playing)

**Important:** Games should avoid writing $4003/$4007 during playback to prevent audible clicks from phase resets. For vibrato, write only $4002/$4006.

---

## Duty Cycle Sequencer

### Waveform Patterns

The 8-step sequencer generates four distinct pulse waveforms:

| Duty | Bit Pattern | Wave | Duty % | Typical Use |
|------|-------------|------|--------|-------------|
| **0** | `0 1 0 0 0 0 0 0` | ▂▅▂▂▂▂▂▂ | 12.5% | Thin, nasal tone |
| **1** | `0 1 1 0 0 0 0 0` | ▂▅▅▂▂▂▂▂ | 25% | Standard square |
| **2** | `0 1 1 1 1 0 0 0` | ▂▅▅▅▅▂▂▂ | 50% | Bright, full tone |
| **3** | `1 0 0 1 1 1 1 1` | ▅▂▂▅▅▅▅▅ | 25% | Inverted (same timbre as duty 1) |

**Sequencer Behavior:**
- Clocked by timer every **8 × (timer + 1)** APU cycles
- Steps through pattern in **reverse order**: 0, 7, 6, 5, 4, 3, 2, 1, 0, ...
- Output is 1 or 0 (before envelope scaling)

### Implementation

```rust
pub struct PulseSequencer {
    duty: u8,           // 0-3
    sequence_step: u8,  // 0-7
}

const DUTY_PATTERNS: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],  // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0],  // 25%
    [0, 1, 1, 1, 1, 0, 0, 0],  // 50%
    [1, 0, 0, 1, 1, 1, 1, 1],  // 25% inverted
];

impl PulseSequencer {
    pub fn clock(&mut self) {
        self.sequence_step = (self.sequence_step + 1) % 8;
    }

    pub fn output(&self) -> u8 {
        DUTY_PATTERNS[self.duty as usize][self.sequence_step as usize]
    }
}
```

---

## Envelope Generator

The envelope provides **hardware-controlled volume fade** for attack-decay-sustain effects.

### Operation Modes

**Mode 1: Constant Volume (C=1)**
```
Output = VVVV (0-15)
```
Volume is fixed at the value in register bits 0-3.

**Mode 2: Envelope Volume (C=0)**
```
Output = Decay Level (15 → 0 over time)
```
Envelope automatically decrements from 15 to 0, providing fade-out.

### Envelope Timing

**Clock Source:** Frame counter (quarter frames)
- 4-step mode: 240 Hz (every ~4167 CPU cycles)
- 5-step mode: 192 Hz (every ~5208 CPU cycles)

**Divider Period:** VVVV controls fade speed
- Period 0: Fastest (240 Hz / 1 = 240 Hz decay rate)
- Period 15: Slowest (240 Hz / 16 = 15 Hz decay rate)

### Envelope State Machine

```
State: decay_level (0-15), divider (0-VVVV)

On quarter frame:
    if divider == 0:
        divider = reload_value (VVVV)
        if decay_level > 0:
            decay_level -= 1
        else if loop_flag:
            decay_level = 15
    else:
        divider -= 1
```

### Implementation

```rust
pub struct Envelope {
    start_flag: bool,
    loop_flag: bool,
    constant_flag: bool,
    reload_value: u8,  // VVVV

    decay_level: u8,   // 0-15
    divider: u8,       // 0-reload_value
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

    pub fn restart(&mut self) {
        self.start_flag = true;
    }
}
```

---

## Sweep Unit

The sweep unit provides **automatic pitch bending** by periodically modifying the timer period.

### Sweep Calculation

**Target Period Formula:**
```
target = current_period >> shift_count

if negate:
    if channel == Pulse1:
        target = current_period + !target    // Ones' complement
    else:  // Pulse 2
        target = current_period - target     // Two's complement
else:
    target = current_period + target
```

**Critical Difference:** Pulse 1 adds the ones' complement (−c − 1), while Pulse 2 adds two's complement (−c). This causes slightly different pitch behavior when sweeping down.

### Sweep Period

The sweep unit is clocked by the **frame counter at half frames** (120 Hz NTSC).

**Divider Period (PPP):**
- 0: 120 Hz (every half frame)
- 1: 60 Hz
- 2: 40 Hz
- ...
- 7: ~17 Hz

### Muting Conditions

The sweep unit **mutes the channel** if:
1. **Current period < 8** (ultrasonic frequency)
2. **Target period > $7FF** (timer overflow)

```rust
fn is_muted(&self) -> bool {
    self.timer_period < 8 || self.calculate_target_period() > 0x7FF
}
```

### Reload Behavior

Writing to $4001/$4005 sets a **reload flag**. On the next sweep clock:
1. Divider is reloaded
2. Reload flag is cleared

This allows games to restart sweep timing.

### Implementation

```rust
pub struct SweepUnit {
    enabled: bool,
    negate: bool,
    shift: u8,
    period: u8,
    reload_flag: bool,

    divider: u8,
    channel: PulseChannel,  // Pulse1 or Pulse2
}

impl SweepUnit {
    pub fn clock(&mut self, timer_period: &mut u16) {
        // Update divider
        if self.divider == 0 && self.enabled && self.shift > 0 {
            let target = self.calculate_target_period(*timer_period);
            if target <= 0x7FF && *timer_period >= 8 {
                *timer_period = target;
            }
        }

        if self.divider == 0 || self.reload_flag {
            self.divider = self.period;
            self.reload_flag = false;
        } else {
            self.divider -= 1;
        }
    }

    fn calculate_target_period(&self, current: u16) -> u16 {
        let delta = current >> self.shift;

        if self.negate {
            match self.channel {
                PulseChannel::Pulse1 => current.wrapping_sub(delta).wrapping_sub(1),
                PulseChannel::Pulse2 => current.wrapping_sub(delta),
            }
        } else {
            current.wrapping_add(delta)
        }
    }

    pub fn is_muted(&self, timer_period: u16) -> bool {
        timer_period < 8 || self.calculate_target_period(timer_period) > 0x7FF
    }
}
```

---

## Length Counter

The length counter provides **automatic note duration** without CPU intervention.

### Lookup Table

Writing to $4003/$4007 loads the length counter from this table (bits 7-3):

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

**Units:** Half frames (120 Hz NTSC), representing ~8.33ms per count

### Halt Flag

The **loop/halt flag** (L bit in $4000/$4004) controls length counter behavior:
- **L=0**: Counter decrements normally, channel stops when reaching 0
- **L=1**: Counter is frozen, channel plays indefinitely

### Disabling via $4015

Writing 0 to the channel's bit in $4015 **immediately sets length counter to 0**, silencing the channel.

### Implementation

```rust
pub struct LengthCounter {
    counter: u8,
    halt: bool,
    enabled: bool,  // From $4015
}

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6,
    160, 1, 20, 2, 40, 4, 80, 6,
    80, 30, 160, 6, 12, 24, 48, 96,
    160, 6, 12, 24, 48, 96, 192, 72,
];

impl LengthCounter {
    pub fn load(&mut self, index: u8) {
        if self.enabled {
            self.counter = LENGTH_TABLE[index as usize];
        }
    }

    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    pub fn is_active(&self) -> bool {
        self.counter > 0
    }
}
```

---

## Timer and Frequency

### Timer Operation

The 11-bit timer counts down at **APU clock rate** (1.789773 MHz for NTSC):

```
Timer = HHHLLLLLLLL (bits 10-0)

Each APU cycle:
    if timer == 0:
        timer = reload_value
        clock_sequencer()
    else:
        timer -= 1
```

**Sequencer Clock Period:** 8 × (timer + 1) APU cycles

### Frequency Calculation

```
f_pulse = f_CPU / (16 × (timer + 1))

NTSC: f_CPU = 1.789773 MHz
PAL:  f_CPU = 1.662607 MHz
```

**Frequency Range (NTSC):**
- Minimum: 54.6 Hz (timer = $7FF)
- Maximum: 12.4 kHz (timer = $008, below this channel mutes)

### Period-to-Note Conversion

For musical applications, calculate timer value for a given frequency:

```rust
fn frequency_to_timer(freq_hz: f32) -> u16 {
    let cpu_clock = 1_789_773.0;
    ((cpu_clock / (16.0 * freq_hz)) - 1.0) as u16
}

// Example: A440 (440 Hz)
let timer = frequency_to_timer(440.0);  // ~253
```

### NTSC vs PAL Differences

| Region | CPU Clock | A440 Timer | Middle C Timer |
|--------|-----------|------------|----------------|
| **NTSC** | 1.789773 MHz | 253 | 478 |
| **PAL** | 1.662607 MHz | 235 | 444 |

Games targeting both regions must adjust timer values or accept slight pitch differences.

---

## Silencing Conditions

The pulse channel output is **forced to zero** when any of these conditions are true:

1. **Length counter = 0**
2. **Sequencer outputs 0** (depends on duty cycle and phase)
3. **Timer period < 8** (below ~12 kHz)
4. **Sweep target period > $7FF** (overflow)
5. **Channel disabled in $4015**

### Implementation

```rust
pub struct PulseChannel {
    timer: Timer,
    sequencer: PulseSequencer,
    envelope: Envelope,
    sweep: SweepUnit,
    length_counter: LengthCounter,
}

impl PulseChannel {
    pub fn output(&self) -> u8 {
        if !self.length_counter.is_active() {
            return 0;
        }

        if self.sweep.is_muted(self.timer.period) {
            return 0;
        }

        if self.sequencer.output() == 0 {
            return 0;
        }

        self.envelope.output()
    }
}
```

---

## Implementation Guide

### Complete Pulse Channel Structure

```rust
pub struct PulseChannel {
    // Component units
    timer: Timer,
    sequencer: PulseSequencer,
    envelope: Envelope,
    sweep: SweepUnit,
    length_counter: LengthCounter,

    // Channel ID (for sweep unit behavior)
    channel_id: PulseChannelId,

    // Enable flag from $4015
    enabled: bool,
}

impl PulseChannel {
    /// Clock the timer (every APU cycle)
    pub fn clock_timer(&mut self) {
        if self.timer.clock() {
            self.sequencer.clock();
        }
    }

    /// Clock envelope and length counter (quarter frame)
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    /// Clock length counter and sweep (half frame)
    pub fn clock_half_frame(&mut self) {
        self.length_counter.clock();
        self.sweep.clock(&mut self.timer.period);
    }

    /// Get current output sample (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        if self.sweep.is_muted(self.timer.period) {
            return 0;
        }

        if self.sequencer.output() == 0 {
            return 0;
        }

        self.envelope.output()
    }

    /// Write to register $4000/$4004
    pub fn write_duty_envelope(&mut self, value: u8) {
        self.sequencer.duty = (value >> 6) & 0x03;
        self.envelope.loop_flag = (value & 0x20) != 0;
        self.envelope.constant_flag = (value & 0x10) != 0;
        self.envelope.reload_value = value & 0x0F;
        self.length_counter.halt = (value & 0x20) != 0;
    }

    /// Write to register $4001/$4005
    pub fn write_sweep(&mut self, value: u8) {
        self.sweep.enabled = (value & 0x80) != 0;
        self.sweep.period = (value >> 4) & 0x07;
        self.sweep.negate = (value & 0x08) != 0;
        self.sweep.shift = value & 0x07;
        self.sweep.reload_flag = true;
    }

    /// Write to register $4002/$4006
    pub fn write_timer_low(&mut self, value: u8) {
        self.timer.period = (self.timer.period & 0xFF00) | (value as u16);
    }

    /// Write to register $4003/$4007
    pub fn write_length_timer_high(&mut self, value: u8) {
        self.timer.period = (self.timer.period & 0x00FF) | (((value & 0x07) as u16) << 8);

        let length_index = value >> 3;
        self.length_counter.load(length_index);

        // Restart envelope and reset sequencer phase
        self.envelope.restart();
        self.sequencer.sequence_step = 0;
    }
}
```

---

## Common Pitfalls

### 1. Phase Reset Click

**Problem:** Writing $4003/$4007 resets sequencer phase, causing audible clicks during playback.

**Solution:** For vibrato effects, only write $4002/$4006 (timer low). Never write timer high during sound playback.

```rust
// WRONG: Causes clicks
fn apply_vibrato_bad(&mut self, freq: u16) {
    self.write_timer_low((freq & 0xFF) as u8);
    self.write_length_timer_high((freq >> 8) as u8);  // BAD: Resets phase
}

// CORRECT: Smooth vibrato
fn apply_vibrato_good(&mut self, freq: u16) {
    self.write_timer_low((freq & 0xFF) as u8);
    // Only write high byte if it actually changed
    let new_high = (freq >> 8) as u8;
    if new_high != ((self.timer.period >> 8) as u8) {
        // Accept minor click for large pitch jumps
        self.timer.period = (self.timer.period & 0x00FF) | ((new_high as u16) << 8);
    }
}
```

### 2. Sweep Unit Differences

**Problem:** Forgetting that Pulse 1 and Pulse 2 use different sweep negation.

**Solution:** Always check channel ID in sweep calculation.

```rust
// Pulse 1: ones' complement
target = current - delta - 1

// Pulse 2: two's complement
target = current - delta
```

### 3. Timer < 8 Silencing

**Problem:** Not muting the channel when timer < 8, causing ultrasonic artifacts.

**Solution:** Always check this condition before outputting audio.

```rust
fn output(&self) -> u8 {
    if self.timer.period < 8 {
        return 0;  // Mute
    }
    // ... rest of output logic
}
```

### 4. Length Counter Load Timing

**Problem:** Loading length counter when channel is disabled in $4015 has no effect.

**Solution:** Track enable state and ignore length loads when disabled.

```rust
pub fn load(&mut self, index: u8) {
    if self.enabled {  // Only load if enabled in $4015
        self.counter = LENGTH_TABLE[index as usize];
    }
}
```

---

## Testing and Validation

### Test ROMs

| ROM | Tests | Pass Criteria |
|-----|-------|---------------|
| **apu_test** | Basic pulse functionality | All tests pass |
| **blargg_apu_2005.nes** | Comprehensive APU behavior | Text output "Passed" |
| **apu_mixer.nes** | Output levels and mixing | Correct waveform rendering |
| **square_timer_div2.nes** | Timer edge cases | Frequency accuracy |

### Manual Testing

**Duty Cycle Verification:**
```
For each duty (0-3):
    Play middle C (261.63 Hz, timer ~426)
    Verify waveform shape matches expected pattern
```

**Envelope Test:**
```
Set: Constant=0, Loop=0, Volume=15
Trigger note with $4003
Verify: Volume decays from 15 to 0 over ~1 second
```

**Sweep Test:**
```
// Upward sweep
Set: Enable=1, Negate=0, Period=0, Shift=1
Start: Timer = $100
Verify: Frequency increases exponentially

// Downward sweep
Set: Enable=1, Negate=1, Period=0, Shift=1
Start: Timer = $200
Verify: Frequency decreases exponentially
```

---

## Related Documentation

- [APU_OVERVIEW.md](APU_OVERVIEW.md) - General APU architecture
- [APU_TIMING.md](APU_TIMING.md) - Frame counter and timing details
- [APU_2A03_SPECIFICATION.md](APU_2A03_SPECIFICATION.md) - Complete APU reference
- [APU_CHANNEL_TRIANGLE.md](APU_CHANNEL_TRIANGLE.md) - Triangle channel specification
- [APU_MIXER.md](APU_MIXER.md) - Audio mixing and output

---

## References

- [NESdev Wiki: APU Pulse](https://www.nesdev.org/wiki/APU_Pulse)
- [NESdev Wiki: APU Sweep](https://www.nesdev.org/wiki/APU_Sweep)
- [NESdev Wiki: APU Envelope](https://www.nesdev.org/wiki/APU_Envelope)
- [NESdev Wiki: APU Length Counter](https://www.nesdev.org/wiki/APU_Length_Counter)
- Blargg APU Test ROMs - Comprehensive validation suite
- Visual 2A03 - Hardware simulation

---

**Document Status:** Complete specification for pulse channel implementation with cycle-accurate behavior.
