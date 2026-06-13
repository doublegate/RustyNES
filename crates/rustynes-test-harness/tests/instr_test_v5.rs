//! blargg `instr_test-v5/rom_singles/*.nes` corpus.
//!
//! The 16 sub-ROMs ship as iNES mapper 0 (NROM, 32 KiB PRG + 8 KiB CHR-ROM)
//! and so are runnable purely on the Phase-1 CPU. The `all_instrs.nes` and
//! `official_only.nes` aggregates ship as iNES mapper 1 (MMC1) — Phase-2
//! Sprint 4 / Checkpoint 1 acceptance.
//!
//! Each ROM follows the standard blargg protocol: the test code writes its
//! progress status to `$6000`-`$6003` and an ASCII result message to
//! `$6004..`. We let the runner step until the `$6000` byte transitions
//! from `$80` (running) to a final result code, then assert pass.
//!
//! Per `docs/testing-strategy.md` §Layer 3, we treat each sub-ROM as a
//! pass/fail unit test. A failure prints the ROM-supplied message verbatim
//! so the diagnostic comes straight from the test author.

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

fn run_single(name: &str) {
    // The v5 singles + aggregates live under `blargg/instr_test_v5/`.
    let path = rom_path(&format!("blargg/instr_test_v5/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    // Each sub-ROM finishes in well under 50 million cycles on real
    // hardware. We give a generous 200 M to allow for the harness's
    // approximate reset handling and any retry loops.
    let result = run_blargg_until_complete(&bytes, 200_000_000)
        .unwrap_or_else(|e| panic!("rom must parse and run: {e}"));
    assert_eq!(
        result.status, 0,
        "{name} failed with status {:#x} after {} cycles\nmessage: {}",
        result.status, result.cycles, result.message
    );
}

#[test]
fn instr_test_01_basics() {
    run_single("01-basics.nes");
}

#[test]
fn instr_test_02_implied() {
    run_single("02-implied.nes");
}

#[test]
fn instr_test_03_immediate() {
    run_single("03-immediate.nes");
}

#[test]
fn instr_test_04_zero_page() {
    run_single("04-zero_page.nes");
}

#[test]
fn instr_test_05_zp_xy() {
    run_single("05-zp_xy.nes");
}

#[test]
fn instr_test_06_absolute() {
    run_single("06-absolute.nes");
}

#[test]
fn instr_test_07_abs_xy() {
    run_single("07-abs_xy.nes");
}

#[test]
fn instr_test_08_ind_x() {
    run_single("08-ind_x.nes");
}

#[test]
fn instr_test_09_ind_y() {
    run_single("09-ind_y.nes");
}

#[test]
fn instr_test_10_branches() {
    run_single("10-branches.nes");
}

#[test]
fn instr_test_11_stack() {
    run_single("11-stack.nes");
}

#[test]
fn instr_test_12_jmp_jsr() {
    run_single("12-jmp_jsr.nes");
}

#[test]
fn instr_test_13_rts() {
    run_single("13-rts.nes");
}

#[test]
fn instr_test_14_rti() {
    run_single("14-rti.nes");
}

#[test]
fn instr_test_15_brk() {
    run_single("15-brk.nes");
}

#[test]
fn instr_test_16_special() {
    run_single("16-special.nes");
}

#[test]
fn instr_test_all_instrs_mmc1() {
    // Aggregate ROM, mapper 1 (MMC1). Exercises the consecutive-write bug
    // and PRG bank switching.
    run_single("all_instrs.nes");
}

#[test]
fn instr_test_official_only_mmc1() {
    // Aggregate ROM, mapper 1 (MMC1). Subset of all_instrs covering only
    // the documented opcodes.
    run_single("official_only.nes");
}

// ============================================================================
// blargg `instr_test-v3` aggregates (v2.2.x coverage wiring).
//
// The older v3 corpus ships the same `$6000` blargg status protocol via the
// CPU-only runner. Both aggregates report "All 15 tests passed" on this core
// (`all_instrs.nes` includes the unofficial opcodes and passes too), so both
// are wired STRICT. Subsystem: 6502 instruction behaviour (official +
// unofficial / unstable opcodes).
// ============================================================================

fn run_v3(name: &str) {
    let path = rom_path(&format!("nes-test-roms/instr_test-v3/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let result = run_blargg_until_complete(&bytes, 200_000_000)
        .unwrap_or_else(|e| panic!("rom must parse and run: {e}"));
    assert_eq!(
        result.status, 0,
        "{name} failed with status {:#x} after {} cycles\nmessage: {}",
        result.status, result.cycles, result.message
    );
}

#[test]
fn instr_test_v3_official_only() {
    // instr_test-v3 aggregate: documented opcodes only. Reports
    // "All 15 tests passed".
    run_v3("official_only.nes");
}

#[test]
fn instr_test_v3_all_instrs() {
    // instr_test-v3 aggregate: (almost) all opcodes incl. unofficial. This
    // core passes the unofficial set too ("All 15 tests passed").
    run_v3("all_instrs.nes");
}
