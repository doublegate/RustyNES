//! Visual regression corpus (T-24-009).
//!
//! Captures the framebuffer of small, deterministic NROM/MMC1 ROMs at
//! specific frame counts and compares against committed golden snapshots
//! using `insta`.  The test is the cheapest way to prove that a refactor
//! hasn't silently shifted a pixel anywhere on screen — every regression
//! shows up as a visible diff in the snapshot file.
//!
//! Each snapshot stores the FNV-1a hash of the framebuffer (text-form so it
//! shows up as a one-line PR diff) plus a small statistic block (frame
//! number, ROM filename, byte length).  The full RGBA framebuffer is far
//! too large to commit; the hash is the canonical determinism gate that
//! Checkpoint 4's `nes_determinism_two_runs_match` test already validates,
//! so a stable hash is sufficient as a regression sentinel here.
//!
//! Per `docs/testing-strategy.md` §Layer 4 and `to-dos/phase-2-graphics-
//! timing/sprint-4-system-integration-and-acceptance.md` ticket T-24-009.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_core::Nes;

fn rom_bytes(rel: &str) -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel);
    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// Run `nes` for `frames` frames and capture an FNV-1a 64-bit hash of the
/// framebuffer at the end.  Determinism is required: the same inputs MUST
/// produce the same hash on every run, and Phase 2's
/// `nes_determinism_two_runs_match` test in `rustynes-core` already enforces
/// this for synthetic ROMs.  This corpus extends that guarantee to real
/// homebrew test ROMs.
fn run_and_hash(rom: &str, frames: u64) -> String {
    let bytes = rom_bytes(rom);
    let mut nes = Nes::from_rom(&bytes).expect("rom must parse");
    for _ in 0..frames {
        nes.run_frame();
    }
    let fb = nes.framebuffer();
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in fb {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!(
        "rom={rom} frames={frames} fb_bytes={} fnv1a64={h:016x}",
        fb.len()
    )
}

#[test]
fn full_palette_frame_60() {
    let snap = run_and_hash("assorted/full_palette.nes", 60);
    insta::assert_snapshot!("full_palette_frame_60", snap);
}

#[test]
fn full_palette_frame_180() {
    let snap = run_and_hash("assorted/full_palette.nes", 180);
    insta::assert_snapshot!("full_palette_frame_180", snap);
}

#[test]
fn flowing_palette_frame_60() {
    let snap = run_and_hash("assorted/flowing_palette.nes", 60);
    insta::assert_snapshot!("flowing_palette_frame_60", snap);
}

#[test]
fn flowing_palette_frame_180() {
    let snap = run_and_hash("assorted/flowing_palette.nes", 180);
    insta::assert_snapshot!("flowing_palette_frame_180", snap);
}

#[test]
fn flowing_palette_frame_300() {
    let snap = run_and_hash("assorted/flowing_palette.nes", 300);
    insta::assert_snapshot!("flowing_palette_frame_300", snap);
}

#[test]
fn ppu_vbl_nmi_basics_frame_60() {
    // NROM, displays "Passed/Failed" status text — exercises both the BG
    // pipeline and CPU/PPU lockstep timing for the test outcome.
    let snap = run_and_hash("blargg/ppu_vbl_nmi/01-vbl_basics.nes", 120);
    insta::assert_snapshot!("ppu_vbl_nmi_basics_frame_120", snap);
}

#[test]
fn instr_test_basics_frame_60() {
    // MMC1 — extends coverage past NROM into the only other implemented
    // mapper-with-passing-test-corpus at this checkpoint.
    let snap = run_and_hash("blargg/instr_test_v5/01-basics.nes", 120);
    insta::assert_snapshot!("instr_test_basics_frame_120", snap);
}

#[test]
fn scanline_frame_180() {
    // `scanline/scanline.nes` (mapper 1) — a mid-frame scanline-effect demo
    // with no $6000 status protocol. Framebuffer hash is the only regression
    // sentinel; exercises mid-scanline scroll/timing in the BG pipeline.
    let snap = run_and_hash("nes-test-roms/scanline/scanline.nes", 180);
    insta::assert_snapshot!("scanline_frame_180", snap);
}

#[test]
fn nmi_sync_demo_ntsc_frame_180() {
    // `nmi_sync/demo_ntsc.nes` (UNROM) — an NMI-synchronised raster demo with
    // no $6000 status protocol. Framebuffer hash exercises precise NMI/PPU
    // timing alignment (the demo glitches visibly if NMI timing drifts).
    let snap = run_and_hash("nes-test-roms/nmi_sync/demo_ntsc.nes", 180);
    insta::assert_snapshot!("nmi_sync_demo_ntsc_frame_180", snap);
}
