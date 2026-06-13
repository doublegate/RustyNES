# [Milestone 6] Sprint 6.3: Audio Output

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer
**Sprint:** M6-S3 (GUI - Audio System)
**Progress:** 0%

---

## Overview

This sprint implements **audio output** for the desktop frontend, enabling playback of APU-generated audio samples through the system audio device. This includes ring buffer management, resampling, volume control, and low-latency playback.

### Goals

- ⏳ Audio backend (cpal cross-platform audio)
- ⏳ Ring buffer for APU samples
- ⏳ Resampling (1.789 MHz APU → 48 kHz output)
- ⏳ Audio callback integration
- ⏳ Volume control
- ⏳ Mute toggle
- ⏳ <20ms latency
- ⏳ No crackling or popping
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M5 Complete (APU produces audio samples)
- ✅ Console audio buffer API
- ✅ M6-S1 Application structure

---

## Tasks

### Task 1: Audio Backend Setup (2 hours)

**File:** `crates/rustynes-desktop/src/audio.rs`

**Objective:** Initialize cpal audio output device and stream.

#### Subtasks

1. Add cpal dependency to Cargo.toml
2. Create `AudioOutput` struct
3. Initialize audio device (48 kHz, stereo)
4. Create output stream
5. Handle device enumeration and errors

**Acceptance Criteria:**

- [ ] Audio device initializes successfully
- [ ] Works on Linux (ALSA/PulseAudio), Windows (WASAPI), macOS (CoreAudio)
- [ ] Graceful fallback if no audio device
- [ ] Clean error messages

**Dependencies:**

```toml
# Add to crates/rustynes-desktop/Cargo.toml

[dependencies]
cpal = "0.15"
ringbuf = "0.3"
dasp = "0.11"  # Digital audio signal processing (resampling)
```

**Implementation:**

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig, SampleFormat};
use ringbuf::{HeapRb, Producer, Consumer};
use std::sync::{Arc, Mutex};

/// Audio output system
pub struct AudioOutput {
    /// Audio stream (keeps device open)
    _stream: Stream,

    /// Sample producer (write from emulator thread)
    producer: Arc<Mutex<Producer<f32, Arc<HeapRb<f32>>>>>,

    /// Volume (0.0 to 1.0)
    volume: Arc<Mutex<f32>>,

    /// Mute state
    muted: Arc<Mutex<bool>>,

    /// Sample rate
    sample_rate: u32,
}

impl AudioOutput {
    /// Create new audio output system
    pub fn new() -> Result<Self, AudioError> {
        log::info!("Initializing audio output...");

        // Get default audio host
        let host = cpal::default_host();
        log::info!("Audio host: {:?}", host.id());

        // Get default output device
        let device = host.default_output_device()
            .ok_or(AudioError::NoDevice)?;

        log::info!("Audio device: {:?}", device.name());

        // Get default output config
        let config = device.default_output_config()
            .map_err(|e| AudioError::ConfigError(e.to_string()))?;

        log::info!("Audio config: {:?}", config);

        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        // Create ring buffer (4096 samples = ~85ms at 48 kHz)
        let ring_buffer = HeapRb::<f32>::new(4096);
        let (producer, consumer) = ring_buffer.split();

        let producer = Arc::new(Mutex::new(producer));
        let consumer = Arc::new(Mutex::new(consumer));

        // Shared state
        let volume = Arc::new(Mutex::new(1.0));
        let muted = Arc::new(Mutex::new(false));

        // Clone for audio callback
        let consumer_clone = Arc::clone(&consumer);
        let volume_clone = Arc::clone(&volume);
        let muted_clone = Arc::clone(&muted);

        // Build output stream
        let stream = match config.sample_format() {
            SampleFormat::F32 => Self::build_stream::<f32>(
                &device,
                &config.into(),
                consumer_clone,
                volume_clone,
                muted_clone,
                channels,
            )?,
            SampleFormat::I16 => Self::build_stream::<i16>(
                &device,
                &config.into(),
                consumer_clone,
                volume_clone,
                muted_clone,
                channels,
            )?,
            SampleFormat::U16 => Self::build_stream::<u16>(
                &device,
                &config.into(),
                consumer_clone,
                volume_clone,
                muted_clone,
                channels,
            )?,
            _ => return Err(AudioError::UnsupportedFormat),
        };

        // Start playback
        stream.play()
            .map_err(|e| AudioError::PlaybackError(e.to_string()))?;

        log::info!("Audio output initialized successfully");

        Ok(Self {
            _stream: stream,
            producer,
            volume,
            muted,
            sample_rate,
        })
    }

