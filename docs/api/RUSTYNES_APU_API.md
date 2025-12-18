# RustyNES APU Crate API Reference

**Crate:** `rustynes-apu`
**Version:** 0.1.0
**License:** MIT/Apache-2.0

The `rustynes-apu` crate provides a cycle-accurate implementation of the NES 2A03 Audio Processing Unit, including all five standard channels (2 pulse, triangle, noise, DMC) and support for expansion audio chips.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Types](#core-types)
- [APU Struct](#apu-struct)
- [Register Interface](#register-interface)
- [Audio Channels](#audio-channels)
- [Frame Counter](#frame-counter)
- [Sample Generation](#sample-generation)
- [Expansion Audio](#expansion-audio)
- [Debug Interface](#debug-interface)
- [Examples](#examples)

---

## Quick Start

```rust
use rustynes_apu::{Apu, ApuBus, SampleFormat};

// Implement APU bus for DMC sample fetches
struct NesApuBus {
    cpu_memory: [u8; 65536],
}

impl ApuBus for NesApuBus {
    fn read_sample(&mut self, addr: u16) -> u8 {
        self.cpu_memory[addr as usize]
    }
}

fn main() {
    let bus = NesApuBus {
        cpu_memory: [0; 65536],
    };

    let mut apu = Apu::new(bus, 44100.0); // 44.1 kHz output

    // Run APU for some cycles
    for _ in 0..1000 {
        apu.tick();
    }

    // Get audio samples
    let samples: Vec<f32> = apu.drain_samples();
}
```

---

## Core Types

### Sample Types

```rust
/// Audio sample in floating-point format
pub type Sample = f32;

/// Stereo sample pair
#[derive(Debug, Clone, Copy)]
pub struct StereoSample {
    pub left: Sample,
    pub right: Sample,
}

/// Sample rate in Hz
pub type SampleRate = f64;

/// APU cycle count
pub type ApuCycles = u64;
```

### Channel Enable Flags

```rust
use bitflags::bitflags;

bitflags! {
    /// Channel enable flags from $4015
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ChannelStatus: u8 {
        const PULSE1   = 0b0000_0001;
        const PULSE2   = 0b0000_0010;
        const TRIANGLE = 0b0000_0100;
        const NOISE    = 0b0000_1000;
        const DMC      = 0b0001_0000;
    }
}
```

### Frame Counter Mode

```rust
/// Frame counter sequencer mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCounterMode {
    /// 4-step sequence: 120 Hz envelope/triangle, 240 Hz sweep
    FourStep,
    /// 5-step sequence: 96 Hz envelope/triangle, 192 Hz sweep
    FiveStep,
}
```

---

## APU Struct

### Definition

```rust
/// NES 2A03 Audio Processing Unit
pub struct Apu<B: ApuBus> {
    /// DMC sample bus
    bus: B,

    /// Pulse channel 1
    pulse1: PulseChannel,

    /// Pulse channel 2
    pulse2: PulseChannel,

    /// Triangle channel
    triangle: TriangleChannel,

    /// Noise channel
    noise: NoiseChannel,

    /// DMC (delta modulation) channel
    dmc: DmcChannel,

    /// Frame counter
    frame_counter: FrameCounter,

    /// Channel enable status
    status: ChannelStatus,

    /// Output sample buffer
    sample_buffer: Vec<Sample>,

    /// Target sample rate
    sample_rate: SampleRate,

    /// CPU cycles since last sample
    cycles_since_sample: f64,

    /// Cycles per sample (for resampling)
    cycles_per_sample: f64,

    /// Total APU cycles
    total_cycles: ApuCycles,

    /// IRQ pending from frame counter or DMC
    irq_pending: bool,
}
```

### Constructor

```rust
impl<B: ApuBus> Apu<B> {
    /// Create new APU with specified sample rate
    ///
    /// Common sample rates: 44100.0, 48000.0, 22050.0
    pub fn new(bus: B, sample_rate: SampleRate) -> Self {
        const CPU_CLOCK: f64 = 1_789_773.0; // NTSC

        Self {
            bus,
            pulse1: PulseChannel::new(true),  // Sweep negate difference
            pulse2: PulseChannel::new(false),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
            frame_counter: FrameCounter::new(),
            status: ChannelStatus::empty(),
            sample_buffer: Vec::with_capacity(2048),
            sample_rate,
            cycles_since_sample: 0.0,
            cycles_per_sample: CPU_CLOCK / sample_rate,
            total_cycles: 0,
            irq_pending: false,
        }
    }

    /// Reset APU to power-on state
    pub fn reset(&mut self) {
        self.pulse1.reset();
        self.pulse2.reset();
        self.triangle.reset();
        self.noise.reset();
        self.dmc.reset();
        self.frame_counter.reset();
        self.status = ChannelStatus::empty();
        self.irq_pending = false;
    }
}
```

### Tick Method

```rust
impl<B: ApuBus> Apu<B> {
    /// Execute one APU cycle (called at CPU rate)
    pub fn tick(&mut self) {
        self.total_cycles += 1;

        // Triangle ticks every CPU cycle
        self.triangle.tick();

        // Even cycles: tick pulse and noise
        if self.total_cycles % 2 == 0 {
            self.pulse1.tick();
            self.pulse2.tick();
            self.noise.tick();
        }

        // DMC ticks every CPU cycle
        if self.dmc.tick(&mut self.bus) {
            self.irq_pending = true;
        }

        // Frame counter
        if let Some(frame_event) = self.frame_counter.tick() {
            self.clock_frame_event(frame_event);
        }

        // Generate output sample
        self.cycles_since_sample += 1.0;
        if self.cycles_since_sample >= self.cycles_per_sample {
            self.cycles_since_sample -= self.cycles_per_sample;
            let sample = self.mix_output();
            self.sample_buffer.push(sample);
        }
    }

    /// Run APU for specified number of cycles
    pub fn run(&mut self, cycles: u64) {
        for _ in 0..cycles {
            self.tick();
        }
    }
}
```

---

## Register Interface

### CPU Register Access

```rust
impl<B: ApuBus> Apu<B> {
    /// Read APU register (called by CPU)
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => self.read_status(),
            _ => 0, // Other registers are write-only
        }
    }

    /// Write APU register (called by CPU)
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            // Pulse 1
            0x4000 => self.pulse1.write_control(value),
            0x4001 => self.pulse1.write_sweep(value),
            0x4002 => self.pulse1.write_timer_lo(value),
            0x4003 => self.pulse1.write_timer_hi(value),

            // Pulse 2
            0x4004 => self.pulse2.write_control(value),
            0x4005 => self.pulse2.write_sweep(value),
            0x4006 => self.pulse2.write_timer_lo(value),
            0x4007 => self.pulse2.write_timer_hi(value),

            // Triangle
            0x4008 => self.triangle.write_control(value),
            0x400A => self.triangle.write_timer_lo(value),
            0x400B => self.triangle.write_timer_hi(value),

            // Noise
            0x400C => self.noise.write_control(value),
            0x400E => self.noise.write_mode_period(value),
            0x400F => self.noise.write_length(value),

            // DMC
            0x4010 => self.dmc.write_control(value),
            0x4011 => self.dmc.write_direct_load(value),
            0x4012 => self.dmc.write_sample_address(value),
            0x4013 => self.dmc.write_sample_length(value),

            // Status
            0x4015 => self.write_status(value),

            // Frame counter
            0x4017 => self.write_frame_counter(value),

            _ => {}
        }
    }
}
```

### Status Register

```rust
impl<B: ApuBus> Apu<B> {
    /// Read $4015 (status)
    fn read_status(&mut self) -> u8 {
        let mut status = 0u8;

        if self.pulse1.length_counter() > 0 { status |= 0x01; }
        if self.pulse2.length_counter() > 0 { status |= 0x02; }
        if self.triangle.length_counter() > 0 { status |= 0x04; }
        if self.noise.length_counter() > 0 { status |= 0x08; }
        if self.dmc.bytes_remaining() > 0 { status |= 0x10; }

        if self.frame_counter.irq_flag() { status |= 0x40; }
        if self.dmc.irq_flag() { status |= 0x80; }

        // Reading clears frame IRQ flag
        self.frame_counter.clear_irq_flag();

        status
    }

    /// Write $4015 (status/enable)
    fn write_status(&mut self, value: u8) {
        self.status = ChannelStatus::from_bits_truncate(value);

        // Enable/disable channels
        self.pulse1.set_enabled(self.status.contains(ChannelStatus::PULSE1));
        self.pulse2.set_enabled(self.status.contains(ChannelStatus::PULSE2));
        self.triangle.set_enabled(self.status.contains(ChannelStatus::TRIANGLE));
        self.noise.set_enabled(self.status.contains(ChannelStatus::NOISE));
        self.dmc.set_enabled(self.status.contains(ChannelStatus::DMC), &mut self.bus);

        // Clear DMC interrupt flag
        self.dmc.clear_irq_flag();
    }

    /// Write $4017 (frame counter)
    fn write_frame_counter(&mut self, value: u8) {
        self.frame_counter.write(value);

        if value & 0x40 != 0 {
            // IRQ inhibit flag set
            self.frame_counter.clear_irq_flag();
        }

        if value & 0x80 != 0 {
            // 5-step mode: clock all units immediately
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
    }
}
```

---

## Audio Channels

### Pulse Channel

```rust
/// Pulse wave channel (2 in APU)
pub struct PulseChannel {
    /// Timer period (11 bits)
    timer_period: u16,
    /// Current timer value
    timer: u16,
    /// Duty cycle (0-3)
    duty: u8,
    /// Duty cycle position (0-7)
    duty_pos: u8,
    /// Length counter
    length_counter: u8,
    /// Envelope generator
    envelope: Envelope,
    /// Sweep unit
    sweep: Sweep,
    /// Channel enabled
    enabled: bool,
    /// Is this pulse 1 (affects sweep negate)
    is_pulse1: bool,
}

impl PulseChannel {
    /// Get current output level (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled { return 0; }
        if self.length_counter == 0 { return 0; }
        if self.timer_period < 8 { return 0; } // High frequency mute
        if self.sweep.muting() { return 0; }

        let duty_output = DUTY_TABLE[self.duty as usize][self.duty_pos as usize];
        if duty_output {
            self.envelope.output()
        } else {
            0
        }
    }
}

/// Duty cycle waveforms
const DUTY_TABLE: [[bool; 8]; 4] = [
    [false, true,  false, false, false, false, false, false], // 12.5%
    [false, true,  true,  false, false, false, false, false], // 25%
    [false, true,  true,  true,  true,  false, false, false], // 50%
    [true,  false, false, true,  true,  true,  true,  true],  // 25% negated
];
```

### Triangle Channel

```rust
/// Triangle wave channel
pub struct TriangleChannel {
    /// Timer period (11 bits)
    timer_period: u16,
    /// Current timer value
    timer: u16,
    /// Sequence position (0-31)
    sequence_pos: u8,
    /// Length counter
    length_counter: u8,
    /// Linear counter
    linear_counter: u8,
    /// Linear counter reload value
    linear_counter_reload: u8,
    /// Linear counter reload flag
    counter_reload: bool,
    /// Control flag (length counter halt / linear counter control)
    control: bool,
    /// Channel enabled
    enabled: bool,
}

impl TriangleChannel {
    /// Get current output level (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled { return 0; }
        if self.length_counter == 0 { return 0; }
        if self.linear_counter == 0 { return 0; }

        TRIANGLE_TABLE[self.sequence_pos as usize]
    }
}

/// Triangle waveform (32 steps)
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];
```

### Noise Channel

```rust
/// Noise channel
pub struct NoiseChannel {
    /// Timer period
    timer_period: u16,
    /// Current timer value
    timer: u16,
    /// Linear feedback shift register (15-bit)
    lfsr: u16,
    /// Mode flag (short/long)
    mode: bool,
    /// Length counter
    length_counter: u8,
    /// Envelope generator
    envelope: Envelope,
    /// Channel enabled
    enabled: bool,
}

impl NoiseChannel {
    /// Get current output level (0-15)
    pub fn output(&self) -> u8 {
        if !self.enabled { return 0; }
        if self.length_counter == 0 { return 0; }
        if self.lfsr & 1 != 0 { return 0; } // Bit 0 gates output

        self.envelope.output()
    }

    /// Clock LFSR
    fn clock_lfsr(&mut self) {
        let feedback_bit = if self.mode { 6 } else { 1 };
        let feedback = (self.lfsr & 1) ^ ((self.lfsr >> feedback_bit) & 1);
        self.lfsr = (self.lfsr >> 1) | (feedback << 14);
    }
}

/// Noise period lookup table (NTSC)
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
```

### DMC Channel

```rust
/// Delta Modulation Channel
pub struct DmcChannel {
    /// Sample buffer (8 bits of data)
    sample_buffer: u8,
    /// Sample buffer empty flag
    buffer_empty: bool,
    /// Output level (0-127)
    output_level: u8,
    /// Bits remaining in current sample
    bits_remaining: u8,
    /// Current sample address
    current_addr: u16,
    /// Bytes remaining
    bytes_remaining: u16,
    /// Sample start address
    sample_addr: u16,
    /// Sample length
    sample_length: u16,
    /// Timer period
    timer_period: u16,
    /// Current timer value
    timer: u16,
    /// Loop flag
    loop_flag: bool,
    /// IRQ enable flag
    irq_enable: bool,
    /// IRQ pending flag
    irq_flag: bool,
    /// Channel enabled
    enabled: bool,
}

impl DmcChannel {
    /// Clock DMC (returns true if IRQ triggered)
    pub fn tick(&mut self, bus: &mut impl ApuBus) -> bool {
        if self.timer > 0 {
            self.timer -= 1;
            return false;
        }

        self.timer = self.timer_period;

        // Output unit
        if self.bits_remaining > 0 {
            if self.sample_buffer & 1 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }
            self.sample_buffer >>= 1;
            self.bits_remaining -= 1;
        }

        // Memory reader
        if self.bits_remaining == 0 && !self.buffer_empty {
            self.sample_buffer = self.read_sample_buffer;
            self.buffer_empty = true;
            self.bits_remaining = 8;
        }

        if self.buffer_empty && self.bytes_remaining > 0 {
            // Fetch next sample (stalls CPU 4 cycles)
            self.read_sample_buffer = bus.read_sample(self.current_addr);
            self.buffer_empty = false;

            self.current_addr = self.current_addr.wrapping_add(1) | 0x8000;
            self.bytes_remaining -= 1;

            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    self.restart();
                } else if self.irq_enable {
                    self.irq_flag = true;
                    return true;
                }
            }
        }

        false
    }

    /// Get current output level (0-127)
    pub fn output(&self) -> u8 {
        self.output_level
    }
}

/// DMC rate table (NTSC)
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];
```

---

## Frame Counter

### Frame Sequencer

```rust
/// APU frame counter/sequencer
pub struct FrameCounter {
    /// Current mode
    mode: FrameCounterMode,
    /// Current step (0-4)
    step: u8,
    /// Cycles until next step
    cycles: u16,
    /// IRQ inhibit flag
    irq_inhibit: bool,
    /// IRQ flag
    irq_flag: bool,
    /// Reset delay counter
    reset_delay: u8,
}

/// Frame counter event
#[derive(Debug, Clone, Copy)]
pub enum FrameEvent {
    /// Clock envelope and linear counter
    QuarterFrame,
    /// Clock length counter and sweep
    HalfFrame,
    /// Frame IRQ (4-step mode only)
    Irq,
}

impl FrameCounter {
    /// Clock frame counter, return event if any
    pub fn tick(&mut self) -> Option<FrameEvent> {
        if self.reset_delay > 0 {
            self.reset_delay -= 1;
            if self.reset_delay == 0 {
                self.cycles = 0;
                self.step = 0;
            }
        }

        self.cycles += 1;

        match self.mode {
            FrameCounterMode::FourStep => self.tick_4step(),
            FrameCounterMode::FiveStep => self.tick_5step(),
        }
    }

    fn tick_4step(&mut self) -> Option<FrameEvent> {
        match (self.step, self.cycles) {
            (0, 3729) => {
                self.step = 1;
                Some(FrameEvent::QuarterFrame)
            }
            (1, 7457) => {
                self.step = 2;
                Some(FrameEvent::HalfFrame)
            }
            (2, 11186) => {
                self.step = 3;
                Some(FrameEvent::QuarterFrame)
            }
            (3, 14915) => {
                self.step = 0;
                self.cycles = 0;
                if !self.irq_inhibit {
                    self.irq_flag = true;
                    return Some(FrameEvent::Irq);
                }
                Some(FrameEvent::HalfFrame)
            }
            _ => None,
        }
    }

    fn tick_5step(&mut self) -> Option<FrameEvent> {
        match (self.step, self.cycles) {
            (0, 3729) => {
                self.step = 1;
                Some(FrameEvent::QuarterFrame)
            }
            (1, 7457) => {
                self.step = 2;
                Some(FrameEvent::HalfFrame)
            }
            (2, 11186) => {
                self.step = 3;
                Some(FrameEvent::QuarterFrame)
            }
            (3, 14915) => {
                self.step = 4;
                None // No event
            }
            (4, 18641) => {
                self.step = 0;
                self.cycles = 0;
                Some(FrameEvent::HalfFrame)
            }
            _ => None,
        }
    }
}
```

---

## Sample Generation

### Mixer

```rust
impl<B: ApuBus> Apu<B> {
    /// Mix channel outputs to final sample
    fn mix_output(&self) -> Sample {
        let pulse1 = self.pulse1.output() as f32;
        let pulse2 = self.pulse2.output() as f32;
        let triangle = self.triangle.output() as f32;
        let noise = self.noise.output() as f32;
        let dmc = self.dmc.output() as f32;

        // Non-linear mixing (NES hardware behavior)
        let pulse_out = if pulse1 + pulse2 > 0.0 {
            95.88 / (8128.0 / (pulse1 + pulse2) + 100.0)
        } else {
            0.0
        };

        let tnd_out = if triangle + noise + dmc > 0.0 {
            159.79 / (1.0 / (triangle / 8227.0 + noise / 12241.0 + dmc / 22638.0) + 100.0)
        } else {
            0.0
        };

        (pulse_out + tnd_out) as Sample
    }

    /// Drain sample buffer
    pub fn drain_samples(&mut self) -> Vec<Sample> {
        std::mem::take(&mut self.sample_buffer)
    }

    /// Get samples without draining
    pub fn peek_samples(&self) -> &[Sample] {
        &self.sample_buffer
    }

    /// Get number of available samples
    pub fn samples_available(&self) -> usize {
        self.sample_buffer.len()
    }

    /// Read samples into buffer (returns samples read)
    pub fn read_samples(&mut self, buffer: &mut [Sample]) -> usize {
        let count = buffer.len().min(self.sample_buffer.len());
        buffer[..count].copy_from_slice(&self.sample_buffer[..count]);
        self.sample_buffer.drain(..count);
        count
    }

    /// Read stereo samples (mono duplicated to both channels)
    pub fn read_stereo_samples(&mut self, buffer: &mut [StereoSample]) -> usize {
        let count = buffer.len().min(self.sample_buffer.len());
        for i in 0..count {
            let sample = self.sample_buffer[i];
            buffer[i] = StereoSample {
                left: sample,
                right: sample,
            };
        }
        self.sample_buffer.drain(..count);
        count
    }
}
```

### Audio Callback Integration

```rust
/// Audio callback for real-time output
pub fn audio_callback<B: ApuBus>(apu: &mut Apu<B>, output: &mut [f32]) {
    let samples_needed = output.len();
    let samples_available = apu.samples_available();

    if samples_available >= samples_needed {
        apu.read_samples(output);
    } else {
        // Fill with available samples, pad with last sample or silence
        let read = apu.read_samples(output);
        let fill_value = if read > 0 { output[read - 1] } else { 0.0 };
        for i in read..samples_needed {
            output[i] = fill_value;
        }
    }
}
```

---

## Expansion Audio

### Expansion Audio Trait

```rust
/// Expansion audio chip interface
pub trait ExpansionAudio {
    /// Write to expansion register
    fn write(&mut self, addr: u16, value: u8);

    /// Read from expansion register
    fn read(&self, addr: u16) -> u8;

    /// Clock expansion chip
    fn tick(&mut self);

    /// Get current output sample
    fn output(&self) -> Sample;

    /// Reset expansion chip
    fn reset(&mut self);
}
```

### VRC6 Expansion

```rust
/// Konami VRC6 expansion audio (Castlevania III)
pub struct Vrc6Audio {
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,
}

impl ExpansionAudio for Vrc6Audio {
    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x9000 => self.pulse1.write_control(value),
            0x9001 => self.pulse1.write_period_lo(value),
            0x9002 => self.pulse1.write_period_hi(value),
            0xA000 => self.pulse2.write_control(value),
            0xA001 => self.pulse2.write_period_lo(value),
            0xA002 => self.pulse2.write_period_hi(value),
            0xB000 => self.saw.write_accumulator(value),
            0xB001 => self.saw.write_period_lo(value),
            0xB002 => self.saw.write_period_hi(value),
            _ => {}
        }
    }

    fn output(&self) -> Sample {
        let pulse1 = self.pulse1.output() as f32 / 15.0;
        let pulse2 = self.pulse2.output() as f32 / 15.0;
        let saw = self.saw.output() as f32 / 255.0;

        (pulse1 + pulse2 + saw) / 3.0
    }

    fn tick(&mut self) {
        self.pulse1.tick();
        self.pulse2.tick();
        self.saw.tick();
    }

    fn read(&self, _addr: u16) -> u8 { 0 }
    fn reset(&mut self) {
        self.pulse1.reset();
        self.pulse2.reset();
        self.saw.reset();
    }
}
```

### APU with Expansion

```rust
impl<B: ApuBus> Apu<B> {
    /// Set expansion audio chip
    pub fn set_expansion(&mut self, expansion: Box<dyn ExpansionAudio>) {
        self.expansion = Some(expansion);
    }

    /// Mix with expansion audio
    fn mix_with_expansion(&self) -> Sample {
        let internal = self.mix_output();

        if let Some(ref expansion) = self.expansion {
            // Mix expansion at appropriate level
            let exp_output = expansion.output();
            internal * 0.75 + exp_output * 0.25
        } else {
            internal
        }
    }
}
```

---

## Debug Interface

### Channel Inspection

```rust
impl<B: ApuBus> Apu<B> {
    /// Get channel debug info
    pub fn get_channel_info(&self) -> ApuChannelInfo {
        ApuChannelInfo {
            pulse1: ChannelInfo {
                enabled: self.status.contains(ChannelStatus::PULSE1),
                output: self.pulse1.output(),
                length: self.pulse1.length_counter(),
                period: self.pulse1.timer_period(),
                frequency: self.calculate_frequency(self.pulse1.timer_period()),
            },
            pulse2: ChannelInfo {
                enabled: self.status.contains(ChannelStatus::PULSE2),
                output: self.pulse2.output(),
                length: self.pulse2.length_counter(),
                period: self.pulse2.timer_period(),
                frequency: self.calculate_frequency(self.pulse2.timer_period()),
            },
            triangle: ChannelInfo {
                enabled: self.status.contains(ChannelStatus::TRIANGLE),
                output: self.triangle.output(),
                length: self.triangle.length_counter(),
                period: self.triangle.timer_period(),
                frequency: self.calculate_frequency(self.triangle.timer_period()),
            },
            noise: ChannelInfo {
                enabled: self.status.contains(ChannelStatus::NOISE),
                output: self.noise.output(),
                length: self.noise.length_counter(),
                period: self.noise.timer_period(),
                frequency: 0.0, // Noise has no frequency
            },
            dmc: DmcInfo {
                enabled: self.status.contains(ChannelStatus::DMC),
                output: self.dmc.output(),
                bytes_remaining: self.dmc.bytes_remaining(),
                sample_addr: self.dmc.sample_addr(),
            },
        }
    }

    /// Calculate frequency from timer period
    fn calculate_frequency(&self, period: u16) -> f32 {
        if period == 0 {
            return 0.0;
        }
        const CPU_CLOCK: f32 = 1_789_773.0;
        CPU_CLOCK / (16.0 * (period as f32 + 1.0))
    }
}

/// APU channel debug information
#[derive(Debug, Clone)]
pub struct ApuChannelInfo {
    pub pulse1: ChannelInfo,
    pub pulse2: ChannelInfo,
    pub triangle: ChannelInfo,
    pub noise: ChannelInfo,
    pub dmc: DmcInfo,
}

#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub enabled: bool,
    pub output: u8,
    pub length: u8,
    pub period: u16,
    pub frequency: f32,
}

#[derive(Debug, Clone)]
pub struct DmcInfo {
    pub enabled: bool,
    pub output: u8,
    pub bytes_remaining: u16,
    pub sample_addr: u16,
}
```

---

## Examples

### Basic Audio Playback

```rust
use rustynes_apu::{Apu, ApuBus};
use sdl2::audio::{AudioCallback, AudioSpecDesired};

struct ApuCallback {
    apu: Apu<NesApuBus>,
}

impl AudioCallback for ApuCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        // Run APU to generate samples
        let cycles_needed = out.len() * 40; // Approximate
        self.apu.run(cycles_needed as u64);

        // Fill output buffer
        let samples = self.apu.drain_samples();
        for (i, sample) in out.iter_mut().enumerate() {
            *sample = samples.get(i).copied().unwrap_or(0.0);
        }
    }
}
```

### NSF Player Integration

```rust
fn play_nsf_song(apu: &mut Apu<impl ApuBus>, song_data: &[u8]) {
    // Reset APU
    apu.reset();

    // Initialize channels for song
    // (NSF INIT routine would write to APU registers)

    // Generate audio
    loop {
        apu.tick();

        if apu.samples_available() >= 1024 {
            let samples = apu.drain_samples();
            // Send to audio output...
        }
    }
}
```

---

## References

- [NESdev Wiki: APU](https://www.nesdev.org/wiki/APU)
- [NESdev Wiki: APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
- [NESdev Wiki: APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)

---

**Related Documents:**
- [APU_2A03_SPECIFICATION.md](../apu/APU_2A03_SPECIFICATION.md)
- [APU_CHANNEL_PULSE.md](../apu/APU_CHANNEL_PULSE.md)
- [APU_CHANNEL_TRIANGLE.md](../apu/APU_CHANNEL_TRIANGLE.md)
- [APU_CHANNEL_NOISE.md](../apu/APU_CHANNEL_NOISE.md)
- [APU_CHANNEL_DMC.md](../apu/APU_CHANNEL_DMC.md)
