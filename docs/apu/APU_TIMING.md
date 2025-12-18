# APU Timing and Sample Generation

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Clock Specifications](#clock-specifications)
- [Frame Counter Timing](#frame-counter-timing)
- [Channel Timing](#channel-timing)
- [Sample Generation](#sample-generation)
- [IRQ Timing](#irq-timing)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The APU operates on the **CPU clock** (1.789773 MHz for NTSC) and generates audio samples at a configurable output rate (typically 48 kHz for emulators). Understanding APU timing is critical for:

- **Frame Counter Accuracy** - Envelope, sweep, and length counter updates
- **Sample Generation** - Converting hardware output to digital audio
- **IRQ Timing** - Frame counter and DMC interrupts
- **DMA Timing** - DMC sample reads and CPU stalls

**Key Timing Points:**
- Frame counter divides time into quarter-frames and half-frames
- Channels run at different clock rates (pulse/noise/DMC vs. triangle)
- Sample generation requires resampling from ~1.79 MHz to 48 kHz

---

## Clock Specifications

### Master Clock Hierarchy

```
NTSC (2A03):
  Master Clock:   21.477272 MHz
  CPU/APU Clock:  1.789773 MHz (÷12)
  Sample Output:  48000 Hz (typical for emulators)

PAL (2A07):
  Master Clock:   26.601712 MHz
  CPU/APU Clock:  1.662607 MHz (÷16)
  Sample Output:  48000 Hz (typical)
```

### Clock Ratios

```
NTSC:
  CPU cycles per sample @ 48 kHz:
    1.789773 MHz / 48000 Hz = ~37.28 cycles/sample

  Samples per frame (60 Hz):
    48000 Hz / 60.0988 Hz = ~798.75 samples/frame
```

---

## Frame Counter Timing

The **frame counter** operates on CPU cycles and provides timing signals for envelope generators, sweep units, and length counters.

### Frame Counter Modes

#### 4-Step Mode (Mode 0)

```
Step   CPU Cycle   Action
----------------------------------------
0      7457        Quarter frame (envelopes, linear counter)
1      14913       Half frame (envelopes, linear, length, sweep)
2      22371       Quarter frame (envelopes, linear counter)
3      29829       Half frame (envelopes, linear, length, sweep)
                   + Set frame IRQ flag
       29830       Set frame IRQ flag
       29831       Set frame IRQ flag

Frame time: 29,829 CPU cycles (~16.67 ms)
```

**IRQ Behavior:**
- Frame IRQ flag set on cycle 29829
- Flag persists for 3 CPU cycles (29829-29831)
- IRQ triggered if not inhibited

**Implementation:**
```rust
pub fn clock_frame_counter_mode0(&mut self) {
    self.frame_cycle += 1;

    match self.frame_cycle {
        7457 => self.clock_quarter_frame(),
        14913 => {
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
        22371 => self.clock_quarter_frame(),
        29829 => {
            self.clock_quarter_frame();
            self.clock_half_frame();

            if !self.irq_inhibit {
                self.frame_irq = true;
            }
        }
        29830 | 29831 => {
            if !self.irq_inhibit {
                self.frame_irq = true;
            }
        }
        29832 => {
            self.frame_cycle = 0;
        }
        _ => {}
    }
}
```

#### 5-Step Mode (Mode 1)

```
Step   CPU Cycle   Action
----------------------------------------
0      7457        Quarter frame (envelopes, linear counter)
1      14913       Half frame (envelopes, linear, length, sweep)
2      22371       Quarter frame (envelopes, linear counter)
3      29829       (nothing)
4      37281       Half frame (envelopes, linear, length, sweep)

Frame time: 37,281 CPU cycles (~20.83 ms)
No IRQ generation
```

**Important:** 5-step mode does NOT generate IRQ.

**Implementation:**
```rust
pub fn clock_frame_counter_mode1(&mut self) {
    self.frame_cycle += 1;

    match self.frame_cycle {
        7457 => self.clock_quarter_frame(),
        14913 => {
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
        22371 => self.clock_quarter_frame(),
        37281 => {
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
        37282 => {
            self.frame_cycle = 0;
        }
        _ => {}
    }
}
```

### Frame Counter Register ($4017)

```
MI-- ----
||
|+-------- IRQ inhibit (0: enable IRQ, 1: disable IRQ)
+--------- Mode (0: 4-step, 1: 5-step)
```

**Write Side Effects:**
- If written with mode 1 or IRQ inhibit 1: clear frame IRQ flag
- If written with mode 1: immediately clock quarter frame and half frame
- Reset frame counter divider (occurs 3-4 CPU cycles after write)

**Implementation:**
```rust
pub fn write_frame_counter(&mut self, value: u8) {
    let mode = (value & 0x80) != 0;
    let irq_inhibit = (value & 0x40) != 0;

    self.mode = if mode { 1 } else { 0 };
    self.irq_inhibit = irq_inhibit;

    if irq_inhibit {
        self.frame_irq = false;
    }

    if mode {
        // Mode 1: immediately clock
        self.clock_quarter_frame();
        self.clock_half_frame();
    }

    // Reset will occur 3-4 cycles later (simplified to immediate)
    self.frame_cycle = 0;
}
```

---

## Channel Timing

### Pulse and Noise Channels

Pulse and noise channels run at **CPU clock rate**:

```
Timer frequency = CPU_CLOCK / (16 × (period + 1))

Example (Pulse, period = 500):
  frequency = 1.789773 MHz / (16 × 501)
            = 1.789773 MHz / 8016
            = ~223 Hz
```

**Timer Clock:**
```rust
impl PulseChannel {
    pub fn clock_timer(&mut self) {
        if self.timer.counter == 0 {
            self.timer.counter = self.timer.period;
            self.clock_sequencer();  // Advance duty cycle
        } else {
            self.timer.counter -= 1;
        }
    }
}
```

### Triangle Channel

Triangle channel runs at **half CPU clock rate** (clocked every other CPU cycle):

```
Timer frequency = CPU_CLOCK / (32 × (period + 1))

Example (Triangle, period = 1000):
  frequency = 1.789773 MHz / (32 × 1001)
            = 1.789773 MHz / 32032
            = ~56 Hz
```

**Timer Clock:**
```rust
impl Apu {
    pub fn step(&mut self, cpu_cycles: u8) {
        for _ in 0..cpu_cycles {
            self.cycles += 1;

            // Triangle clocks every other cycle
            if (self.cycles & 1) == 0 {
                self.triangle.clock_timer();
            }

            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
            self.dmc.clock_timer();
        }
    }
}
```

### DMC Channel

DMC has 16 selectable sample rates:

```rust
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106,  84,  72,  54,
];

Timer frequency = CPU_CLOCK / rate_table[index]

Examples:
  Index 0: 1.789773 MHz / 428 = 4.18 kHz
  Index 15: 1.789773 MHz / 54 = 33.14 kHz
```

---

## Sample Generation

### Output Sample Rate

Emulators typically output at **48 kHz**:

```
Sample period = CPU_CLOCK / output_rate
              = 1.789773 MHz / 48000 Hz
              = ~37.28 CPU cycles per sample
```

### Sample Accumulation

Track fractional cycles to determine when to generate samples:

```rust
pub struct Apu {
    cycles: u64,
    sample_counter: f32,
    output_rate: u32,        // 48000 Hz
}

impl Apu {
    pub fn step(&mut self, cpu_cycles: u8) -> Vec<f32> {
        let mut samples = Vec::new();
        let cycles_per_sample = CPU_CLOCK / self.output_rate as f32;

        for _ in 0..cpu_cycles {
            // Clock APU components
            self.clock_channels();

            // Track sample generation
            self.sample_counter += 1.0;

            if self.sample_counter >= cycles_per_sample {
                self.sample_counter -= cycles_per_sample;

                let sample = self.mix_channels();
                samples.push(sample);
            }

            self.cycles += 1;
        }

        samples
    }
}
```

### Resampling Strategies

#### Strategy 1: Nearest Neighbor

Simplest approach - output current channel state when sample is due:

```rust
if self.sample_counter >= cycles_per_sample {
    let sample = self.mix_channels();
    samples.push(sample);
}
```

**Pros:** Fast, simple
**Cons:** Aliasing artifacts at high frequencies

#### Strategy 2: Linear Interpolation

Interpolate between current and previous sample:

```rust
let t = self.sample_counter / cycles_per_sample;
let sample = self.prev_sample * (1.0 - t) + self.current_sample * t;
```

**Pros:** Better high-frequency response
**Cons:** Slightly more complex

#### Strategy 3: Sinc Interpolation

High-quality resampling using windowed sinc function:

```rust
// Requires buffering several samples
let sample = self.resample_sinc(self.sample_buffer, fraction);
```

**Pros:** Best quality, minimal aliasing
**Cons:** Highest CPU cost, requires buffering

**Recommendation:** Start with nearest neighbor, upgrade to linear interpolation for better quality.

---

## IRQ Timing

### Frame Counter IRQ

Generated in 4-step mode when frame counter reaches cycle 29829:

```
Cycle 29829: Set frame IRQ flag
Cycle 29830: IRQ flag still set
Cycle 29831: IRQ flag still set
Cycle 29832: Reset frame counter, IRQ flag cleared (if not re-triggered)
```

**IRQ Polling:**
```rust
pub fn irq_pending(&self) -> bool {
    (!self.irq_inhibit && self.frame_irq) || self.dmc_irq
}
```

**Clearing IRQ:**
- Reading $4015 clears frame IRQ flag
- Writing to $4017 with IRQ inhibit = 1 clears frame IRQ flag

### DMC IRQ

Generated when DMC sample playback completes (if IRQ enabled):

```
Sample playback:
  1. bytes_remaining decrements to 0
  2. If loop: restart sample
  3. Else if IRQ enabled: set DMC IRQ flag
```

**DMC IRQ Handling:**
```rust
impl DmcChannel {
    pub fn on_sample_complete(&mut self) {
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart_sample();
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        }
    }
}
```

**Clearing DMC IRQ:**
- Writing to $4015 clears DMC IRQ flag

---

## Implementation Guide

### Core Timing Loop

```rust
pub struct Apu {
    // Channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    // Frame counter
    frame_mode: u8,
    frame_cycle: u32,
    irq_inhibit: bool,

    // IRQ flags
    frame_irq: bool,
    dmc_irq: bool,

    // Cycle tracking
    cycles: u64,

    // Sample generation
    sample_counter: f32,
    output_rate: u32,
}

impl Apu {
    pub fn step(&mut self, cpu_cycles: u8) -> Vec<f32> {
        let mut samples = Vec::new();
        let cycles_per_sample = CPU_CLOCK / self.output_rate as f32;

        for _ in 0..cpu_cycles {
            // Clock frame counter
            self.clock_frame_counter();

            // Clock channel timers
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
            self.dmc.clock_timer();

            // Triangle at half rate
            if (self.cycles & 1) == 0 {
                self.triangle.clock_timer();
            }

            // Generate sample if needed
            self.sample_counter += 1.0;
            if self.sample_counter >= cycles_per_sample {
                self.sample_counter -= cycles_per_sample;
                samples.push(self.mix_channels());
            }

            self.cycles += 1;
        }

        samples
    }
}
```

### Frame Counter Update

```rust
fn clock_frame_counter(&mut self) {
    if self.frame_mode == 0 {
        // 4-step mode
        match self.frame_cycle {
            7457 => self.clock_quarter_frame(),
            14913 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            22371 => self.clock_quarter_frame(),
            29829 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
                if !self.irq_inhibit {
                    self.frame_irq = true;
                }
            }
            29830 | 29831 => {
                if !self.irq_inhibit {
                    self.frame_irq = true;
                }
            }
            29832 => {
                self.frame_cycle = 0;
                return;
            }
            _ => {}
        }
    } else {
        // 5-step mode
        match self.frame_cycle {
            7457 => self.clock_quarter_frame(),
            14913 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            22371 => self.clock_quarter_frame(),
            37281 => {
                self.clock_quarter_frame();
                self.clock_half_frame();
            }
            37282 => {
                self.frame_cycle = 0;
                return;
            }
            _ => {}
        }
    }

    self.frame_cycle += 1;
}

fn clock_quarter_frame(&mut self) {
    self.pulse1.clock_envelope();
    self.pulse2.clock_envelope();
    self.triangle.clock_linear_counter();
    self.noise.clock_envelope();
}

fn clock_half_frame(&mut self) {
    self.pulse1.clock_length_counter();
    self.pulse1.clock_sweep();
    self.pulse2.clock_length_counter();
    self.pulse2.clock_sweep();
    self.triangle.clock_length_counter();
    self.noise.clock_length_counter();
}
```

### Mixing Channels

```rust
fn mix_channels(&self) -> f32 {
    // Get channel outputs
    let pulse1 = self.pulse1.output();
    let pulse2 = self.pulse2.output();
    let triangle = self.triangle.output();
    let noise = self.noise.output();
    let dmc = self.dmc.output();

    // Non-linear mixing
    let pulse_sum = pulse1 + pulse2;
    let pulse_out = if pulse_sum == 0 {
        0.0
    } else {
        95.88 / ((8128.0 / pulse_sum as f32) + 100.0)
    };

    let tnd = (triangle as f32 / 8227.0)
            + (noise as f32 / 12241.0)
            + (dmc as f32 / 22638.0);

    let tnd_out = if tnd == 0.0 {
        0.0
    } else {
        159.79 / ((1.0 / tnd) + 100.0)
    };

    pulse_out + tnd_out
}
```

---

## Test ROM Validation

### APU Timing Test ROMs

1. **apu_test**
   - Tests basic APU functionality
   - Validates register writes

2. **apu_reset**
   - Tests APU state after reset
   - Validates frame counter reset

3. **blargg_apu_2005**
   - Comprehensive APU tests
   - Frame counter timing validation

4. **dmc_dma_during_read4**
   - Tests DMC DMA timing
   - Validates CPU stall behavior

5. **frame_irq**
   - Tests frame counter IRQ timing
   - Validates IRQ inhibit

### Validation Checklist

- [ ] Frame counter 4-step mode timing matches hardware
- [ ] Frame counter 5-step mode timing matches hardware
- [ ] Frame IRQ flag set at cycle 29829 in 4-step mode
- [ ] Frame IRQ persists for 3 cycles
- [ ] 5-step mode does not generate IRQ
- [ ] Quarter frame events clock envelopes and linear counter
- [ ] Half frame events clock length counters and sweep units
- [ ] Triangle channel runs at half CPU clock rate
- [ ] Pulse/noise channels run at full CPU clock rate
- [ ] DMC rate table produces correct frequencies
- [ ] Sample generation produces correct output rate (48 kHz)

---

## References

- [NesDev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
- [APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
- [APU Length Counter](https://www.nesdev.org/wiki/APU_Length_Counter)
- [APU Envelope](https://www.nesdev.org/wiki/APU_Envelope)

---

**Back to:** [APU Overview](APU_OVERVIEW.md) | [APU Channels](APU_CHANNELS.md)
