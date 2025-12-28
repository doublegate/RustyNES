//! Audio resampler for converting APU rate to target sample rate
//!
//! The NES APU runs at approximately 1.789773 MHz (NTSC) or 1.662607 MHz (PAL),
//! but modern audio systems expect 44.1 kHz or 48 kHz. This module provides
//! high-quality resampling using:
//!
//! 1. **Rubato sinc interpolation** - Band-limited resampling for accurate frequency content
//! 2. **Multi-stage filter chain** - High-pass and low-pass filtering matching NES hardware
//! 3. **Fallback linear interpolation** - For when rubato isn't available
//!
//! # NES Audio Characteristics
//!
//! The NES audio path has several filtering stages:
//! - 90 Hz high-pass filter (removes DC offset)
//! - 440 Hz high-pass filter (second stage)
//! - 14 kHz low-pass filter (anti-aliasing before output)
//!
//! # Example
//!
//! ```rust
//! use rustynes_apu::resampler::HighQualityResampler;
//!
//! let mut resampler = HighQualityResampler::new(48000);
//!
//! // Add APU samples (called every CPU cycle at ~1.79 MHz)
//! for i in 0..10000 {
//!     let sample = (i as f32 * 0.001).sin();
//!     resampler.add_sample(sample);
//! }
//!
//! // Retrieve output samples for audio device
//! let output = resampler.samples();
//! println!("Generated {} output samples", output.len());
//!
//! // Clear buffer after consuming
//! resampler.clear();
//! ```

use log::{debug, warn};
use rubato::{FftFixedInOut, Resampler as RubatoResampler};
use std::f32::consts::PI;

/// NTSC APU sample rate (CPU clock: 1.789773 MHz)
pub const APU_RATE_NTSC: u32 = 1_789_773;

/// PAL APU sample rate (CPU clock: 1.662607 MHz)
pub const APU_RATE_PAL: u32 = 1_662_607;

/// Common output sample rate (CD quality)
pub const SAMPLE_RATE_44100: u32 = 44_100;

/// Common output sample rate (professional audio)
pub const SAMPLE_RATE_48000: u32 = 48_000;

/// NES high-pass filter 1 frequency (Hz)
const HIGHPASS_1_FREQ: f32 = 90.0;

/// NES high-pass filter 2 frequency (Hz)
const HIGHPASS_2_FREQ: f32 = 440.0;

/// NES low-pass filter frequency (Hz)
const LOWPASS_FREQ: f32 = 14_000.0;

/// High-quality audio resampler with sinc interpolation and NES filter chain
///
/// Uses rubato for band-limited resampling and implements the NES analog
/// audio filter chain for authentic sound reproduction.
pub struct HighQualityResampler {
    /// Target output sample rate (e.g., 48000 Hz)
    output_rate: u32,

    /// APU sample rate (1.789773 MHz for NTSC)
    input_rate: u32,

    /// Intermediate sample rate for decimation
    intermediate_rate: u32,

    /// First-stage decimation resampler (APU rate -> intermediate)
    decimator: Option<Box<FftFixedInOut<f32>>>,

    /// Second-stage resampler (intermediate -> output)
    final_resampler: Option<Box<FftFixedInOut<f32>>>,

    /// Input buffer for decimator
    decimator_input: Vec<f32>,

    /// Intermediate buffer between stages
    intermediate_buffer: Vec<f32>,

    /// Output sample buffer
    output_buffer: Vec<f32>,

    /// Filter chain for NES audio characteristics
    filter_chain: FilterChain,

    /// Fallback linear resampler (used when rubato setup fails)
    fallback_resampler: LinearResampler,

    /// Whether we're using the fallback resampler
    using_fallback: bool,

    /// Chunk size for rubato processing
    chunk_size: usize,

    /// Preallocated stage 1 input work buffer
    stage1_input_work: Vec<f32>,

    /// Preallocated stage 1 output work buffer
    stage1_output_work: Vec<f32>,

    /// Preallocated stage 2 input work buffer
    stage2_input_work: Vec<f32>,

