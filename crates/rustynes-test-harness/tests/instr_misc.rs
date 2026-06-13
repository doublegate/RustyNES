//! blargg `instr_misc` corpus (T-71-003, Phase 7 Sprint 1).
//!
//! Four sub-ROMs exercising CPU instruction edge cases that the larger
//! `instr_test-v5` suite does not: `abs,X` address wraparound, branch
//! wraparound, and APU/PPU dummy-read side effects. The aggregate
//! `instr_misc.nes` runs all four in sequence. iNES mapper 1 (MMC1).
//!
//! `04-dummy_reads_apu` reads APU/PPU registers to observe dummy-read
//! side effects, so these run on the **full** lockstep `Nes`
//! (`run_nes_blargg`), not the CPU-only `BlarggBus` — the CPU-only bus has
//! no real APU and the sub-test cannot pass against it.
//!
//! Source: blargg's NES test ROMs (public domain). See `tests/roms/LICENSES.md`.
//!
//! v2.1.0 coverage wiring also folds in the `cpu_exec_space/` corpus
//! (Quietust's "NES Memory Execution Tests"): verifies the CPU can execute
//! opcode fetches from I/O space (`$2000-$401F`), the PPU open-bus
//! write-then-read-back rule, and the one-byte-opcode dummy read of the
//! following byte. Two ROMs:
//!
//! - `test_cpu_exec_space_ppuio.nes` — exec from PPU I/O space + PPU open
//!   bus. PASSES strictly ("JSR+RTS / JMP+RTS / RTS+RTS / JMP+RTI / JMP+BRK
//!   TEST OK ... Passed").
//! - `test_cpu_exec_space_apu.nes` — exec from APU I/O + open bus on the
//!   write-only APU ports and the unallocated `$4018-$40FF` window. PASSES
//!   strictly ("... Passed").
//!
//! Like the `sprdma_and_dmc_dma` corpus, these use the `$6000` status
//! protocol but write the result WITHOUT the canonical `$DE $B0 $61` magic
//! preamble, so the strict assertion additionally requires "Passed" in the
//! result text to guard against a false `$6000 == 0` fall-through.

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

/// The aggregate completes by ~frame 225; singles finish sooner. A 400-frame
/// budget gives comfortable headroom without bloating CI.
fn run_single(name: &str, max_frames: u64) {
    let path = rom_path(&format!("blargg/instr_misc/{name}"));
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
fn instr_misc_01_abs_x_wrap() {
    run_single("01-abs_x_wrap.nes", 400);
}

#[test]
fn instr_misc_02_branch_wrap() {
    run_single("02-branch_wrap.nes", 400);
}

#[test]
fn instr_misc_03_dummy_reads() {
    run_single("03-dummy_reads.nes", 400);
}

#[test]
fn instr_misc_04_dummy_reads_apu() {
    run_single("04-dummy_reads_apu.nes", 400);
}

#[test]
fn instr_misc_all() {
    run_single("instr_misc.nes", 400);
}

// ============================================================================
// cpu_exec_space — execute opcode fetches from I/O space + PPU/APU open bus +
// one-byte-opcode dummy read. Both ROMs PASS strictly on the R1 master-clock
// default build. The `$6000` status is written without the `$DE $B0 $61`
// magic preamble, so we also require "Passed" to rule out a false
// `$6000 == 0` fall-through.
// ============================================================================

fn run_exec_space(name: &str, max_frames: u64) {
    let path = rom_path(&format!("nes-test-roms/cpu_exec_space/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let result = run_nes_blargg(&bytes, max_frames)
        .unwrap_or_else(|e| panic!("rom must parse and run: {e}"));
    assert_eq!(
        result.status, 0,
        "{name} failed with status {:#x} after {} frames\nmessage: {}",
        result.status, result.frames, result.message
    );
    assert!(
        result.message.contains("Passed"),
        "{name} did not report Passed (false $6000==0 fall-through?): {}",
        result.message
    );
}

#[test]
fn cpu_exec_space_ppuio() {
    run_exec_space("test_cpu_exec_space_ppuio.nes", 600);
}

#[test]
fn cpu_exec_space_apu() {
    run_exec_space("test_cpu_exec_space_apu.nes", 600);
}
