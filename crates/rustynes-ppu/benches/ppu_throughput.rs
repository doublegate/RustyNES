#![allow(missing_docs)]
//! Criterion bench — `Ppu::tick` throughput on a synthetic CHR + nametable
//! image.
//!
//! Per Track B6 of the gap-analysis remediation plan. Baseline numbers for
//! `docs/performance.md`.
//!
//! Methodology: a minimal `PpuBus` returns a deterministic byte for every
//! read (PRG patterns + nametable patterns are unimportant for the dot
//! advancement cost; the bench measures the per-dot scheduler + register
//! updates, NOT BG/sprite pattern correctness). The PPU runs through N
//! full frames (89,342 dots/frame at NTSC).

use criterion::{criterion_group, criterion_main, Criterion};
use rustynes_ppu::{Ppu, PpuBus, PpuRegion};
use std::hint::black_box;

/// Trivial `PpuBus` that returns 0xA5 (a value with a bit pattern that
/// exercises the BG / sprite shifters) for every read.
struct SyntheticBus;

impl PpuBus for SyntheticBus {
    fn ppu_read(&mut self, _addr: u16) -> u8 {
        0xA5
    }
    fn ppu_write(&mut self, _addr: u16, _value: u8) {}
}

fn bench_one_frame(c: &mut Criterion) {
    // NTSC frame: 89,342 PPU dots (262 scanlines * 341 dots, plus pre-render line dot skip).
    const DOTS_PER_FRAME: usize = 89_342;
    c.bench_function("ppu_tick_one_frame", |b| {
        b.iter_batched(
            || (Ppu::new(PpuRegion::Ntsc), SyntheticBus),
            |(mut ppu, mut bus)| {
                for _ in 0..DOTS_PER_FRAME {
                    ppu.tick(&mut bus);
                }
                black_box(&ppu);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_one_frame);
criterion_main!(benches);
