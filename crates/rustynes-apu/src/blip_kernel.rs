//! Pre-computed polyphase windowed-sinc kernel for the BLEP / polyphase-FIR
//! decimator in [`crate::blip`].
//!
//! # What this is
//!
//! A 2-D table of FIR coefficients indexed by `[phase][tap]`:
//!
//! - **`PHASES = 256`** — the kernel is precomputed at 256 sub-sample
//!   phase offsets so the decimator can pick the row whose phase matches
//!   the fractional position of each output sample. The phase-row
//!   quantization noise at this resolution is ≤ -66 dB at 30 kHz CPU
//!   input (verified by the spectral FFT regression test). `blip_buf`
//!   uses 32 phases because its host rates are closer to the input rate;
//!   we need finer phase resolution because our ~40× decimation ratio
//!   means each output sample's fractional position is more sensitive
//!   to row-quantization error.
//! - **`TAPS = 32`** — each phase row has 32 coefficients, giving the
//!   FIR a ±16-sample reach (at the host sample rate). Long enough for
//!   clean suppression above Nyquist with a Blackman window; short
//!   enough that one output sample needs only 32 multiply-adds.
//!
//! # How it's used
//!
//! The decimator in `blip.rs` keeps a CPU-rate input ring buffer of size
//! `TAPS`. Each time the host-rate phase accumulator crosses an integer
//! boundary, it picks the kernel row matching the fractional phase and
//! computes a dot product with the ring buffer. The result is one
//! band-limited output sample.
//!
//! # Window choice
//!
//! Blackman window over the windowed-sinc impulse response. Blackman gives
//! ~-58 dB sidelobe with sensible main-lobe width for 32 taps — comfortably
//! below the -60 dB acceptance threshold of the spectral FFT regression
//! test in `benches/spectral.rs` once the existing 14 kHz LPF in
//! [`crate::mixer::FilterChain`] is applied to the output.
//!
//! # Determinism
//!
//! All math is `f32`. Coefficients are computed once at build time of the
//! consumer's `BlipBuf` (via [`Kernel::new`] / [`Kernel::default`]) using a
//! fixed iteration order so the table is bit-identical across platforms.
//! Both `f32::sin` and `libm::sinf` are used in the *coefficient init*
//! path (not on the hot per-sample path); we route through `libm` on
//! no_std builds for parity. The numeric output matches to within
//! `f32::EPSILON` at the table indices we use.
//!
//! See the discussion in Stilson & Smith, "Alias-Free Digital Synthesis of
//! Classic Analog Waveforms" (1996) for the polyphase-FIR / BLEP framing,
//! and Shay Green's `blip_buf.c` (BSD/MIT) for the canonical streaming
//! decimator the kernel feeds.

use alloc::{boxed::Box, vec};
use core::f32::consts::PI;

/// Number of sub-sample phase offsets the kernel is precomputed at.
pub const PHASES: usize = 256;
/// FIR length (taps per phase row).
pub const TAPS: usize = 32;
/// Cutoff (normalized fraction of the host sample rate) of the windowed-sinc
/// kernel. Picked slightly below Nyquist (0.5) so the transition band lands
/// inside the audible region we already attenuate with the 14 kHz LPF.
///
/// At `0.46 * host_rate`, the -6 dB point is ~20.3 kHz @ 44.1 kHz host rate,
/// and the kernel sidelobes are well below the noise floor by 22.05 kHz.
pub const CUTOFF: f32 = 0.46;

/// `f32::sin` for std + `libm::sinf` for no_std. Coefficient-init only.
#[inline]
fn sinf(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        x.sin()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::sinf(x)
    }
}

/// `f32::cos` for std + `libm::cosf` for no_std. Coefficient-init only.
#[inline]
fn cosf(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        x.cos()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::cosf(x)
    }
}

