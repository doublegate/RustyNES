//! blargg `nes_instr_test/rom_singles/*.nes` corpus (v2.2.x coverage wiring).
//!
//! 11 single-instruction-group ROMs (NROM/MMC1) using the standard `$6000`
//! blargg status protocol, driven through the full lockstep [`Nes`] via
//! [`run_nes_blargg`] (the PPU-online runner, since these report the
//! `$DE $B0 $61` magic + on-screen "Passed"/"Failed <opcode>" text).
//!
//! All 11 singles report "Passed" on this core, so every one is wired STRICT.
//! Subsystem: 6502 instruction behaviour per addressing-mode group.
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

fn run_single(name: &str) {
    let path = rom_path(&format!("nes-test-roms/nes_instr_test/rom_singles/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, 4000).expect("rom must parse + run");
    assert_eq!(
        r.status, 0,
        "{name} failed with status {:#x} after {} frames\nmessage: {}",
        r.status, r.frames, r.message
    );
}

#[test]
fn nes_instr_test_01_implied() {
    run_single("01-implied.nes");
}

#[test]
fn nes_instr_test_02_immediate() {
    run_single("02-immediate.nes");
}

#[test]
fn nes_instr_test_03_zero_page() {
    run_single("03-zero_page.nes");
}

#[test]
fn nes_instr_test_04_zp_xy() {
    run_single("04-zp_xy.nes");
}

#[test]
fn nes_instr_test_05_absolute() {
    run_single("05-absolute.nes");
}

#[test]
fn nes_instr_test_06_abs_xy() {
    run_single("06-abs_xy.nes");
}

#[test]
fn nes_instr_test_07_ind_x() {
    run_single("07-ind_x.nes");
}

#[test]
fn nes_instr_test_08_ind_y() {
    run_single("08-ind_y.nes");
}

#[test]
fn nes_instr_test_09_branches() {
    run_single("09-branches.nes");
}

#[test]
fn nes_instr_test_10_stack() {
    run_single("10-stack.nes");
}

#[test]
fn nes_instr_test_11_special() {
    run_single("11-special.nes");
}