    /// Preallocated stage 2 output work buffer
    stage2_output_work: Vec<f32>,
}

impl HighQualityResampler {
    /// Create a new high-quality resampler with specified output rate
    ///
    /// # Arguments
    ///
    /// * `output_rate` - Target sample rate (typically 44100 or 48000 Hz)
    #[must_use]
    pub fn new(output_rate: u32) -> Self {
        Self::with_input_rate(output_rate, APU_RATE_NTSC)
    }

    /// Create a resampler with custom input rate (for PAL systems)
    ///
    /// # Arguments
    ///
    /// * `output_rate` - Target sample rate (Hz)
    /// * `input_rate` - APU sample rate (Hz)
    #[must_use]
    pub fn with_input_rate(output_rate: u32, input_rate: u32) -> Self {
        // Use an intermediate rate for two-stage resampling
        // This helps with the extreme ratio (~1.79MHz -> 48kHz = 37:1)
        let intermediate_rate = output_rate * 4; // 192 kHz intermediate

        let filter_chain = FilterChain::new(output_rate);
        let fallback_resampler = LinearResampler::with_input_rate(output_rate, input_rate);

        // Try to create rubato resamplers
        let (decimator, final_resampler, chunk_size, using_fallback) =
            Self::create_resamplers(input_rate, intermediate_rate, output_rate);

        // Preallocate work buffers for rubato processing
        // Stage 1: APU rate (1.79MHz) -> intermediate (192kHz), ~9.3:1 ratio
        // Stage 2: intermediate (192kHz) -> output (48kHz), 4:1 ratio
        let stage1_input_cap = chunk_size;
        let stage1_output_cap = chunk_size / 4; // Conservative estimate
        let stage2_input_cap = stage1_output_cap;
        let stage2_output_cap = stage2_input_cap;

        Self {
            output_rate,
            input_rate,
            intermediate_rate,
            decimator,
            final_resampler,
            decimator_input: Vec::with_capacity(chunk_size * 2),
            intermediate_buffer: Vec::with_capacity(4096),
            output_buffer: Vec::with_capacity(4096),
            filter_chain,
            fallback_resampler,
            using_fallback,
            chunk_size,
            stage1_input_work: Vec::with_capacity(stage1_input_cap),
            stage1_output_work: Vec::with_capacity(stage1_output_cap),
            stage2_input_work: Vec::with_capacity(stage2_input_cap),
            stage2_output_work: Vec::with_capacity(stage2_output_cap),
        }
    }

    /// Create rubato resamplers for two-stage decimation
    #[allow(clippy::type_complexity)]
    fn create_resamplers(
        input_rate: u32,
        intermediate_rate: u32,
        output_rate: u32,
    ) -> (
        Option<Box<FftFixedInOut<f32>>>,
        Option<Box<FftFixedInOut<f32>>>,
        usize,
        bool,
    ) {
        // Calculate the decimation ratio
        let decimation_ratio = f64::from(input_rate) / f64::from(intermediate_rate);

        // Use a reasonable chunk size that works with the ratio
        // For 1.79MHz -> 192kHz, ratio is ~9.3:1
        // We need chunk_size * ratio to give integer outputs
        let chunk_size = 1024;

        // Try to create the decimator (APU rate -> intermediate)
        let decimator = match FftFixedInOut::<f32>::new(
            input_rate as usize,
            intermediate_rate as usize,
            chunk_size,
            1,
        ) {
            Ok(r) => {
                debug!(
                    "Created decimator: {input_rate} Hz -> {intermediate_rate} Hz (ratio {decimation_ratio:.4})"
                );
                Some(Box::new(r))
            }
            Err(e) => {
                warn!("Failed to create decimator: {e}, falling back to linear interpolation");
                return (None, None, chunk_size, true);
            }
        };

        // Try to create the final resampler (intermediate -> output)
        let final_resampler = match FftFixedInOut::<f32>::new(
            intermediate_rate as usize,
            output_rate as usize,
            256,
            1,
        ) {
            Ok(r) => {
                debug!("Created final resampler: {intermediate_rate} Hz -> {output_rate} Hz");
                Some(Box::new(r))
            }
            Err(e) => {
                warn!(
                    "Failed to create final resampler: {e}, falling back to linear interpolation"
                );
                return (None, None, chunk_size, true);
            }
        };

        (decimator, final_resampler, chunk_size, false)
    }

