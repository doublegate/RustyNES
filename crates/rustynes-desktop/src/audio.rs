//! Audio output using cpal for low-latency playback.
//!
//! This module provides a thread-safe audio output system that:
//! - Uses a ring buffer for lock-free sample transfer
//! - Handles buffer underruns gracefully with silence
//! - Supports dynamic volume control
//! - Provides mute functionality

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleRate, Stream, StreamConfig};
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// Size of the ring buffer in samples (mono).
const RING_BUFFER_SIZE: usize = 8192;

/// Thread-safe ring buffer for audio samples.
struct RingBuffer {
    buffer: Box<[f32; RING_BUFFER_SIZE]>,
    read_pos: AtomicU32,
    write_pos: AtomicU32,
}

impl RingBuffer {
    fn new() -> Self {
        // Use vec! to avoid large stack allocation, then convert to boxed slice and array
        let buffer_vec: Vec<f32> = vec![0.0; RING_BUFFER_SIZE];
        let buffer_slice: Box<[f32]> = buffer_vec.into_boxed_slice();
        // SAFETY: We know the Vec was exactly RING_BUFFER_SIZE elements
        let buffer: Box<[f32; RING_BUFFER_SIZE]> =
            buffer_slice.try_into().expect("buffer size mismatch");

        Self {
            buffer,
            read_pos: AtomicU32::new(0),
            write_pos: AtomicU32::new(0),
        }
    }

    /// Returns the number of samples available for reading.
    fn available(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        ((write.wrapping_sub(read)) as usize) % RING_BUFFER_SIZE
    }

    /// Returns the number of free slots for writing.
    fn free(&self) -> usize {
        RING_BUFFER_SIZE - self.available() - 1
    }

    /// Write samples to the buffer. Returns number of samples written.
    fn write(&mut self, samples: &[f32]) -> usize {
        let free = self.free();
        let to_write = samples.len().min(free);

        let write_pos = self.write_pos.load(Ordering::Acquire) as usize;

        for (i, &sample) in samples.iter().take(to_write).enumerate() {
            let pos = (write_pos + i) % RING_BUFFER_SIZE;
            self.buffer[pos] = sample;
        }

        self.write_pos.store(
            ((write_pos + to_write) % RING_BUFFER_SIZE) as u32,
            Ordering::Release,
        );

        to_write
    }

    /// Read samples from the buffer. Returns number of samples read.
    fn read(&self, output: &mut [f32]) -> usize {
        let available = self.available();
        let to_read = output.len().min(available);

        let read_pos = self.read_pos.load(Ordering::Acquire) as usize;

        for (i, sample) in output.iter_mut().take(to_read).enumerate() {
            let pos = (read_pos + i) % RING_BUFFER_SIZE;
            // SAFETY: We're reading from a fixed-size array with modulo indexing
            *sample = self.buffer[pos];
        }

        self.read_pos.store(
            ((read_pos + to_read) % RING_BUFFER_SIZE) as u32,
            Ordering::Release,
        );

        to_read
    }
}

/// Audio output system using cpal.
pub struct AudioOutput {
    /// The cpal audio stream (must be kept alive).
    _stream: Stream,
    /// Shared ring buffer for sample transfer.
    buffer: Arc<std::sync::Mutex<RingBuffer>>,
    /// Volume level (0.0 - 1.0).
    volume: Arc<AtomicU32>,
    /// Mute state.
    muted: Arc<AtomicBool>,
    /// Sample rate of the output device.
    sample_rate: u32,
}

impl AudioOutput {
    /// Create a new audio output system.
    ///
    /// # Errors
    ///
    /// Returns an error if no audio device is available or stream creation fails.
    pub fn new(sample_rate: u32, volume: f32, muted: bool) -> Result<Self> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .context("No audio output device available")?;

        info!(
            "Using audio device: {}",
            device.name().unwrap_or_else(|_| "Unknown".to_string())
        );

        let config = Self::find_config(&device, sample_rate)?;
        let actual_sample_rate = config.sample_rate.0;

