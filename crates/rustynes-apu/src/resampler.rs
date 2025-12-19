//! Audio resampler for converting APU rate to target sample rate
//!
//! The NES APU runs at approximately 1.789773 MHz (NTSC) or 1.662607 MHz (PAL),
//! but modern audio systems expect 44.1 kHz or 48 kHz. This module resamples
//! APU output to the target rate using linear interpolation.
//!
//! # Resampling Strategy
//!
//! 1. APU produces samples at ~1.79 MHz
//! 2. Resampler accumulates samples and tracks fractional time
//! 3. When time threshold is crossed, output samples using linear interpolation
//! 4. Output buffer stores samples for audio device consumption
//!
//! # Example
//!
//! ```rust
//! use rustynes_apu::resampler::Resampler;
//!
//! let mut resampler = Resampler::new(48000); // 48 kHz output
//!
//! // Add APU samples (called every CPU cycle at ~1.79 MHz)
//! for i in 0..10000 {
//!     let sample = (i as f32 * 0.001).sin(); // Example waveform
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

/// Audio resampler with linear interpolation
///
/// Converts APU rate (~1.79 MHz) to target sample rate (typically 48 kHz).
pub struct Resampler {
    /// Target output sample rate (e.g., 48000 Hz)
    output_rate: u32,

    /// APU sample rate (1.789773 MHz for NTSC)
    input_rate: u32,

    /// Fractional time accumulator for sample generation
    time_accumulator: f32,

    /// Previous sample for linear interpolation
    prev_sample: f32,

    /// Output sample buffer
    buffer: Vec<f32>,
}

impl Resampler {
    /// NTSC APU sample rate (CPU clock: 1.789773 MHz)
    pub const APU_RATE_NTSC: u32 = 1_789_773;

    /// PAL APU sample rate (CPU clock: 1.662607 MHz)
    pub const APU_RATE_PAL: u32 = 1_662_607;

    /// Common output sample rate (CD quality)
    pub const SAMPLE_RATE_44100: u32 = 44_100;

    /// Common output sample rate (professional audio)
    pub const SAMPLE_RATE_48000: u32 = 48_000;

    /// Create a new resampler with specified output rate
    ///
    /// # Arguments
    ///
    /// * `output_rate` - Target sample rate (typically 44100 or 48000 Hz)
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// let resampler = Resampler::new(48000);
    /// ```
    #[must_use]
    pub fn new(output_rate: u32) -> Self {
        Self {
            output_rate,
            input_rate: Self::APU_RATE_NTSC,
            time_accumulator: 0.0,
            prev_sample: 0.0,
            buffer: Vec::with_capacity(2048),
        }
    }

    /// Create a resampler with custom input rate (for PAL systems)
    ///
    /// # Arguments
    ///
    /// * `output_rate` - Target sample rate (Hz)
    /// * `input_rate` - APU sample rate (Hz, typically 1789773 NTSC or 1662607 PAL)
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// // PAL system
    /// let resampler = Resampler::with_input_rate(48000, Resampler::APU_RATE_PAL);
    /// ```
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
    ///
    /// This should be called every APU cycle (~1.79 MHz). The resampler will
    /// generate output samples as needed using linear interpolation.
    ///
    /// # Arguments
    ///
    /// * `sample` - Audio sample from APU mixer (typically 0.0-2.0 range)
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// let mut resampler = Resampler::new(48000);
    ///
    /// // Add samples from APU
    /// for _ in 0..1789 {
    ///     resampler.add_sample(0.5); // 1ms of audio at APU rate
    /// }
    ///
    /// // Should produce approximately 48 output samples (1ms at 48kHz)
    /// assert!(resampler.samples().len() >= 47 && resampler.samples().len() <= 49);
    /// ```
    pub fn add_sample(&mut self, sample: f32) {
        #[allow(clippy::cast_precision_loss)]
        let time_step = self.output_rate as f32 / self.input_rate as f32;
        self.time_accumulator += time_step;

        // Generate output samples when time threshold is crossed
        while self.time_accumulator >= 1.0 {
            // Linear interpolation between previous and current sample
            let t = self.time_accumulator - 1.0;
            let output = self.prev_sample + (sample - self.prev_sample) * t;

            self.buffer.push(output);
            self.time_accumulator -= 1.0;
        }

        self.prev_sample = sample;
    }

