# NES Expansion Audio Implementation Guide

Complete reference for implementing expansion audio chips in RustyNES, covering all major cartridge audio extensions used in NES/Famicom games.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Audio Mixing](#audio-mixing)
4. [VRC6 (Konami)](#vrc6-konami)
5. [VRC7 (Konami)](#vrc7-konami)
6. [N163 (Namco)](#n163-namco)
7. [MMC5 (Nintendo)](#mmc5-nintendo)
8. [Sunsoft 5B (FME-7)](#sunsoft-5b-fme-7)
9. [FDS Audio (Famicom Disk System)](#fds-audio-famicom-disk-system)
10. [Implementation Checklist](#implementation-checklist)
11. [Testing](#testing)
12. [References](#references)

---

## Overview

The NES/Famicom supported expansion audio through the cartridge connector. Japanese Famicom cartridges could add extra sound channels that mixed with the standard 2A03 APU output. This feature was unavailable on the NES due to different cartridge pinout.

### Supported Expansion Chips

| Chip | Manufacturer | Channels | Notable Games |
|------|--------------|----------|---------------|
| **VRC6** | Konami | 2 pulse + 1 saw | Castlevania III, Madara |
| **VRC7** | Konami | 6 FM channels | Lagrange Point |
| **N163** | Namco | 1-8 wavetable | Final Lap, Rolling Thunder |
| **MMC5** | Nintendo | 2 pulse + PCM | Castlevania III (US), Just Breed |
| **Sunsoft 5B** | Sunsoft | 3 square (AY-3-8910) | Gimmick! |
| **FDS** | Nintendo | 1 wavetable + modulation | Many FDS games |

### Design Goals

1. **Accuracy**: Match hardware behavior and sound characteristics
2. **Modularity**: Clean trait-based integration with existing APU
3. **Performance**: Efficient mixing without audio artifacts
4. **Extensibility**: Easy addition of new expansion chips

---

## Architecture

### Expansion Audio Trait

```rust
/// Trait for expansion audio chips
pub trait ExpansionAudio: Send {
    /// Process audio for one APU cycle
    fn clock(&mut self);

    /// Get current audio output (-1.0 to 1.0)
    fn output(&self) -> f32;

    /// Write to expansion audio register
    fn write(&mut self, addr: u16, data: u8);

    /// Read from expansion audio register (if readable)
    fn read(&self, addr: u16) -> Option<u8> {
        None
    }

    /// Reset the expansion audio chip
    fn reset(&mut self);

    /// Get chip identifier
    fn chip_type(&self) -> ExpansionChipType;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExpansionChipType {
    None,
    Vrc6,
    Vrc7,
    N163,
    Mmc5,
    Sunsoft5B,
    Fds,
}
```

### APU Integration

```rust
pub struct Apu {
    // Standard channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    // Expansion audio
    expansion: Option<Box<dyn ExpansionAudio>>,

    // Mixing
    mixer: AudioMixer,
}

impl Apu {
    pub fn set_expansion(&mut self, expansion: Box<dyn ExpansionAudio>) {
        self.expansion = Some(expansion);
    }

    pub fn clock(&mut self) {
        // Clock standard channels
        self.pulse1.clock();
        self.pulse2.clock();
        self.triangle.clock();
        self.noise.clock();
        self.dmc.clock();

        // Clock expansion audio
        if let Some(ref mut exp) = self.expansion {
            exp.clock();
        }
    }

    pub fn output(&self) -> f32 {
        let base = self.mixer.mix_base(
            self.pulse1.output(),
            self.pulse2.output(),
            self.triangle.output(),
            self.noise.output(),
            self.dmc.output(),
        );

        let expansion = self.expansion
            .as_ref()
            .map(|e| e.output())
            .unwrap_or(0.0);

        self.mixer.mix_final(base, expansion)
    }
}
```

---

## Audio Mixing

### Famicom Audio Path

```
┌─────────────────────────────────────────────────────────────┐
│                      Famicom Audio                          │
│                                                             │
│  ┌─────────┐                                               │
│  │  2A03   │──► Internal Mixing ──┐                        │
│  │   APU   │                      │                        │
│  └─────────┘                      ▼                        │
│                              ┌─────────┐                   │
│  ┌─────────┐                 │  Audio  │──► Line Out       │
│  │Expansion│──► Cartridge ──►│  Mixer  │                   │
│  │  Audio  │    Connector    └─────────┘                   │
│  └─────────┘                                               │
└─────────────────────────────────────────────────────────────┘
```

### Mixing Ratios

Each expansion chip has a different relative volume level compared to the base APU:

| Chip | Approximate Level | Mix Factor |
|------|-------------------|------------|
| VRC6 | Slightly louder | 0.85 |
| VRC7 | Much quieter | 0.40 |
| N163 | Variable (channels) | 0.60 - 0.80 |
| MMC5 | Similar to APU | 0.75 |
| Sunsoft 5B | Louder | 0.70 |
| FDS | Similar to APU | 0.80 |

### Mixer Implementation

```rust
pub struct AudioMixer {
    /// Base APU mixing tables (non-linear)
    pulse_table: [f32; 31],
    tnd_table: [f32; 203],

    /// Expansion mix factor
    expansion_factor: f32,

    /// Master volume (0.0 - 1.0)
    master_volume: f32,
}

impl AudioMixer {
    pub fn new() -> Self {
        let mut mixer = Self {
            pulse_table: [0.0; 31],
            tnd_table: [0.0; 203],
            expansion_factor: 0.75,
            master_volume: 1.0,
        };

        // Build non-linear mixing tables
        mixer.build_lookup_tables();
        mixer
    }

    fn build_lookup_tables(&mut self) {
        // Pulse mixing (non-linear)
        for n in 0..31 {
            if n == 0 {
                self.pulse_table[n] = 0.0;
            } else {
                self.pulse_table[n] = 95.52 / (8128.0 / n as f32 + 100.0);
            }
        }

        // TND mixing (triangle, noise, DMC)
        for n in 0..203 {
            if n == 0 {
                self.tnd_table[n] = 0.0;
            } else {
                self.tnd_table[n] = 163.67 / (24329.0 / n as f32 + 100.0);
            }
        }
    }

    /// Mix base APU channels
    pub fn mix_base(&self, p1: u8, p2: u8, tri: u8, noise: u8, dmc: u8) -> f32 {
        let pulse_out = self.pulse_table[(p1 + p2) as usize];
        let tnd_out = self.tnd_table[(3 * tri + 2 * noise + dmc) as usize];
        pulse_out + tnd_out
    }

    /// Mix base with expansion audio
    pub fn mix_final(&self, base: f32, expansion: f32) -> f32 {
        let mixed = base + (expansion * self.expansion_factor);
        (mixed * self.master_volume).clamp(-1.0, 1.0)
    }

    /// Set expansion chip mix factor
    pub fn set_expansion_factor(&mut self, factor: f32) {
        self.expansion_factor = factor.clamp(0.0, 2.0);
    }
}
```

---

## VRC6 (Konami)

### Overview

The VRC6 adds two pulse channels with 8 duty cycle settings (vs 4 on 2A03) and one sawtooth wave channel.

### Registers

| Address | Channel | Description |
|---------|---------|-------------|
| $9000 | Pulse 1 | Volume/Duty |
| $9001 | Pulse 1 | Period Low |
| $9002 | Pulse 1 | Period High/Enable |
| $A000 | Pulse 2 | Volume/Duty |
| $A001 | Pulse 2 | Period Low |
| $A002 | Pulse 2 | Period High/Enable |
| $B000 | Saw | Accumulator Rate |
| $B001 | Saw | Period Low |
| $B002 | Saw | Period High/Enable |

### Implementation

```rust
pub struct Vrc6Audio {
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,
    halt: bool,
}

impl Vrc6Audio {
    pub fn new() -> Self {
        Self {
            pulse1: Vrc6Pulse::new(),
            pulse2: Vrc6Pulse::new(),
            saw: Vrc6Saw::new(),
            halt: false,
        }
    }
}

impl ExpansionAudio for Vrc6Audio {
    fn clock(&mut self) {
        if !self.halt {
            self.pulse1.clock();
            self.pulse2.clock();
            self.saw.clock();
        }
    }

    fn output(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let saw = self.saw.output() as f32;

        // VRC6 output range: 0-30 per channel
        // Normalize to -1.0 to 1.0
        ((p1 + p2 + saw) / 45.0) - 1.0
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x9000 => self.pulse1.write_control(data),
            0x9001 => self.pulse1.write_period_low(data),
            0x9002 => self.pulse1.write_period_high(data),
            0xA000 => self.pulse2.write_control(data),
            0xA001 => self.pulse2.write_period_low(data),
            0xA002 => self.pulse2.write_period_high(data),
            0xB000 => self.saw.write_accumulator(data),
            0xB001 => self.saw.write_period_low(data),
            0xB002 => self.saw.write_period_high(data),
            0x9003 => self.halt = (data & 0x01) != 0,
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.pulse1 = Vrc6Pulse::new();
        self.pulse2 = Vrc6Pulse::new();
        self.saw = Vrc6Saw::new();
        self.halt = false;
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::Vrc6
    }
}

/// VRC6 Pulse Channel (8 duty cycle settings)
pub struct Vrc6Pulse {
    volume: u8,      // 4 bits (0-15)
    duty: u8,        // 3 bits (0-7), 8 settings
    period: u16,     // 12 bits
    timer: u16,
    sequence_pos: u8,
    enabled: bool,
    ignore_duty: bool, // "Mode" bit - output constant volume
}

impl Vrc6Pulse {
    pub fn new() -> Self {
        Self {
            volume: 0,
            duty: 0,
            period: 0,
            timer: 0,
            sequence_pos: 0,
            enabled: false,
            ignore_duty: false,
        }
    }

    pub fn clock(&mut self) {
        if !self.enabled || self.period == 0 {
            return;
        }

        if self.timer == 0 {
            self.timer = self.period;
            self.sequence_pos = (self.sequence_pos + 1) & 0x0F;
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }

        if self.ignore_duty {
            // Mode bit: constant volume output
            return self.volume;
        }

        // 8 duty cycle settings (duty+1)/16 high
        // sequence_pos 0-15, duty 0-7
        // Output high when sequence_pos <= duty
        if self.sequence_pos <= self.duty {
            self.volume
        } else {
            0
        }
    }

    pub fn write_control(&mut self, data: u8) {
        self.ignore_duty = (data & 0x80) != 0;
        self.duty = (data >> 4) & 0x07;
        self.volume = data & 0x0F;
    }

    pub fn write_period_low(&mut self, data: u8) {
        self.period = (self.period & 0x0F00) | data as u16;
    }

    pub fn write_period_high(&mut self, data: u8) {
        self.enabled = (data & 0x80) != 0;
        self.period = (self.period & 0x00FF) | ((data as u16 & 0x0F) << 8);
    }
}

/// VRC6 Sawtooth Channel
pub struct Vrc6Saw {
    accumulator_rate: u8, // 6 bits
    period: u16,          // 12 bits
    timer: u16,
    accumulator: u8,
    step: u8,             // 0-13 (14 steps per cycle)
    enabled: bool,
}

impl Vrc6Saw {
    pub fn new() -> Self {
        Self {
            accumulator_rate: 0,
            period: 0,
            timer: 0,
            accumulator: 0,
            step: 0,
            enabled: false,
        }
    }

    pub fn clock(&mut self) {
        if !self.enabled || self.period == 0 {
            return;
        }

        if self.timer == 0 {
            self.timer = self.period;
            self.step += 1;

            if self.step >= 14 {
                // Reset cycle
                self.step = 0;
                self.accumulator = 0;
            } else if self.step % 2 == 0 {
                // Accumulate on even steps (0,2,4,6,8,10,12)
                self.accumulator = self.accumulator.wrapping_add(self.accumulator_rate);
            }
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        // Output is top 5 bits of accumulator
        self.accumulator >> 3
    }

    pub fn write_accumulator(&mut self, data: u8) {
        self.accumulator_rate = data & 0x3F;
    }

    pub fn write_period_low(&mut self, data: u8) {
        self.period = (self.period & 0x0F00) | data as u16;
    }

    pub fn write_period_high(&mut self, data: u8) {
        self.enabled = (data & 0x80) != 0;
        self.period = (self.period & 0x00FF) | ((data as u16 & 0x0F) << 8);
    }
}
```

---

## VRC7 (Konami)

### Overview

The VRC7 uses a Yamaha YM2413 (OPLL) compatible FM synthesis chip with 6 channels. It features 15 preset instruments and 1 user-definable instrument.

### Registers

| Address | Description |
|---------|-------------|
| $9010 | Register select |
| $9030 | Register write |

### Internal Registers

| Register | Description |
|----------|-------------|
| $00-$07 | Custom instrument definition |
| $10-$15 | Channel frequency low |
| $20-$25 | Channel frequency high, key on/off |
| $30-$35 | Channel instrument, volume |

### Implementation

```rust
pub struct Vrc7Audio {
    /// Register address latch
    reg_select: u8,

    /// 6 FM channels
    channels: [Vrc7Channel; 6],

    /// Custom instrument patch
    custom_patch: [u8; 8],

    /// Preset instruments (15 built-in)
    presets: [[u8; 8]; 16],

    /// Global state
    rhythm_mode: bool,
}

impl Vrc7Audio {
    pub fn new() -> Self {
        Self {
            reg_select: 0,
            channels: [Vrc7Channel::new(); 6],
            custom_patch: [0; 8],
            presets: Self::init_presets(),
            rhythm_mode: false,
        }
    }

    /// Initialize VRC7 preset instruments
    fn init_presets() -> [[u8; 8]; 16] {
        // VRC7 uses a subset of YM2413 presets
        // These values are from hardware analysis
        [
            [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // Custom
            [0x03, 0x21, 0x05, 0x06, 0xB8, 0x82, 0x42, 0x27], // Bell
            [0x13, 0x41, 0x13, 0x0D, 0xD8, 0xD6, 0x23, 0x12], // Guitar
            [0x31, 0x11, 0x08, 0x08, 0xFA, 0x9A, 0x22, 0x02], // Piano
            [0x31, 0x61, 0x18, 0x07, 0x78, 0x64, 0x30, 0x27], // Flute
            [0x22, 0x21, 0x1E, 0x06, 0xF0, 0x76, 0x08, 0x28], // Clarinet
            [0x02, 0x01, 0x06, 0x00, 0xF0, 0xF2, 0x03, 0xF5], // Oboe
            [0x21, 0x61, 0x1D, 0x07, 0x82, 0x81, 0x16, 0x07], // Trumpet
            [0x23, 0x21, 0x1A, 0x17, 0xCF, 0x72, 0x25, 0x17], // Organ
            [0x15, 0x11, 0x25, 0x00, 0x4F, 0x71, 0x00, 0x11], // Horn
            [0x85, 0x01, 0x12, 0x0F, 0x99, 0xA2, 0x40, 0x02], // Synth
            [0x07, 0xC1, 0x69, 0x07, 0xF3, 0xF5, 0xA7, 0x12], // Harpsichord
            [0x71, 0x23, 0x0D, 0x06, 0x66, 0x75, 0x23, 0x16], // Vibraphone
            [0x01, 0x02, 0xD3, 0x05, 0xA3, 0x92, 0xF7, 0x52], // Synth Bass
            [0x61, 0x63, 0x0C, 0x00, 0x94, 0xAF, 0x34, 0x06], // Acoustic Bass
            [0x21, 0x72, 0x0D, 0x00, 0xC1, 0xA0, 0x54, 0x16], // Electric Guitar
        ]
    }
}

impl ExpansionAudio for Vrc7Audio {
    fn clock(&mut self) {
        for channel in &mut self.channels {
            channel.clock();
        }
    }

    fn output(&self) -> f32 {
        let mut sum: i32 = 0;
        for channel in &self.channels {
            sum += channel.output() as i32;
        }
        // Normalize VRC7 output
        (sum as f32 / (6.0 * 255.0)) * 0.5
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x9010 => self.reg_select = data,
            0x9030 => self.write_register(data),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.reg_select = 0;
        for channel in &mut self.channels {
            *channel = Vrc7Channel::new();
        }
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::Vrc7
    }
}

impl Vrc7Audio {
    fn write_register(&mut self, data: u8) {
        let reg = self.reg_select;

        match reg {
            0x00..=0x07 => {
                // Custom instrument definition
                self.custom_patch[reg as usize] = data;
            }
            0x10..=0x15 => {
                // Frequency low
                let ch = (reg - 0x10) as usize;
                self.channels[ch].write_freq_low(data);
            }
            0x20..=0x25 => {
                // Frequency high + key on/off
                let ch = (reg - 0x20) as usize;
                self.channels[ch].write_freq_high(data);
            }
            0x30..=0x35 => {
                // Instrument + volume
                let ch = (reg - 0x30) as usize;
                let instrument = (data >> 4) & 0x0F;
                let volume = data & 0x0F;
                let patch = if instrument == 0 {
                    &self.custom_patch
                } else {
                    &self.presets[instrument as usize]
                };
                self.channels[ch].set_patch(patch, volume);
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
pub struct Vrc7Channel {
    // FM synthesis state
    freq: u16,
    octave: u8,
    key_on: bool,
    sustain: bool,
    volume: u8,

    // Operator state (2-op FM)
    modulator: FmOperator,
    carrier: FmOperator,
}

impl Vrc7Channel {
    pub fn new() -> Self {
        Self {
            freq: 0,
            octave: 0,
            key_on: false,
            sustain: false,
            volume: 0,
            modulator: FmOperator::new(),
            carrier: FmOperator::new(),
        }
    }

    pub fn clock(&mut self) {
        if !self.key_on {
            return;
        }

        // FM synthesis algorithm
        let mod_out = self.modulator.clock(0);
        let _car_out = self.carrier.clock(mod_out);
    }

    pub fn output(&self) -> i16 {
        if !self.key_on {
            return 0;
        }
        self.carrier.output()
    }

    pub fn write_freq_low(&mut self, data: u8) {
        self.freq = (self.freq & 0x100) | data as u16;
    }

    pub fn write_freq_high(&mut self, data: u8) {
        self.freq = (self.freq & 0xFF) | ((data as u16 & 0x01) << 8);
        self.octave = (data >> 1) & 0x07;
        self.key_on = (data & 0x10) != 0;
        self.sustain = (data & 0x20) != 0;
    }

    pub fn set_patch(&mut self, patch: &[u8; 8], volume: u8) {
        self.volume = volume;
        self.modulator.load_patch(&patch[0..4]);
        self.carrier.load_patch(&patch[4..8]);
    }
}

#[derive(Clone, Copy)]
pub struct FmOperator {
    // Envelope state
    attack_rate: u8,
    decay_rate: u8,
    sustain_level: u8,
    release_rate: u8,

    // Phase state
    phase: u32,
    multiple: u8,

    // Modulation
    total_level: u8,
    key_scale: u8,

    // Output
    output_level: i16,
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            attack_rate: 0,
            decay_rate: 0,
            sustain_level: 0,
            release_rate: 0,
            phase: 0,
            multiple: 0,
            total_level: 0,
            key_scale: 0,
            output_level: 0,
        }
    }

    pub fn clock(&mut self, modulation: i16) -> i16 {
        // Simplified FM operator
        // Full implementation requires sine table, envelope, etc.
        self.phase = self.phase.wrapping_add(1);
        let sine_index = (self.phase >> 10) as usize & 0x3FF;
        self.output_level = Self::sine_table(sine_index + modulation as usize);
        self.output_level
    }

    pub fn output(&self) -> i16 {
        self.output_level
    }

    pub fn load_patch(&mut self, data: &[u8]) {
        // Parse patch data
        // Format varies by register
        self.multiple = data[0] & 0x0F;
        self.key_scale = (data[0] >> 6) & 0x03;
        self.total_level = data[1] & 0x3F;
        self.attack_rate = (data[2] >> 4) & 0x0F;
        self.decay_rate = data[2] & 0x0F;
        self.sustain_level = (data[3] >> 4) & 0x0F;
        self.release_rate = data[3] & 0x0F;
    }

    fn sine_table(index: usize) -> i16 {
        // Simplified - real implementation uses log-sin table
        let angle = (index as f32 / 1024.0) * std::f32::consts::TAU;
        (angle.sin() * 127.0) as i16
    }
}
```

---

## N163 (Namco)

### Overview

The Namco 163 provides 1-8 wavetable channels with shared RAM for waveform data. More channels = lower update rate per channel.

### Registers

| Address | Description |
|---------|-------------|
| $4800 | Data port (read/write) |
| $F800 | Address port (write) |

### Internal Memory Map

| Address | Description |
|---------|-------------|
| $00-$77 | Waveform RAM (120 bytes) |
| $78-$7F | Channel registers (8 bytes per channel) |

### Implementation

```rust
pub struct N163Audio {
    /// 128 bytes internal RAM (waveforms + registers)
    ram: [u8; 128],

    /// Address register
    address: u8,

    /// Auto-increment flag
    auto_increment: bool,

    /// Number of active channels (1-8)
    num_channels: u8,

    /// Channel state
    channels: [N163Channel; 8],

    /// Current channel being updated
    current_channel: u8,

    /// Clock divider
    divider: u8,
}

impl N163Audio {
    pub fn new() -> Self {
        Self {
            ram: [0; 128],
            address: 0,
            auto_increment: false,
            num_channels: 1,
            channels: [N163Channel::new(); 8],
            current_channel: 0,
            divider: 0,
        }
    }

    fn update_channel_from_ram(&mut self, ch: usize) {
        let base = 0x78 + (7 - ch) * 8;

        let freq_low = self.ram[base] as u32;
        let freq_mid = self.ram[base + 2] as u32;
        let freq_high = (self.ram[base + 4] & 0x03) as u32;
        let wave_length = (self.ram[base + 4] >> 2) & 0x3F;
        let wave_addr = self.ram[base + 6];
        let volume = self.ram[base + 7] & 0x0F;

        self.channels[ch].frequency = freq_low | (freq_mid << 8) | (freq_high << 16);
        self.channels[ch].wave_length = 256 - (wave_length as u16 * 4);
        self.channels[ch].wave_address = wave_addr;
        self.channels[ch].volume = volume;
    }
}

impl ExpansionAudio for N163Audio {
    fn clock(&mut self) {
        self.divider += 1;
        if self.divider < 15 {
            return;
        }
        self.divider = 0;

        // Update one channel per 15 CPU cycles
        let ch = self.current_channel as usize;
        if ch < self.num_channels as usize {
            self.update_channel_from_ram(ch);
            self.channels[ch].clock(&self.ram);
        }

        self.current_channel = (self.current_channel + 1) % 8;
    }

    fn output(&self) -> f32 {
        let mut sum: i32 = 0;

        for i in 0..self.num_channels as usize {
            sum += self.channels[i].output() as i32;
        }

        // Normalize based on active channels
        let max_out = self.num_channels as f32 * 15.0;
        (sum as f32 / max_out) - 0.5
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x4800 => {
                // Data write
                self.ram[self.address as usize & 0x7F] = data;

                // Update channel count from register $7F
                if self.address == 0x7F {
                    self.num_channels = ((data >> 4) & 0x07) + 1;
                }

                if self.auto_increment {
                    self.address = self.address.wrapping_add(1) & 0x7F;
                }
            }
            0xF800 => {
                // Address write
                self.address = data & 0x7F;
                self.auto_increment = (data & 0x80) != 0;
            }
            _ => {}
        }
    }

    fn read(&self, addr: u16) -> Option<u8> {
        if addr == 0x4800 {
            Some(self.ram[self.address as usize & 0x7F])
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.ram = [0; 128];
        self.address = 0;
        self.auto_increment = false;
        self.num_channels = 1;
        self.channels = [N163Channel::new(); 8];
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::N163
    }
}

#[derive(Clone, Copy)]
pub struct N163Channel {
    frequency: u32,
    phase: u32,
    wave_address: u8,
    wave_length: u16,
    volume: u8,
}

impl N163Channel {
    pub fn new() -> Self {
        Self {
            frequency: 0,
            phase: 0,
            wave_address: 0,
            wave_length: 4,
            volume: 0,
        }
    }

    pub fn clock(&mut self, ram: &[u8; 128]) {
        self.phase = self.phase.wrapping_add(self.frequency);

        // Wrap phase at wave length
        while self.phase >= (self.wave_length as u32) << 16 {
            self.phase -= (self.wave_length as u32) << 16;
        }
    }

    pub fn output(&self) -> u8 {
        if self.volume == 0 {
            return 0;
        }

        // Calculate sample index
        let sample_index = (self.phase >> 16) as usize;
        let addr = self.wave_address as usize + sample_index;

        // Each byte holds two 4-bit samples
        let byte = addr / 2;
        let sample = if addr & 1 == 0 {
            // Low nibble
            self.wave_address as usize // Simplified
        } else {
            // High nibble
            self.wave_address as usize >> 4
        };

        (sample as u8 * self.volume) >> 4
    }
}
```

---

## MMC5 (Nintendo)

### Overview

The MMC5 adds two pulse channels (similar to 2A03) and an 8-bit PCM channel.

### Registers

| Address | Description |
|---------|-------------|
| $5000 | Pulse 1 control |
| $5002 | Pulse 1 period low |
| $5003 | Pulse 1 period high |
| $5004 | Pulse 2 control |
| $5006 | Pulse 2 period low |
| $5007 | Pulse 2 period high |
| $5010 | PCM mode/IRQ |
| $5011 | PCM output |
| $5015 | Channel enable |

### Implementation

```rust
pub struct Mmc5Audio {
    pulse1: Mmc5Pulse,
    pulse2: Mmc5Pulse,
    pcm: Mmc5Pcm,
    enabled: u8,
}

impl Mmc5Audio {
    pub fn new() -> Self {
        Self {
            pulse1: Mmc5Pulse::new(),
            pulse2: Mmc5Pulse::new(),
            pcm: Mmc5Pcm::new(),
            enabled: 0,
        }
    }
}

impl ExpansionAudio for Mmc5Audio {
    fn clock(&mut self) {
        if self.enabled & 0x01 != 0 {
            self.pulse1.clock();
        }
        if self.enabled & 0x02 != 0 {
            self.pulse2.clock();
        }
    }

    fn output(&self) -> f32 {
        let p1 = if self.enabled & 0x01 != 0 {
            self.pulse1.output() as f32
        } else {
            0.0
        };

        let p2 = if self.enabled & 0x02 != 0 {
            self.pulse2.output() as f32
        } else {
            0.0
        };

        let pcm = self.pcm.output() as f32;

        // Normalize to -1.0 to 1.0
        ((p1 + p2) / 30.0 + pcm / 255.0) - 0.5
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x5000 => self.pulse1.write_control(data),
            0x5002 => self.pulse1.write_period_low(data),
            0x5003 => self.pulse1.write_period_high(data),
            0x5004 => self.pulse2.write_control(data),
            0x5006 => self.pulse2.write_period_low(data),
            0x5007 => self.pulse2.write_period_high(data),
            0x5010 => self.pcm.write_mode(data),
            0x5011 => self.pcm.write_output(data),
            0x5015 => self.enabled = data,
            _ => {}
        }
    }

    fn read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x5010 => Some(self.pcm.read_status()),
            0x5015 => Some(self.enabled),
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.pulse1 = Mmc5Pulse::new();
        self.pulse2 = Mmc5Pulse::new();
        self.pcm = Mmc5Pcm::new();
        self.enabled = 0;
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::Mmc5
    }
}

/// MMC5 Pulse Channel
/// Similar to 2A03 pulse but without sweep or length counter
pub struct Mmc5Pulse {
    duty: u8,
    constant_volume: bool,
    volume: u8,
    period: u16,
    timer: u16,
    sequence_pos: u8,
}

impl Mmc5Pulse {
    const DUTY_TABLE: [[u8; 8]; 4] = [
        [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
        [0, 1, 1, 0, 0, 0, 0, 0], // 25%
        [0, 1, 1, 1, 1, 0, 0, 0], // 50%
        [1, 0, 0, 1, 1, 1, 1, 1], // 75% (inverted 25%)
    ];

    pub fn new() -> Self {
        Self {
            duty: 0,
            constant_volume: false,
            volume: 0,
            period: 0,
            timer: 0,
            sequence_pos: 0,
        }
    }

    pub fn clock(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.sequence_pos = (self.sequence_pos + 1) & 0x07;
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if self.period < 8 {
            return 0; // Mute on very high frequencies
        }

        let duty_out = Self::DUTY_TABLE[self.duty as usize][self.sequence_pos as usize];
        if duty_out != 0 {
            self.volume
        } else {
            0
        }
    }

    pub fn write_control(&mut self, data: u8) {
        self.duty = (data >> 6) & 0x03;
        self.constant_volume = (data & 0x10) != 0;
        self.volume = data & 0x0F;
    }

    pub fn write_period_low(&mut self, data: u8) {
        self.period = (self.period & 0x0700) | data as u16;
    }

    pub fn write_period_high(&mut self, data: u8) {
        self.period = (self.period & 0x00FF) | ((data as u16 & 0x07) << 8);
    }
}

/// MMC5 PCM Channel
pub struct Mmc5Pcm {
    output: u8,
    read_mode: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Mmc5Pcm {
    pub fn new() -> Self {
        Self {
            output: 0,
            read_mode: false,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    pub fn output(&self) -> u8 {
        self.output
    }

    pub fn write_mode(&mut self, data: u8) {
        self.read_mode = (data & 0x01) != 0;
        self.irq_enabled = (data & 0x80) != 0;
    }

    pub fn write_output(&mut self, data: u8) {
        if !self.read_mode {
            self.output = data;
            self.irq_pending = false;
        }
    }

    pub fn read_status(&self) -> u8 {
        let mut status = 0;
        if self.irq_pending {
            status |= 0x80;
        }
        status
    }
}
```

---

## Sunsoft 5B (FME-7)

### Overview

The Sunsoft 5B uses a YM2149F (AY-3-8910 compatible) PSG chip with 3 square wave channels, noise, and envelope.

### Registers

| Address | Description |
|---------|-------------|
| $C000 | Register select |
| $E000 | Register write |

### Implementation

```rust
pub struct Sunsoft5BAudio {
    reg_select: u8,
    channels: [SunsoftSquare; 3],
    noise: SunsoftNoise,
    mixer: u8,
    envelope: SunsoftEnvelope,
}

impl Sunsoft5BAudio {
    pub fn new() -> Self {
        Self {
            reg_select: 0,
            channels: [SunsoftSquare::new(); 3],
            noise: SunsoftNoise::new(),
            mixer: 0xFF, // All disabled initially
            envelope: SunsoftEnvelope::new(),
        }
    }
}

impl ExpansionAudio for Sunsoft5BAudio {
    fn clock(&mut self) {
        for ch in &mut self.channels {
            ch.clock();
        }
        self.noise.clock();
        self.envelope.clock();
    }

    fn output(&self) -> f32 {
        let mut sum: i32 = 0;

        for (i, ch) in self.channels.iter().enumerate() {
            let tone_enabled = (self.mixer & (1 << i)) == 0;
            let noise_enabled = (self.mixer & (8 << i)) == 0;

            let tone = if tone_enabled { ch.output() } else { 1 };
            let noise = if noise_enabled { self.noise.output() } else { 1 };

            if (tone | noise) != 0 {
                let vol = if ch.envelope_enabled {
                    self.envelope.volume()
                } else {
                    ch.volume
                };
                sum += vol as i32;
            }
        }

        // Normalize to -1.0 to 1.0
        (sum as f32 / 45.0) - 0.5
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0xC000 => self.reg_select = data & 0x0F,
            0xE000 => self.write_register(data),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.reg_select = 0;
        self.channels = [SunsoftSquare::new(); 3];
        self.noise = SunsoftNoise::new();
        self.mixer = 0xFF;
        self.envelope = SunsoftEnvelope::new();
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::Sunsoft5B
    }
}

impl Sunsoft5BAudio {
    fn write_register(&mut self, data: u8) {
        match self.reg_select {
            0 => self.channels[0].write_period_low(data),
            1 => self.channels[0].write_period_high(data),
            2 => self.channels[1].write_period_low(data),
            3 => self.channels[1].write_period_high(data),
            4 => self.channels[2].write_period_low(data),
            5 => self.channels[2].write_period_high(data),
            6 => self.noise.write_period(data),
            7 => self.mixer = data,
            8 => self.channels[0].write_volume(data),
            9 => self.channels[1].write_volume(data),
            10 => self.channels[2].write_volume(data),
            11 => self.envelope.write_period_low(data),
            12 => self.envelope.write_period_high(data),
            13 => self.envelope.write_shape(data),
            _ => {}
        }
    }
}

#[derive(Clone, Copy)]
pub struct SunsoftSquare {
    period: u16,
    timer: u16,
    output_state: u8,
    volume: u8,
    envelope_enabled: bool,
}

impl SunsoftSquare {
    pub fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            output_state: 0,
            volume: 0,
            envelope_enabled: false,
        }
    }

    pub fn clock(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.output_state ^= 1;
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        self.output_state
    }

    pub fn write_period_low(&mut self, data: u8) {
        self.period = (self.period & 0x0F00) | data as u16;
    }

    pub fn write_period_high(&mut self, data: u8) {
        self.period = (self.period & 0x00FF) | ((data as u16 & 0x0F) << 8);
    }

    pub fn write_volume(&mut self, data: u8) {
        self.envelope_enabled = (data & 0x10) != 0;
        self.volume = data & 0x0F;
    }
}

#[derive(Clone, Copy)]
pub struct SunsoftNoise {
    period: u8,
    timer: u8,
    lfsr: u16,
}

impl SunsoftNoise {
    pub fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            lfsr: 1,
        }
    }

    pub fn clock(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            // LFSR feedback
            let feedback = ((self.lfsr ^ (self.lfsr >> 3)) & 1) as u16;
            self.lfsr = (self.lfsr >> 1) | (feedback << 16);
        } else {
            self.timer -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        (self.lfsr & 1) as u8
    }

    pub fn write_period(&mut self, data: u8) {
        self.period = data & 0x1F;
    }
}

#[derive(Clone, Copy)]
pub struct SunsoftEnvelope {
    period: u16,
    timer: u16,
    counter: u8,
    shape: u8,
    holding: bool,
}

impl SunsoftEnvelope {
    pub fn new() -> Self {
        Self {
            period: 0,
            timer: 0,
            counter: 0,
            shape: 0,
            holding: false,
        }
    }

    pub fn clock(&mut self) {
        if self.holding {
            return;
        }

        if self.timer == 0 {
            self.timer = self.period;
            self.counter = (self.counter + 1) & 0x1F;

            // Check for hold/alternate/attack
            if self.counter == 0x10 {
                // Cycle complete
                let hold = (self.shape & 0x01) != 0;
                let alternate = (self.shape & 0x02) != 0;

                if hold {
                    self.holding = true;
                } else if !alternate {
                    self.counter = 0;
                }
            }
        } else {
            self.timer -= 1;
        }
    }

    pub fn volume(&self) -> u8 {
        let attack = (self.shape & 0x04) != 0;
        let vol = self.counter & 0x0F;

        if attack {
            vol
        } else {
            15 - vol
        }
    }

    pub fn write_period_low(&mut self, data: u8) {
        self.period = (self.period & 0xFF00) | data as u16;
    }

    pub fn write_period_high(&mut self, data: u8) {
        self.period = (self.period & 0x00FF) | ((data as u16) << 8);
    }

    pub fn write_shape(&mut self, data: u8) {
        self.shape = data & 0x0F;
        self.counter = 0;
        self.holding = false;
    }
}
```

---

## FDS Audio (Famicom Disk System)

### Overview

The FDS audio unit provides a single wavetable channel with frequency modulation capabilities.

### Registers

| Address | Description |
|---------|-------------|
| $4040-$407F | Wavetable (64 samples, 6-bit each) |
| $4080 | Volume envelope |
| $4082 | Frequency low |
| $4083 | Frequency high |
| $4084 | Mod envelope |
| $4085 | Mod counter |
| $4086 | Mod frequency low |
| $4087 | Mod frequency high |
| $4088 | Mod table write |
| $4089 | Wave write enable |
| $408A | Envelope speed |

### Implementation

```rust
pub struct FdsAudio {
    /// 64-sample wavetable (6-bit values)
    wavetable: [u8; 64],

    /// Modulation table (32 entries, 3-bit values)
    mod_table: [i8; 32],

    /// Main oscillator
    wave_phase: u32,
    wave_freq: u16,
    wave_enabled: bool,

    /// Volume envelope
    volume: u8,
    volume_gain: u8,
    volume_direction: bool,
    volume_speed: u8,
    volume_enabled: bool,

    /// Modulator
    mod_phase: u32,
    mod_freq: u16,
    mod_counter: i8,
    mod_enabled: bool,
    mod_gain: u8,

    /// Envelope base speed
    envelope_speed: u8,

    /// Write enable
    wave_write_enabled: bool,
    mod_table_index: u8,
}

impl FdsAudio {
    pub fn new() -> Self {
        Self {
            wavetable: [0; 64],
            mod_table: [0; 32],
            wave_phase: 0,
            wave_freq: 0,
            wave_enabled: false,
            volume: 0,
            volume_gain: 0,
            volume_direction: false,
            volume_speed: 0,
            volume_enabled: false,
            mod_phase: 0,
            mod_freq: 0,
            mod_counter: 0,
            mod_enabled: false,
            mod_gain: 0,
            envelope_speed: 0,
            wave_write_enabled: false,
            mod_table_index: 0,
        }
    }

    fn clock_envelope(&mut self) {
        if !self.volume_enabled || self.envelope_speed == 0 {
            return;
        }

        if self.volume_direction {
            // Increasing
            if self.volume_gain < 32 {
                self.volume_gain += 1;
            }
        } else {
            // Decreasing
            if self.volume_gain > 0 {
                self.volume_gain -= 1;
            }
        }
    }

    fn clock_modulator(&mut self) {
        if !self.mod_enabled || self.mod_freq == 0 {
            return;
        }

        // Advance modulator phase
        self.mod_phase = self.mod_phase.wrapping_add(self.mod_freq as u32);

        // Get modulation value from table
        let mod_index = ((self.mod_phase >> 16) & 0x1F) as usize;
        let mod_value = self.mod_table[mod_index];

        // Update modulation counter
        self.mod_counter = self.mod_counter.wrapping_add(mod_value);
    }

    fn calculate_pitch(&self) -> u32 {
        // Apply frequency modulation
        let mut freq = self.wave_freq as i32;

        if self.mod_enabled {
            let mod_amount = (self.mod_counter as i32 * self.mod_gain as i32) / 64;
            freq = freq.wrapping_add(mod_amount);
        }

        freq.max(0) as u32
    }
}

impl ExpansionAudio for FdsAudio {
    fn clock(&mut self) {
        self.clock_envelope();
        self.clock_modulator();

        if !self.wave_enabled {
            return;
        }

        // Advance wave phase with modulated frequency
        let pitch = self.calculate_pitch();
        self.wave_phase = self.wave_phase.wrapping_add(pitch);
    }

    fn output(&self) -> f32 {
        if !self.wave_enabled || self.wave_write_enabled {
            return 0.0;
        }

        // Get sample from wavetable
        let sample_index = ((self.wave_phase >> 16) & 0x3F) as usize;
        let sample = self.wavetable[sample_index] as i32;

        // Apply volume
        let vol = self.volume_gain.min(32) as i32;
        let output = (sample * vol) >> 5;

        // Normalize to -1.0 to 1.0
        (output as f32 / 32.0) - 0.5
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x4040..=0x407F if self.wave_write_enabled => {
                let index = (addr - 0x4040) as usize;
                self.wavetable[index] = data & 0x3F;
            }
            0x4080 => {
                self.volume_enabled = (data & 0x80) == 0;
                self.volume_direction = (data & 0x40) != 0;
                self.volume_speed = data & 0x3F;
                if !self.volume_enabled {
                    self.volume_gain = self.volume_speed;
                }
            }
            0x4082 => {
                self.wave_freq = (self.wave_freq & 0x0F00) | data as u16;
            }
            0x4083 => {
                self.wave_enabled = (data & 0x80) == 0;
                self.wave_freq = (self.wave_freq & 0x00FF) | ((data as u16 & 0x0F) << 8);
                if data & 0x40 != 0 {
                    // Reset envelope
                    self.volume_gain = self.volume_speed;
                }
            }
            0x4084 => {
                self.mod_enabled = (data & 0x80) == 0;
                self.mod_gain = data & 0x3F;
            }
            0x4085 => {
                self.mod_counter = (data & 0x7F) as i8;
            }
            0x4086 => {
                self.mod_freq = (self.mod_freq & 0x0F00) | data as u16;
            }
            0x4087 => {
                self.mod_freq = (self.mod_freq & 0x00FF) | ((data as u16 & 0x0F) << 8);
                if data & 0x80 != 0 {
                    // Reset modulator
                    self.mod_phase = 0;
                }
            }
            0x4088 => {
                // Write to mod table (two entries at once)
                let value = ((data & 0x07) as i8) - 4; // Convert to -4..+3
                let idx = self.mod_table_index as usize;
                if idx < 32 {
                    self.mod_table[idx] = value;
                }
                self.mod_table_index = (self.mod_table_index + 1) & 0x1F;
            }
            0x4089 => {
                self.wave_write_enabled = (data & 0x80) != 0;
                self.volume = data & 0x03;
            }
            0x408A => {
                self.envelope_speed = data;
            }
            _ => {}
        }
    }

    fn read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x4040..=0x407F => {
                let index = (addr - 0x4040) as usize;
                Some(self.wavetable[index])
            }
            0x4090 => Some(self.volume_gain | 0x40),
            0x4092 => Some(self.mod_gain | 0x40),
            _ => None,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn chip_type(&self) -> ExpansionChipType {
        ExpansionChipType::Fds
    }
}
```

---

## Implementation Checklist

### Core Requirements

- [ ] ExpansionAudio trait implementation
- [ ] Register read/write handling
- [ ] Accurate clocking/timing
- [ ] Audio output normalization
- [ ] Reset behavior

### Per-Chip Requirements

#### VRC6
- [ ] Two pulse channels with 8 duty settings
- [ ] Sawtooth channel with accumulator
- [ ] Frequency halt register
- [ ] Correct address mapping (may vary by variant)

#### VRC7
- [ ] 6 FM channels
- [ ] 15 preset instruments + custom
- [ ] Proper FM synthesis (sine tables, envelopes)
- [ ] OPLL-compatible timing

#### N163
- [ ] Variable channel count (1-8)
- [ ] Shared wavetable RAM
- [ ] Per-channel update scheduling
- [ ] Address auto-increment

#### MMC5
- [ ] Two pulse channels (no sweep/length)
- [ ] 8-bit PCM channel
- [ ] PCM IRQ support
- [ ] Correct mixing levels

#### Sunsoft 5B
- [ ] Three square wave channels
- [ ] Noise generator (LFSR)
- [ ] Hardware envelope
- [ ] Mixer control

#### FDS
- [ ] 64-sample wavetable
- [ ] Frequency modulation
- [ ] Volume envelope
- [ ] Mod table programming

---

## Testing

### Test ROMs

| Test ROM | Purpose |
|----------|---------|
| `vrc6_test.nes` | VRC6 audio accuracy |
| `vrc7_test.nes` | VRC7 FM synthesis |
| `n163_test.nes` | N163 wavetable |
| `mmc5_test.nes` | MMC5 audio channels |
| `5b_test.nes` | Sunsoft 5B accuracy |
| `fds_audio_test.fds` | FDS audio |

### Audio Comparison

Record output from:
1. Real hardware (Famicom + cartridge)
2. Accurate emulators (Mesen, puNES)
3. RustyNES implementation

Compare spectrograms for frequency accuracy and waveform shapes.

---

## References

### Documentation

- [NESdev Wiki: Expansion Audio](https://www.nesdev.org/wiki/Expansion_audio)
- [VRC6 Audio](https://www.nesdev.org/wiki/VRC6_audio)
- [VRC7 Audio](https://www.nesdev.org/wiki/VRC7_audio)
- [Namco 163 Audio](https://www.nesdev.org/wiki/Namco_163_audio)
- [MMC5 Audio](https://www.nesdev.org/wiki/MMC5_audio)
- [Sunsoft 5B Audio](https://www.nesdev.org/wiki/Sunsoft_5B_audio)
- [FDS Audio](https://www.nesdev.org/wiki/FDS_audio)

### Source Files

```
crates/rustynes-apu/src/
├── expansion/
│   ├── mod.rs          # ExpansionAudio trait
│   ├── vrc6.rs         # VRC6 implementation
│   ├── vrc7.rs         # VRC7 implementation
│   ├── n163.rs         # Namco 163 implementation
│   ├── mmc5.rs         # MMC5 audio implementation
│   ├── sunsoft5b.rs    # Sunsoft 5B implementation
│   └── fds.rs          # FDS audio implementation
└── mixer.rs            # Audio mixing with expansion
```
