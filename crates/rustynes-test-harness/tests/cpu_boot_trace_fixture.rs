//! Per-CPU-instruction boot-trace fixture (Session-12 observability).
//!
//! Drives `AccuracyCoin` (default) OR a caller-specified ROM from cold
//! boot with the `cpu-boot-trace` cargo feature enabled, then dumps a
//! binary [`CpuBootTrace`](rustynes_core::cpu_boot_trace::CpuBootTrace) to
//! `target/cpu_boot_trace/<rom_stem>_boot.bin` for diff against a
//! Mesen2-emitted reference trace.
//!
//! See `scripts/mesen2_cpu_boot_trace.lua` for the Mesen2-side reference
//! generator and `crates/rustynes-core/src/cpu_boot_trace.rs` for the schema.
//!
//! # Capture window
//!
//! Default: CPU cycles `0..=200_000` (~5 cold-boot frames at NTSC's
//! ~29,780 cycles/frame).  Override via env vars:
//!
//! * `RUSTYNES_CPU_BOOT_TRACE_START_CYCLE` (default `0`).  Session-17
//!   extended the original Session-12 fixture to use this cutoff for
//!   post-boot windows on failing test ROMs (`cpu_interrupts_v2/{2,3,5}`
//!   + `mmc3_test_2/4`).
//! * `RUSTYNES_CPU_BOOT_TRACE_END_CYCLE` (default `200_000`)
//! * `RUSTYNES_CPU_BOOT_TRACE_OUT` (default
//!   `target/cpu_boot_trace/<rom_stem>_boot.bin`)
//! * `RUSTYNES_CPU_BOOT_TRACE_ROM` (default `tests/roms/accuracycoin/AccuracyCoin.nes`).
//!   Absolute or workspace-relative path. Used by Session-17 to point
//!   the fixture at the failing `cpu_interrupts_v2` / `mmc3_test_2`
//!   ROMs for per-instruction divergence diffs against Mesen2.
//!
//! # Why a dedicated fixture
//!
//! Like the IRQ and PPU-state trace fixtures before it, the boot trace
//! is heavy enough (~60 k records at the 200 k cycle window times 32
//! bytes = ~2 MB) that we do NOT want it running on every
//! `cargo test --workspace`.  Gated behind TWO cargo features
//! (`test-roms` + `cpu-boot-trace`) and invoked explicitly:
//!
//! ```bash
//! cargo test -p rustynes-test-harness \
//!     --features test-roms,cpu-boot-trace \
//!     --test cpu_boot_trace_fixture -- --nocapture
//! ```

#![cfg(all(feature = "test-roms", feature = "cpu-boot-trace"))]

use std::env;
use std::fs;
use std::path::PathBuf;

use rustynes_core::cpu_boot_trace::{CpuBootTrace, CpuBootTraceConfig};
use rustynes_core::Nes;

/// Per-fixture record cap.  Sized generously at 1M records to comfortably
/// cover the default 200 k cycle window (~60 k records expected; cap
/// gives 16x headroom).
const TRACE_CAPACITY: usize = 1_000_000;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn default_rom_path() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("roms")
        .join("accuracycoin")
        .join("AccuracyCoin.nes")
}

/// Resolve the active ROM path. Honors `RUSTYNES_CPU_BOOT_TRACE_ROM`
/// (absolute or workspace-relative); falls back to `AccuracyCoin` (the
/// Session-12 default).
fn rom_path() -> PathBuf {
    match env::var("RUSTYNES_CPU_BOOT_TRACE_ROM") {
        Ok(s) if !s.is_empty() => {
            let candidate = PathBuf::from(&s);
            if candidate.is_absolute() {
                candidate
            } else {
                workspace_root().join(candidate)
            }
        }
        _ => default_rom_path(),
    }
}

/// Stem used to name the output binary. Mirrors the ROM file name
/// without extension, so different ROMs do not stomp each other.
fn rom_stem() -> String {
    rom_path()
        .file_stem()
        .and_then(|s| s.to_str())
        .map_or_else(|| "boot".to_string(), str::to_string)
}

