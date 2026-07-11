//! Non-linear lookup-table mixer + analog-style filter chain.
//!
//! Per `docs/apu-2a03.md` §Mixer.  Two stages:
//!
//! 1. **Mixer** — non-linear sum of channel outputs into a normalized
//!    floating-point sample in `[0.0, ~1.0]`.  Implemented as two lookup
//!    tables computed once at construction.
//!
//! 2. **Filter chain** — first-order high-pass at 90 Hz, first-order high-pass
//!    at 440 Hz, first-order low-pass at 14 kHz.  Applied at the host sample
//!    rate (44.1 kHz default; configurable).  Bilinear-transform coefficients.

use core::f32::consts::PI;

// `f32::exp` is in `std::f32::FloatCore` (auto-imported in libstd) but not in
// `core` (no_std). We route through `libm::expf` so the same math compiles on
// both desktop and the `thumbv7em-none-eabihf` no_std target. The numeric
// output matches `f32::exp` for the inputs we use (filter coefficient
// initialization only; not on the per-sample hot path).
#[inline]
fn expf(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        x.exp()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::expf(x)
    }
}

/// Pre-computed mixer state.
#[derive(Debug, Clone)]
pub struct Mixer {
    /// `pulse_table[i]` for `i = pulse1 + pulse2`, 0..=30.
    pulse_table: [f32; 31],
    /// `tnd_table[i]` for `i = 3*tri + 2*noise + dmc`, 0..=202.
    tnd_table: [f32; 203],
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    /// Build the lookup tables.  Closed-form formulas from blargg's
    /// "APU Mixer" docs.
    #[must_use]
    pub fn new() -> Self {
        let mut pulse_table = [0.0f32; 31];
        for (i, slot) in pulse_table.iter_mut().enumerate().skip(1) {
            #[allow(clippy::cast_precision_loss)]
            let n = i as f32;
            *slot = 95.52 / (8128.0 / n + 100.0);
        }
        let mut tnd_table = [0.0f32; 203];
        for (i, slot) in tnd_table.iter_mut().enumerate().skip(1) {
            #[allow(clippy::cast_precision_loss)]
            let n = i as f32;
            *slot = 163.67 / (24329.0 / n + 100.0);
        }
        Self {
            pulse_table,
            tnd_table,
        }
    }

    /// Mix one sample.  Inputs are the per-cycle channel outputs:
    /// pulse 1/2 (0..=15), triangle (0..=15), noise (0..=15), dmc (0..=127).
    /// Returns a value in `[0.0, ~1.0]`.
    #[must_use]
    pub fn mix(&self, p1: u8, p2: u8, tri: u8, noise: u8, dmc: u8) -> f32 {
        let p_idx = (p1 + p2) as usize;
        let t_idx = (3 * u16::from(tri) + 2 * u16::from(noise) + u16::from(dmc)) as usize;
        // Indexing is safe since p1+p2 <= 30 and 3*tri+2*noise+dmc <= 3*15+2*15+127 = 202.
        self.pulse_table[p_idx] + self.tnd_table[t_idx]
    }
}

/// Single-pole IIR filter.  `lpf` flag controls whether it's a low-pass or
/// high-pass.
///
/// Bilinear-transform of the analog single-pole prototype.  See nesdev wiki
/// "APU Mixer" §Emulation.
#[derive(Debug, Clone, Copy)]
pub struct OnePole {
    /// Filter coefficient.  For HPF: `b1 = exp(-2*pi*fc/fs)`.  For LPF:
    /// `a0 = 1 - exp(-2*pi*fc/fs)`.
    pub(crate) coeff: f32,
    /// Last input sample.
    pub(crate) prev_in: f32,
    /// Last output sample.
    pub(crate) prev_out: f32,
    /// Filter mode.
    pub(crate) is_hpf: bool,
}

impl OnePole {
    /// New high-pass filter at `cutoff` Hz, sample rate `fs` Hz.
    #[must_use]
    pub fn high_pass(cutoff: f32, fs: f32) -> Self {
        // y[n] = b1 * (y[n-1] + x[n] - x[n-1])
        // b1 = exp(-2*pi*fc/fs)
        let coeff = expf(-2.0 * PI * cutoff / fs);
        Self {
            coeff,
            prev_in: 0.0,
            prev_out: 0.0,
            is_hpf: true,
        }
    }

