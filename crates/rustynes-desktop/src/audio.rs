//! Audio output using cpal for low-latency playback.
//!
//! This module provides a thread-safe audio output system that:
//! - Uses a truly lock-free SPSC ring buffer for sample transfer
//! - Implements adaptive latency adjustment
//! - Handles buffer underruns gracefully with silence
//! - Supports dynamic volume control
//! - Provides mute functionality
//! - Monitors buffer health for A/V synchronization
//!
//! # Safety
//!
//! This module uses unsafe code for the SPSC ring buffer implementation.
//! This is permitted per project guidelines for platform-specific audio code.

// Allow unsafe code for lock-free SPSC ring buffer (per project guidelines)
#![allow(unsafe_code)]

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleRate, Stream, StreamConfig};
use log::{debug, error, info};
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Default size of the ring buffer in samples (mono).
const DEFAULT_RING_BUFFER_SIZE: usize = 8192;

/// Minimum ring buffer size (for low latency)
const MIN_RING_BUFFER_SIZE: usize = 2048;

/// Maximum ring buffer size (for stability)
const MAX_RING_BUFFER_SIZE: usize = 16384;

/// Target buffer fill percentage for optimal A/V sync (reserved for future dynamic adjustment)
#[allow(dead_code)]
const TARGET_BUFFER_FILL_PERCENT: f32 = 0.5;

/// Minimum buffer fill before we risk underrun
const MIN_BUFFER_FILL_PERCENT: f32 = 0.25;

/// Maximum buffer fill before we risk latency
const MAX_BUFFER_FILL_PERCENT: f32 = 0.75;

/// Number of frames to track for latency calculations (reserved for future use)
#[allow(dead_code)]
const LATENCY_HISTORY_SIZE: usize = 60;

/// Size of the preallocated mono buffer for audio callback (avoids per-callback allocation).
/// Based on typical audio callback sizes (256-4096 samples per channel).
const PREALLOCATED_MONO_BUFFER_SIZE: usize = 4096;

/// Lock-free SPSC (Single-Producer Single-Consumer) ring buffer for audio samples.
///
/// This implementation is designed for the specific use case where:
/// - One thread (main/emulation) produces samples via `write()`
/// - One thread (audio callback) consumes samples via `read()`
///
/// The lock-free design eliminates mutex contention that can cause audio stuttering.
struct SpscRingBuffer {
    /// Sample data wrapped in `UnsafeCell` for interior mutability.
    ///
    /// SAFETY: Only the producer writes (after `write_pos`), only the consumer reads (before `write_pos`).
    buffer: UnsafeCell<Vec<f32>>,
    capacity: usize,
    /// Read position - only modified by consumer, read by producer for `available()` check.
    read_pos: AtomicUsize,
    /// Write position - only modified by producer, read by consumer for `available()` check.
    write_pos: AtomicUsize,
}

// SAFETY: SpscRingBuffer is Send+Sync because:
// - The buffer is accessed through atomic coordination of read_pos and write_pos
// - Producer only writes to positions [write_pos..write_pos+n) then advances write_pos
// - Consumer only reads from positions [read_pos..read_pos+n) then advances read_pos
// - The atomic operations with Acquire/Release ordering ensure proper synchronization
unsafe impl Send for SpscRingBuffer {}
unsafe impl Sync for SpscRingBuffer {}