    /// Add a sample from the APU
    ///
    /// This should be called every APU cycle (~1.79 MHz).
    #[inline]
    pub fn add_sample(&mut self, sample: f32) {
        if self.using_fallback {
            // Use linear interpolation fallback
            self.fallback_resampler.add_sample(sample);
            return;
        }

        // Accumulate samples for chunked processing
        self.decimator_input.push(sample);

        // Process when we have enough samples
        if self.decimator_input.len() >= self.chunk_size {
            self.process_chunk();
        }
    }

    /// Process a chunk of samples through the resampling stages
    fn process_chunk(&mut self) {
        if self.decimator_input.len() < self.chunk_size {
            return;
        }

        // Stage 1: Decimate from APU rate to intermediate rate
        if let Some(ref mut decimator) = self.decimator {
            let input_frames = decimator.input_frames_next();
            if self.decimator_input.len() >= input_frames {
                // Reuse preallocated work buffer instead of allocating new Vec each call
                self.stage1_input_work.clear();
                self.stage1_input_work
                    .extend(self.decimator_input.drain(..input_frames));

                let output_frames = decimator.output_frames_next();
                self.stage1_output_work.clear();
                self.stage1_output_work.resize(output_frames, 0.0);

                // rubato needs Vec<Vec<f32>> format - create temporary wrapper slices
                let input_wrapper = vec![std::mem::take(&mut self.stage1_input_work)];
                let mut output_wrapper = vec![std::mem::take(&mut self.stage1_output_work)];

                if let Ok((_, _)) =
                    decimator.process_into_buffer(&input_wrapper, &mut output_wrapper, None)
                {
                    self.intermediate_buffer.extend(&output_wrapper[0]);
                }

                // Restore buffers for reuse (take back ownership)
                if let Some(buf) = input_wrapper.into_iter().next() {
                    self.stage1_input_work = buf;
                }
                if let Some(buf) = output_wrapper.into_iter().next() {
                    self.stage1_output_work = buf;
                }
            }
        }

        // Stage 2: Resample from intermediate to output rate
        if let Some(ref mut final_resampler) = self.final_resampler {
            let input_frames = final_resampler.input_frames_next();
            while self.intermediate_buffer.len() >= input_frames {
                // Reuse preallocated work buffer instead of allocating new Vec each call
                self.stage2_input_work.clear();
                self.stage2_input_work
                    .extend(self.intermediate_buffer.drain(..input_frames));

                let output_frames = final_resampler.output_frames_next();
                self.stage2_output_work.clear();
                self.stage2_output_work.resize(output_frames, 0.0);

                // rubato needs Vec<Vec<f32>> format - create temporary wrapper slices
                let input_wrapper = vec![std::mem::take(&mut self.stage2_input_work)];
                let mut output_wrapper = vec![std::mem::take(&mut self.stage2_output_work)];

                if let Ok((_, _)) =
                    final_resampler.process_into_buffer(&input_wrapper, &mut output_wrapper, None)
                {
                    // Apply filter chain to output samples
                    for sample in &output_wrapper[0] {
                        let filtered = self.filter_chain.process(*sample);
                        self.output_buffer.push(filtered);
                    }
                }

                // Restore buffers for reuse (take back ownership)
                if let Some(buf) = input_wrapper.into_iter().next() {
                    self.stage2_input_work = buf;
                }
                if let Some(buf) = output_wrapper.into_iter().next() {
                    self.stage2_output_work = buf;
                }
            }
        }
    }

