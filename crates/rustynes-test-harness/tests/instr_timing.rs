//! blargg `instr_timing` corpus (T-71-003, Phase 7 Sprint 1).
//!
//! Two sub-ROMs validating per-instruction and per-branch CPU cycle counts
//! against the documented 6502 timing table (including NOPs, alternate SBC,
//! and unofficial instructions). iNES mapper 1 (MMC1).
//!
//! These run on the **full** lockstep `Nes` (`run_nes_blargg`): the timing
//! harness depends on APU frame-counter cadence, which the CPU-only
//! `BlarggBus` does not model. `1-instr_timing` completes by ~frame 1016
//! (the ROM advertises "about 25 seconds"); `2-branch_timing` is much
//! shorter.
//!
//! Source: blargg's NES test ROMs (public domain). See `tests/roms/LICENSES.md`.

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

fn run_single(name: &str, max_frames: u64) {
    let path = rom_path(&format!("blargg/instr_timing/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let result = run_nes_blargg(&bytes, max_frames)
        .unwrap_or_else(|e| panic!("rom must parse and run: {e}"));
    assert_eq!(
        result.status, 0,
        "{name} failed with status {:#x} after {} frames\nmessage: {}",
        result.status, result.frames, result.message
    );
}

#[test]
fn instr_timing_1_instr_timing() {
    run_single("1-instr_timing.nes", 1300);
}

#[test]
fn instr_timing_2_branch_timing() {
    run_single("2-branch_timing.nes", 400);
}
