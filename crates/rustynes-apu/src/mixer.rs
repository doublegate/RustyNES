//! Non-linear audio mixer for NES APU channels
//!
//! The NES APU uses non-linear mixing curves that approximate the behavior of analog
//! circuits. This module implements hardware-accurate mixing using lookup tables.
//!
//! # Mixing Formula
//!
//! The APU combines channels using two separate non-linear formulas:
//!
//! **Pulse channels:**
//! ```text
//! pulse_out = 95.88 / ((8128.0 / (pulse1 + pulse2)) + 100.0)
//! ```
//!
//! **Triangle, Noise, and DMC (TND) channels:**
//! ```text
//! tnd_out = 159.79 / ((1.0 / (triangle/8227 + noise/12241 + dmc/22638)) + 100.0)
//! ```
//!
//! Final output: `output = pulse_out + tnd_out`
//!
//! # Performance
//!
//! Pre-computed lookup tables eliminate expensive floating-point division during
//! mixing, reducing the mixing operation to two array lookups and one addition.
//!
//! # Example
//!
//! ```rust
//! use rustynes_apu::mixer::Mixer;
//!
//! let mixer = Mixer::new();
//!
//! // Mix all channels (pulse1=15, pulse2=15, triangle=15, noise=10, dmc=64)
//! let output = mixer.mix(15, 15, 15, 10, 64);
//! assert!(output > 0.0 && output < 2.0);
//! ```

/// Non-linear mixer for NES APU audio channels
///
/// Combines 5 channels (2 pulse, 1 triangle, 1 noise, 1 DMC) using hardware-accurate
/// non-linear mixing curves.
pub struct Mixer {
    /// Pulse mixing lookup table (31 entries: 0-30)
    ///
    /// Index = pulse1 + pulse2 (each 0-15)
    pulse_table: [f32; 31],

    /// TND mixing lookup table (203 entries: 0-202)
    ///
    /// Index = 3*triangle + 2*noise + dmc
    /// - triangle: 0-15 (multiply by 3 for weight)
    /// - noise: 0-15 (multiply by 2 for weight)
    /// - dmc: 0-127 (1:1 weight)
    tnd_table: [f32; 203],
}

impl Mixer {
    /// Create a new mixer with pre-computed lookup tables
    #[must_use]
    pub fn new() -> Self {
        Self {
            pulse_table: Self::generate_pulse_table(),
            tnd_table: Self::generate_tnd_table(),
        }
    }

    /// Generate pulse mixing lookup table
    ///
    /// Uses formula: `output = 95.88 / ((8128.0 / input) + 100.0)`
    ///
    /// Special case: index 0 = 0.0 (silence)
    fn generate_pulse_table() -> [f32; 31] {
        let mut table = [0.0; 31];

        for (i, entry) in table.iter_mut().enumerate() {
            if i == 0 {
                *entry = 0.0;
            } else {
                #[allow(clippy::cast_precision_loss)]
                let i_f32 = i as f32;
                *entry = 95.88 / ((8128.0 / i_f32) + 100.0);
            }
        }

        table
    }

    /// Generate TND (Triangle/Noise/DMC) mixing lookup table
    ///
    /// Uses hardware-accurate formula from `NESdev`:
    /// ```text
    /// output = 159.79 / ((1 / (triangle/8227 + noise/12241 + dmc/22638)) + 100)
    /// ```
    ///
    /// The table is indexed by a weighted sum: `3*triangle + 2*noise + dmc`
    /// This allows us to pre-compute the complex non-linear mixing operation.
    ///
    /// To reconstruct the individual channel contributions from the index:
    /// - Maximum index: 3*15 + 2*15 + 127 = 202
    /// - We need to compute: triangle/8227 + noise/12241 + dmc/22638
    /// - But we only have the weighted sum, so we use approximation weights
    ///
    /// Simplified implementation: Index directly represents the combined
    /// contribution scaled appropriately for the mixing curve.
    ///
    /// Special case: index 0 = 0.0 (silence)
    fn generate_tnd_table() -> [f32; 203] {
        let mut table = [0.0; 203];

        // Pre-compute for all possible combinations
        // Index = 3*triangle + 2*noise + dmc
        for triangle in 0..=15 {
            for noise in 0..=15 {
                for dmc in 0..=127 {
                    #[allow(clippy::cast_sign_loss)]
                    let index = (3 * triangle + 2 * noise + dmc) as usize;

                    #[allow(clippy::cast_precision_loss)]
                    let triangle_contrib = triangle as f32 / 8227.0;
                    #[allow(clippy::cast_precision_loss)]
                    let noise_contrib = noise as f32 / 12241.0;
                    #[allow(clippy::cast_precision_loss)]
                    let dmc_contrib = dmc as f32 / 22638.0;

                    let tnd_sum = triangle_contrib + noise_contrib + dmc_contrib;

                    if tnd_sum == 0.0 {
                        table[index] = 0.0;
                    } else {
                        table[index] = 159.79 / ((1.0 / tnd_sum) + 100.0);
                    }
                }
            }
        }

        table
    }

