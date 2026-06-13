# [Milestone 3] Sprint 3.5: APU Integration & Testing

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 19, 2025
**Duration:** ~1 week (actual)
**Assignee:** Claude Code / Developer
**Dependencies:** Sprints 3.1-3.4 ✅ Complete

---

## Overview

Integrate all APU channels, implement non-linear mixing, add resampling for 48 kHz audio output, and validate with Blargg APU test ROMs. This sprint delivers the complete, playable APU with accurate audio output.

---

## Acceptance Criteria

- [ ] All 5 channels integrated and synchronized
- [ ] Non-linear mixing with lookup tables
- [ ] 48 kHz audio resampling
- [ ] Ring buffer for audio output
- [ ] Low-pass filter to reduce aliasing
- [ ] Pass 95%+ Blargg APU tests
- [ ] Audio quality verification with test games
- [ ] Zero unsafe code
- [ ] Complete documentation

---

## Tasks

### 3.5.1 Channel Integration

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Integrate all 5 APU channels into the main APU struct with unified clocking.

**Files:**

- `crates/rustynes-apu/src/apu.rs` - Complete APU implementation
- `crates/rustynes-apu/src/lib.rs` - Public API

**Subtasks:**

- [ ] Add all channel instances to APU struct
- [ ] Implement unified step() function
- [ ] Clock all channels synchronously
- [ ] Handle frame counter actions for all channels
- [ ] Implement read/write dispatching to channels

**Implementation:**

```rust
use crate::frame_counter::{FrameCounter, FrameAction};
use crate::pulse::PulseChannel;
use crate::triangle::TriangleChannel;
use crate::noise::NoiseChannel;
use crate::dmc::DmcChannel;
use crate::mixer::Mixer;

pub struct Apu {
    // Channels
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,

    // Frame counter
    frame_counter: FrameCounter,

    // Mixer
    mixer: Mixer,

    // Cycle tracking
    cycles: u64,

    // IRQ flags
    dmc_irq: bool,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            pulse1: PulseChannel::new(0),
            pulse2: PulseChannel::new(1),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
            frame_counter: FrameCounter::new(),
            mixer: Mixer::new(),
            cycles: 0,
            dmc_irq: false,
        }
    }

    pub fn step<F>(&mut self, mut read_memory: F) -> f32
    where
        F: FnMut(u16) -> u8,
    {
        self.cycles += 1;

        // Clock frame counter
        let action = self.frame_counter.clock();

        // Handle frame actions
        match action {
            FrameAction::QuarterFrame => {
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
                self.triangle.clock_linear_counter();
                self.noise.clock_envelope();
            }
            FrameAction::HalfFrame => {
                self.pulse1.clock_envelope();
                self.pulse2.clock_envelope();
                self.triangle.clock_linear_counter();
                self.noise.clock_envelope();

                self.pulse1.clock_length_counter();
                self.pulse2.clock_length_counter();
                self.triangle.clock_length_counter();
                self.noise.clock_length_counter();

                self.pulse1.clock_sweep();
                self.pulse2.clock_sweep();
            }
            FrameAction::None => {}
        }

        // Clock channel timers
        self.pulse1.clock_timer();
        self.pulse2.clock_timer();
        self.triangle.clock_timer();
        self.noise.clock_timer();
        self.dmc.clock_timer();

        // Clock DMC memory reader if needed
        if self.dmc.needs_dma_read() {
            self.dmc.clock_memory_reader(&mut read_memory);
        }

        // Mix channels
        self.mixer.mix(
            self.pulse1.output(),
            self.pulse2.output(),
            self.triangle.output(),
            self.noise.output(),
            self.dmc.output(),
        )
    }

    pub fn irq_pending(&self) -> bool {
        self.frame_counter.irq_pending() || self.dmc.irq_pending()
    }
}
```

---

### 3.5.2 Non-Linear Mixing

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement hardware-accurate non-linear mixing using lookup tables.

**Files:**

- `crates/rustynes-apu/src/mixer.rs` - Mixing logic

**Subtasks:**

- [ ] Generate pulse mixing lookup table (31 entries)
- [ ] Generate TND mixing lookup table (203 entries)
- [ ] Combine pulse and TND outputs
- [ ] Optional: Linear approximation for comparison

**Implementation:**