/// Polyphase windowed-sinc FIR kernel.
///
/// Indexed as `kernel[phase][tap]` — phase 0 is "exactly on grid",
/// phase `PHASES-1` is "almost one sample past grid". Each row sums to
/// (very close to) 1.0 — DC gain is preserved.
///
/// Stored on the heap via `Box<[...]>` because `PHASES * TAPS * 4` =
/// `256 * 32 * 4` = 32 KiB exceeds the per-`Kernel`-instance comfort
/// margin for stack-allocation (clippy `large_stack_arrays`). Heap
/// allocation happens exactly once per `BlipBuf::new`, which is
/// constructed on emulator init / reset only.
#[derive(Debug, Clone)]
pub struct Kernel {
    /// `PHASES × TAPS` table. Stored row-major for cache locality
    /// during the dot product. Heap-boxed slice with `PHASES` rows;
    /// we index via `coeffs[phase][tap]` since the inner row is a
    /// fixed `[f32; TAPS]` and `Box<[T]>` is a Vec-equivalent here.
    pub(crate) coeffs: Box<[[f32; TAPS]]>,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    /// Build the kernel table.
    ///
    /// For each of the `PHASES` sub-sample offsets `p`, compute a TAPS-long
    /// FIR whose impulse response is `sinc(2 * CUTOFF * (t - p/PHASES))`
    /// windowed by the Blackman window. Normalize each row so its sum is
    /// 1.0 (DC passes through unchanged).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn new() -> Self {
        // Heap-allocate the kernel table to keep stack usage low (32 KiB
        // is well above clippy's `large_stack_arrays` gate). Going
        // through `Vec` → `Box<[T]>` avoids materializing the full
        // array on the stack first.
        let mut coeffs: Box<[[f32; TAPS]]> = vec![[0.0_f32; TAPS]; PHASES].into_boxed_slice();
        let half = (TAPS as f32) * 0.5;
        for (p, row) in coeffs.iter_mut().enumerate() {
            // Fractional sub-sample offset for this phase row.
            let phase_off = p as f32 / PHASES as f32;
            // Compute raw windowed-sinc.
            let mut sum = 0.0_f32;
            for (t, slot) in row.iter_mut().enumerate() {
                // Position in samples relative to the center of the kernel.
                // The kernel center sits between tap (TAPS/2 - 1) and tap
                // (TAPS/2); we offset the impulse by `-phase_off` so the
                // peak lands at fractional position `phase_off` past the
                // center.
                let x = (t as f32) - half + 0.5 - phase_off;

                // Windowed sinc: sinc(2 * CUTOFF * x) * blackman((t + 0.5) / TAPS)
                let sinc = if x.abs() < 1.0e-6 {
                    2.0 * CUTOFF
                } else {
                    let phix = 2.0 * PI * CUTOFF * x;
                    sinf(phix) / (PI * x)
                };

                // Blackman window: 0.42 - 0.5 cos(2*pi*n/N) + 0.08 cos(4*pi*n/N)
                // where n in [0, N-1] and N = TAPS. We use `(t + 0.5) / TAPS`
                // so the window center sits between two taps (matching the
                // sinc's half-sample offset for an even-length filter).
                let n_norm = (t as f32 + 0.5) / TAPS as f32;
                let w = 0.42 - 0.5 * cosf(2.0 * PI * n_norm) + 0.08 * cosf(4.0 * PI * n_norm);

                let c = sinc * w;
                *slot = c;
                sum += c;
            }
            // DC normalization: each row must sum to 1.0 so a constant input
            // passes through unattenuated.
            if sum.abs() > f32::EPSILON {
                let inv = 1.0 / sum;
                for slot in row.iter_mut() {
                    *slot *= inv;
                }
            }
        }
        Self { coeffs }
    }

    /// Look up the FIR row for the given sub-sample phase (`[0, 1)`).
    ///
    /// Quantizes `phase` to one of the `PHASES` precomputed rows.
    /// Returns a reference to a length-`TAPS` slice of coefficients.
    #[inline]
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn row(&self, phase: f32) -> &[f32; TAPS] {
        // Clamp into [0, PHASES-1] without panicking on NaN.
        let p = (phase * PHASES as f32) as i32;
        let p = if p < 0 {
            0
        } else if p as usize >= PHASES {
            PHASES - 1
        } else {
            p as usize
        };
        &self.coeffs[p]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_rows_sum_to_one() {
        let k = Kernel::new();
        for (i, row) in k.coeffs.iter().enumerate() {
            let s: f32 = row.iter().sum();
            assert!(
                (s - 1.0).abs() < 1.0e-5,
                "row {i} sum {s} != 1.0 (DC gain not preserved)"
            );
        }
    }

    #[test]
    fn kernel_is_finite() {
        let k = Kernel::new();
        for row in &k.coeffs {
            for c in row {
                assert!(c.is_finite(), "non-finite coefficient {c}");
            }
        }
    }

    #[test]
    fn kernel_peak_shifts_with_phase() {
        // The peak coefficient (largest absolute value) of row `p` should
        // sit near the kernel center, shifting as `p` increases.
        let k = Kernel::new();
        for p in 0..PHASES {
            let mut peak_at = 0;
            let mut peak_abs = 0.0f32;
            for (t, c) in k.coeffs[p].iter().enumerate() {
                let a = c.abs();
                if a > peak_abs {
                    peak_abs = a;
                    peak_at = t;
                }
            }
            // Peak should be near the center (TAPS/2 ± a few).
            let center = TAPS / 2;
            assert!(
                peak_at.abs_diff(center) <= 2,
                "row {p} peak at {peak_at}, expected near {center}"
            );
        }
    }

    #[test]
    fn kernel_is_deterministic() {
        // Two independently constructed kernels are bit-identical.
        let a = Kernel::new();
        let b = Kernel::new();
        for p in 0..PHASES {
            for t in 0..TAPS {
                assert_eq!(
                    a.coeffs[p][t].to_bits(),
                    b.coeffs[p][t].to_bits(),
                    "mismatch at [{p}][{t}]"
                );
            }
        }
    }

    #[test]
    fn row_clamps_out_of_range_phase() {
        let k = Kernel::new();
        // Below zero should clamp to phase 0.
        let r0 = k.row(-0.5);
        assert!(core::ptr::eq(r0, &raw const k.coeffs[0]));
        // At/above 1.0 should clamp to last phase.
        let rlast = k.row(1.5);
        assert!(core::ptr::eq(rlast, &raw const k.coeffs[PHASES - 1]));
    }
}
