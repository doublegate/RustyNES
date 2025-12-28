# M9 Sprint 1: Audio Improvements

## Overview

Implement dynamic resampling, audio/video synchronization, and buffer optimization to achieve high-quality audio matching reference emulators.

## Current Implementation (v0.7.1)

The desktop frontend already has a functional audio system using cpal 0.15:

**Completed:**
- [x] cpal 0.15 integration for cross-platform audio I/O
- [x] Lock-free ring buffer (8192 samples) with atomic operations
- [x] Volume control via atomic f32 (stored as u32 bits)
- [x] Mute functionality via AtomicBool
- [x] Mono-to-stereo conversion in audio callback
- [x] Configurable sample rate in AudioConfig (default: 44.1kHz)
- [x] Buffer size configuration in AudioConfig (default: 2048)

**Location:** `crates/rustynes-desktop/src/audio.rs`

## Objectives

- [ ] Implement dynamic resampling (NES APU rate → device sample rate)
- [ ] Add audio/video synchronization (prevent drift)
- [x] ~~Use lock-free ring buffer~~ (implemented in v0.7.1)
- [ ] Reduce latency (target <100ms, ideally ~50ms)
- [ ] Fix audio pops and glitches under load
- [ ] Validate audio quality against Mesen2

## Tasks

### Task 1: Dynamic Resampling
- [ ] Integrate rubato crate for high-quality sinc interpolation
- [ ] Configure resampler for NES APU output rate (derived from CPU clock)
- [ ] Handle NTSC (1.789773 MHz / 40 = ~44.7kHz) and PAL rates
- [ ] Resample to device sample rate (typically 44.1kHz or 48kHz)
- [ ] Test with different games (ensure no aliasing artifacts)
- [ ] Benchmark resampling overhead (should be minimal)

### Task 2: Audio/Video Synchronization
- [ ] Add buffer fill level monitoring to AudioOutput
- [ ] Implement adaptive emulation speed in app.rs update loop
- [ ] Speed up (1.01x) when buffer >80% full
- [ ] Slow down (0.99x) when buffer <20% full
- [x] ~~Handle buffer underrun gracefully~~ (current impl fills with silence)
- [ ] Handle buffer overflow gracefully (drop oldest samples)
- [ ] Test with long gameplay sessions (no drift over 30+ minutes)

### Task 3: Buffer Management Optimization
- [ ] Consider replacing custom RingBuffer with ringbuf crate
- [ ] Implement adaptive buffer sizing based on system latency
- [ ] Reduce buffer latency (target <100ms, ideally ~50ms)
- [x] ~~Use lock-free ring buffer~~ (implemented with atomics)
- [ ] Profile buffer operations with cargo flamegraph
- [ ] Test with high-load scenarios (streaming, background apps)

### Task 4: Fix Audio Glitches
- [ ] Identify sources of pops/clicks (buffer underrun, overflow, resampling artifacts)
- [ ] Smooth transitions (fade in/out on buffer changes)
- [ ] Test with games known for audio edge cases (Mega Man, Castlevania)
- [ ] Validate mixer output (ensure no clipping)
- [ ] Compare audio quality to Mesen2 (record samples, compare waveforms)

## Implementation Details

### Current Audio Architecture (v0.7.1)

```rust
// crates/rustynes-desktop/src/audio.rs
pub struct AudioOutput {
    _stream: Stream,                           // cpal audio stream (kept alive)
    buffer: Arc<std::sync::Mutex<RingBuffer>>, // Custom ring buffer
    volume: Arc<AtomicU32>,                    // Volume as f32 bits
    muted: Arc<AtomicBool>,                    // Mute state
    sample_rate: u32,                          // Device sample rate
}

// Custom lock-free ring buffer (8192 samples)
struct RingBuffer {
    buffer: Box<[f32; RING_BUFFER_SIZE]>,
    read_pos: AtomicU32,
    write_pos: AtomicU32,
}
```

### Resampling Algorithm

**Options:**
1. **Linear Interpolation** - Fast, simple, some aliasing
2. **Cubic Interpolation** - Better quality, moderate cost
3. **Sinc Interpolation** - Best quality, highest cost

**Recommendation:** Use rubato with sinc interpolation for best quality.

**Library:** [rubato](https://github.com/HEnquist/rubato) - High-quality Rust resampling library

```rust
use rubato::{SincFixedIn, SincInterpolationType, SincInterpolationParameters, Resampler};

struct AudioResampler {
    resampler: SincFixedIn<f32>,
    input_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
}

impl AudioResampler {
    fn new(input_rate: f32, output_rate: f32) -> Self {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: rubato::WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::<f32>::new(
            output_rate as f64 / input_rate as f64,
            2.0,
            params,
            1024, // chunk size
            1,    // mono
        ).unwrap();

        Self {
            resampler,
            input_buffer: vec![vec![0.0; 1024]],
            output_buffer: vec![vec![0.0; 2048]],
        }
    }

    fn process(&mut self, input: &[f32]) -> &[f32] {
        self.input_buffer[0].copy_from_slice(input);
        let (_, out_len) = self.resampler
            .process_into_buffer(&self.input_buffer, &mut self.output_buffer, None)
            .unwrap();
        &self.output_buffer[0][..out_len]
    }
}
```

### Audio/Video Sync Integration

**Strategy for eframe integration:**
1. Track audio buffer fill level via `AudioOutput::buffer_available()`
2. Adjust frame duration in accumulator-based timing loop
3. Maintain sync without noticeable speed changes

```rust
// In app.rs update loop
impl NesApp {
    fn adjust_frame_timing(&mut self) -> Duration {
        let fill_level = if let Some(ref audio) = self.audio {
            audio.buffer_available() as f32 / RING_BUFFER_SIZE as f32
        } else {
            0.5 // Default to 50% if no audio
        };

        let speed_factor = if fill_level > 0.8 {
            1.01 // Buffer full, speed up slightly
        } else if fill_level < 0.2 {
            0.99 // Buffer empty, slow down slightly
        } else {
            1.0
        };

        Duration::from_nanos((FRAME_DURATION.as_nanos() as f64 / speed_factor) as u64)
    }
}
```

### Alternative: ringbuf Crate (tetanes pattern)

The tetanes project uses the `ringbuf` crate for more sophisticated buffer management:

```rust
use ringbuf::{traits::*, HeapRb, CachingProd, CachingCons};

struct AudioBuffer {
    producer: CachingProd<Arc<HeapRb<f32>>>,
    consumer: CachingCons<Arc<HeapRb<f32>>>,
}

impl AudioBuffer {
    fn new(capacity: usize) -> Self {
        let rb = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = rb.split();
        Self {
            producer: CachingProd::new(producer),
            consumer: CachingCons::new(consumer),
        }
    }

    fn push_slice(&mut self, samples: &[f32]) -> usize {
        self.producer.push_slice(samples)
    }

    fn pop_slice(&mut self, output: &mut [f32]) -> usize {
        self.consumer.pop_slice(output)
    }

    fn len(&self) -> usize {
        self.producer.occupied_len()
    }
}
```

**Advantages of ringbuf:**
- Caching variants reduce atomic overhead
- Slice-based operations for batch processing
- Well-tested library (used by tetanes)

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