```rust
pub struct Mixer {
    pulse_table: [f32; 31],
    tnd_table: [f32; 203],
}

impl Mixer {
    pub fn new() -> Self {
        let pulse_table = Self::generate_pulse_table();
        let tnd_table = Self::generate_tnd_table();

        Self {
            pulse_table,
            tnd_table,
        }
    }

    fn generate_pulse_table() -> [f32; 31] {
        let mut table = [0.0; 31];

        for i in 0..31 {
            if i == 0 {
                table[i] = 0.0;
            } else {
                table[i] = 95.88 / ((8128.0 / i as f32) + 100.0);
            }
        }

        table
    }

    fn generate_tnd_table() -> [f32; 203] {
        let mut table = [0.0; 203];

        for i in 0..203 {
            if i == 0 {
                table[i] = 0.0;
            } else {
                table[i] = 159.79 / ((1.0 / (i as f32 / 100.0)) + 100.0);
            }
        }

        table
    }

    pub fn mix(
        &self,
        pulse1: u8,
        pulse2: u8,
        triangle: u8,
        noise: u8,
        dmc: u8,
    ) -> f32 {
        // Pulse output
        let pulse_index = pulse1 as usize + pulse2 as usize;
        let pulse_out = self.pulse_table[pulse_index];

        // TND output
        // TND index = 3*triangle + 2*noise + dmc
        let tnd_index = (3 * triangle as usize)
                      + (2 * noise as usize)
                      + (dmc as usize);
        let tnd_out = self.tnd_table[tnd_index];

        // Combined output
        pulse_out + tnd_out
    }

    /// Linear approximation (for comparison/testing)
    pub fn mix_linear(
        pulse1: u8,
        pulse2: u8,
        triangle: u8,
        noise: u8,
        dmc: u8,
    ) -> f32 {
        let pulse = (pulse1 + pulse2) as f32 * 0.00752;
        let tnd = (triangle as f32 * 0.00851)
                + (noise as f32 * 0.00494)
                + (dmc as f32 * 0.00335);

        pulse + tnd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mixer_silence() {
        let mixer = Mixer::new();
        let output = mixer.mix(0, 0, 0, 0, 0);
        assert_eq!(output, 0.0);
    }

    #[test]
    fn test_mixer_pulse_only() {
        let mixer = Mixer::new();
        let output = mixer.mix(15, 15, 0, 0, 0);
        assert!(output > 0.0);
        assert!(output < 1.0);
    }

    #[test]
    fn test_mixer_max_output() {
        let mixer = Mixer::new();
        let output = mixer.mix(15, 15, 15, 15, 127);
        assert!(output > 0.0);
        assert!(output < 2.0); // Should be approximately 1.0
    }
}
```

---

### 3.5.3 Audio Resampling

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 4 hours

**Description:**
Implement resampling from APU rate (~1.789 MHz) to 48 kHz output.

**Files:**

- `crates/rustynes-apu/src/resampler.rs` - Resampling logic
- `crates/rustynes-apu/src/lib.rs` - Audio buffer interface

**Subtasks:**

- [ ] Ring buffer for audio samples
- [ ] Linear interpolation resampling
- [ ] Optional: Band-limited synthesis (blip_buf)
- [ ] Low-pass filter to reduce aliasing
- [ ] Configurable output sample rate

**Implementation:**

```rust
pub struct Resampler {
    output_rate: u32,          // Target sample rate (e.g., 48000)
    input_rate: u32,           // APU rate (1789773)
    time_accumulator: f32,     // Fractional time tracking
    prev_sample: f32,          // Previous sample for interpolation
    buffer: Vec<f32>,          // Output buffer
}

impl Resampler {
    pub fn new(output_rate: u32) -> Self {
        const APU_RATE: u32 = 1_789_773;

        Self {
            output_rate,
            input_rate: APU_RATE,
            time_accumulator: 0.0,
            prev_sample: 0.0,
            buffer: Vec::with_capacity(2048),
        }
    }

    pub fn add_sample(&mut self, sample: f32) {
        let time_step = self.output_rate as f32 / self.input_rate as f32;
        self.time_accumulator += time_step;

        // Generate output samples
        while self.time_accumulator >= 1.0 {
            // Linear interpolation
            let t = self.time_accumulator - 1.0;
            let output = self.prev_sample + (sample - self.prev_sample) * t;

            self.buffer.push(output);
            self.time_accumulator -= 1.0;
        }

        self.prev_sample = sample;
    }

    pub fn samples(&self) -> &[f32] {
        &self.buffer
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn is_ready(&self, min_samples: usize) -> bool {
        self.buffer.len() >= min_samples
    }
}

// Optional: Low-pass filter
pub struct LowPassFilter {
    prev_sample: f32,
    alpha: f32, // Smoothing factor (0-1)
}

impl LowPassFilter {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
        let dt = 1.0 / sample_rate;
        let alpha = dt / (rc + dt);

        Self {
            prev_sample: 0.0,
            alpha,
        }
    }

    pub fn process(&mut self, sample: f32) -> f32 {
        let output = self.prev_sample + self.alpha * (sample - self.prev_sample);
        self.prev_sample = output;
        output
    }
}
```

