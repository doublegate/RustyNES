//! blargg `blargg_ppu_tests_2005.09.15b/*.nes` corpus.
//!
//! Five NROM (mapper 0) PPU regression ROMs covering palette RAM
//! read/write, sprite (OAM) RAM, the VBL-flag clear timing, basic VRAM
//! ($2007) access, and the power-on palette state. All use the standard
//! blargg `$6000` status protocol driven through the full lockstep `Nes`.
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! Observed status (v2.1.0 coverage wiring, R1 master-clock default build):
//! all five PASS — including `power_up_palette`, which the v2.1.0 plan
//! flagged as a likely fail but actually clears under R1.

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
    let path = rom_path(&format!(
        "nes-test-roms/blargg_ppu_tests_2005.09.15b/{name}"
    ));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

#[test]
fn blargg_ppu_2005_palette_ram() {
    let (s, m, f) = run("palette_ram.nes", 600);
    eprintln!("palette_ram: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "palette_ram failed: {m}");
}

#[test]
fn blargg_ppu_2005_sprite_ram() {
    let (s, m, f) = run("sprite_ram.nes", 600);
    eprintln!("sprite_ram: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "sprite_ram failed: {m}");
}

#[test]
fn blargg_ppu_2005_vbl_clear_time() {
    let (s, m, f) = run("vbl_clear_time.nes", 600);
    eprintln!("vbl_clear_time: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "vbl_clear_time failed: {m}");
}

#[test]
fn blargg_ppu_2005_vram_access() {
    let (s, m, f) = run("vram_access.nes", 600);
    eprintln!("vram_access: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "vram_access failed: {m}");
}

#[test]
fn blargg_ppu_2005_power_up_palette() {
    let (s, m, f) = run("power_up_palette.nes", 600);
    eprintln!("power_up_palette: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "power_up_palette failed: {m}");
}
