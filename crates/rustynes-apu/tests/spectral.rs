// Integration test — relaxed cast / numeric clippy lints. These analytics
// (FFT bin conversion, dB scaling) intentionally take small precision
// losses that don't affect the acceptance gates.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::needless_range_loop,
    clippy::suboptimal_flops
)]

//! Spectral regression test for the polyphase BLEP / windowed-sinc decimator.
//!
//! This is the **load-bearing acceptance criterion** for the Phase 5 (Track
//! C3) BLEP rewrite: it drives a known-bandwidth input through the
//! [`rustynes_apu::BlipBuf`] and asserts that no aliased energy survives above
//! the host-rate Nyquist (22.05 kHz @ 44.1 kHz output).
//!
//! # Methodology
//!
//! Two complementary tests:
//!
//! - **High-frequency input alias rejection.** Generate a 30 kHz sine
//!   wave at CPU resolution (well above the host's 22.05 kHz Nyquist).
//!   A naive sample-and-hold decimator would fold this back to
//!   `44_100 - 30_000 = 14_100 Hz`, dumping huge energy into the audible
//!   band. The polyphase FIR's stopband must kill this alias to below
//!   -60 dB relative to the residual signal.
//!
//! - **Audible signal preservation.** Generate a 1 kHz square wave at
//!   CPU resolution. The fundamental + first few odd harmonics (3, 5, 7,
//!   9 kHz) all sit well below the FIR cutoff and must survive the
//!   decimation with no significant attenuation.
//!
//! # Why this matters
//!
//! A naive sample-and-hold decimator (the v0.9.x pre-rewrite path) leaves
//! aliased copies of the input spectrum across `[Nyquist, host_rate -
//! Nyquist]` that the existing 14 kHz LPF in [`rustynes_apu::FilterChain`]
//! cannot fully suppress (the LPF rolls off at ~6 dB/oct from 14 kHz, so
//! aliases at 25-40 kHz get only ~5-12 dB attenuation). The polyphase FIR
//! actively kills those aliases via the windowed-sinc kernel BEFORE the
//! analog LPF runs, pushing the alias floor below -60 dB across the full
//! `> Nyquist` band.

use realfft::RealFftPlanner;
use rustynes_apu::{BlipBuf, CPU_HZ_NTSC};

/// Apply a Hann window to a buffer (in-place). This dramatically reduces
/// FFT bin leakage — without it, a single non-bin-aligned tone smears
/// across hundreds of bins, masking the spurious-detection floor.
fn apply_hann_window(buf: &mut [f32]) {
    use core::f32::consts::TAU;
    let n = buf.len();
    for (i, x) in buf.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (TAU * i as f32 / (n - 1) as f32).cos();
        *x *= w;
    }
}

/// Convert a real-valued time-domain buffer to a magnitude spectrum in
/// dB relative to the peak bin. Applies a Hann window first to suppress
/// FFT leakage so the spurious-detection floor isn't masked by the
/// signal tone's spectral skirts.
///
/// Returns a `Vec<f32>` of length `buf.len() / 2 + 1`; index `i`
/// corresponds to frequency `i * sample_rate / buf.len()` Hz.
fn fft_magnitude_db(buf: &[f32]) -> Vec<f32> {
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(buf.len());
    let mut input: Vec<f32> = buf.to_vec();
    apply_hann_window(&mut input);
    let mut output = fft.make_output_vec();
    fft.process(&mut input, &mut output)
        .expect("FFT plan length matches input length");
    // Linear magnitudes.
    let mag: Vec<f32> = output.iter().map(|c| c.norm()).collect();
    // Convert to dB relative to peak.
    let peak = mag.iter().copied().fold(0.0_f32, f32::max).max(1.0e-20);
    mag.iter()
        .map(|m| 20.0 * (m / peak).max(1.0e-20).log10())
        .collect()
}

/// Build a 1 kHz square-wave input stream sampled at the NTSC CPU rate.
/// Returns the per-CPU-cycle amplitude sequence.
fn square_wave_input(duration_cycles: usize, freq_hz: f64) -> Vec<f32> {
    // Cycles per HALF period of the square wave (one cycle = high, next = low).
    let half_period = (CPU_HZ_NTSC / freq_hz / 2.0).round() as usize;
    let mut v = Vec::with_capacity(duration_cycles);
    let mut counter = 0_usize;
    let mut high = true;
    for _ in 0..duration_cycles {
        v.push(if high { 0.4 } else { -0.4 });
        counter += 1;
        if counter >= half_period {
            counter = 0;
            high = !high;
        }
    }
    v
}

