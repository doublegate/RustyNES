//! `TASVideos` / extended emulator-test corpus (v1.5.0 Workstream C1).
//!
//! Tests beyond the 139 `AccuracyCoin` battery, drawn from the
//! `nesdev.org` "Emulator tests" + "Tricky-to-emulate games" indices and
//! the `christopherpow/nes-test-roms` aggregator. Only ROMs with a clear
//! redistribution license are committed here (see `tests/roms/LICENSES.md`);
//! the broader corpus is exercised locally via the gitignored
//! `tests/roms/nes-test-roms/` checkout.
//!
//! ## `DPCM` Letterbox
//!
//! `dpcmletterbox/dpcmletterbox.nes` (Damian Yerrick, royalty-free) abuses
//! the `DMC` sample-playback hardware as a scanline timer to split the screen
//! twice without a mapper `IRQ` — the split positions depend on exact
//! `DMC`-rate timing, the sprite-0-hit dot, and the `NMI`/`DMC` phase. It
//! reports nothing programmatically (no `$6000` protocol); it renders a
//! letterboxed image. So it is wired as a deterministic framebuffer-`FNV-1a`
//! visual smoke (the same pattern as `visual_regression.rs` /
//! `p240_test_suite.rs`): a drift in `DMC`-`IRQ` cadence or sprite-0 timing
//! moves a raster split and changes the hash. The hash is deterministic per
//! the core's determinism contract.
//!
//! Per `docs/testing-strategy.md` §Layer 4 (visual regression corpus) and
//! §"Nesdev Completeness Audit".

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// Run `rel` for `frames` frames with no input and return a one-line
/// framebuffer FNV-1a snapshot string.
fn run_and_hash(rel: &str, frames: u64) -> String {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
    for _ in 0..frames {
        nes.run_frame();
    }
    let fb = nes.framebuffer();
    format!(
        "rom={rel} frames={frames} fb_bytes={} fnv1a64={:016x}",
        fb.len(),
        fnv1a64(fb)
    )
}

#[test]
fn dpcm_letterbox_frame_120() {
    // The raster split has settled by frame 120 (the NMI handler measures
    // the CPU<->PPU phase and recomputes the split each frame). NROM (0).
    let snap = run_and_hash("dpcmletterbox/dpcmletterbox.nes", 120);
    insta::assert_snapshot!("dpcm_letterbox_frame_120", snap);
}
