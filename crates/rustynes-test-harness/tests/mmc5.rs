//! MMC5 (mapper 5) test ROM coverage.
//!
//! Aggregated MMC5 test suite from `christopherpow/nes-test-roms`'s
//! `mmc5test/` directory. These exercise:
//!
//! - Banking and bank-switching invariants (`mmc5test_v1.nes`,
//!   `mmc5test_v2.nes`).
//! - `ExRAM` modes 00/01/10/11 (`mmc5exram.nes`).
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! Note: these ROMs report results visually (palette / on-screen text)
//! rather than through the blargg `$6000` status byte. They also probe
//! deep MMC5 features (split-screen `ExGrafix`, audio extension) where
//! some sub-tests are deferred to v1.x. We can only smoke-test that
//! the emulator runs each ROM to the frame cap without panicking.

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

fn smoke_mmc5(rel: &str) {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, 600).expect("rom must parse + run");
    assert!(
        r.frames > 0,
        "{rel} produced 0 frames — emulator did not advance"
    );
}

#[test]
fn mmc5_test_v1_smoke() {
    smoke_mmc5("mmc5/mapper_mmc5test_v1.nes");
}

#[test]
fn mmc5_test_v2_smoke() {
    smoke_mmc5("mmc5/mapper_mmc5test_v2.nes");
}

#[test]
fn mmc5_exram_smoke() {
    smoke_mmc5("mmc5/mapper_mmc5exram.nes");
}