/// Pad a buffer to the next power of two (FFT works on any length but a
/// power of two is the fast path; the spectral resolution is unchanged).
fn pad_to_pow2(mut v: Vec<f32>) -> Vec<f32> {
    let target = v.len().next_power_of_two();
    v.resize(target, 0.0);
    v
}

/// Build a `dual_tone` input: signal sine at `sig_hz` (passband) and a
/// would-be-alias sine at `alias_src_hz` (stopband). Same amplitude on
/// each component. Sampled at CPU resolution.
fn dual_tone_input(duration_cycles: usize, sig_hz: f64, alias_src_hz: f64) -> Vec<f32> {
    use core::f32::consts::TAU;
    let mut v = Vec::with_capacity(duration_cycles);
    let o_sig = TAU * (sig_hz / CPU_HZ_NTSC) as f32;
    let o_alias = TAU * (alias_src_hz / CPU_HZ_NTSC) as f32;
    for i in 0..duration_cycles {
        v.push(0.3 * (o_sig * i as f32).sin() + 0.3 * (o_alias * i as f32).sin());
    }
    v
}

#[test]
fn spectral_no_aliasing_above_nyquist() {
    // Drive a two-tone input through the BLEP: a 5 kHz "signal" tone
    // (passband — must survive) and a 30 kHz "alias source" tone
    // (stopband — would fold to 14.1 kHz without band-limiting).
    //
    // The polyphase FIR's stopband must drive the 14.1 kHz alias bin
    // to at least -60 dB BELOW the 5 kHz fundamental's bin. That's
    // the standard SFDR (Spurious-Free Dynamic Range) acceptance gate
    // for the BLEP rewrite (Track C3 of the v1.0.0 roadmap).
    let cycles = (CPU_HZ_NTSC as usize) / 6; // 10 frames @ 60 Hz
    let input = dual_tone_input(cycles, 5_000.0, 30_000.0);

    let mut blip = BlipBuf::new(44_100, CPU_HZ_NTSC);
    for v in &input {
        blip.add_sample(*v);
    }
    let raw = blip.drain_all();
    assert!(
        raw.len() >= 4096,
        "not enough output for FFT analysis (got {})",
        raw.len()
    );

    // Strip leading filter ramp-up + take a power-of-two slice.
    let start = raw.len() / 4;
    let slice: Vec<f32> = raw[start..start + 4096].to_vec();
    let samples = pad_to_pow2(slice);

    let spectrum_db = fft_magnitude_db(&samples);
    let n = samples.len() as f32;
    let bin_hz = |i: usize| (i as f32) * 44_100.0 / n;
    let hz_to_bin = |hz: f32| -> usize {
        let b = (hz * n / 44_100.0).round() as i32;
        b.max(0) as usize
    };

    // The 5 kHz signal lands at its real bin (well below kernel cutoff).
    let sig_bin = hz_to_bin(5_000.0);
    // The 30 kHz alias-source would fold to `|30000 - 44100| = 14100 Hz`
    // if the FIR didn't kill it. Scan a few bins around 14.1 kHz to
    // account for FFT bin-edge spreading.
    let alias_bin_center = hz_to_bin(14_100.0);
    let alias_search = 3_usize;
    let alias_lo = alias_bin_center.saturating_sub(alias_search);
    let alias_hi = (alias_bin_center + alias_search).min(spectrum_db.len() - 1);

    // Take the peak in a ±5-bin window around the signal bin
    // (FFT-bin leakage spreads the tone across a couple of bins).
    let sig_db = spectrum_db[(sig_bin - 2)..=(sig_bin + 2)]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let alias_db = spectrum_db[alias_lo..=alias_hi]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);

    eprintln!(
        "spectral: 5 kHz fundamental = {sig_db:.2} dB; \
         14.1 kHz alias = {alias_db:.2} dB; SFDR = {:.2} dB",
        sig_db - alias_db
    );

    // SFDR: difference between fundamental and worst alias. Must be
    // at least 60 dB for the BLEP rewrite to count as "no aliasing".
    let sfdr = sig_db - alias_db;
    assert!(
        sfdr >= 60.0,
        "30 kHz alias at 14.1 kHz is only {sfdr:.2} dB below 5 kHz \
         fundamental (gate: SFDR ≥ 60 dB). The polyphase FIR is not \
         suppressing high-frequency content above Nyquist"
    );

    // The 30 kHz alias should be the dominant non-fundamental component
    // anywhere in the audible band (gates against alternative spurious
    // sources we haven't anticipated). Scan [200 Hz, 20 kHz] EXCLUDING
    // a ±5-bin window around the signal bin.
    let scan_lo = hz_to_bin(200.0).max(1);
    let scan_hi = hz_to_bin(20_000.0);
    // Exclude a wider window around the signal bin — FFT bin leakage
    // spreads a non-power-of-two-aligned tone across ~20 bins at our
    // 4096-sample slice.
    let signal_exclusion = 20_usize;
    let mut max_spur_db = f32::NEG_INFINITY;
    let mut max_spur_bin = scan_lo;
    for i in scan_lo..=scan_hi {
        if i.abs_diff(sig_bin) <= signal_exclusion {
            continue;
        }
        if spectrum_db[i] > max_spur_db {
            max_spur_db = spectrum_db[i];
            max_spur_bin = i;
        }
    }
    let spur_below_sig = sig_db - max_spur_db;
    eprintln!(
        "  worst spurious in audible band (excl. signal) = {max_spur_db:.2} \
         dB at {} Hz ({spur_below_sig:.2} dB below signal)",
        bin_hz(max_spur_bin) as i32
    );
    // The spurious bin should ALSO be at least 60 dB below — this catches
    // any non-folded leakage (e.g., transition-band ripple).
    assert!(
        spur_below_sig >= 60.0,
        "Spurious component at {:.0} Hz is only {spur_below_sig:.2} dB \
         below 5 kHz signal (gate: ≥ 60 dB)",
        bin_hz(max_spur_bin)
    );
}

