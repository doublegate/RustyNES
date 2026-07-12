//! v2.1.8 "Performance" (A2) — Criterion microbenchmark for the software
//! palette-index -> RGBA blitter (`rustynes_frontend::gfx_blit`).
//!
//! Profiles the three byte-identical variants against a representative full
//! frame (256x240 palette indices sweeping the whole 0..512 LUT domain):
//!
//! * `copy4`  — the naive per-pixel `[u8; 4]` `copy_from_slice` (the shape the
//!   core's `emit_pixel` uses), i.e. the reference baseline.
//! * `u32`    — tight scalar gather + 32-bit store.
//! * `simd`   — `wide::u32x8` scalar gather + 256-bit store (the desktop
//!   dispatch target).
//!
//! The point of the bench is the *evidence* behind the adoption decision in
//! `docs/performance.md`: the conversion is a memory-bound LUT gather, so the
//! SIMD path is expected to land within noise of the scalar-`u32` path while
//! both beat the `copy4` baseline. "Adopt only on a measured >3% win" is a
//! decision this bench exists to inform, not to presume.

use criterion::{Criterion, criterion_group, criterion_main};
use rustynes_core::rustynes_ppu::{PpuPalette, build_rgba_lut};
use rustynes_frontend::gfx_blit::{PIXELS, RGBA_LEN, blit_scalar, blit_simd, blit_u32};
use std::hint::black_box;

#[allow(clippy::cast_possible_truncation)] // both operands are `% 512`, so < 512.
fn representative_frame() -> Vec<u16> {
    let mut v = vec![0u16; PIXELS];
    let mut s: u32 = 0x9E37_79B9;
    for (i, slot) in v.iter_mut().enumerate() {
        if i % 3 == 0 {
            *slot = (i % 512) as u16;
        } else {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *slot = (s % 512) as u16;
        }
    }
    v
}

fn bench_blit(c: &mut Criterion) {
    let lut = build_rgba_lut(PpuPalette::Composite2C02);
    let idx = representative_frame();
    let mut out = vec![0u8; RGBA_LEN];

    let mut group = c.benchmark_group("gfx_blit_256x240");
    group.throughput(criterion::Throughput::Bytes(RGBA_LEN as u64));

    group.bench_function("copy4_scalar_reference", |b| {
        b.iter(|| blit_scalar(black_box(&idx), black_box(&lut), black_box(&mut out)));
    });
    group.bench_function("u32_scalar", |b| {
        b.iter(|| blit_u32(black_box(&idx), black_box(&lut), black_box(&mut out)));
    });
    group.bench_function("simd_wide_u32x8", |b| {
        b.iter(|| blit_simd(black_box(&idx), black_box(&lut), black_box(&mut out)));
    });

    group.finish();
}

criterion_group!(benches, bench_blit);
criterion_main!(benches);