    fn build_stream<T>(
        device: &Device,
        config: &StreamConfig,
        consumer: Arc<Mutex<Consumer<f32, Arc<HeapRb<f32>>>>>,
        volume: Arc<Mutex<f32>>,
        muted: Arc<Mutex<bool>>,
        channels: usize,
    ) -> Result<Stream, AudioError>
    where
        T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
    {
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut consumer = consumer.lock().unwrap();
                let volume = *volume.lock().unwrap();
                let muted = *muted.lock().unwrap();

                for frame in data.chunks_mut(channels) {
                    // Pop sample from ring buffer
                    let sample = consumer.pop().unwrap_or(0.0);

                    // Apply volume and mute
                    let output = if muted { 0.0 } else { sample * volume };

                    // Write to all channels (mono → stereo/multi-channel)
                    for channel in frame.iter_mut() {
                        *channel = cpal::Sample::from_sample(output);
                    }
                }
            },
            |err| {
                log::error!("Audio stream error: {}", err);
            },
            None, // No timeout
        ).map_err(|e| AudioError::StreamCreationError(e.to_string()))?;

        Ok(stream)
    }

    /// Queue audio samples from emulator
    ///
    /// # Arguments
    ///
    /// * `samples` - f32 mono samples from APU (normalized -1.0 to 1.0)
    pub fn queue_samples(&self, samples: &[f32]) {
        let mut producer = self.producer.lock().unwrap();

        for &sample in samples {
            // Drop samples if buffer full (prevents audio thread blocking)
            let _ = producer.push(sample);
        }
    }

    /// Set volume (0.0 to 1.0)
    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        *self.volume.lock().unwrap() = clamped;
        log::debug!("Volume set to {:.2}", clamped);
    }

    /// Get current volume
    pub fn volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }

    /// Set mute state
    pub fn set_muted(&self, muted: bool) {
        *self.muted.lock().unwrap() = muted;
        log::info!("Audio {}", if muted { "muted" } else { "unmuted" });
    }

    /// Get mute state
    pub fn is_muted(&self) -> bool {
        *self.muted.lock().unwrap()
    }

    /// Toggle mute
    pub fn toggle_mute(&self) {
        let current = self.is_muted();
        self.set_muted(!current);
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("No audio output device available")]
    NoDevice,

    #[error("Failed to get audio configuration: {0}")]
    ConfigError(String),

    #[error("Unsupported audio sample format")]
    UnsupportedFormat,

    #[error("Failed to create audio stream: {0}")]
    StreamCreationError(String),

    #[error("Audio playback error: {0}")]
    PlaybackError(String),
}
```

---

### Task 2: Resampling (3 hours)

**File:** `crates/rustynes-desktop/src/audio/resampler.rs`

**Objective:** Resample APU output (1.789 MHz) to audio device rate (48 kHz).

#### Subtasks

1. Implement linear interpolation resampler
2. Handle fractional sample positions
3. Maintain phase accuracy
4. Minimize audio artifacts

**Acceptance Criteria:**

- [ ] Clean resampling with minimal aliasing
- [ ] No audible artifacts
- [ ] Efficient (real-time performance)

**Implementation:**

```rust
/// Linear interpolation audio resampler
pub struct Resampler {
    /// Source sample rate (APU frequency)
    source_rate: f64,

    /// Target sample rate (audio device)
    target_rate: f64,

    /// Current fractional position
    position: f64,

    /// Step size per output sample
    step: f64,

    /// Previous sample (for interpolation)
    prev_sample: f32,
}

impl Resampler {
    /// Create new resampler
    ///
    /// # Arguments
    ///
    /// * `source_rate` - Source sample rate (e.g., APU frequency)
    /// * `target_rate` - Target sample rate (e.g., 48000 Hz)
    pub fn new(source_rate: f64, target_rate: f64) -> Self {
        let step = source_rate / target_rate;

        Self {
            source_rate,
            target_rate,
            position: 0.0,
            step,
            prev_sample: 0.0,
        }
    }