impl SpscRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: UnsafeCell::new(vec![0.0; capacity]),
            capacity,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
        }
    }

    /// Returns the number of samples available for reading.
    #[inline]
    fn available(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read) % self.capacity
    }

    /// Returns the number of free slots for writing.
    #[inline]
    fn free(&self) -> usize {
        // Leave one slot empty to distinguish full from empty
        self.capacity - self.available() - 1
    }

    /// Get the buffer fill percentage (0.0 - 1.0)
    #[inline]
    #[allow(clippy::cast_precision_loss)]
    fn fill_percent(&self) -> f32 {
        self.available() as f32 / self.capacity as f32
    }

    /// Write samples to the buffer (producer side). Returns number of samples written.
    ///
    /// # Safety
    /// Must only be called from the producer thread.
    fn write(&self, samples: &[f32]) -> usize {
        let free = self.free();
        let to_write = samples.len().min(free);
        if to_write == 0 {
            return 0;
        }

        let write_pos = self.write_pos.load(Ordering::Relaxed);

        // SAFETY: We only write to positions that the consumer has already read past
        // (between current write_pos and read_pos). The consumer won't access these
        // positions until we advance write_pos with Release ordering.
        let buffer = unsafe { &mut *self.buffer.get() };

        for (i, &sample) in samples.iter().take(to_write).enumerate() {
            let pos = (write_pos + i) % self.capacity;
            buffer[pos] = sample;
        }

        // Release ordering ensures writes to buffer are visible before write_pos update
        self.write_pos
            .store((write_pos + to_write) % self.capacity, Ordering::Release);

        to_write
    }

    /// Read samples from the buffer (consumer side). Returns number of samples read.
    ///
    /// # Safety
    /// Must only be called from the consumer thread.
    fn read(&self, output: &mut [f32]) -> usize {
        let available = self.available();
        let to_read = output.len().min(available);
        if to_read == 0 {
            return 0;
        }

        let read_pos = self.read_pos.load(Ordering::Relaxed);

        // SAFETY: We only read from positions that the producer has written to
        // (between read_pos and write_pos). The producer won't overwrite these
        // positions until we advance read_pos with Release ordering.
        let buffer = unsafe { &*self.buffer.get() };

        for (i, sample) in output.iter_mut().take(to_read).enumerate() {
            let pos = (read_pos + i) % self.capacity;
            *sample = buffer[pos];
        }

        // Release ordering ensures we've read all data before advancing read_pos
        self.read_pos
            .store((read_pos + to_read) % self.capacity, Ordering::Release);

        to_read
    }

    /// Clear the buffer (must be called when neither producer nor consumer is active)
    fn clear(&self) {
        self.read_pos.store(0, Ordering::Release);
        self.write_pos.store(0, Ordering::Release);
    }
}

/// Audio latency statistics for monitoring and adjustment
#[derive(Debug, Clone, Default)]
pub struct AudioLatencyStats {
    /// Current buffer fill level (0.0 - 1.0)
    pub buffer_fill: f32,
    /// Number of underruns since last check
    pub underruns: u32,
    /// Estimated latency in milliseconds
    pub latency_ms: f32,
    /// Whether audio is healthy (no recent underruns)
    pub is_healthy: bool,
}

/// Audio output system using cpal with adaptive latency.
pub struct AudioOutput {
    /// The cpal audio stream (must be kept alive).
    _stream: Stream,
    /// Lock-free SPSC ring buffer for sample transfer.
    buffer: Arc<SpscRingBuffer>,
    /// Volume level (0.0 - 1.0).
    volume: Arc<AtomicU32>,
    /// Mute state.
    muted: Arc<AtomicBool>,
    /// Sample rate of the output device.
    sample_rate: u32,
    /// Underrun counter for monitoring
    underrun_count: Arc<AtomicUsize>,
    /// Buffer size in samples
    buffer_size: usize,
}

impl AudioOutput {
    /// Create a new audio output system.
    ///
    /// # Errors
    ///
    /// Returns an error if no audio device is available or stream creation fails.
    pub fn new(sample_rate: u32, volume: f32, muted: bool) -> Result<Self> {
        Self::with_buffer_size(sample_rate, volume, muted, DEFAULT_RING_BUFFER_SIZE)
    }

    /// Create a new audio output with custom buffer size.
    ///
    /// # Arguments
    ///
    /// * `sample_rate` - Target sample rate in Hz
    /// * `volume` - Initial volume (0.0 - 1.0)
    /// * `muted` - Initial mute state
    /// * `buffer_size` - Ring buffer size in samples
    ///
    /// # Errors
    ///
    /// Returns an error if no audio device is available or stream creation fails.
    pub fn with_buffer_size(
        sample_rate: u32,
        volume: f32,
        muted: bool,
        buffer_size: usize,
    ) -> Result<Self> {
        let buffer_size = buffer_size.clamp(MIN_RING_BUFFER_SIZE, MAX_RING_BUFFER_SIZE);

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
            "Audio config: {} Hz, {} channels, buffer size: {} samples",
            actual_sample_rate, config.channels, buffer_size
        );

