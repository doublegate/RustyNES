//! Sprite overflow + sprite-zero hit ROM corpus.
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
// sprite_overflow_tests (5 sub-ROMs)
// ============================================================================

#[test]
fn sprite_overflow_1_basics() {
    let (s, m, f) = run_one("blargg/sprite_overflow_tests/1.Basics.nes", 600);
    eprintln!("sprite_overflow 1.Basics: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "1.Basics failed: {m}");
}

#[test]
fn sprite_overflow_2_details() {
    let (s, m, f) = run_one("blargg/sprite_overflow_tests/2.Details.nes", 600);
    eprintln!("sprite_overflow 2.Details: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "2.Details failed: {m}");
}

#[test]
fn sprite_overflow_3_timing() {
    let (s, m, f) = run_one("blargg/sprite_overflow_tests/3.Timing.nes", 600);
    eprintln!("sprite_overflow 3.Timing: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "3.Timing failed: {m}");
}

#[test]
fn sprite_overflow_4_obscure() {
    let (s, m, f) = run_one("blargg/sprite_overflow_tests/4.Obscure.nes", 600);
    eprintln!("sprite_overflow 4.Obscure: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "4.Obscure failed: {m}");
}

#[test]
fn sprite_overflow_5_emulator() {
    let (s, m, f) = run_one("blargg/sprite_overflow_tests/5.Emulator.nes", 600);
    eprintln!("sprite_overflow 5.Emulator: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "5.Emulator failed: {m}");
}

// ============================================================================
// sprite_hit_tests_2005.10.05 (11 sub-ROMs)
// ============================================================================

macro_rules! sprite_hit_test {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (s, m, f) = run_one(&format!("blargg/sprite_hit_tests/{}", $rom), 600);
            eprintln!("{}: status={s:#x} frames={f} msg={m:?}", $rom);
            assert_eq!(s, 0, "{} failed: {m}", $rom);
        }
    };
}

sprite_hit_test!(sprite_hit_01_basics, "01.basics.nes");
sprite_hit_test!(sprite_hit_02_alignment, "02.alignment.nes");
sprite_hit_test!(sprite_hit_03_corners, "03.corners.nes");
sprite_hit_test!(sprite_hit_04_flip, "04.flip.nes");
sprite_hit_test!(sprite_hit_05_left_clip, "05.left_clip.nes");
sprite_hit_test!(sprite_hit_06_right_edge, "06.right_edge.nes");
sprite_hit_test!(sprite_hit_07_screen_bottom, "07.screen_bottom.nes");
sprite_hit_test!(sprite_hit_08_double_height, "08.double_height.nes");
sprite_hit_test!(sprite_hit_09_timing_basics, "09.timing_basics.nes");
sprite_hit_test!(sprite_hit_10_timing_order, "10.timing_order.nes");
sprite_hit_test!(sprite_hit_11_edge_timing, "11.edge_timing.nes");

// ============================================================================
// oam_read / oam_stress (these were vendored in Phase 1's assorted/)
// ============================================================================

#[test]
fn oam_read_via_nes_runner() {
    let (s, m, f) = run_one("assorted/oam_read.nes", 600);
    eprintln!("oam_read: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "oam_read failed: {m}");
}

#[test]
fn oam_read_nes_test_roms_corpus() {
    // The standalone `oam_read/oam_read.nes` (40 KiB, distinct from the
    // Phase-1-vendored `assorted/oam_read.nes`): reads OAM through $2004 and
    // verifies the value matches what was written. PASSES under R1.
    let (s, m, f) = run_one("nes-test-roms/oam_read/oam_read.nes", 600);
    eprintln!("oam_read (nes-test-roms): status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "oam_read (nes-test-roms) failed: {m}");
}

#[test]
fn oam_stress_via_nes_runner() {
    // oam_stress runs ~30 seconds of NES time before reporting; the default
    // 600-frame budget cuts it off at status=$80 (still running).  3000
    // frames (~50 seconds) is comfortably past completion.
    let (s, m, f) = run_one("assorted/oam_stress.nes", 3000);
    eprintln!("oam_stress: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "oam_stress failed: {m}");
}
