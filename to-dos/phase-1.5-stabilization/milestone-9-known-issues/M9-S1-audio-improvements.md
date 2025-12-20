# M9 Sprint 1: Audio Improvements

## Overview

Implement dynamic resampling, audio/video synchronization, and buffer optimization to achieve high-quality audio matching reference emulators.

## Objectives

- [ ] Implement dynamic resampling (NES ~1.79MHz → 48kHz output)
- [ ] Add audio/video synchronization (prevent drift)
- [ ] Optimize buffer management (reduce latency to <100ms)
- [ ] Fix audio pops and glitches
- [ ] Validate audio quality against Mesen2

## Tasks

### Task 1: Dynamic Resampling
- [ ] Research resampling algorithms (sinc, linear, cubic)
- [ ] Integrate resampling library (rubato or dasp)
- [ ] Implement variable rate input (NTSC 1.789773 MHz, PAL 1.662607 MHz)
- [ ] Target output rate: 48kHz (standard audio hardware)
- [ ] Test with different games (ensure no artifacts)

### Task 2: Audio/Video Synchronization
- [ ] Track audio buffer fill level
- [ ] Implement adaptive emulation speed (slow down/speed up to maintain sync)
- [ ] Handle buffer underrun gracefully (insert silence, don't crash)
- [ ] Handle buffer overflow gracefully (drop samples, don't crash)
- [ ] Test with long gameplay sessions (no drift over 30+ minutes)

### Task 3: Buffer Management Optimization
- [ ] Implement adaptive buffer sizing (based on system latency)
- [ ] Reduce buffer latency (target <100ms, ideally ~50ms)
- [ ] Use lock-free ring buffer (reduce contention)
- [ ] Profile buffer operations (measure overhead)
- [ ] Test with different audio backends (SDL2, cpal)

### Task 4: Fix Audio Glitches
- [ ] Identify sources of pops/clicks (buffer underrun, overflow, resampling artifacts)
- [ ] Smooth transitions (fade in/out on buffer changes)
- [ ] Test with games known for audio edge cases (Mega Man, Castlevania)
- [ ] Validate mixer output (ensure no clipping)
- [ ] Compare audio quality to Mesen2 (record samples, compare waveforms)

## Implementation Details

### Resampling Algorithm

**Options:**
1. **Linear Interpolation** - Fast, simple, some aliasing
2. **Cubic Interpolation** - Better quality, moderate cost
3. **Sinc Interpolation** - Best quality, highest cost

**Recommendation:** Start with linear, upgrade to sinc if quality issues persist.

**Library:** [rubato](https://github.com/HEnquist/rubato) - High-quality Rust resampling library

```rust
use rubato::{Resampler, SincFixedIn, InterpolationType};

fn resample_audio(input: &[f32], input_rate: f32, output_rate: f32) -> Vec<f32> {
    let resampler = SincFixedIn::<f32>::new(
        output_rate / input_rate,
        2.0,
        InterpolationType::Linear,
        256,
        1, // mono
    ).unwrap();

    resampler.process(&[input], None).unwrap()[0].clone()
}
```

### Audio/Video Sync

**Strategy:**
1. Track audio buffer fill level (samples queued)
2. If buffer too full (>80%): Speed up emulation slightly (1.01x)
3. If buffer too empty (<20%): Slow down emulation slightly (0.99x)
4. Target: 40-60% buffer fill (headroom for variance)

```rust
fn adjust_emulation_speed(&mut self) {
    let fill_level = self.audio_buffer.len() as f32 / self.audio_buffer.capacity() as f32;

    if fill_level > 0.8 {
        self.emulation_speed = 1.01; // Speed up
    } else if fill_level < 0.2 {
        self.emulation_speed = 0.99; // Slow down
    } else {
        self.emulation_speed = 1.0; // Normal
    }
}
```

### Buffer Management

**Lock-Free Ring Buffer:**
```rust
use ringbuf::{HeapRb, Producer, Consumer};

struct AudioBuffer {
    producer: Producer<f32, Arc<HeapRb<f32>>>,
    consumer: Consumer<f32, Arc<HeapRb<f32>>>,
}

impl AudioBuffer {
    fn new(capacity: usize) -> Self {
        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();
        Self { producer, consumer }
    }

    fn push(&mut self, sample: f32) -> bool {
        self.producer.try_push(sample).is_ok()
    }

    fn pop(&mut self) -> Option<f32> {
        self.producer.try_pop()
    }
}
```

## Test Cases

| Test | Description | Expected Result |
|------|-------------|-----------------|
| **Long Gameplay** | Play for 30+ minutes | No audio drift, no glitches |
| **Mega Man 2** | Test with complex music | No pops, accurate sound |
| **Castlevania** | Test with percussion | No clicks, accurate timing |
| **Super Mario Bros.** | Test with sound effects | No distortion, clean audio |
| **Buffer Underrun** | Simulate slow system | Graceful degradation (silence, not crash) |
| **Buffer Overflow** | Simulate fast system | Graceful handling (drop samples, not crash) |

## Acceptance Criteria

- [ ] Dynamic resampling implemented (NES rate → 48kHz)
- [ ] Audio/video sync working (<10ms drift over 30 minutes)
- [ ] Buffer latency <100ms (ideally ~50ms)
- [ ] Zero pops/glitches in normal gameplay
- [ ] Audio quality comparable to Mesen2
- [ ] Tested with 5+ different games
- [ ] No performance regression (maintain 100+ FPS)

## Known Issues to Fix

From v0.5.0 implementation report:

1. **No Dynamic Resampling** - Fixed by Task 1
2. **No Audio/Video Sync** - Fixed by Task 2
3. **Basic Buffer Management** - Fixed by Task 3
4. **Occasional Pops/Glitches** - Fixed by Task 4

## Libraries & Tools

| Library | Purpose | Link |
|---------|---------|------|
| **rubato** | High-quality resampling | [GitHub](https://github.com/HEnquist/rubato) |
| **dasp** | Digital signal processing | [GitHub](https://github.com/RustAudio/dasp) |
| **ringbuf** | Lock-free ring buffer | [crates.io](https://crates.io/crates/ringbuf) |
| **cpal** | Cross-platform audio | [GitHub](https://github.com/RustAudio/cpal) |

## Version Target

v0.8.0