        // Create lock-free SPSC buffer - no mutex needed!
        let buffer = Arc::new(SpscRingBuffer::new(buffer_size));
        let buffer_clone = Arc::clone(&buffer);

        let volume_atomic = Arc::new(AtomicU32::new(volume.to_bits()));
        let volume_clone = Arc::clone(&volume_atomic);

        let muted_atomic = Arc::new(AtomicBool::new(muted));
        let muted_clone = Arc::clone(&muted_atomic);

        let underrun_count = Arc::new(AtomicUsize::new(0));
        let underrun_clone = Arc::clone(&underrun_count);

        let channels = config.channels as usize;

        // Preallocate a reusable buffer for the audio callback to avoid per-callback allocations.
        // If the callback needs more than PREALLOCATED_MONO_BUFFER_SIZE, it will resize gracefully.
        let mut preallocated_buffer = vec![0.0f32; PREALLOCATED_MONO_BUFFER_SIZE];

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

                    // Use preallocated buffer when possible to avoid heap allocation in hot path.
                    // Only allocate if callback requests more than our preallocated size (rare).
                    let mono_buffer: &mut [f32] =
                        if mono_samples_needed <= preallocated_buffer.len() {
                            // Zero the portion we'll use
                            preallocated_buffer[..mono_samples_needed].fill(0.0);
                            &mut preallocated_buffer[..mono_samples_needed]
                        } else {
                            // Fallback: allocate if needed (should be rare with 4096 sample buffer)
                            preallocated_buffer.resize(mono_samples_needed, 0.0);
                            &mut preallocated_buffer[..mono_samples_needed]
                        };

                    // Lock-free read from SPSC buffer - no mutex contention!
                    let samples_read = buffer_clone.read(mono_buffer);

                    // Check for underrun
                    if samples_read < mono_samples_needed {
                        // Fill remaining with silence
                        mono_buffer[samples_read..].fill(0.0);

                        // Track underrun
                        if samples_read == 0 {
                            underrun_clone.fetch_add(1, Ordering::Relaxed);
                        }
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
            underrun_count,
            buffer_size,
        })
    }

    /// Create audio output optimized for low latency
    ///
    /// # Errors
    ///
    /// Returns an error if no audio device is available or stream creation fails.
    pub fn low_latency(sample_rate: u32, volume: f32, muted: bool) -> Result<Self> {
        Self::with_buffer_size(sample_rate, volume, muted, MIN_RING_BUFFER_SIZE)
    }

    /// Create audio output optimized for stability (higher latency)
    ///
    /// # Errors
    ///
    /// Returns an error if no audio device is available or stream creation fails.
    pub fn high_stability(sample_rate: u32, volume: f32, muted: bool) -> Result<Self> {
        Self::with_buffer_size(sample_rate, volume, muted, MAX_RING_BUFFER_SIZE)
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
    ///
    /// This method is lock-free and safe to call from the main/emulation thread.
    pub fn queue_samples(&self, samples: &[f32]) -> usize {
        // Lock-free write to SPSC buffer
        self.buffer.write(samples)
    }

    /// Queue samples with dynamic rate adjustment hint
    ///
    /// Returns a speed adjustment factor that can be used to slightly
    /// speed up or slow down emulation to maintain audio sync.
    ///
    /// This method is lock-free and safe to call from the main/emulation thread.
    pub fn queue_samples_with_sync(&self, samples: &[f32]) -> (usize, f32) {
        let queued = self.queue_samples(samples);

        // Calculate speed adjustment based on buffer fill
        let fill = self.buffer_fill_percent();
        let adjustment = if fill < MIN_BUFFER_FILL_PERCENT {
            // Buffer is low, slow down slightly to let it fill
            0.99
        } else if fill > MAX_BUFFER_FILL_PERCENT {
            // Buffer is high, speed up slightly to drain it
            1.01
        } else {
            // Buffer is healthy, no adjustment
            1.0
        };

        (queued, adjustment)
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
        self.buffer.available()
    }

    /// Get the number of free slots in the buffer.
    #[must_use]
    pub fn buffer_free(&self) -> usize {
        self.buffer.free()
    }

    /// Get the buffer fill percentage (0.0 - 1.0)
    #[must_use]
    pub fn buffer_fill_percent(&self) -> f32 {
        self.buffer.fill_percent()
    }

    /// Get the total buffer size in samples
    #[must_use]
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get and reset the underrun count
    pub fn take_underrun_count(&self) -> usize {
        self.underrun_count.swap(0, Ordering::Relaxed)
    }

    /// Get current audio latency statistics
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn latency_stats(&self) -> AudioLatencyStats {
        let buffer_fill = self.buffer_fill_percent();
        let underruns = self.underrun_count.load(Ordering::Relaxed) as u32;

        // Calculate estimated latency in ms
        // latency = buffer_samples / sample_rate * 1000
        let buffer_samples = buffer_fill * self.buffer_size as f32;
        let latency_ms = buffer_samples / self.sample_rate as f32 * 1000.0;

        // Consider audio healthy if buffer is in acceptable range and no recent underruns
        let is_healthy = (MIN_BUFFER_FILL_PERCENT..=MAX_BUFFER_FILL_PERCENT).contains(&buffer_fill)
            && underruns == 0;

        AudioLatencyStats {
            buffer_fill,
            underruns,
            latency_ms,
            is_healthy,
        }
    }

    /// Clear the audio buffer (for seeking, reset, etc.)
    ///
    /// Note: This should only be called when playback is paused to avoid
    /// race conditions with the audio callback.
    pub fn clear_buffer(&self) {
        self.buffer.clear();
    }

    /// Check if audio output is likely experiencing issues
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        let fill = self.buffer_fill_percent();
        let underruns = self.underrun_count.load(Ordering::Relaxed);

        !(MIN_BUFFER_FILL_PERCENT..=MAX_BUFFER_FILL_PERCENT).contains(&fill) || underruns > 5
    }
}

