//! `read_joy3/*.nes` corpus (controller-read regression ROMs).
//!
//! IMPORTANT — protocol finding (v2.1.0 coverage wiring): unlike the blargg
//! `$6000` corpora, the `read_joy3` ROMs do **not** use the `$6000` status
//! protocol. They report results as on-screen text (the `$6000..$6003`
//! window stays `0x00 0x00 0x00 0x00` for the entire run — no `$DE $B0 $61`
//! magic is ever written). So there is no status byte to strict-assert
//! against; a `run_nes_blargg` "status == 0" check would be a FALSE pass
//! (it would read the never-initialised `$6000 = 0x00`).
//!
//! These ROMs are therefore wired as framebuffer-FNV-1a visual smokes: each
//! ROM is run for a fixed number of frames and the framebuffer hash is
//! snapshotted via `insta`. The rendered framebuffer is where the
//! "passed/failed" / per-button error text actually appears, so a hash
//! change flags either a controller-read regression or a render regression.
//! The hashes are deterministic (verified stable across two runs).
//!
//! Subsystems exercised: the standard-controller serial `$4016`/`$4017`
//! read path. `count_errors` / `count_errors_fast` specifically stress
//! controller reads that race DMC DMA (the DMA-during-$4016 bit-steal
//! corner) — suspect subsystem if these hashes ever drift: the controller
//! shift-register read during an active DMC/OAM DMA.
//!
//! DMC-controller-conflict status (v2.2.x accuracy-polish research,
//! Item 1): the hardware bug — a `$4016`/`$4017` read whose cycle coincides
//! with a DMC DMA double-clocks the controller shift register, losing or
//! duplicating a joypad bit — is ALREADY MODELLED in
//! `rustynes_core::bus::Bus::dmc_dma_read` (the `$4016`/`$4017` conflict arms call
//! `controllers[port].read()`, advancing the shift register during the DMC
//! fetch). Verified empirically: `count_errors.nes` runs its 1000-iteration
//! loop and renders **"Conflicts: 149/1000"** at frame 240 — i.e. our core
//! produces 149 real DMC-vs-`$4016` conflicts, and the conflict-tolerant
//! `read_joy` routine compensates for every one (the ROM never hits its
//! `test_failed` halt; the loop runs to completion). This older test shell
//! reports ONLY on-screen (no `$6000` magic), so the framebuffer-FNV-1a
//! snapshot below LOCKS that exact completed screen: a regression that
//! DISABLED the conflict model (dropping the count toward 0) or that broke the
//! compensation (halting at "Failed") would change the hash and trip the test.
//! No chip change was made for Item 1 — the model is already correct and the
//! `AccuracyCoin` gate is already 100% without it.
//!
//! Per `docs/testing-strategy.md` §Layer 4 (visual regression corpus).

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// Run `rel` for `frames` frames with no input and return the framebuffer
/// FNV-1a hash. (The `read_joy3` ROMs self-drive — no controller input is
/// needed to reach the on-screen result text.)
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
fn read_joy3_test_buttons() {
    let snap = run_and_hash("nes-test-roms/read_joy3/test_buttons.nes", 240);
    insta::assert_snapshot!("read_joy3_test_buttons_f240", snap);
}

#[test]
fn read_joy3_thorough_test() {
    let snap = run_and_hash("nes-test-roms/read_joy3/thorough_test.nes", 240);
    insta::assert_snapshot!("read_joy3_thorough_test_f240", snap);
}

#[test]
fn read_joy3_count_errors() {
    // DMC-DMA-during-$4016 bit-steal stress (controller read during DMA).
    let snap = run_and_hash("nes-test-roms/read_joy3/count_errors.nes", 240);
    insta::assert_snapshot!("read_joy3_count_errors_f240", snap);
}

#[test]
fn read_joy3_count_errors_fast() {
    // DMC-DMA-during-$4016 bit-steal stress (controller read during DMA).
    let snap = run_and_hash("nes-test-roms/read_joy3/count_errors_fast.nes", 240);
    insta::assert_snapshot!("read_joy3_count_errors_fast_f240", snap);
}
