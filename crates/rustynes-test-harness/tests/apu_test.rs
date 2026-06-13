//! blargg `apu_test/rom_singles/*.nes` corpus.
//!
//! 8 sub-ROMs covering APU register I/O, frame counter, length-counter
//! halt timing, IRQ flag timing, and DMC basics.  All ship as NROM
//! mapper 0 and use the standard `$6000` blargg status protocol.
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

fn run(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("blargg/apu_test/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

// 1-len_ctr: length-counter behavior.
#[test]
fn apu_test_1_len_ctr() {
    let (s, m, f) = run("1-len_ctr.nes", 1500);
    eprintln!("apu_test/1-len_ctr: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "1-len_ctr failed: {m}");
}

#[test]
fn apu_test_2_len_table() {
    let (s, m, f) = run("2-len_table.nes", 1500);
    eprintln!("apu_test/2-len_table: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "2-len_table failed: {m}");
}

#[test]
fn apu_test_3_irq_flag() {
    let (s, m, f) = run("3-irq_flag.nes", 1500);
    eprintln!("apu_test/3-irq_flag: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "3-irq_flag failed: {m}");
}

#[test]
fn apu_test_4_jitter() {
    let (s, m, f) = run("4-jitter.nes", 1500);
    eprintln!("apu_test/4-jitter: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "4-jitter failed: {m}");
}

#[test]
fn apu_test_5_len_timing() {
    let (s, m, f) = run("5-len_timing.nes", 1500);
    eprintln!("apu_test/5-len_timing: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "5-len_timing failed: {m}");
}

#[test]
fn apu_test_6_irq_flag_timing() {
    let (s, m, f) = run("6-irq_flag_timing.nes", 1500);
    eprintln!("apu_test/6-irq_flag_timing: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "6-irq_flag_timing failed: {m}");
}

#[test]
fn apu_test_7_dmc_basics() {
    let (s, m, f) = run("7-dmc_basics.nes", 1500);
    eprintln!("apu_test/7-dmc_basics: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "7-dmc_basics failed: {m}");
}

#[test]
fn apu_test_8_dmc_rates() {
    let (s, m, f) = run("8-dmc_rates.nes", 1500);
    eprintln!("apu_test/8-dmc_rates: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "8-dmc_rates failed: {m}");
}