impl std::fmt::Debug for AudioOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioOutput")
            .field("sample_rate", &self.sample_rate)
            .field("volume", &self.volume())
            .field("muted", &self.is_muted())
            .field("buffer_size", &self.buffer_size)
            .field("buffer_fill", &self.buffer_fill_percent())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_buffer_new() {
        let buffer = SpscRingBuffer::new(1024);
        assert_eq!(buffer.capacity, 1024);
        assert_eq!(buffer.available(), 0);
        assert_eq!(buffer.free(), 1023);
    }

    #[test]
    fn test_spsc_buffer_write_read() {
        let buffer = SpscRingBuffer::new(1024);

        let samples = vec![0.5f32; 100];
        let written = buffer.write(&samples);
        assert_eq!(written, 100);
        assert_eq!(buffer.available(), 100);

        let mut output = vec![0.0f32; 50];
        let read = buffer.read(&mut output);
        assert_eq!(read, 50);
        assert_eq!(buffer.available(), 50);

        for sample in &output {
            assert!((*sample - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn test_spsc_buffer_wrap() {
        let buffer = SpscRingBuffer::new(100);

        // Fill most of the buffer
        let samples = vec![0.5f32; 80];
        buffer.write(&samples);

        // Read half
        let mut output = vec![0.0f32; 40];
        buffer.read(&mut output);

        // Write more (should wrap around)
        let samples = vec![0.75f32; 40];
        let written = buffer.write(&samples);
        assert_eq!(written, 40);
    }

    #[test]
    fn test_spsc_buffer_fill_percent() {
        let buffer = SpscRingBuffer::new(100);
        assert!((buffer.fill_percent() - 0.0).abs() < 0.01);

        let samples = vec![0.5f32; 50];
        buffer.write(&samples);
        assert!((buffer.fill_percent() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_latency_stats() {
        // This is a unit test for the stats struct
        let stats = AudioLatencyStats {
            buffer_fill: 0.5,
            underruns: 0,
            latency_ms: 85.0,
            is_healthy: true,
        };

        assert!(stats.is_healthy);
        assert_eq!(stats.underruns, 0);
    }
}