    /// Mix all five APU channels using hardware-accurate non-linear curves
    ///
    /// # Arguments
    ///
    /// * `pulse1` - Pulse channel 1 output (0-15)
    /// * `pulse2` - Pulse channel 2 output (0-15)
    /// * `triangle` - Triangle channel output (0-15)
    /// * `noise` - Noise channel output (0-15)
    /// * `dmc` - DMC channel output (0-127)
    ///
    /// # Returns
    ///
    /// Mixed audio sample in range approximately [0.0, 2.0]
    ///
    /// # Panics
    ///
    /// Panics if any channel output exceeds its valid range (debug builds only).
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::mixer::Mixer;
    ///
    /// let mixer = Mixer::new();
    ///
    /// // All channels at maximum (hardware-accurate formula produces ~0.999)
    /// let output = mixer.mix(15, 15, 15, 15, 127);
    /// assert!(output > 0.9 && output < 1.0);
    ///
    /// // All channels silent
    /// let silence = mixer.mix(0, 0, 0, 0, 0);
    /// assert_eq!(silence, 0.0);
    /// ```
    #[must_use]
    pub fn mix(&self, pulse1: u8, pulse2: u8, triangle: u8, noise: u8, dmc: u8) -> f32 {
        // Validate inputs (debug only, optimized out in release)
        debug_assert!(pulse1 <= 15, "pulse1 out of range: {pulse1}");
        debug_assert!(pulse2 <= 15, "pulse2 out of range: {pulse2}");
        debug_assert!(triangle <= 15, "triangle out of range: {triangle}");
        debug_assert!(noise <= 15, "noise out of range: {noise}");
        debug_assert!(dmc <= 127, "dmc out of range: {dmc}");

        // Pulse mixer: simple addition
        let pulse_index = (pulse1 + pulse2) as usize;
        let pulse_out = self.pulse_table[pulse_index];

        // TND mixer: weighted sum
        // Weights: triangle×3, noise×2, dmc×1
        let tnd_index = (3 * u16::from(triangle) + 2 * u16::from(noise) + u16::from(dmc)) as usize;
        let tnd_out = self.tnd_table[tnd_index];

        // Combine outputs
        pulse_out + tnd_out
    }

    /// Mix channels using linear approximation (for comparison/testing)
    ///
    /// This is less accurate than non-linear mixing but useful for debugging
    /// and comparing against other emulators.
    ///
    /// # Arguments
    ///
    /// Same as [`mix()`](Self::mix)
    ///
    /// # Returns
    ///
    /// Mixed audio sample using linear approximation
    ///
    /// # Example
    ///
    /// ```rust
    /// use rustynes_apu::mixer::Mixer;
    ///
    /// // Linear mixing is a fast approximation
    /// let output_linear = Mixer::mix_linear(15, 15, 15, 10, 64);
    ///
    /// // Result is in the 0.0-1.0 range
    /// assert!(output_linear > 0.0 && output_linear < 1.0);
    /// ```
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn mix_linear(pulse1: u8, pulse2: u8, triangle: u8, noise: u8, dmc: u8) -> f32 {
        let pulse = f32::from(pulse1 + pulse2) * 0.00752;
        let tnd = (f32::from(triangle) * 0.00851)
            + (f32::from(noise) * 0.00494)
            + (f32::from(dmc) * 0.00335);

        pulse + tnd
    }
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_mixer_creation() {
        let mixer = Mixer::new();

        // Verify table sizes
        assert_eq!(mixer.pulse_table.len(), 31);
        assert_eq!(mixer.tnd_table.len(), 203);

        // Verify silence entries
        assert_eq!(mixer.pulse_table[0], 0.0);
        assert_eq!(mixer.tnd_table[0], 0.0);
    }

    #[test]
    fn test_mixer_silence() {
        let mixer = Mixer::new();
        let output = mixer.mix(0, 0, 0, 0, 0);
        assert_eq!(output, 0.0);
    }

    #[test]
    fn test_mixer_pulse_only() {
        let mixer = Mixer::new();

        // Single pulse channel
        let output1 = mixer.mix(15, 0, 0, 0, 0);
        assert!(output1 > 0.0);
        assert!(output1 < 1.0);

        // Both pulse channels
        let output2 = mixer.mix(15, 15, 0, 0, 0);
        assert!(output2 > output1);
        assert!(output2 < 1.0);
    }

    #[test]
    fn test_mixer_triangle_only() {
        let mixer = Mixer::new();
        let output = mixer.mix(0, 0, 15, 0, 0);
        assert!(output > 0.0);
        assert!(output < 2.0);
    }

