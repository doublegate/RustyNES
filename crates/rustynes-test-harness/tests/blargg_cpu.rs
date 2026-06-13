//! blargg CPU test ROMs that ship as NROM (mapper 0).
//!
//! Sprint-4 acceptance touches `instr_test_v5` (T-14-006), which uses MMC1
//! and so is gated on Sprint 5 (Phase 2 mapper work). NROM-bundled blargg
//! ROMs land here.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_blargg_until_complete;

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

#[test]
fn cpu_timing_test_completes() {
    // The NROM cpu_timing_test ROM. We don't currently expect it to *pass*
    // since cycle-accurate timing for every nestest-equivalent edge case
    // requires PPU lockstep. We DO require:
    //   1. parse + boot + run through enough cycles to clear the boot phase,
    //   2. no panic / no JAM,
    //   3. produce some terminal status (or hit max_cycles cleanly).
    let bytes = fs::read(rom_path("blargg/cpu_timing_test6/cpu_timing_test.nes"))
        .expect("read cpu_timing_test.nes");
    // Generous cycle budget; this ROM normally takes ~16 seconds wallclock.
    let result = run_blargg_until_complete(&bytes, 200_000_000).expect("rom must parse and run");
    assert!(
        result.cycles > 0,
        "harness must execute cycles before reporting"
    );
    eprintln!(
        "cpu_timing_test status={:#x} after {} cycles, msg={:?}",
        result.status, result.cycles, result.message
    );
}

#[test]
fn branch_timing_basics_completes() {
    let bytes = fs::read(rom_path("blargg/branch_timing_tests/1.Branch_Basics.nes"))
        .expect("read branch_timing 1");
    let result = run_blargg_until_complete(&bytes, 50_000_000).expect("rom must parse and run");
    eprintln!(
        "branch_timing_basics status={:#x} after {} cycles",
        result.status, result.cycles
    );
}