    /// Get reference to output samples
    ///
    /// Returns samples ready for audio device consumption. Call [`clear()`](Self::clear)
    /// after consuming samples.
    ///
    /// # Returns
    ///
    /// Slice of f32 audio samples at target sample rate
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// let mut resampler = Resampler::new(48000);
    ///
    /// // Add samples...
    /// for _ in 0..10000 {
    ///     resampler.add_sample(0.5);
    /// }
    ///
    /// // Retrieve for audio output
    /// let samples = resampler.samples();
    /// // Send to audio device...
    ///
    /// // Clear after consuming
    /// resampler.clear();
    /// ```
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.buffer
    }

    /// Clear the output buffer
    ///
    /// Call this after consuming samples to free memory and prepare for next batch.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// let mut resampler = Resampler::new(48000);
    ///
    /// // Add enough samples to produce output (resampler downsamples from ~1.79 MHz)
    /// for _ in 0..100 {
    ///     resampler.add_sample(0.5);
    /// }
    ///
    /// // After clearing, buffer should be empty
    /// resampler.clear();
    /// assert!(resampler.samples().is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Check if resampler has at least `min_samples` ready
    ///
    /// Useful for determining when to pull samples for audio output.
    ///
    /// # Arguments
    ///
    /// * `min_samples` - Minimum number of samples required
    ///
    /// # Returns
    ///
    /// `true` if buffer contains at least `min_samples`
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::Resampler;
    ///
    /// let mut resampler = Resampler::new(48000);
    ///
    /// // Audio callback wants 512 samples
    /// while !resampler.is_ready(512) {
    ///     // Add more APU samples...
    ///     resampler.add_sample(0.5);
    /// }
    /// ```
    #[must_use]
    pub fn is_ready(&self, min_samples: usize) -> bool {
        self.buffer.len() >= min_samples
    }

    /// Get the current number of samples in the buffer
    ///
    /// # Returns
    ///
    /// Number of output samples ready
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty
    ///
    /// # Returns
    ///
    /// `true` if no samples are buffered
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Reset the resampler state
    ///
    /// Clears buffer and resets internal state. Useful when seeking or resetting emulation.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.time_accumulator = 0.0;
        self.prev_sample = 0.0;
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
}

impl Default for Resampler {
    /// Create a resampler with default settings (48 kHz output, NTSC APU rate)
    fn default() -> Self {
        Self::new(Self::SAMPLE_RATE_48000)
    }
}

/// Simple low-pass filter for reducing aliasing
///
/// Uses a first-order IIR filter: `output = prev + α * (input - prev)`
///
/// # Example
///
/// ```rust
/// use rustynes_apu::resampler::LowPassFilter;
///
/// let mut filter = LowPassFilter::new(14000.0, 48000.0);
///
/// let filtered = filter.process(0.8);
/// assert!(filtered >= 0.0 && filtered <= 1.0);
/// ```
pub struct LowPassFilter {
    /// Previous output sample
    prev_sample: f32,

    /// Smoothing factor (0.0-1.0)
    alpha: f32,
}

impl LowPassFilter {
    /// Create a new low-pass filter
    ///
    /// # Arguments
    ///
    /// * `cutoff_hz` - Cutoff frequency in Hz (e.g., 14000.0)
    /// * `sample_rate` - Sample rate in Hz (e.g., 48000.0)
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::LowPassFilter;
    ///
    /// // 14 kHz cutoff at 48 kHz sample rate
    /// let filter = LowPassFilter::new(14000.0, 48000.0);
    /// ```
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
        let dt = 1.0 / sample_rate;
        let alpha = dt / (rc + dt);