    /// Get reference to output samples
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        if self.using_fallback {
            self.fallback_resampler.samples()
        } else {
            &self.output_buffer
        }
    }

    /// Clear the output buffer
    pub fn clear(&mut self) {
        if self.using_fallback {
            self.fallback_resampler.clear();
        } else {
            self.output_buffer.clear();
        }
    }

    /// Check if resampler has at least `min_samples` ready
    #[must_use]
    pub fn is_ready(&self, min_samples: usize) -> bool {
        self.samples().len() >= min_samples
    }

    /// Get the current number of samples in the buffer
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples().len()
    }

    /// Check if the buffer is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples().is_empty()
    }

    /// Reset the resampler state
    pub fn reset(&mut self) {
        self.decimator_input.clear();
        self.intermediate_buffer.clear();
        self.output_buffer.clear();
        self.filter_chain.reset();
        self.fallback_resampler.reset();
    }

    /// Get the target output sample rate
    #[must_use]
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// Get the input sample rate (APU rate)
    #[must_use]
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Check if using fallback linear interpolation
    #[must_use]
    pub fn is_using_fallback(&self) -> bool {
        self.using_fallback
    }

    /// Flush any remaining samples in the pipeline
    pub fn flush(&mut self) {
        // Process any remaining samples with zero-padding if needed
        while !self.decimator_input.is_empty() {
            let remaining = self.decimator_input.len();
            let padding_needed = self.chunk_size.saturating_sub(remaining);
            self.decimator_input.extend(vec![0.0f32; padding_needed]);
            self.process_chunk();
        }
    }
}

impl Default for HighQualityResampler {
    fn default() -> Self {
        Self::new(SAMPLE_RATE_48000)
    }
}

/// NES audio filter chain
///
/// Implements the analog filter stages present in the NES audio output:
/// - 90 Hz first-order high-pass (removes DC offset)
/// - 440 Hz first-order high-pass (shapes bass response)
/// - 14 kHz first-order low-pass (anti-aliasing)
pub struct FilterChain {
    /// First high-pass filter (90 Hz)
    highpass_1: HighPassFilter,
    /// Second high-pass filter (440 Hz)
    highpass_2: HighPassFilter,
    /// Low-pass filter (14 kHz)
    lowpass: LowPassFilter,
    /// Sample rate for filter calculations
    sample_rate: u32,
}

impl FilterChain {
    /// Create a new filter chain for the specified sample rate
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let rate = sample_rate as f32;

        Self {
            highpass_1: HighPassFilter::new(HIGHPASS_1_FREQ, rate),
            highpass_2: HighPassFilter::new(HIGHPASS_2_FREQ, rate),
            lowpass: LowPassFilter::new(LOWPASS_FREQ, rate),
            sample_rate,
        }
    }

    /// Process a sample through all filter stages
    #[inline]
    pub fn process(&mut self, sample: f32) -> f32 {
        let hp1 = self.highpass_1.process(sample);
        let hp2 = self.highpass_2.process(hp1);
        self.lowpass.process(hp2)
    }

    /// Reset all filters to initial state
    pub fn reset(&mut self) {
        self.highpass_1.reset();
        self.highpass_2.reset();
        self.lowpass.reset();
    }

    /// Get the sample rate
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// First-order high-pass filter (IIR)
///
/// Implements: `y[n] = a * (y[n-1] + x[n] - x[n-1])`
/// where `a = RC / (RC + dt)` and `RC = 1 / (2 * pi * fc)`
pub struct HighPassFilter {
    /// Previous input sample
    prev_input: f32,
    /// Previous output sample
    prev_output: f32,
    /// Filter coefficient
    alpha: f32,
}

impl HighPassFilter {
    /// Create a new high-pass filter
    ///
    /// # Arguments
    ///
    /// * `cutoff_hz` - Cutoff frequency in Hz
    /// * `sample_rate` - Sample rate in Hz
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let dt = 1.0 / sample_rate;
        let alpha = rc / (rc + dt);

        Self {
            prev_input: 0.0,
            prev_output: 0.0,
            alpha,
        }
    }

    /// Process a sample through the filter
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let output = self.alpha * (self.prev_output + input - self.prev_input);
        self.prev_input = input;
        self.prev_output = output;
        output
    }

    /// Reset filter state
    pub fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}