    #[test]
    fn test_mixer_noise_only() {
        let mixer = Mixer::new();
        let output = mixer.mix(0, 0, 0, 15, 0);
        assert!(output > 0.0);
        assert!(output < 2.0);
    }

    #[test]
    fn test_mixer_dmc_only() {
        let mixer = Mixer::new();
        let output = mixer.mix(0, 0, 0, 0, 127);
        assert!(output > 0.0);
        assert!(output < 2.0);
    }

    #[test]
    fn test_mixer_max_output() {
        let mixer = Mixer::new();
        let output = mixer.mix(15, 15, 15, 15, 127);
        assert!(output > 0.0);
        assert!(output < 2.0);
    }

    #[test]
    fn test_mixer_incremental() {
        let mixer = Mixer::new();

        // Adding channels should increase output
        let out1 = mixer.mix(15, 0, 0, 0, 0);
        let out2 = mixer.mix(15, 15, 0, 0, 0);
        let out3 = mixer.mix(15, 15, 15, 0, 0);
        let out4 = mixer.mix(15, 15, 15, 15, 0);
        let out5 = mixer.mix(15, 15, 15, 15, 127);

        assert!(out2 > out1);
        assert!(out3 > out2);
        assert!(out4 > out3);
        assert!(out5 > out4);
    }

    #[test]
    fn test_mixer_linear_approximation() {
        // Linear should produce non-zero output
        let output = Mixer::mix_linear(15, 15, 15, 10, 64);
        assert!(output > 0.0);
        assert!(output < 2.0);

        // Silence should produce zero
        let silence = Mixer::mix_linear(0, 0, 0, 0, 0);
        assert_eq!(silence, 0.0);
    }

    #[test]
    fn test_mixer_linear_vs_nonlinear() {
        let mixer = Mixer::new();

        // Compare outputs (should be similar but not identical)
        let linear = Mixer::mix_linear(10, 10, 10, 10, 64);
        let nonlinear = mixer.mix(10, 10, 10, 10, 64);

        // Should be within reasonable range of each other (linear is simpler approximation)
        // Non-linear is more accurate, so we just verify both produce reasonable output
        assert!(linear > 0.0 && linear < 2.0);
        assert!(nonlinear > 0.0 && nonlinear < 2.0);
    }

    #[test]
    fn test_pulse_table_values() {
        let mixer = Mixer::new();

        // Check known values
        assert_eq!(mixer.pulse_table[0], 0.0);
        assert!(mixer.pulse_table[1] > 0.0);
        assert!(mixer.pulse_table[30] > mixer.pulse_table[1]);

        // Table should be monotonically increasing
        for i in 1..30 {
            assert!(
                mixer.pulse_table[i + 1] > mixer.pulse_table[i],
                "Table not monotonic at index {i}"
            );
        }
    }

    #[test]
    fn test_tnd_table_values() {
        let mixer = Mixer::new();

        // Check known values
        assert_eq!(mixer.tnd_table[0], 0.0);
        assert!(mixer.tnd_table[1] > 0.0);
        assert!(mixer.tnd_table[202] > mixer.tnd_table[1]);

        // Table should be monotonically increasing
        for i in 1..202 {
            assert!(
                mixer.tnd_table[i + 1] > mixer.tnd_table[i],
                "Table not monotonic at index {i}"
            );
        }
    }

    #[test]
    fn test_tnd_index_calculation() {
        let mixer = Mixer::new();

        // TND index = 3*tri + 2*noise + dmc
        // Max: 3*15 + 2*15 + 127 = 45 + 30 + 127 = 202
        let output = mixer.mix(0, 0, 15, 15, 127);
        assert!(output > 0.0);

        // Verify index doesn't overflow
        // (would panic in debug, undefined in release)
    }

    #[test]
    fn test_mixer_symmetry() {
        let mixer = Mixer::new();

        // Pulse channels should be interchangeable
        let out1 = mixer.mix(10, 5, 0, 0, 0);
        let out2 = mixer.mix(5, 10, 0, 0, 0);
        assert_eq!(out1, out2);
    }

    #[test]
    fn test_mixer_edge_cases() {
        let mixer = Mixer::new();

        // Maximum individual channels
        let _out1 = mixer.mix(15, 0, 0, 0, 0);
        let _out2 = mixer.mix(0, 15, 0, 0, 0);
        let _out3 = mixer.mix(0, 0, 15, 0, 0);
        let _out4 = mixer.mix(0, 0, 0, 15, 0);
        let _out5 = mixer.mix(0, 0, 0, 0, 127);

        // All should produce valid output (no panics)
    }

    #[test]
    fn test_default_trait() {
        let mixer = Mixer::default();
        let output = mixer.mix(10, 10, 10, 10, 64);
        assert!(output > 0.0);
    }
}
