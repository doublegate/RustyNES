#![allow(missing_docs)]
//! Criterion bench — `Nes::run_frame` end-to-end cost on a real NROM ROM.
//!
//! Per Track B6 of the gap-analysis remediation plan. The number this
//! bench produces is the headline "ms/frame headless" claim in
//! `docs/performance.md` (formerly un-evidenced ≤ 2 ms/frame).
//!
//! Methodology: load `nestest.nes` (kevtris, NROM, public domain),
//! reset, then iterate `run_frame()` N times. The bench captures the
//! whole lockstep stack: CPU per-cycle bus interleaving + PPU dot
//! scheduler + APU sample emit + mapper dispatch + framebuffer write.
//!
//! Note: nestest in normal mode (PC ← reset vector, not PC=$C000) runs
//! a small interactive menu screen waiting for input. The frame cost is
//! representative of "rendering a static screen with sprite eval and
//! BG fetch active" — close to typical real-game cost.

use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use rustynes_core::Nes;
use std::hint::black_box;

fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

fn bench_full_frame(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("nestest/nestest.nes"))
        .expect("nestest/nestest.nes vendored in tests/roms/");

    c.bench_function("nes_run_frame_nestest", |b| {
        b.iter_batched(
            || {
                let mut nes = Nes::from_rom(&bytes).expect("nestest parses");
                // Burn 60 frames to skip past the reset / blank period so the
                // bench measures steady-state work, not init.
                for _ in 0..60 {
                    nes.run_frame();
                }
                nes
            },
            |mut nes| {
                let fb = nes.run_frame();
                black_box(fb.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Rendering-heavy companion to `bench_full_frame`.
///
/// nestest sits on a near-static menu, so its frame cost under-represents a
/// real game's PPU work. `flowing_palette.nes` (CC0, the same ROM the TAS
/// determinism tests use) continuously rewrites the palette and renders a full
/// background every frame, so this bench exercises the PPU emit + palette path
/// far harder — it is the recommended `perf record` input (see
/// `docs/performance.md`).
fn bench_full_frame_rendering(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("sprint-2/flowing_palette.nes"))
        .expect("sprint-2/flowing_palette.nes vendored in tests/roms/");

    c.bench_function("nes_run_frame_flowing_palette", |b| {
        b.iter_batched(
            || {
                let mut nes = Nes::from_rom(&bytes).expect("flowing_palette parses");
                for _ in 0..60 {
                    nes.run_frame();
                }
                nes
            },
            |mut nes| {
                let fb = nes.run_frame();
                black_box(fb.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// v2.1.8 A1 — the fast-dot-path A/B companions. Identical to the two benches
/// above except `set_fast_dotloop(true)` is applied after boot, so a
/// back-to-back Criterion run of `*_fast` vs the stock bench isolates the
/// speedup the specialized visible-scanline handler buys (the emulated output
/// is byte-identical — proven by `fast_dotloop_diff`). The delta on the
/// rendering-heavy `flowing_palette` path is the headline figure.
fn bench_full_frame_fast(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("nestest/nestest.nes"))
        .expect("nestest/nestest.nes vendored in tests/roms/");

    c.bench_function("nes_run_frame_nestest_fast", |b| {
        b.iter_batched(
            || {
                let mut nes = Nes::from_rom(&bytes).expect("nestest parses");
                nes.set_fast_dotloop(true);
                for _ in 0..60 {
                    nes.run_frame();
                }
                nes
            },
            |mut nes| {
                let fb = nes.run_frame();
                black_box(fb.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_full_frame_rendering_fast(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("sprint-2/flowing_palette.nes"))
        .expect("sprint-2/flowing_palette.nes vendored in tests/roms/");

    c.bench_function("nes_run_frame_flowing_palette_fast", |b| {
        b.iter_batched(
            || {
                let mut nes = Nes::from_rom(&bytes).expect("flowing_palette parses");
                nes.set_fast_dotloop(true);
                for _ in 0..60 {
                    nes.run_frame();
                }
                nes
            },
            |mut nes| {
                let fb = nes.run_frame();
                black_box(fb.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_full_frame,
    bench_full_frame_rendering,
    bench_full_frame_fast,
    bench_full_frame_rendering_fast
);
criterion_main!(benches);