/// First-order low-pass filter (IIR)
///
/// Implements: `y[n] = y[n-1] + a * (x[n] - y[n-1])`
/// where `a = dt / (RC + dt)` and `RC = 1 / (2 * pi * fc)`
pub struct LowPassFilter {
    /// Previous output sample
    prev_sample: f32,
    /// Filter coefficient (0.0-1.0)
    alpha: f32,
}

impl LowPassFilter {
    /// Create a new low-pass filter
    ///
    /// # Arguments
    ///
    /// * `cutoff_hz` - Cutoff frequency in Hz
    /// * `sample_rate` - Sample rate in Hz
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let dt = 1.0 / sample_rate;
        let alpha = dt / (rc + dt);

        Self {
            prev_sample: 0.0,
            alpha,
        }
    }

    /// Process a sample through the filter
    #[inline]
    pub fn process(&mut self, sample: f32) -> f32 {
        let output = self.prev_sample + self.alpha * (sample - self.prev_sample);
        self.prev_sample = output;
        output
    }

    /// Reset filter state
    pub fn reset(&mut self) {
        self.prev_sample = 0.0;
    }
}

/// Simple linear interpolation resampler (fallback)
///
/// Used when rubato cannot be initialized or for simpler use cases.
pub struct LinearResampler {
    /// Target output sample rate
    output_rate: u32,
    /// APU sample rate
    input_rate: u32,
    /// Fractional time accumulator
    time_accumulator: f32,
    /// Previous sample for interpolation
    prev_sample: f32,
    /// Output sample buffer
    buffer: Vec<f32>,
}

impl LinearResampler {
    /// Create a new linear resampler
    #[must_use]
    pub fn new(output_rate: u32) -> Self {
        Self::with_input_rate(output_rate, APU_RATE_NTSC)
    }

    /// Create a resampler with custom input rate
    #[must_use]
    pub fn with_input_rate(output_rate: u32, input_rate: u32) -> Self {
        Self {
            output_rate,
            input_rate,
            time_accumulator: 0.0,
            prev_sample: 0.0,
            buffer: Vec::with_capacity(2048),
        }
    }

    /// Add a sample from the APU
    pub fn add_sample(&mut self, sample: f32) {
        #[allow(clippy::cast_precision_loss)]
        let time_step = self.output_rate as f32 / self.input_rate as f32;
        self.time_accumulator += time_step;

        while self.time_accumulator >= 1.0 {
            let t = self.time_accumulator - 1.0;
            let output = self.prev_sample + (sample - self.prev_sample) * t;
            self.buffer.push(output);
            self.time_accumulator -= 1.0;
        }

        self.prev_sample = sample;
    }

    /// Get reference to output samples
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.buffer
    }

    /// Clear the output buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Check if resampler has at least `min_samples` ready
    #[must_use]
    pub fn is_ready(&self, min_samples: usize) -> bool {
        self.buffer.len() >= min_samples
    }

    /// Get the current number of samples
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Reset the resampler state
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.time_accumulator = 0.0;
        self.prev_sample = 0.0;
    }

    /// Get output sample rate
    #[must_use]
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// Get input sample rate
    #[must_use]
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }
}

impl Default for LinearResampler {
    fn default() -> Self {
        Self::new(SAMPLE_RATE_48000)
    }
}