    /// Resample input samples to output buffer
    ///
    /// # Arguments
    ///
    /// * `input` - Source samples
    ///
    /// # Returns
    ///
    /// Vector of resampled output samples
    pub fn resample(&mut self, input: &[f32]) -> Vec<f32> {
        let output_len = ((input.len() as f64) / self.step).ceil() as usize;
        let mut output = Vec::with_capacity(output_len);

        for &current_sample in input {
            // Emit output samples while position < 1.0
            while self.position < 1.0 {
                // Linear interpolation
                let t = self.position as f32;
                let interpolated = self.prev_sample * (1.0 - t) + current_sample * t;

                output.push(interpolated);

                self.position += self.step;
            }

            // Advance to next input sample
            self.position -= 1.0;
            self.prev_sample = current_sample;
        }

        output
    }

    /// Reset resampler state
    pub fn reset(&mut self) {
        self.position = 0.0;
        self.prev_sample = 0.0;
    }

    /// Get resampling ratio
    pub fn ratio(&self) -> f64 {
        self.source_rate / self.target_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_upsampling() {
        // 1 kHz → 2 kHz (upsample)
        let mut resampler = Resampler::new(1000.0, 2000.0);

        let input = vec![0.0, 1.0, 0.0, -1.0];
        let output = resampler.resample(&input);

        // Should produce ~8 samples (2x upsampling)
        assert!(output.len() >= 7 && output.len() <= 9);
    }

    #[test]
    fn test_resampler_downsampling() {
        // 2 kHz → 1 kHz (downsample)
        let mut resampler = Resampler::new(2000.0, 1000.0);

        let input = vec![0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5];
        let output = resampler.resample(&input);

        // Should produce ~4 samples (2x downsampling)
        assert!(output.len() >= 3 && output.len() <= 5);
    }
}
```

---

### Task 3: Application Integration (2 hours)

**File:** `crates/rustynes-desktop/src/app.rs`

**Objective:** Integrate audio output with emulator loop.

#### Subtasks

1. Initialize AudioOutput in app creation
2. Get audio samples from Console each frame
3. Resample to audio device rate
4. Queue samples to audio output
5. Handle audio errors gracefully

**Acceptance Criteria:**

- [ ] Audio plays during emulation
- [ ] Synchronized with video
- [ ] No stuttering or dropouts
- [ ] Graceful handling if no audio device

**Implementation:**

```rust
use crate::audio::{AudioOutput, Resampler};

pub struct RustyNesApp {
    // ... existing fields ...

    /// Audio output (None if initialization failed)
    audio: Option<AudioOutput>,

    /// Audio resampler
    resampler: Option<Resampler>,
}

impl RustyNesApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // ... existing initialization ...

        // Initialize audio (optional, graceful failure)
        let (audio, resampler) = match AudioOutput::new() {
            Ok(audio_output) => {
                // Create resampler: APU → Audio device
                // APU produces samples at CPU frequency (1.789773 MHz)
                let resampler = Resampler::new(
                    1_789_773.0,  // APU sample rate
                    audio_output.sample_rate() as f64,
                );

                log::info!("Audio initialized: {} Hz", audio_output.sample_rate());
                (Some(audio_output), Some(resampler))
            }
            Err(e) => {
                log::warn!("Failed to initialize audio: {}", e);
                log::warn!("Continuing without audio");
                (None, None)
            }
        };

        Self {
            // ... existing fields ...
            audio,
            resampler,
        }
    }

    fn step_frame(&mut self) {
        if let Some(console) = &mut self.console {
            // Step emulator frame
            console.step_frame();

            // Update FPS counter
            self.fps_counter.tick();

            // Queue audio samples
            if let (Some(audio), Some(resampler)) = (&self.audio, &mut self.resampler) {
                // Get APU samples from console
                let apu_samples = console.audio_buffer();

                // Resample to audio device rate
                let resampled = resampler.resample(apu_samples);

                // Queue to audio output
                audio.queue_samples(&resampled);
            }
        }
    }
}
```

---

### Task 4: Audio Settings UI (2 hours)

**File:** `crates/rustynes-desktop/src/ui/audio_settings.rs`

**Objective:** Create audio settings UI for volume, mute, sample rate display.

#### Subtasks

1. Create audio settings window
2. Volume slider (0-100%)
3. Mute checkbox
4. Display audio device info
5. Latency indicator

**Acceptance Criteria:**

- [ ] Settings window opens from menu
- [ ] Volume control works in real-time
- [ ] Mute toggle responsive
- [ ] Audio info displayed correctly

**Implementation:**

```rust
use eframe::egui;
use crate::app::RustyNesApp;

