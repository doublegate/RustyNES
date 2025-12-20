//! Audio playback using cpal.
//!
//! Manages audio stream creation and sample buffering for NES audio output.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

/// Audio player wrapping cpal audio stream
pub struct AudioPlayer {
    /// Audio output stream (must be kept alive)
    _stream: Stream,
    /// Shared audio buffer for communicating samples from emulator to audio thread
    buffer: Arc<Mutex<Vec<f32>>>,
    /// Sample rate (Hz)
    sample_rate: u32,
}

impl AudioPlayer {
    /// Create new audio player with default device
    ///
    /// # Returns
    ///
    /// Audio player or error if audio device/stream creation failed
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - No audio output device is available
    /// - Audio stream cannot be created
    /// - Stream configuration is not supported
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Get default audio host
        let host = cpal::default_host();

        // Get default output device
        let device = host
            .default_output_device()
            .ok_or("No audio output device available")?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        info!("Using audio device: {}", device_name);

        // Get default output config
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        info!(
            "Audio config: {} Hz, {} channels, {:?}",
            sample_rate,
            channels,
            config.sample_format()
        );

        // Create shared buffer for audio samples
        // Increased capacity to reduce buffer underruns (735 samples/frame * 4 frames = ~3000)
        let buffer = Arc::new(Mutex::new(Vec::with_capacity(8192)));
        let buffer_clone = Arc::clone(&buffer);

        // Build audio stream
        let stream = Self::build_stream(&device, &config.into(), buffer_clone, channels)?;

        // Start playback
        stream.play()?;

        Ok(Self {
            _stream: stream,
            buffer,
            sample_rate,
        })
    }

    /// Build audio output stream
    fn build_stream(
        device: &Device,
        config: &StreamConfig,
        buffer: Arc<Mutex<Vec<f32>>>,
        channels: u16,
    ) -> Result<Stream, Box<dyn std::error::Error>> {
        let stream = device.build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Lock buffer and consume available samples
                let mut buf = buffer.lock().unwrap();

                if buf.is_empty() {
                    // No samples available - output silence
                    data.fill(0.0);
                    return;
                }

                // Fill output buffer with available samples
                let frames = data.len() / channels as usize;
                let samples_needed = frames.min(buf.len());

                for i in 0..samples_needed {
                    let sample = buf[i];
                    // Duplicate mono sample to all channels
                    for c in 0..channels as usize {
                        data[i * channels as usize + c] = sample;
                    }
                }

                // If we didn't fill the entire buffer, pad with silence
                if samples_needed < frames {
                    for item in data.iter_mut().skip(samples_needed * channels as usize) {
                        *item = 0.0;
                    }
                }

                // Remove consumed samples from buffer
                buf.drain(0..samples_needed);
            },
            move |err| {
                error!("Audio stream error: {}", err);
            },
            None,
        )?;

        Ok(stream)
    }

    /// Queue audio samples for playback
    ///
    /// # Arguments
    ///
    /// * `samples` - Audio samples to queue (mono, -1.0 to 1.0 range)
    ///
    /// # Buffer Management
    ///
    /// The buffer uses a soft limit and hard limit approach:
    /// - Soft limit (16384): Warning threshold for monitoring
    /// - Hard limit (24576): Maximum capacity before dropping samples
    ///
    /// This larger buffer accommodates timing variances between emulation
    /// frame generation (~60Hz) and audio hardware consumption rates.
    pub fn queue_samples(&self, samples: &[f32]) {
        const SOFT_LIMIT: usize = 16384; // ~370ms at 44.1kHz (warn but don't drop)
        const HARD_LIMIT: usize = 24576; // ~555ms at 44.1kHz (drop to prevent unbounded growth)

        if samples.is_empty() {
            return;
        }

        let mut buf = self.buffer.lock().unwrap();

        // Check buffer health
        if buf.len() > SOFT_LIMIT && buf.len() < HARD_LIMIT {
            // Log warning but don't drop samples yet
            if buf.len() % 4096 == 0 {
                // Only log every 4096 samples to reduce spam
                warn!(
                    "Audio buffer growing large: {} samples ({:.1}ms latency)",
                    buf.len(),
                    (buf.len() as f32 / self.sample_rate as f32) * 1000.0
                );
            }
        }

        // Prevent unbounded buffer growth (drop oldest samples if necessary)
        if buf.len() + samples.len() > HARD_LIMIT {
            let excess = (buf.len() + samples.len()) - HARD_LIMIT;
            let drop_count = excess.min(buf.len());

            // Only log if we're dropping a significant amount
            if drop_count > 0 {
                warn!(
                    "Audio buffer overflow, dropping {} samples to maintain latency",
                    drop_count
                );
            }

            buf.drain(0..drop_count);
        }

        // Add new samples
        buf.extend_from_slice(samples);
    }

    /// Get audio sample rate
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get current buffer size (for debugging)
    #[must_use]
    #[allow(dead_code)]
    pub fn buffer_size(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new().expect("Failed to create audio player")
    }
}
