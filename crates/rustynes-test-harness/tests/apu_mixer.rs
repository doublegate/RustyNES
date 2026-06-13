//! blargg `apu_mixer/*.nes` corpus.  Validates the lookup-table non-linear
//! mixer.  4 ROMs (square, triangle, noise, dmc) + the readme.

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
    let path = rom_path(&format!("blargg/apu_mixer/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

#[test]
fn apu_mixer_square() {
    let (s, m, f) = run("square.nes", 2500);
    eprintln!("apu_mixer/square: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "apu_mixer/square failed: {m}");
}

#[test]
fn apu_mixer_triangle() {
    let (s, m, f) = run("triangle.nes", 2500);
    eprintln!("apu_mixer/triangle: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "apu_mixer/triangle failed: {m}");
}

#[test]
fn apu_mixer_noise() {
    let (s, m, f) = run("noise.nes", 2500);
    eprintln!("apu_mixer/noise: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "apu_mixer/noise failed: {m}");
}

#[test]
fn apu_mixer_dmc() {
    let (s, m, f) = run("dmc.nes", 2500);
    eprintln!("apu_mixer/dmc: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "apu_mixer/dmc failed: {m}");
}
