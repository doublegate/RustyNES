#![allow(
    missing_docs,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::suboptimal_flops
)]
//! Criterion bench — `BlipBuf::add_sample` throughput on a synthetic
//! square-wave input.
//!
//! Part of the Phase 5 (Track C3) BLEP rewrite. The point is to:
//!
//! 1. Provide a measurable baseline so future kernel-tuning work can
//!    catch regressions in the FIR convolution path.
//! 2. Pair with the spectral correctness assertion in
//!    `tests/spectral.rs` — that test gates `cargo test`, this bench
//!    gates `cargo bench`.
//!
//! Methodology: feed 1 second of NTSC CPU cycles (1 789 773 calls to
//! `add_sample`) carrying a 1 kHz square wave, then drain. The bench
//! measures the full `add_sample` + decimation + drain path under
//! release-mode optimization.

use criterion::{criterion_group, criterion_main, Criterion};
use rustynes_apu::{BlipBuf, CPU_HZ_NTSC};
use std::hint::black_box;

fn build_square(cycles: usize, freq_hz: f64) -> Vec<f32> {
    let half_period = (CPU_HZ_NTSC / freq_hz / 2.0).round() as usize;
    let mut v = Vec::with_capacity(cycles);
    let mut counter = 0;
    let mut high = true;
    for _ in 0..cycles {
        v.push(if high { 0.4 } else { -0.4 });
        counter += 1;
        if counter >= half_period {
            counter = 0;
            high = !high;
        }
    }
    v
}

fn bench_blip_square_wave(c: &mut Criterion) {
    // 0.1 seconds of NTSC cycles, ~178_977 sample emissions.
    let cycles = (CPU_HZ_NTSC as usize) / 10;
    let input = build_square(cycles, 1_000.0);

    c.bench_function("blip_square_wave_0_1s_ntsc", |b| {
        b.iter(|| {
            let mut blip = BlipBuf::new(44_100, CPU_HZ_NTSC);
            for v in &input {
                blip.add_sample(*v);
            }
            black_box(blip.drain_all());
        });
    });
}

fn bench_blip_silence(c: &mut Criterion) {
    // Pure-silence path — measures the FIR + ring-buffer overhead with no
    // signal energy. Useful for diff-comparing the FIR cost vs the legacy
    // sample-and-hold path.
    let cycles = (CPU_HZ_NTSC as usize) / 10;
    c.bench_function("blip_silence_0_1s_ntsc", |b| {
        b.iter(|| {
            let mut blip = BlipBuf::new(44_100, CPU_HZ_NTSC);
            for _ in 0..cycles {
                blip.add_sample(0.0);
            }
            black_box(blip.drain_all());
        });
    });
}

criterion_group!(spectral, bench_blip_square_wave, bench_blip_silence);
criterion_main!(spectral);