    /// New low-pass filter at `cutoff` Hz, sample rate `fs` Hz.
    #[must_use]
    pub fn low_pass(cutoff: f32, fs: f32) -> Self {
        // y[n] = y[n-1] + a0 * (x[n] - y[n-1])
        let a0 = 1.0 - expf(-2.0 * PI * cutoff / fs);
        Self {
            coeff: a0,
            prev_in: 0.0,
            prev_out: 0.0,
            is_hpf: false,
        }
    }

    /// Process one sample.
    pub fn process(&mut self, x: f32) -> f32 {
        let y = if self.is_hpf {
            self.coeff * (self.prev_out + x - self.prev_in)
        } else {
            self.prev_out + self.coeff * (x - self.prev_out)
        };
        self.prev_in = x;
        self.prev_out = y;
        y
    }

    /// Reset filter state.
    pub fn reset(&mut self) {
        self.prev_in = 0.0;
        self.prev_out = 0.0;
    }
}

/// v2.1.3 — the analog output-filter model the APU emulates.
///
/// The console hardware genuinely differs here (nesdev "APU Mixer"): the NES
/// front-loader's RF/composite circuit high-passes aggressively, while the
/// Famicom uses only a gentle 37 Hz high-pass. `Clean` is a modern full-range
/// option (no aggressive high-pass — the character Mesen2 / FCEUX / Nestopia
/// produce, which omit the 90/440 Hz cascade). This only affects the tonal
/// balance of the output; it never changes channel content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterModel {
    /// **NES front-loader** — HPF 90 Hz → HPF 440 Hz → LPF 14 kHz (nesdev). The
    /// authentic (and thinnest) output; byte-identical to ares/tetanes. Default.
    #[default]
    NesRf,
    /// **Famicom** — a single HPF ~37 Hz → LPF 14 kHz (nesdev Famicom spec).
    /// Much fuller low end than the NES RF circuit (drops the 440 Hz HPF).
    Famicom,
    /// **Clean / full-range** — only a gentle ~10 Hz DC-block HPF → LPF 14 kHz.
    /// Keeps all bass; closest to Mesen2 / FCEUX (which apply no high-pass).
    Clean,
}

/// 3-stage filter chain. The stages are model-dependent (see [`FilterModel`]);
/// the default [`FilterModel::NesRf`] is HPF 90 Hz → HPF 440 Hz → LPF 14 kHz.
#[derive(Debug, Clone, Copy)]
pub struct FilterChain {
    pub(crate) hp1: OnePole,
    pub(crate) hp2: OnePole,
    pub(crate) lp: OnePole,
}