        Self {
            prev_sample: 0.0,
            alpha,
        }
    }

    /// Process a sample through the filter
    ///
    /// # Arguments
    ///
    /// * `sample` - Input sample
    ///
    /// # Returns
    ///
    /// Filtered sample
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::resampler::LowPassFilter;
    ///
    /// let mut filter = LowPassFilter::new(14000.0, 48000.0);
    ///
    /// let samples = [0.5, 0.8, 0.3, 0.9];
    /// let filtered: Vec<f32> = samples.iter()
    ///     .map(|&s| filter.process(s))
    ///     .collect();
    /// ```
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_creation() {
        let resampler = Resampler::new(48000);
        assert_eq!(resampler.output_rate(), 48000);
        assert_eq!(resampler.input_rate(), Resampler::APU_RATE_NTSC);
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_resampler_with_input_rate() {
        let resampler = Resampler::with_input_rate(48000, Resampler::APU_RATE_PAL);
        assert_eq!(resampler.output_rate(), 48000);
        assert_eq!(resampler.input_rate(), Resampler::APU_RATE_PAL);
    }

    #[test]
    fn test_resampler_add_sample() {
        let mut resampler = Resampler::new(48000);

        // Add 1ms of audio (should produce ~48 samples)
        for _ in 0..1790 {
            resampler.add_sample(0.5);
        }

        let len = resampler.len();
        assert!(len >= 47 && len <= 49, "Expected ~48 samples, got {len}");
    }

    #[test]
    fn test_resampler_clear() {
        let mut resampler = Resampler::new(48000);

        // Add enough samples to generate output
        for _ in 0..100 {
            resampler.add_sample(0.5);
        }
        assert!(!resampler.is_empty());

        resampler.clear();
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_resampler_is_ready() {
        let mut resampler = Resampler::new(48000);

        assert!(!resampler.is_ready(100));

        // Add enough samples
        for _ in 0..5000 {
            resampler.add_sample(0.5);
        }

        assert!(resampler.is_ready(100));
    }

    #[test]
    fn test_resampler_reset() {
        let mut resampler = Resampler::new(48000);

        for _ in 0..1000 {
            resampler.add_sample(0.5);
        }

        assert!(!resampler.is_empty());

        resampler.reset();
        assert!(resampler.is_empty());
    }

    #[test]
    fn test_resampler_interpolation() {
        let mut resampler = Resampler::new(48000);

        // Add linearly increasing samples to test interpolation
        for i in 0..1000 {
            #[allow(clippy::cast_precision_loss)]
            let sample = (i % 100) as f32 / 100.0;
            resampler.add_sample(sample);
        }

        // Output should have interpolated values
        let samples = resampler.samples();
        assert!(!samples.is_empty());

        // With a linear ramp, we should have a variety of values
        let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(max > min, "Expected varying output values");
    }

    #[test]
    fn test_resampler_dc_signal() {
        let mut resampler = Resampler::new(48000);

        // Constant signal
        for _ in 0..10000 {
            resampler.add_sample(0.5);
        }

        // All outputs should be approximately 0.5
        let samples = resampler.samples();
        for &sample in samples {
            assert!((sample - 0.5).abs() < 0.01, "Sample: {sample}");
        }
    }

    #[test]
    fn test_resampler_default() {
        let resampler = Resampler::default();
        assert_eq!(resampler.output_rate(), 48000);
        assert_eq!(resampler.input_rate(), Resampler::APU_RATE_NTSC);
    }

    #[test]
    fn test_low_pass_filter_creation() {
        let filter = LowPassFilter::new(14000.0, 48000.0);
        assert!(filter.alpha > 0.0 && filter.alpha < 1.0);
    }

    #[test]
    fn test_low_pass_filter_process() {
        let mut filter = LowPassFilter::new(14000.0, 48000.0);

        let output = filter.process(1.0);
        assert!(output > 0.0 && output <= 1.0);

        // Second sample should move closer to input
        let output2 = filter.process(1.0);
        assert!(output2 > output);
    }

    #[test]
    fn test_low_pass_filter_smoothing() {
        let mut filter = LowPassFilter::new(14000.0, 48000.0);

        // Step function: 0 -> 1
        let out1 = filter.process(1.0);
        assert!(out1 < 1.0, "Filter should smooth step");

        let out2 = filter.process(1.0);
        assert!(out2 > out1, "Output should converge");
    }

    #[test]
    fn test_low_pass_filter_reset() {
        let mut filter = LowPassFilter::new(14000.0, 48000.0);

        filter.process(1.0);
        filter.reset();

        let output = filter.process(1.0);
        assert!(output < 1.0, "Filter should be reset");
    }

    #[test]
    fn test_resampler_rate_constants() {
        assert_eq!(Resampler::APU_RATE_NTSC, 1_789_773);
        assert_eq!(Resampler::APU_RATE_PAL, 1_662_607);
        assert_eq!(Resampler::SAMPLE_RATE_44100, 44_100);
        assert_eq!(Resampler::SAMPLE_RATE_48000, 48_000);
    }

    #[test]
    fn test_resampler_44100() {
        let mut resampler = Resampler::new(44100);

        // 1ms of audio
        for _ in 0..1790 {
            resampler.add_sample(0.5);
        }

        let len = resampler.len();
        assert!(len >= 43 && len <= 45, "Expected ~44 samples, got {len}");
    }
}