#[test]
fn spectral_audible_band_preserved() {
    // 1 kHz square wave at CPU resolution. The fundamental and first few
    // odd harmonics (3, 5, 7, 9 kHz) all sit well below the FIR's
    // 20.3 kHz cutoff and must survive decimation. This is the sanity
    // gate against "FIR kernel zeros everything".
    let cycles = (CPU_HZ_NTSC as usize) / 6;
    let input = square_wave_input(cycles, 1_000.0);

    let mut blip = BlipBuf::new(44_100, CPU_HZ_NTSC);
    for v in &input {
        blip.add_sample(*v);
    }
    let raw = blip.drain_all();
    let start = raw.len() / 4;
    let slice: Vec<f32> = raw[start..start + 4096].to_vec();
    let samples = pad_to_pow2(slice);

    let spectrum_db = fft_magnitude_db(&samples);
    let n = samples.len() as f32;
    let hz_to_bin = |hz: f32| -> usize {
        let b = (hz * n / 44_100.0).round() as i32;
        b.max(0) as usize
    };

    // Fundamental at 1 kHz must be the dominant peak.
    let fundamental_bin = hz_to_bin(1_000.0);
    let fundamental_db = spectrum_db[fundamental_bin];
    assert!(
        fundamental_db > -3.0,
        "1 kHz fundamental at {fundamental_db:.2} dB (expected > -3 dB)"
    );

    // First few odd harmonics survive at roughly their square-wave-
    // theoretical levels (1/k).
    for harmonic_idx in [3, 5, 7, 9] {
        let f = 1_000.0 * harmonic_idx as f32;
        let bin = hz_to_bin(f);
        let db = spectrum_db[bin];
        // Square wave harmonic k is at 1/k = -20*log10(k) dB. Allow
        // 10 dB margin for FFT bin leakage + filter slope.
        let expected_floor = -20.0 * (harmonic_idx as f32).log10() - 10.0;
        assert!(
            db > expected_floor,
            "harmonic {harmonic_idx} ({f:.0} Hz) at {db:.2} dB \
             (expected > {expected_floor:.2} dB)"
        );
    }

    eprintln!("spectral: 1 kHz square — fundamental = {fundamental_db:.2} dB");
}

#[test]
fn spectral_dc_input_is_dc_suppressed() {
    // A constant input must not produce DC leakage to the output —
    // the HPF chain blocks it. (Sanity check that the new FIR didn't
    // bypass the FilterChain.)
    let mut blip = BlipBuf::new(44_100, CPU_HZ_NTSC);
    for _ in 0..(CPU_HZ_NTSC as usize / 6) {
        blip.add_sample(0.5);
    }
    let out = blip.drain_all();
    let start = out.len() / 2;
    let tail = &out[start..];
    let mean = tail.iter().sum::<f32>() / (tail.len() as f32);
    assert!(
        mean.abs() < 0.005,
        "DC input not blocked: tail mean = {mean}"
    );
}
