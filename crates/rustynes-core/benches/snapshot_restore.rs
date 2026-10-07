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
//!
//! v2.9.1 (NL-09) — and a Vs. `DualSystem` cabinet, whose save-state path is
//! what the libretro core calls for every `retro_serialize` and
//! `retro_unserialize` of one of the four `DualSystem` titles. Named for the
//! JOB, not the method (`vs_dual_serialize`, `vs_dual_restore`), so an A/B
//! across the change that replaced the method compares the same work.

use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use rustynes_core::{Nes, VsDualSystem};
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
    // over into a claim about TIME. A 245 KiB memcpy is ~12-25 us at an
    // ordinary ~10-20 GB/s (245,760 B / 20 GB/s = 12.3 us; / 10 GB/s =
    // 24.6 us), so if `restore_quiet` costs 121 us the framebuffer cannot be
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

/// A minimal `DualSystem` cabinet image: NES 2.0, mapper 99, Vs. hardware type
/// 5 (`DualSystem`), 64 KiB PRG (both CPUs' halves) and 8 KiB CHR-RAM. Each
/// half's reset vector points at a `JMP` to itself, so both consoles run
/// indefinitely with the state a real cabinet carries (two full machines plus
/// the shared WRAM); the content of that state does not change the work a
/// serialize does, only its bytes.
fn dual_cabinet_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x10000];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = 0x04; // 4 x 16 KiB PRG
    rom[6] = 0x30; // mapper 99, low nibble
    rom[7] = 0x69; // mapper 99 high nibble | NES 2.0 | Vs. System
    rom[11] = 0x07; // CHR-RAM: 64 << 7 = 8 KiB
    rom[13] = 0x50; // Vs. hardware type 5 (DualSystem)
    for half in [0usize, 0x8000] {
        let prg = &mut rom[16 + half..16 + half + 0x8000];
        prg[0..3].copy_from_slice(&[0x4C, 0x00, 0x80]); // $8000: JMP $8000
        prg[0x7FFA..0x8000].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
    }
    rom
}

/// A cabinet run past power-on, as a game in progress would be.
fn warmed_dual() -> VsDualSystem {
    let mut dual = VsDualSystem::from_rom(&dual_cabinet_rom()).expect("cabinet image parses");
    for _ in 0..60 {
        dual.run_frame();
    }
    dual
}

/// NL-09: the cabinet's serialize and restore, as the libretro core runs
/// them. Until v2.9.1 that was `VsDualSystem::snapshot`, which built both
/// consoles' full snapshots (thumbnails included) and a third buffer holding
/// both, on every call; it is now `snapshot_into` a reused buffer.
fn bench_dual(c: &mut Criterion) {
    c.bench_function("vs_dual_serialize", |b| {
        let mut dual = warmed_dual();
        let mut buf = Vec::new();
        b.iter(|| {
            dual.snapshot_into(&mut buf);
            black_box(buf.len());
        });
    });
    c.bench_function("vs_dual_restore", |b| {
        let mut dual = warmed_dual();
        let mut blob = Vec::new();
        dual.snapshot_into(&mut blob);
        b.iter(|| {
            dual.restore(black_box(&blob)).expect("restore round-trips");
        });
    });
}

/// v2.9.9 (libretro re-audit NL-15): a 512 KiB self-flashable image for the
/// two flash boards, GTROM (mapper 111) and UNROM 512 (mapper 30 with the
/// battery bit, whose `$8000-$BFFF` window is the SST39SF040 flash). Their
/// `load_state` rebuilds the whole flash from a sector diff, so a restore's
/// cost scales with the chip, not the game; run-ahead and rollback pay it
/// every frame.
///
/// One image serves both boards. Every 16 KiB bank starts with `JMP $C000`
/// and ends with all three vectors at `$C000`, so whichever bank is mapped
/// at `$C000-$FFFF` (UNROM 512's fixed last bank, GTROM's odd half of a
/// 32 KiB bank) loops in place. The flash content is unflashed ROM, so the
/// sector diff is the bitmap alone: what is measured is the board's restore
/// work, not the copy of flashed sectors.
fn flash_board_rom(mapper: u8, battery: bool) -> Vec<u8> {
    const PRG: usize = 512 * 1024;
    // Derived from `PRG` so the header cannot disagree with the image. The
    // assert runs at compile time and turns a future `PRG` past the header's
    // 255-bank field into a build error, so the cast below cannot truncate.
    // (`u8::try_from` would say this directly, but `TryFrom` is not const.)
    #[allow(clippy::cast_possible_truncation)] // guarded by the assert
    const PRG_BANKS: u8 = {
        assert!(
            PRG / 0x4000 <= u8::MAX as usize,
            "PRG exceeds the iNES bank count"
        );
        (PRG / 0x4000) as u8
    };
    let mut rom = vec![0u8; 16 + PRG];
    rom[0..4].copy_from_slice(b"NES\x1a");
    rom[4] = PRG_BANKS; // 32 x 16 KiB PRG
    rom[5] = 0; // CHR-RAM
    rom[6] = ((mapper & 0x0F) << 4) | if battery { 0x02 } else { 0 };
    rom[7] = mapper & 0xF0;
    for bank in rom[16..].as_chunks_mut::<0x4000>().0 {
        bank[0..3].copy_from_slice(&[0x4C, 0x00, 0xC0]); // JMP $C000
        bank[0x3FFA..0x4000].copy_from_slice(&[0x00, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
    }
    rom
}

fn bench_flash_restore(c: &mut Criterion, label: &str, rom: &[u8]) {
    c.bench_function(&format!("nes_restore_flash_{label}"), |b| {
        let mut nes = warmed_nes(rom);
        let blob = nes.snapshot();
        b.iter(|| {
            nes.restore(black_box(&blob)).expect("restore round-trips");
        });
    });
    c.bench_function(&format!("nes_restore_quiet_flash_{label}"), |b| {
        let mut nes = warmed_nes(rom);
        let mut blob = Vec::new();
        nes.snapshot_core_into(&mut blob);
        b.iter(|| {
            nes.restore_quiet(black_box(&blob))
                .expect("restore round-trips");
        });
    });
}

fn bench_snapshot_restore(c: &mut Criterion) {
    bench_rom(c, "flowing_palette", "assorted/flowing_palette.nes");
    bench_rom(c, "mmc3", "holy_mapperel/M4_P128K_CR8K.nes");
    bench_dual(c);
    bench_flash_restore(c, "gtrom", &flash_board_rom(111, false));
    bench_flash_restore(c, "unrom512", &flash_board_rom(30, true));
}

criterion_group!(benches, bench_snapshot_restore);
criterion_main!(benches);