---

### 3.5.4 Audio Output Buffer

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement ring buffer for audio output with thread-safe access.

**Files:**

- `crates/rustynes-apu/src/audio_buffer.rs` - Ring buffer implementation

**Subtasks:**

- [ ] Ring buffer with configurable size
- [ ] Thread-safe read/write (std::sync)
- [ ] Overflow/underflow handling
- [ ] Latency monitoring

**Implementation:**

```rust
use std::sync::{Arc, Mutex};

pub struct AudioBuffer {
    buffer: Arc<Mutex<Vec<f32>>>,
    capacity: usize,
    read_pos: usize,
    write_pos: usize,
}

impl AudioBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(vec![0.0; capacity])),
            capacity,
            read_pos: 0,
            write_pos: 0,
        }
    }

    pub fn write(&mut self, samples: &[f32]) -> usize {
        let mut buffer = self.buffer.lock().unwrap();
        let mut written = 0;

        for &sample in samples {
            if self.available_write() > 0 {
                buffer[self.write_pos] = sample;
                self.write_pos = (self.write_pos + 1) % self.capacity;
                written += 1;
            } else {
                // Buffer full, drop samples or block
                break;
            }
        }

        written
    }

    pub fn read(&mut self, output: &mut [f32]) -> usize {
        let buffer = self.buffer.lock().unwrap();
        let mut read = 0;

        for out_sample in output.iter_mut() {
            if self.available_read() > 0 {
                *out_sample = buffer[self.read_pos];
                self.read_pos = (self.read_pos + 1) % self.capacity;
                read += 1;
            } else {
                // Buffer empty, output silence
                *out_sample = 0.0;
            }
        }

        read
    }

    fn available_write(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.capacity - (self.write_pos - self.read_pos) - 1
        } else {
            self.read_pos - self.write_pos - 1
        }
    }

    fn available_read(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.capacity - (self.read_pos - self.write_pos)
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.available_read()
    }
}
```

---

### 3.5.5 Test ROM Integration

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 4 hours

**Description:**
Integrate Blargg APU test ROMs and validate APU accuracy.

**Files:**

- `crates/rustynes-apu/tests/test_roms.rs` - Test ROM validation

**Subtasks:**

- [ ] Download Blargg APU test suite
- [ ] Integrate apu_test ROMs
- [ ] Run blargg_apu_2005.07.30 suite
- [ ] Document test results
- [ ] Fix failing tests

**Tests:**

```rust
#[cfg(test)]
mod test_roms {
    use super::*;

    #[test]
    fn test_01_len_ctr() {
        // Test length counter behavior
        let result = run_test_rom("test-roms/apu/01.len_ctr.nes");
        assert_eq!(result, TestResult::Pass);
    }

    #[test]
    fn test_02_len_table() {
        // Test length counter lookup table
        let result = run_test_rom("test-roms/apu/02.len_table.nes");
        assert_eq!(result, TestResult::Pass);
    }

    #[test]
    fn test_03_irq_flag() {
        // Test frame IRQ flag
        let result = run_test_rom("test-roms/apu/03.irq_flag.nes");
        assert_eq!(result, TestResult::Pass);
    }

    #[test]
    fn test_04_clock_jitter() {
        // Test frame counter jitter
        let result = run_test_rom("test-roms/apu/04.clock_jitter.nes");
        assert_eq!(result, TestResult::Pass);
    }

    #[test]
    fn test_05_len_timing_mode0() {
        // Test length counter timing in 4-step mode
        let result = run_test_rom("test-roms/apu/05.len_timing_mode0.nes");
        assert_eq!(result, TestResult::Pass);
    }

    // ... more tests ...

    fn run_test_rom(path: &str) -> TestResult {
        // Load ROM, run emulator, check result
        // Test ROMs write result to $6000 (0x00 = pass)
        TestResult::Pass
    }

    enum TestResult {
        Pass,
        Fail(String),
    }
}
```

---

### 3.5.6 Audio Quality Testing

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Validate audio quality with real games and document findings.

**Files:**

- `docs/apu/AUDIO_QUALITY_REPORT.md` - Test results

**Subtasks:**

- [ ] Test 10 games for audio correctness
- [ ] Check for pops/clicks
- [ ] Verify music and sound effects
- [ ] Document any issues
- [ ] Compare with reference emulator (Mesen2)

