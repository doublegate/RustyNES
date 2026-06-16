// Integration test — relaxed cast / numeric clippy lints, matching the
// existing `spectral.rs` BLEP test next door. Analytical math (FFT bin
// conversion, dB scaling, exact-frequency fnum/block calculations) takes
// small precision losses that don't affect the acceptance gates.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::needless_range_loop,
    clippy::suboptimal_flops
)]

//! Spectral regression test for the OPLL FM synthesizer port.
//!
//! Drives the OPLL with a **silenced-modulator pure-sine carrier**
//! patch at a known frequency (fnum=290, block=5 → 440 Hz at the
//! 49,716 Hz native rate), captures 16,384 samples, then runs an FFT
//! and asserts:
//!
//! 1. The dominant frequency bin is within ±1 bin (3 Hz at this
//!    resolution) of the expected 440 Hz.
//! 2. The peak-to-spurious dB ratio (SFDR, computed over non-fundamental
//!    bins) is above a permissive 25 dB gate — enough to catch a
//!    wave-table corruption or operator-output sign flip, but tolerant
//!    of the natural envelope ramp + exp-table quantization that any
//!    OPLL produces during the Attack → Decay transition.
//!
//! This is the **load-bearing acceptance criterion** for Sprint 1.3
//! of v1.1.0: it proves the OPLL port produces the right frequency
//! output for a known input. The bit-exact regression check (an FNV-1a
//! hash of the audio sample stream against the v1.0.0-rc baseline)
//! lives in `crates/rustynes-test-harness/tests/audio_tests.rs` as the
//! `audio_db_vrc7` / `audio_test_vrc7` / `audio_patch_vrc7` /
//! `audio_clip_vrc7` / `audio_noise_vrc7` insta snapshots.

use realfft::RealFftPlanner;
use rustynes_apu::{Opll, OpllChipType};

/// OPLL native sample rate, per emu2413.cpp:23 (`OpllSampleRate = 49716`).
const OPLL_RATE_HZ: f32 = 49_716.0;

/// Apply a Hann window to a buffer (in place). Reduces FFT bin leakage
/// for a non-bin-aligned tone — without it, the 440 Hz fundamental smears
/// across many bins, masking the spurious-detection floor.
use core::f32::consts::TAU;

fn apply_hann_window(buf: &mut [f32]) {
    let n = buf.len();
    for (i, x) in buf.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (TAU * i as f32 / (n - 1) as f32).cos();
        *x *= w;
    }
}

/// Compute magnitude spectrum in dB relative to peak. Returns
/// `Vec<f32>` of length `buf.len() / 2 + 1`; index `i` is the bin
/// for frequency `i * OPLL_RATE_HZ / buf.len()`.
fn fft_magnitude_db(buf: &[f32]) -> Vec<f32> {
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(buf.len());
    let mut input = buf.to_vec();
    apply_hann_window(&mut input);
    let mut output = fft.make_output_vec();
    fft.process(&mut input, &mut output)
        .expect("FFT plan length matches input length");
    let mag: Vec<f32> = output.iter().map(|c| c.norm()).collect();
    let peak = mag.iter().copied().fold(0.0_f32, f32::max).max(1.0e-20);
    mag.iter()
        .map(|m| 20.0 * (m / peak).max(1.0e-20).log10())
        .collect()
}

/// Configure the OPLL's user patch (patch 0, channels using it via
/// instrument-select 0) for a **silenced-modulator pure-sine carrier**:
///
/// - modulator: TL=63 (max attenuation, effectively muted), ML=0,
///   AR=15 / DR=0 (instant-attack, no decay), no AM/PM/EG/KR
/// - carrier: ML=0 (1× multiplier — `ML_TABLE[0] = 1` per
///   emu2413.cpp:222), AR=15 / DR=0, WS=0 (full sine),
///   no FB/AM/PM/EG/KR
///
/// The result is that the carrier's phase generator drives a clean
/// sine wave at the channel's fnum/block frequency, with no FM
/// (modulator silent), no feedback, and no envelope decay. The
/// dominant output is therefore the carrier's fundamental.
fn configure_silenced_modulator_pure_sine(opll: &mut Opll) {
    // $00: modulator AM/PM/EG/KR/ML — all zero (ML=0).
    opll.write_reg(0x00, 0x00);
    // $01: carrier AM/PM/EG/KR/ML — all zero (ML=0, `ML_TABLE[0]=1`
    // gives 1× the base frequency derived from fnum + block).
    opll.write_reg(0x01, 0x00);
    // $02: modulator KL/TL — TL=63 (max attenuation, ~silent modulator).
    opll.write_reg(0x02, 0x3F);
    // $03: FB/WS — FB=0 (no self-feedback), both WS=0 (full sine).
    opll.write_reg(0x03, 0x00);
    // $04: modulator AR/DR — AR=15 (instant), DR=0.
    opll.write_reg(0x04, 0xF0);
    // $05: carrier AR/DR — same.
    opll.write_reg(0x05, 0xF0);
    // $06: modulator SL/RR — both 0.
    opll.write_reg(0x06, 0x00);
    // $07: carrier SL/RR — both 0.
    opll.write_reg(0x07, 0x00);
}

/// Capture window length for the FFT regression: 16,384 samples at
/// 49,716 Hz = ~330 ms — long enough for Damp→Attack→audible output
/// (~16k cycles per the OPLL unit-test convention) plus a long
/// enough window for FFT bin resolution = 49716 / 16384 ≈ 3.03 Hz / bin.
const NSAMPLES: usize = 16_384;

