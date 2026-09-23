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

/// Frames run before the first measured one, to get past reset and the blank
/// period so every bench measures steady-state work, not initialisation.
const WARM_FRAMES: usize = 60;

/// A save state of `rom` after [`WARM_FRAMES`] frames, taken ONCE per bench.
///
/// Until 2026-09-23 each measured iteration built a fresh `Nes` and ran the 60
/// warm-up frames in `iter_batched`'s setup: ~175-240 ms of setup for a ~4 ms
/// measured frame. Criterion budgets its sampling on wall time including that
/// setup, so 100 samples could never fit the gate's 3 s target and every bench
/// printed "Unable to complete 100 samples", taking 15-24 s instead. Restoring
/// this snapshot yields the same warmed state -- save-state round-trips are
/// this project's determinism contract -- so the measured routine, one
/// `run_frame` from that state, is unchanged, while setup drops to a restore.
fn warmed_snapshot(rom: &[u8], fast_dotloop: bool) -> Vec<u8> {
    let mut nes = Nes::from_rom(rom).expect("bench ROM parses");
    if fast_dotloop {
        nes.set_fast_dotloop(true);
    }
    for _ in 0..WARM_FRAMES {
        nes.run_frame();
    }
    nes.snapshot()
}

/// A fresh machine in the warmed state. `fast_dotloop` is a runtime setting,
/// re-applied rather than trusted to the snapshot.
///
/// `true` calls `set_fast_dotloop(true)` exactly as the `*_fast` benches always
/// did; `false` leaves the setting ALONE, exactly as the stock benches did. It
/// must not call `set_fast_dotloop(false)`: the fast path is the DEFAULT, so
/// forcing it off measured a different routine -- +12.7% on nestest in the
/// first A/B of this change, which is how that was caught.
fn warmed(rom: &[u8], snapshot: &[u8], fast_dotloop: bool) -> Nes {
    let mut nes = Nes::from_rom(rom).expect("bench ROM parses");
    nes.restore_quiet(snapshot)
        .expect("a snapshot this bench just took restores");
    // After the restore, so a restore can never override it (review, #547).
    if fast_dotloop {
        nes.set_fast_dotloop(true);
    }
    nes
}

fn bench_full_frame(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("nestest/nestest.nes"))
        .expect("nestest/nestest.nes vendored in tests/roms/");
    let snapshot = warmed_snapshot(&bytes, false);

    c.bench_function("nes_run_frame_nestest", |b| {
        b.iter_batched(
            || warmed(&bytes, &snapshot, false),
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
    let bytes = std::fs::read(rom_path("assorted/flowing_palette.nes"))
        .expect("assorted/flowing_palette.nes vendored in tests/roms/");
    let snapshot = warmed_snapshot(&bytes, false);

    c.bench_function("nes_run_frame_flowing_palette", |b| {
        b.iter_batched(
            || warmed(&bytes, &snapshot, false),
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
/// is byte-identical — proven by `fast_dotloop_diff`). The headline figure is
/// the **`nestest`** delta: nestest renders a menu (rendering ENABLED), so the
/// fast path engages. `flowing_palette` is a rendering-DISABLED 64-colour
/// backdrop-override demo — the fast path never engages there, so its `*_fast`
/// variant is expected to be NEUTRAL and serves only as the guard-bail control
/// (see `docs/performance.md` §"v2.1.8 A1").
///
/// **Known, not changed here (found 2026-09-23):** the fast dot path is now the
/// DEFAULT (`fast_dotloop: true` in the PPU's constructor), and the stock
/// benches never call `set_fast_dotloop`, so each stock/`*_fast` pair measures
/// the same routine -- 3.936 vs 3.937 ms for nestest on one run. The pairs no
/// longer isolate the speedup. Restoring the A/B means the stock benches
/// calling `set_fast_dotloop(false)`, which changes the headline ms/frame
/// figure in `docs/performance.md`, so it is a decision for its own change.
fn bench_full_frame_fast(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("nestest/nestest.nes"))
        .expect("nestest/nestest.nes vendored in tests/roms/");
    let snapshot = warmed_snapshot(&bytes, true);

    c.bench_function("nes_run_frame_nestest_fast", |b| {
        b.iter_batched(
            || warmed(&bytes, &snapshot, true),
            |mut nes| {
                let fb = nes.run_frame();
                black_box(fb.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_full_frame_rendering_fast(c: &mut Criterion) {
    let bytes = std::fs::read(rom_path("assorted/flowing_palette.nes"))
        .expect("assorted/flowing_palette.nes vendored in tests/roms/");
    let snapshot = warmed_snapshot(&bytes, true);

    c.bench_function("nes_run_frame_flowing_palette_fast", |b| {
        b.iter_batched(
            || warmed(&bytes, &snapshot, true),
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