        info!(
            "Audio config: {} Hz, {} channels",
            actual_sample_rate, config.channels
        );

        let buffer = Arc::new(std::sync::Mutex::new(RingBuffer::new()));
        let buffer_clone = Arc::clone(&buffer);

        let volume_atomic = Arc::new(AtomicU32::new(volume.to_bits()));
        let volume_clone = Arc::clone(&volume_atomic);

        let muted_atomic = Arc::new(AtomicBool::new(muted));
        let muted_clone = Arc::clone(&muted_atomic);

        let channels = config.channels as usize;

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let vol = f32::from_bits(volume_clone.load(Ordering::Relaxed));
                    let is_muted = muted_clone.load(Ordering::Relaxed);

                    if is_muted {
                        // Fill with silence when muted
                        data.fill(0.0);
                        return;
                    }

                    // Read mono samples and duplicate to all channels
                    let mono_samples_needed = data.len() / channels;
                    let mut mono_buffer = vec![0.0f32; mono_samples_needed];

                    if let Ok(buf) = buffer_clone.lock() {
                        let read = buf.read(&mut mono_buffer);
                        // Fill remaining with silence if underrun
                        if read < mono_samples_needed {
                            mono_buffer[read..].fill(0.0);
                        }
                    } else {
                        // Lock failed, fill with silence
                        mono_buffer.fill(0.0);
                    }

                    // Distribute mono samples to all channels with volume
                    for (i, chunk) in data.chunks_mut(channels).enumerate() {
                        let sample = mono_buffer.get(i).copied().unwrap_or(0.0) * vol;
                        chunk.fill(sample);
                    }
                },
                move |err| {
                    error!("Audio stream error: {err}");
                },
                None,
            )
            .context("Failed to build audio output stream")?;

        stream.play().context("Failed to start audio stream")?;

        debug!("Audio output initialized successfully");

        Ok(Self {
            _stream: stream,
            buffer,
            volume: volume_atomic,
            muted: muted_atomic,
            sample_rate: actual_sample_rate,
        })
    }

    /// Find a suitable audio configuration for the device.
    fn find_config(device: &Device, preferred_rate: u32) -> Result<StreamConfig> {
        let supported_configs = device
            .supported_output_configs()
            .context("Failed to query supported audio configs")?;

        // Try to find a config with the preferred sample rate
        for config in supported_configs {
            if config.min_sample_rate().0 <= preferred_rate
                && config.max_sample_rate().0 >= preferred_rate
            {
                return Ok(config.with_sample_rate(SampleRate(preferred_rate)).into());
            }
        }

        // Fall back to default config
        device
            .default_output_config()
            .map(std::convert::Into::into)
            .context("No suitable audio config found")
    }

    /// Queue audio samples for playback.
    ///
    /// Samples should be mono f32 values in the range -1.0 to 1.0.
    /// Returns the number of samples actually queued.
    pub fn queue_samples(&mut self, samples: &[f32]) -> usize {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.write(samples)
        } else {
            warn!("Failed to lock audio buffer for writing");
            0
        }
    }

    /// Set the volume level (0.0 - 1.0).
    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.volume.store(volume.to_bits(), Ordering::Relaxed);
    }

    /// Get the current volume level.
    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Set the mute state.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Get the current mute state.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    /// Toggle mute state.
    pub fn toggle_mute(&self) {
        let current = self.muted.load(Ordering::Relaxed);
        self.muted.store(!current, Ordering::Relaxed);
    }

    /// Get the output sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the number of samples available in the buffer.
    #[must_use]
    pub fn buffer_available(&self) -> usize {
        self.buffer.lock().map(|b| b.available()).unwrap_or(0)
    }

    /// Get the number of free slots in the buffer.
    #[must_use]
    pub fn buffer_free(&self) -> usize {
        self.buffer.lock().map(|b| b.free()).unwrap_or(0)
    }
}

impl std::fmt::Debug for AudioOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioOutput")
            .field("sample_rate", &self.sample_rate)
            .field("volume", &self.volume())
            .field("muted", &self.is_muted())
            .finish_non_exhaustive()
    }
}