fn read_env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

#[test]
fn accuracycoin_cold_boot_emits_binary_trace() {
    let start_cycle = read_env_u64("RUSTYNES_CPU_BOOT_TRACE_START_CYCLE", 0);
    let end_cycle = read_env_u64("RUSTYNES_CPU_BOOT_TRACE_END_CYCLE", 200_000);
    assert!(
        start_cycle <= end_cycle,
        "RUSTYNES_CPU_BOOT_TRACE_START_CYCLE ({start_cycle}) must be <= END_CYCLE ({end_cycle})"
    );

    let stem = rom_stem();
    let out_path = env::var("RUSTYNES_CPU_BOOT_TRACE_OUT").map_or_else(
        |_| {
            workspace_root()
                .join("target")
                .join("cpu_boot_trace")
                .join(format!("{stem}_boot.bin"))
        },
        PathBuf::from,
    );

    let rom = rom_path();
    println!("[cpu_boot_trace_fixture] rom: {}", rom.display());
    let bytes = fs::read(&rom).unwrap_or_else(|e| panic!("read rom: {e} (path={rom:?})"));
    let mut nes = Nes::from_rom(&bytes).expect("parse rom");

    let cfg = CpuBootTraceConfig::cycles(start_cycle..=end_cycle);
    println!("[cpu_boot_trace_fixture] config: cycles={start_cycle}..={end_cycle}");
    nes.enable_cpu_boot_trace(CpuBootTrace::with_capacity(TRACE_CAPACITY, cfg));

    // Run enough frames to cover `end_cycle` plus a small tail margin.
    // At NTSC ~29,780 cycles/frame, frame count = end_cycle / 29_780 + 1.
    let target_frames = (end_cycle / 29_780) + 2;
    for _ in 0..target_frames {
        nes.run_frame();
        if nes.bus().cycle() > end_cycle {
            break;
        }
    }

    let trace = nes.take_cpu_boot_trace().expect("trace was enabled above");
    println!(
        "[cpu_boot_trace_fixture] captured cycles={}..={} records={} overflow={} (cap={})",
        start_cycle,
        end_cycle,
        trace.len(),
        trace.overflow(),
        TRACE_CAPACITY,
    );
    assert_eq!(
        trace.overflow(),
        0,
        "trace buffer overflowed - raise TRACE_CAPACITY or shrink the window"
    );
    assert!(
        !trace.is_empty(),
        "trace captured zero records - is the cycle window in range?"
    );

    let out_dir = out_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out_dir.display()));
    let binary = trace.to_binary();
    fs::write(&out_path, &binary).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
    println!(
        "[cpu_boot_trace_fixture] wrote {} bytes to {}",
        binary.len(),
        out_path.display()
    );

    // CSV preview for human inspection (first 500 records).
    let csv_path = out_path.with_extension("preview.csv");
    let csv = preview_csv(&trace, 500);
    fs::write(&csv_path, csv).unwrap_or_else(|e| panic!("write {}: {e}", csv_path.display()));
    println!(
        "[cpu_boot_trace_fixture] wrote {}-record CSV preview to {}",
        trace.len().min(500),
        csv_path.display()
    );

    // Roundtrip sanity check.
    let parsed = CpuBootTrace::from_binary(&binary).expect("binary trace must parse");
    assert_eq!(parsed.len(), trace.len(), "roundtrip record count mismatch");
    if !trace.is_empty() {
        assert_eq!(
            parsed.records()[0],
            trace.records()[0],
            "roundtrip first-record mismatch"
        );
    }
}

fn preview_csv(trace: &CpuBootTrace, limit: usize) -> String {
    if trace.len() <= limit {
        return trace.to_csv();
    }
    let cfg = trace.config().clone();
    let mut sub = CpuBootTrace::with_capacity(limit, cfg);
    for r in trace.records().iter().take(limit) {
        sub.maybe_push(r.clone());
    }
    sub.to_csv()
}
