//! blargg `ppu_vbl_nmi/rom_singles/*.nes` corpus.
//!
//! Ten sub-ROMs that exercise the precise (PPU-clock-resolution) timing of
//! the VBL flag set/clear, NMI assertion, the $2002-race suppression, and
//! the odd-frame dot skip. Per `docs/testing-strategy.md` §Layer 3.
//!
//! All sub-ROMs use NROM (mapper 0). Driven through the full lockstep
//! `Nes` facade in `rustynes-core`.

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

/// Run a single PPU-VBL-NMI sub-ROM up to `max_frames` and report the result.
/// Stops on terminal status; does NOT panic on non-zero result so callers
/// can choose pass/fail granularity.
fn run_ppu(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("blargg/ppu_vbl_nmi/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

/// All 10 sub-ROMs pass cleanly as of v0.9.0 (see CHANGELOG). Each is
/// strict-asserted; surprise failures fail CI loudly.

#[test]
fn ppu_vbl_nmi_01_vbl_basics() {
    let (s, m, _) = run_ppu("01-vbl_basics.nes", 600);
    assert_eq!(s, 0, "01-vbl_basics failed: {m}");
}

#[test]
fn ppu_vbl_nmi_02_vbl_set_time() {
    let (s, m, _) = run_ppu("02-vbl_set_time.nes", 600);
    assert_eq!(s, 0, "02-vbl_set_time failed: {m}");
}

#[test]
fn ppu_vbl_nmi_03_vbl_clear_time() {
    let (s, m, _) = run_ppu("03-vbl_clear_time.nes", 600);
    assert_eq!(s, 0, "03-vbl_clear_time failed: {m}");
}

#[test]
fn ppu_vbl_nmi_04_nmi_control() {
    let (s, m, _) = run_ppu("04-nmi_control.nes", 600);
    assert_eq!(s, 0, "04-nmi_control failed: {m}");
}

#[test]
fn ppu_vbl_nmi_05_nmi_timing() {
    let (s, m, _) = run_ppu("05-nmi_timing.nes", 600);
    assert_eq!(s, 0, "05-nmi_timing failed: {m}");
}

#[test]
fn ppu_vbl_nmi_06_suppression() {
    let (s, m, _) = run_ppu("06-suppression.nes", 600);
    assert_eq!(s, 0, "06-suppression failed: {m}");
}

#[test]
fn ppu_vbl_nmi_07_nmi_on_timing() {
    let (s, m, _) = run_ppu("07-nmi_on_timing.nes", 600);
    assert_eq!(s, 0, "07-nmi_on_timing failed: {m}");
}

#[test]
fn ppu_vbl_nmi_08_nmi_off_timing() {
    let (s, m, _) = run_ppu("08-nmi_off_timing.nes", 600);
    assert_eq!(s, 0, "08-nmi_off_timing failed: {m}");
}

#[test]
fn ppu_vbl_nmi_09_even_odd_frames() {
    let (s, m, _) = run_ppu("09-even_odd_frames.nes", 600);
    assert_eq!(s, 0, "09-even_odd_frames failed: {m}");
}

#[test]
fn ppu_vbl_nmi_10_even_odd_timing() {
    let (s, m, _) = run_ppu("10-even_odd_timing.nes", 600);
    assert_eq!(s, 0, "10-even_odd_timing failed: {m}");
}