impl RustyNesApp {
    pub fn show_audio_settings(&mut self, ctx: &egui::Context) {
        egui::Window::new("Audio Settings")
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(audio) = &self.audio {
                    // Volume control
                    ui.heading("Volume");

                    let mut volume = audio.volume();
                    if ui.add(egui::Slider::new(&mut volume, 0.0..=1.0)
                        .text("Volume")
                        .show_value(true))
                        .changed()
                    {
                        audio.set_volume(volume);
                    }

                    ui.label(format!("{}%", (volume * 100.0) as u32));

                    ui.separator();

                    // Mute toggle
                    let mut muted = audio.is_muted();
                    if ui.checkbox(&mut muted, "Mute").changed() {
                        audio.set_muted(muted);
                    }

                    ui.separator();

                    // Audio info
                    ui.heading("Audio Info");

                    ui.label(format!("Sample Rate: {} Hz", audio.sample_rate()));
                    ui.label("Channels: Stereo");
                    ui.label("Latency: ~85ms (4096 samples)");

                    ui.separator();

                    // APU info
                    ui.heading("APU Info");
                    ui.label("APU Frequency: 1.789773 MHz");
                    ui.label("Channels: 5 (Pulse 1/2, Triangle, Noise, DMC)");
                } else {
                    ui.colored_label(egui::Color32::RED, "Audio unavailable");
                    ui.label("No audio output device detected");
                }
            });
    }

    /// Add mute hotkey (M key)
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // ... existing shortcuts ...

        // M: Toggle mute
        if ctx.input_mut(|i| i.key_pressed(egui::Key::M)) {
            if let Some(audio) = &self.audio {
                audio.toggle_mute();
            }
        }
    }
}
```

---

### Task 5: Audio Menu Integration (1 hour)

**File:** `crates/rustynes-desktop/src/ui/menu_bar.rs`

**Objective:** Add audio menu items.

#### Subtasks

1. Add Audio submenu
2. Mute/Unmute toggle
3. Volume increase/decrease shortcuts
4. Audio settings window trigger

**Acceptance Criteria:**

- [ ] Audio menu accessible
- [ ] Keyboard shortcuts work
- [ ] Settings window opens

**Implementation:**

```rust
impl RustyNesApp {
    fn audio_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Audio", |ui| {
            if let Some(audio) = &self.audio {
                // Mute toggle
                let mute_text = if audio.is_muted() {
                    "Unmute"
                } else {
                    "Mute"
                };

                if ui.add(egui::Button::new(mute_text)
                    .shortcut_text("M"))
                    .clicked()
                {
                    audio.toggle_mute();
                    ui.close_menu();
                }

                ui.separator();

                // Volume controls
                if ui.add(egui::Button::new("Volume Up")
                    .shortcut_text("Ctrl+="))
                    .clicked()
                {
                    let new_volume = (audio.volume() + 0.1).min(1.0);
                    audio.set_volume(new_volume);
                }

                if ui.add(egui::Button::new("Volume Down")
                    .shortcut_text("Ctrl+-"))
                    .clicked()
                {
                    let new_volume = (audio.volume() - 0.1).max(0.0);
                    audio.set_volume(new_volume);
                }

                ui.separator();

                // Audio settings
                if ui.button("Audio Settings...").clicked() {
                    self.show_audio_settings_window = true;
                    ui.close_menu();
                }
            } else {
                ui.label("Audio unavailable");
            }
        });
    }

    pub fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            self.file_menu(ui);
            self.emulation_menu(ui);
            self.audio_menu(ui);   // Add audio menu
            self.settings_menu(ui);
            self.help_menu(ui);
        });
    }
}
```

---

### Task 6: Performance Tuning (2 hours)

**File:** `crates/rustynes-desktop/src/audio.rs` (optimization)

**Objective:** Minimize audio latency and prevent dropouts.

#### Subtasks

1. Tune ring buffer size (balance latency vs. stability)
2. Profile resampling performance
3. Test on various audio devices
4. Handle buffer underruns gracefully

**Acceptance Criteria:**

- [ ] Latency <20ms
- [ ] No crackling or popping
- [ ] Stable on all platforms
- [ ] Works with USB audio devices

**Optimization Notes:**

```rust
// Ring buffer sizing:
// - Too small: Buffer underruns, crackling
// - Too large: High latency