impl FilterChain {
    /// Build the default (NES front-loader) filter chain at `sample_rate` Hz.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self::for_model(sample_rate, FilterModel::NesRf)
    }

    /// Build the filter chain for a specific [`FilterModel`] at `sample_rate` Hz.
    ///
    /// The struct always keeps three one-pole stages; the softer models neutralize
    /// the second high-pass by setting its corner near-DC (~1 Hz, coefficient
    /// ≈ 1.0 → effectively transparent), so no struct/save-state layout changes.
    #[must_use]
    pub fn for_model(sample_rate: u32, model: FilterModel) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let fs = sample_rate as f32;
        // A ~1 Hz high-pass is an all-but-transparent DC blocker used to
        // "disable" the second HPF stage for the softer models.
        let neutral_hpf = 1.0;
        match model {
            FilterModel::NesRf => Self {
                hp1: OnePole::high_pass(90.0, fs),
                hp2: OnePole::high_pass(440.0, fs),
                lp: OnePole::low_pass(14_000.0, fs),
            },
            FilterModel::Famicom => Self {
                hp1: OnePole::high_pass(37.0, fs),
                hp2: OnePole::high_pass(neutral_hpf, fs),
                lp: OnePole::low_pass(14_000.0, fs),
            },
            FilterModel::Clean => Self {
                hp1: OnePole::high_pass(10.0, fs),
                hp2: OnePole::high_pass(neutral_hpf, fs),
                lp: OnePole::low_pass(14_000.0, fs),
            },
        }
    }

    /// Process one sample through all three stages.
    pub fn process(&mut self, x: f32) -> f32 {
        let a = self.hp1.process(x);
        let b = self.hp2.process(a);
        self.lp.process(b)
    }

    /// Reset filter state.
    pub fn reset(&mut self) {
        self.hp1.reset();
        self.hp2.reset();
        self.lp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_table_zero_at_zero() {
        let m = Mixer::new();
        assert_eq!(m.pulse_table[0], 0.0);
    }

    #[test]
    fn pulse_table_within_tolerance() {
        // Spot-check pulse_table[15+15] (max pulse output).
        let m = Mixer::new();
        // Closed-form: 95.52 / (8128/30 + 100) = ~0.2581.
        let expected = 95.52 / (8128.0 / 30.0 + 100.0);
        let diff = (m.pulse_table[30] - expected).abs();
        assert!(diff < 0.001 * expected.max(0.0001));
    }

    #[test]
    fn tnd_table_zero_at_zero() {
        let m = Mixer::new();
        assert_eq!(m.tnd_table[0], 0.0);
    }

    #[test]
    fn filter_models_differ_in_low_end() {
        // v2.1.3 — the softer models keep more bass. For a one-pole HPF,
        // coeff = exp(-2*pi*fc/fs): a HIGHER corner → SMALLER coeff → more
        // low-end removed. NesRf keeps the aggressive 440 Hz second HPF; the
        // softer models neutralize it (near-DC corner → coeff ≈ 1.0).
        let fs = 44_100;
        let nes = FilterChain::for_model(fs, FilterModel::NesRf);
        let fami = FilterChain::for_model(fs, FilterModel::Famicom);
        let clean = FilterChain::for_model(fs, FilterModel::Clean);
        // NesRf default is byte-identical to the historical `new`.
        let legacy = FilterChain::new(fs);
        assert_eq!(nes.hp1.coeff, legacy.hp1.coeff);
        assert_eq!(nes.hp2.coeff, legacy.hp2.coeff);
        // Second HPF: real 440 Hz on NesRf, transparent on the softer models.
        assert!(nes.hp2.coeff < 0.95, "NesRf hp2 is a real 440 Hz HPF");
        assert!(fami.hp2.coeff > 0.999, "Famicom hp2 near-transparent");
        assert!(clean.hp2.coeff > 0.999, "Clean hp2 near-transparent");
        // First HPF corner gentler on Famicom (37) than NesRf (90), gentlest on
        // Clean (10): gentler corner → larger coeff.
        assert!(
            fami.hp1.coeff > nes.hp1.coeff,
            "Famicom 37Hz gentler than 90Hz"
        );
        assert!(clean.hp1.coeff > fami.hp1.coeff, "Clean 10Hz gentlest");
        // All keep the same 14 kHz low-pass.
        assert_eq!(nes.lp.coeff, fami.lp.coeff);
        assert_eq!(nes.lp.coeff, clean.lp.coeff);
    }

    #[test]
    fn mix_zero_when_all_silent() {
        let m = Mixer::new();
        assert_eq!(m.mix(0, 0, 0, 0, 0), 0.0);
    }

    #[test]
    fn mix_within_unit_range() {
        let m = Mixer::new();
        let v = m.mix(15, 15, 15, 15, 127);
        assert!(v > 0.0 && v < 1.5, "max-mixed sample = {v}");
    }

    #[test]
    fn highpass_decays_dc() {
        let mut hp = OnePole::high_pass(90.0, 44_100.0);
        let mut last = 0.0;
        for _ in 0..1000 {
            last = hp.process(0.5);
        }
        // DC should be heavily attenuated -- result near zero.
        assert!(last.abs() < 0.01);
    }

    #[test]
    fn lowpass_passes_dc() {
        let mut lp = OnePole::low_pass(14_000.0, 44_100.0);
        let mut last = 0.0;
        for _ in 0..100 {
            last = lp.process(0.5);
        }
        // After settling, output ~= input.
        assert!((last - 0.5).abs() < 0.01);
    }
}