// Re-export the old Resampler name for backwards compatibility
/// Audio resampler (legacy alias for `HighQualityResampler`)
pub type Resampler = HighQualityResampler;

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_high_quality_resampler_creation() {
        let resampler = HighQualityResampler::new(48000);
        assert_eq!(resampler.output_rate(), 48000);
        assert_eq!(resampler.input_rate(), APU_RATE_NTSC);
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_linear_resampler_add_sample() {
        let mut resampler = LinearResampler::new(48000);

        // Add 1ms of audio (should produce ~48 samples)
        for _ in 0..1790 {
            resampler.add_sample(0.5);
        }

        let len = resampler.len();
        assert!((47..=49).contains(&len), "Expected ~48 samples, got {len}");
    }

    #[test]
    fn test_linear_resampler_clear() {
        let mut resampler = LinearResampler::new(48000);

        for _ in 0..100 {
            resampler.add_sample(0.5);
        }
        assert!(!resampler.is_empty());

        resampler.clear();
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_linear_resampler_reset() {
        let mut resampler = LinearResampler::new(48000);

        for _ in 0..1000 {
            resampler.add_sample(0.5);
        }

        assert!(!resampler.is_empty());

        resampler.reset();
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_linear_resampler_dc_signal() {
        let mut resampler = LinearResampler::new(48000);

        for _ in 0..10000 {
            resampler.add_sample(0.5);
        }

        let samples = resampler.samples();
        for &sample in samples {
            assert!((sample - 0.5).abs() < 0.01, "Sample: {sample}");
        }
    }

    #[test]
    fn test_filter_chain_creation() {
        let chain = FilterChain::new(48000);
        assert_eq!(chain.sample_rate(), 48000);
    }

    #[test]
    fn test_filter_chain_dc_removal() {
        let mut chain = FilterChain::new(48000);

        // Feed DC signal - high-pass should remove it over time
        for _ in 0..1000 {
            let _ = chain.process(1.0);
        }

        // After settling, DC should be mostly removed
        let output = chain.process(1.0);
        assert!(output.abs() < 0.5, "DC should be attenuated, got {output}");
    }

    #[test]
    fn test_low_pass_filter() {
        let mut filter = LowPassFilter::new(14000.0, 48000.0);

        let output = filter.process(1.0);
        assert!(output > 0.0 && output <= 1.0);

        let output2 = filter.process(1.0);
        assert!(output2 > output);
    }

    #[test]
    fn test_high_pass_filter() {
        let mut filter = HighPassFilter::new(90.0, 48000.0);

        // Step response
        let output1 = filter.process(1.0);
        assert!(output1 > 0.0);

        // Continued DC should decay
        let mut output = output1;
        for _ in 0..100 {
            output = filter.process(1.0);
        }
        assert!(output < output1, "High-pass should attenuate DC");
    }

    #[test]
    fn test_filter_reset() {
        let mut hp = HighPassFilter::new(90.0, 48000.0);
        let mut lp = LowPassFilter::new(14000.0, 48000.0);

        hp.process(1.0);
        lp.process(1.0);

        hp.reset();
        lp.reset();

        // After reset, filters should be in initial state
        assert_eq!(hp.prev_input, 0.0);
        assert_eq!(hp.prev_output, 0.0);
        assert_eq!(lp.prev_sample, 0.0);
    }

    #[test]
    fn test_rate_constants() {
        assert_eq!(APU_RATE_NTSC, 1_789_773);
        assert_eq!(APU_RATE_PAL, 1_662_607);
        assert_eq!(SAMPLE_RATE_44100, 44_100);
        assert_eq!(SAMPLE_RATE_48000, 48_000);
    }

    #[test]
    fn test_linear_resampler_44100() {
        let mut resampler = LinearResampler::new(44100);

        // 1ms of audio
        for _ in 0..1790 {
            resampler.add_sample(0.5);
        }

        let len = resampler.len();
        assert!((43..=45).contains(&len), "Expected ~44 samples, got {len}");
    }

    #[test]
    fn test_high_quality_resampler_fallback() {
        // This test verifies the resampler initializes (either with rubato or fallback)
        let resampler = HighQualityResampler::new(48000);
        assert!(resampler.output_rate() == 48000);
        // Either rubato works or we're using fallback - both are valid
    }

    #[test]
    fn test_high_quality_resampler_reset() {
        let mut resampler = HighQualityResampler::new(48000);

        // Add some samples
        for _ in 0..10000 {
            resampler.add_sample(0.5);
        }

        resampler.reset();
        assert!(resampler.is_empty());
    }
}