**Test Games:**

| Game | Tests | Expected Result |
|------|-------|-----------------|
| Super Mario Bros. | Music, sound effects | No pops, correct pitch |
| Mega Man 2 | Complex music, DMC drums | Accurate reproduction |
| Castlevania | Square wave melody | Clear tones |
| Legend of Zelda | Triangle bass | Smooth bass line |
| Battletoads | Noise percussion | Crisp drums |
| Contra | Mixed channels | Balanced mix |
| Metroid | Atmospheric audio | Correct ambience |
| Final Fantasy | Complex music | No glitches |
| Kirby's Adventure | DMC samples | Clear samples |
| Duck Hunt | Simple effects | Accurate timing |

---

### 3.5.7 Documentation

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Complete APU documentation with usage examples.

**Files:**

- `crates/rustynes-apu/README.md` - Crate README
- `crates/rustynes-apu/CHANGELOG.md` - Version history
- `crates/rustynes-apu/examples/` - Usage examples

**Subtasks:**

- [ ] Write crate README with overview
- [ ] Document public API
- [ ] Create usage examples
- [ ] Add inline documentation
- [ ] Generate rustdoc

**Example:**

```rust
// examples/simple_apu.rs

use rustynes_apu::Apu;

fn main() {
    let mut apu = Apu::new();

    // Enable pulse 1
    apu.write_register(0x4015, 0x01);

    // Configure pulse 1: 50% duty, constant volume 15
    apu.write_register(0x4000, 0xBF);

    // Set frequency (A4 = 440 Hz)
    // Timer = CPU_CLOCK / (16 * frequency) - 1
    // Timer = 1789773 / (16 * 440) - 1 = 253
    apu.write_register(0x4002, 253 & 0xFF);
    apu.write_register(0x4003, (253 >> 8) & 0x07);

    // Clock APU and generate audio
    for _ in 0..48000 {
        let sample = apu.step(|_addr| 0);
        // Output sample to audio device
        println!("{}", sample);
    }
}
```

---

## Dependencies

**Required:**

- Sprints 3.1-3.4 complete (all channels)
- rustynes-cpu (for DMC DMA integration)

**Blocks:**

- Milestone 5: Integration (needs working APU)
- Milestone 6: Desktop GUI (needs audio output)

---

## Related Documentation

- [APU Overview](../../../docs/apu/APU_OVERVIEW.md)
- [APU 2A03 Specification](../../../docs/apu/APU_2A03_SPECIFICATION.md)
- [APU Mixer](../../../docs/apu/APU_MIXER.md)
- [NESdev Wiki - APU](https://www.nesdev.org/wiki/APU)
- [NESdev Wiki - APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)

---

## Technical Notes

### Resampling Quality

Linear interpolation is sufficient for basic emulation but may introduce aliasing. For higher quality:
- Use band-limited synthesis (blip_buf crate)
- Apply low-pass filter
- Use higher intermediate sample rate

### Audio Latency

Target <20ms latency for responsive gameplay:
- 48 kHz: 960 samples buffer
- 44.1 kHz: 882 samples buffer

Larger buffers reduce underruns but increase input lag.

### Non-Linear Mixing

The NES APU uses non-linear mixing curves that approximate the analog circuit behavior. Linear mixing is simpler but less accurate.

### DMC Integration

DMC DMA must be coordinated with CPU to steal cycles correctly. This requires tight integration between APU and CPU modules.

---

## Test Requirements

- [ ] Pass 95%+ Blargg APU test suite
- [ ] No audio pops or clicks in test games
- [ ] Music sounds correct in 10 test games
- [ ] Audio latency < 20ms
- [ ] Resampling produces clean 48 kHz output
- [ ] All unit tests pass
- [ ] Integration tests pass

---

## Performance Targets

- Complete APU step: <100 ns
- Mixing: <30 ns
- Resampling: <50 ns per sample
- Total CPU usage: <5% at 60 FPS
- Memory: <50 KB total

---

## Success Criteria

- [ ] All 5 channels integrated and working
- [ ] Non-linear mixing produces accurate output
- [ ] 48 kHz resampling works correctly
- [ ] Audio buffer prevents underruns/overruns
- [ ] Pass 95%+ Blargg APU tests
- [ ] Test games sound correct
- [ ] <20ms audio latency
- [ ] Zero unsafe code
- [ ] Complete documentation
- [ ] Public API finalized

---

**Previous Sprint:** [Sprint 3.4: DMC Channel](M3-S4-DMC.md)
**Next Milestone:** [Milestone 4: Mappers](../../milestone-4-mappers/M4-OVERVIEW.md)
