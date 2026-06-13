//! blargg `blargg_nes_cpu_test5/*.nes` corpus (v2.2.x coverage wiring).
//!
//! PROTOCOL FINDING (verified by probing the `$6000..$6003` window over 20k
//! frames): unlike the v5 `instr_test` corpus, these two ROMs use blargg's
//! **older shell** that reports results ONLY on-screen — the `$6000` window
//! stays `00 00 00 00` for the entire run (no `$DE $B0 $61` magic is ever
//! written). A `run_nes_blargg` "status == 0" check would be a FALSE pass
//! (it reads the never-initialised `$6000 = 0x00`).
//!
//! These ROMs are therefore wired as framebuffer-FNV-1a visual smokes. Each
//! ROM is run to a fixed frame count well past the point where the result
//! screen stabilises (~frame 1000), and the framebuffer hash is snapshotted
//! via `insta`. The rendered framebuffer is where the per-opcode
//! "Failed <opcode>" / "All tests complete" text appears, so a hash change
//! flags a CPU-instruction regression (the screen would list the failing
//! opcodes) or a render regression.
//!
//! Both ROMs render "All tests complete" with NO opcode failures listed
//! (the unofficial-opcode `cpu.nes` passes too on this core); their final
//! framebuffers are byte-identical (the "all passed" screen). The hashes are
//! deterministic (verified stable across two runs).
//!
//! Subsystem: 6502 instruction behaviour (official + unofficial opcodes,
//! checksum-validated). Suspect on a hash drift: an instruction-semantics
//! or addressing-mode regression, or a BG-render regression.
//!
//! Per `docs/testing-strategy.md` §Layer 4 (visual regression corpus).

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// Run `rel` for `frames` frames (no input) and return the framebuffer
/// FNV-1a hash + a stable snapshot line.
fn run_and_snapshot(rel: &str, frames: u64) -> String {
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
fn blargg5_official() {
    // Documented opcodes. Result screen stable by ~frame 1000; 1500 is a
    // safe margin.
    let snap = run_and_snapshot("nes-test-roms/blargg_nes_cpu_test5/official.nes", 1500);
    insta::assert_snapshot!("blargg5_official_f1500", snap);
}

#[test]
fn blargg5_cpu() {
    // All opcodes incl. unofficial / unstable. Passes on this core (no opcode
    // failures listed on screen).
    let snap = run_and_snapshot("nes-test-roms/blargg_nes_cpu_test5/cpu.nes", 1500);
    insta::assert_snapshot!("blargg5_cpu_f1500", snap);
}