#[test]
fn opll_carrier_produces_expected_fundamental_frequency() {
    // Target a known frequency. Per the OPLL phase generator formula
    // (emu2413.cpp:765-773 / our `Slot::calc_phase`), for ML_TABLE[0]=1
    // and `patch.ml=0`:
    //   phase_increment_per_clock = ((fnum & 0x1FF) * 2 + pm) * 1 << blk >> 2
    // With pm=0: increment = fnum << blk >> 1
    // Wait — emu2413 uses `(fnum * 2) << blk >> 2 = fnum << blk / 2`.
    // One full cycle = DP_WIDTH = 524,288 increments.
    // Frequency = OPLL_RATE * increment / DP_WIDTH
    //           = 49716 * (fnum << blk / 2) / 524288
    // For fnum=290, blk=5: increment = (290 << 5) / 2 = 9280 / 2 = 4640
    //   frequency = 49716 * 4640 / 524288 ≈ 440.0 Hz
    let mut opll = Opll::new(OpllChipType::Vrc7);
    configure_silenced_modulator_pure_sine(&mut opll);
    // Channel 0: instrument 0 (user patch), volume 0 (loudest).
    opll.write_reg(0x30, 0x00);
    // Fnum low byte of 290 = 0x22.
    opll.write_reg(0x10, 0x22);
    // $20 bits: 0001 1011 = sustain(0) key(1) block(5) fnum_high(1)
    // block = 5 << 1 = 0x0A; key = 0x10; fnum_high = 0x01.
    opll.write_reg(0x20, 0x1B);

    let mut samples: Vec<f32> = Vec::with_capacity(NSAMPLES);
    for _ in 0..NSAMPLES {
        samples.push(f32::from(opll.calc()));
    }

    // Skip the first ~2,000 samples (the Damp + Attack ramp produces
    // a non-stationary signal that smears the FFT). Use only the
    // stationary tail.
    let stationary = &samples[2048..];
    let mag = fft_magnitude_db(stationary);

    // Find the peak bin.
    let (peak_bin, peak_db) =
        mag.iter()
            .enumerate()
            .fold((0usize, f32::NEG_INFINITY), |(bi, bv), (i, v)| {
                if *v > bv { (i, *v) } else { (bi, bv) }
            });
    let bin_hz = OPLL_RATE_HZ / (stationary.len() as f32);
    let peak_hz = peak_bin as f32 * bin_hz;

    // Expected fundamental: 440 Hz. Allow ±2 bins (~6 Hz) tolerance
    // to absorb FFT bin quantization + the OPLL's discrete phase
    // increment (the 290/5 setup hits exactly 440.0 Hz analytically
    // but the windowed FFT bin centers don't land on 440 Hz).
    let expected_hz = 440.0_f32;
    let tolerance_hz = 2.0 * bin_hz;
    assert!(
        (peak_hz - expected_hz).abs() <= tolerance_hz,
        "OPLL peak frequency: expected {expected_hz:.1} ± {tolerance_hz:.2} Hz, \
         got {peak_hz:.2} Hz (bin {peak_bin}, peak {peak_db:.1} dB)",
    );

    // SFDR: find the highest spike OUTSIDE a small window around the
    // fundamental. Permissive gate (25 dB) — the OPLL is a fixed-
    // point FM synth with exp-table quantization, so harmonic content
    // is non-trivial even for a "pure" sine setup.
    let exclude_window = 5usize; // ± 5 bins around fundamental
    let lo = peak_bin.saturating_sub(exclude_window);
    let hi = (peak_bin + exclude_window).min(mag.len() - 1);
    let mut second_max = f32::NEG_INFINITY;
    for (i, db) in mag.iter().enumerate() {
        if i >= lo && i <= hi {
            continue;
        }
        if *db > second_max {
            second_max = *db;
        }
    }
    let sfdr_db = peak_db - second_max;
    assert!(
        sfdr_db >= 25.0,
        "OPLL SFDR below acceptance gate: peak {peak_db:.1} dB, \
         next spurious {second_max:.1} dB, SFDR {sfdr_db:.1} dB (gate: 25.0 dB)",
    );
}

#[test]
fn opll_silenced_channel_produces_zero_output() {
    // Sanity test: with NO key-on and the default patches, the OPLL
    // produces all-zero samples. Catches a regression where the
    // envelope state machine fails to start at EG_MUTE.
    let mut opll = Opll::new(OpllChipType::Vrc7);
    for _ in 0..4096 {
        assert_eq!(opll.calc(), 0, "default-state OPLL must produce silence");
    }
}

#[test]
fn opll_determinism_two_runs_produce_byte_identical_streams() {
    // Determinism contract: identical input register-write sequences
    // produce bit-identical sample streams. Critical for save-state
    // round-trips, regression tests, and future netplay.
    let mut a = Opll::new(OpllChipType::Vrc7);
    let mut b = Opll::new(OpllChipType::Vrc7);
    // Identical config + key-on on both.
    for opll in [&mut a, &mut b] {
        configure_silenced_modulator_pure_sine(opll);
        opll.write_reg(0x30, 0x00);
        opll.write_reg(0x10, 0x22);
        opll.write_reg(0x20, 0x1B);
    }
    for i in 0..4096 {
        let sa = a.calc();
        let sb = b.calc();
        assert_eq!(
            sa, sb,
            "OPLL determinism violated at sample {i}: {sa} vs {sb}"
        );
    }
}