// Recommended sizes:
// - 2048 samples @ 48 kHz = ~42ms latency
// - 4096 samples @ 48 kHz = ~85ms latency
// - 8192 samples @ 48 kHz = ~170ms latency

// Target: 4096 samples (~85ms) for balance

// Buffer underrun handling:
impl AudioOutput {
    fn audio_callback(consumer: &mut Consumer<f32>, data: &mut [f32]) {
        for sample in data.iter_mut() {
            // Pop sample, or output silence if buffer empty
            *sample = consumer.pop().unwrap_or(0.0);
        }

        // Log underruns for debugging
        if consumer.is_empty() {
            log::warn!("Audio buffer underrun");
        }
    }
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Audio plays during emulation
- [ ] Volume control works (0-100%)
- [ ] Mute toggle works
- [ ] Audio synchronized with video
- [ ] Settings persist (Sprint 5)
- [ ] Works on Linux, Windows, macOS

### Quality

- [ ] Latency <20ms
- [ ] No crackling or popping
- [ ] No audible aliasing artifacts
- [ ] Clean resampling
- [ ] Smooth playback at 60 FPS

### User Experience

- [ ] Audio controls accessible
- [ ] Keyboard shortcuts work
- [ ] Graceful fallback if no audio device
- [ ] Clear error messages

---

## Dependencies

### External Crates

```toml
cpal = "0.15"      # Cross-platform audio I/O
ringbuf = "0.3"    # Lock-free ring buffer
dasp = "0.11"      # Digital audio signal processing
```

### Internal Dependencies

- rustynes-core (Console audio buffer API)
- rustynes-apu (generates audio samples)

---

## Related Documentation

- [APU_OVERVIEW.md](../../../docs/apu/APU_OVERVIEW.md) - APU architecture
- [APU_2A03_SPECIFICATION.md](../../../docs/apu/APU_2A03_SPECIFICATION.md) - APU technical details
- [M6-S2-wgpu-rendering.md](M6-S2-wgpu-rendering.md) - Video rendering

---

## Technical Notes

### APU Sample Rate

The NES APU produces one audio sample per CPU cycle:

- CPU frequency: 1.789773 MHz (NTSC)
- APU sample rate: 1.789773 MHz

This requires downsampling to standard audio rates (44.1 kHz or 48 kHz).

### Resampling Quality

**Linear interpolation** is sufficient for real-time NES audio:

- Simple, efficient (minimal CPU usage)
- Acceptable quality for 8-bit audio
- No significant aliasing artifacts

**Higher-quality options** (optional, future):

- Cubic interpolation
- Sinc resampling (best quality, high CPU cost)

### Buffer Sizing

Ring buffer size affects latency/stability trade-off:

| Size (samples @ 48 kHz) | Latency | Stability |
|-------------------------|---------|-----------|
| 1024 | ~21ms | Prone to underruns |
| 2048 | ~43ms | Good for most systems |
| 4096 | ~85ms | Excellent stability |
| 8192 | ~170ms | High latency, very stable |

**Recommendation:** 4096 samples for balance.

### Platform Notes

- **Linux:** Supports ALSA, PulseAudio, JACK
- **Windows:** WASAPI (low-latency)
- **macOS:** CoreAudio (excellent latency)

---

## Performance Targets

- **Latency:** <20ms (target), <100ms (acceptable)
- **CPU Usage:** <5% for audio processing
- **Resampling Time:** <0.1ms per frame
- **Buffer Underruns:** 0 per minute during normal playback

---

## Success Criteria

- [ ] All tasks complete
- [ ] Audio plays cleanly
- [ ] Latency targets met
- [ ] Volume/mute controls work
- [ ] Works on all platforms
- [ ] Graceful degradation if no audio
- [ ] Ready for controller integration (Sprint 4)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1 (Application shell)
**Next Sprint:** [M6-S4 Controller Support](M6-S4-controller-support.md)
