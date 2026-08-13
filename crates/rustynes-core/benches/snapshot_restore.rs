#![allow(missing_docs)]
//! Criterion bench — `Nes::snapshot` / `Nes::restore` cost, and the combined
//! per-visible-frame budget of run-ahead N=1 (snapshot + restore + one extra
//! `run_frame`).
//!
//! Per the v2.8.0 performance plan (Phase 0): `docs/performance.md` has
//! carried a "save-state ≤ 1 ms" target since v0.9 that was never actually
//! measured; this bench closes that gap and provides the budget evidence for
//! the run-ahead feature (Phase 3), whose steady-state cost per visible frame
//! is exactly `snapshot + (N extra run_frame) + restore`.
//!
//! Two ROMs bracket the mapper-state spectrum:
//! - `flowing_palette.nes` (NROM, CC0) — minimal `MAP ` section; PPU-heavy
//!   rendering load for the run-ahead probe.
//! - `holy_mapperel M4_P128K_CR8K.nes` (MMC3 + 8 KiB PRG-RAM + 8 KiB CHR-RAM,
//!   zlib) — the realistic upper end: bank registers + both RAMs serialize.

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

/// Boot a ROM and run it past the reset/blank period so snapshots capture
/// steady-state (rendering enabled, OAM/palette populated).
fn warmed_nes(bytes: &[u8]) -> Nes {
    let mut nes = Nes::from_rom(bytes).expect("bench ROM parses");
    for _ in 0..60 {
        nes.run_frame();
    }
    nes
}

fn bench_rom(c: &mut Criterion, label: &str, rel: &str) {
    let bytes = std::fs::read(rom_path(rel))
        .unwrap_or_else(|e| panic!("bench ROM {rel} vendored in tests/roms/: {e}"));

    // snapshot() alone — includes the THM thumbnail build (the fast path
    // added in Phase 3 will get its own bench entry when it lands, so the
    // delta is measurable).
    c.bench_function(&format!("nes_snapshot_{label}"), |b| {
        let nes = warmed_nes(&bytes);
        b.iter(|| black_box(nes.snapshot().len()));
    });

    // restore() alone, from a pre-built blob into a warmed instance.
    c.bench_function(&format!("nes_restore_{label}"), |b| {
        let mut nes = warmed_nes(&bytes);
        let blob = nes.snapshot();
        b.iter(|| {
            nes.restore(black_box(&blob)).expect("restore round-trips");
        });
    });

    // v2.8.0 Phase 3 — the fast path: no THM thumbnail, caller-owned reused
    // buffer. This is what run-ahead / netplay / rewind actually pay.
    c.bench_function(&format!("nes_snapshot_core_into_{label}"), |b| {
        let nes = warmed_nes(&bytes);
        let mut buf = Vec::new();
        b.iter(|| {
            nes.snapshot_core_into(&mut buf);
            black_box(buf.len());
        });
    });

    // v2.8.0 Phase 3 — restore_quiet (no rewind-ring clear) from the fast-
    // path blob.
    c.bench_function(&format!("nes_restore_quiet_{label}"), |b| {
        let mut nes = warmed_nes(&bytes);
        let mut blob = Vec::new();
        nes.snapshot_core_into(&mut blob);
        b.iter(|| {
            nes.restore_quiet(black_box(&blob))
                .expect("restore round-trips");
        });
    });

    // The run-ahead N=1 budget probe: what one visible frame pays ON TOP of
    // its own run_frame — snapshot, one hidden run_frame, restore — on the
    // Phase 3 fast path (the shipping run-ahead configuration).
    // v2.3.3 F19 PROBE — the SLIM restore, to test whether skipping the
    // 245,760-byte framebuffer is actually worth anything on the run-ahead /
    // netplay / TAS paths.
    //
    // Written because the estimate that motivated F19 was unsound: the
    // framebuffer is 94% of the snapshot BYTES, and that was silently carried
    // over into a claim about TIME. A 245 KiB memcpy is ~12-25 us at ordinary
    // bandwidth, so if `restore_quiet` costs 121 us the framebuffer cannot be
    // 94% of it, and the win could be a fraction of what F19 assumed. Measure
    // the pair rather than reason about the ratio.
    c.bench_function(&format!("nes_restore_quiet_slim_{label}"), |b| {
        // `warmed_nes`, matching `nes_restore_quiet_*` exactly. The first
        // version booted a fresh `Nes` and ran ONE frame, so it compared a
        // cold-start state against the other bench's 60-frame steady state
        // (rendering enabled, OAM and palette populated) — different serialized
        // content, and therefore a confounded full-vs-slim delta on which the
        // F19 rejection rested. Raised in review on PR #365.
        let mut nes = warmed_nes(&bytes);
        let mut blob = Vec::new();
        nes.snapshot_core_into_slim(&mut blob);
        b.iter(|| {
            nes.restore_quiet(black_box(&blob))
                .expect("slim snapshot round-trips");
        });
    });

    c.bench_function(&format!("nes_runahead_budget_{label}"), |b| {
        b.iter_batched(
            || (warmed_nes(&bytes), Vec::new()),
            |(mut nes, mut blob)| {
                nes.snapshot_core_into(&mut blob);
                let fb = nes.run_frame();
                black_box(fb.len());
                nes.restore_quiet(&blob).expect("restore round-trips");
                black_box(blob.len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_snapshot_restore(c: &mut Criterion) {
    bench_rom(c, "flowing_palette", "assorted/flowing_palette.nes");
    bench_rom(c, "mmc3", "holy_mapperel/M4_P128K_CR8K.nes");
}

criterion_group!(benches, bench_snapshot_restore);
criterion_main!(benches);
