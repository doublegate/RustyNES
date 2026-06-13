//! Misc blargg ROMs run through the lockstep `Nes` facade.
//!
//! Includes Phase 1 deferred ROMs (`cpu_timing_test6`, `branch_timing_tests`),
//! `cpu_dummy_reads`, `cpu_dummy_writes_oam`, `cpu_dummy_writes_ppumem`, and
//! `ppu_open_bus`. All ship as NROM (mapper 0) and need a working PPU.
//!
//! Per `docs/testing-strategy.md` §Layer 3.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::run_nes_blargg;

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

fn run_one(rel: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

// ============================================================================
// Phase 1 deferrals: re-run through the Nes facade now that PPU is online.
// These could not pass the Phase-1 stub bus because they read $2002 and need
// VBL timing.
// ============================================================================

#[test]
fn cpu_timing_test_phase1_deferred() {
    let (s, m, f) = run_one("blargg/cpu_timing_test6/cpu_timing_test.nes", 1500);
    eprintln!("cpu_timing_test: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "cpu_timing_test failed: {m}");
}

#[test]
fn branch_timing_basics_phase1_deferred() {
    let (s, m, f) = run_one("blargg/branch_timing_tests/1.Branch_Basics.nes", 600);
    eprintln!("branch_timing_basics: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "branch_timing_basics failed: {m}");
}

#[test]
fn branch_timing_backward_phase1_deferred() {
    let (s, m, f) = run_one("blargg/branch_timing_tests/2.Backward_Branch.nes", 600);
    eprintln!("branch_timing_backward: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "branch_timing_backward failed: {m}");
}

#[test]
fn branch_timing_forward_phase1_deferred() {
    let (s, m, f) = run_one("blargg/branch_timing_tests/3.Forward_Branch.nes", 600);
    eprintln!("branch_timing_forward: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "branch_timing_forward failed: {m}");
}

// ============================================================================
// Sprint-2 acceptance: open bus, dummy reads/writes.
// ============================================================================

#[test]
fn ppu_open_bus() {
    let (s, m, f) = run_one("blargg/ppu_open_bus/ppu_open_bus.nes", 1500);
    eprintln!("ppu_open_bus: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "ppu_open_bus failed: {m}");
}

#[test]
fn cpu_dummy_reads() {
    let (s, m, f) = run_one("blargg/cpu_dummy_reads/cpu_dummy_reads.nes", 1500);
    eprintln!("cpu_dummy_reads: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "cpu_dummy_reads failed: {m}");
}

#[test]
fn cpu_dummy_writes_oam() {
    let (s, m, f) = run_one("blargg/cpu_dummy_writes/cpu_dummy_writes_oam.nes", 1500);
    eprintln!("cpu_dummy_writes_oam: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "cpu_dummy_writes_oam failed: {m}");
}

#[test]
fn cpu_dummy_writes_ppumem() {
    let (s, m, _) = run_one("blargg/cpu_dummy_writes/cpu_dummy_writes_ppumem.nes", 1500);
    assert_eq!(s, 0, "cpu_dummy_writes_ppumem failed: {m}");
}
