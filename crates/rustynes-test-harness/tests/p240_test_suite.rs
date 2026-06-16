//! `240pee/*.nes` corpus — the "240p test suite" (Damian Yerrick, free /
//! all-permissive homebrew, committed in-tree).
//!
//! The 240p test suite is a visual hardware-characterization menu ROM (color
//! bars, grids, sprite/scroll/overscan probes). It has no `$6000` status
//! protocol and reports nothing programmatically — it renders a menu and a
//! battery of visual test patterns. So both variants are wired here as
//! framebuffer-FNV-1a visual smokes (the same pattern `visual_regression.rs`
//! and `read_joy3.rs` use): run a fixed number of frames with no input and
//! snapshot the framebuffer hash. A hash change flags a regression in the
//! BG/sprite render pipeline or the mapper bank-switching these ROMs rely on.
//!
//! The two committed variants differ only in mapper, which is the point of
//! wiring both — they gate two already-implemented mappers through a real,
//! complex homebrew ROM:
//!   - `240pee.nes` — mapper 2 (`UxROM`).
//!   - `240pee-bnrom.nes` — mapper 34 (`BNROM`).
//!
//! Both render the same title/menu screen with no input (the menu content is
//! mapper-independent), so the two variants share a framebuffer hash — but
//! each test still independently gates that ITS mapper boots, switches its
//! PRG/CHR banks, and drives the render pipeline to that exact 8-color,
//! ~217K-nonzero-byte screen. A hash change flags a regression in that path.
//!
//! The hashes are deterministic (the determinism contract guarantees the same
//! seed+ROM+input ⇒ bit-identical framebuffer). Captured at frame 60 (the menu
//! has settled by then) with no input.
//!
//! Per `docs/testing-strategy.md` §Layer 4 (visual regression corpus).

#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

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
fn p240_uxrom_frame_60() {
    // mapper 2 (UxROM).
    let snap = run_and_hash("nes-test-roms/240pee/240pee.nes", 60);
    insta::assert_snapshot!("p240_uxrom_frame_60", snap);
}

#[test]
fn p240_bnrom_frame_60() {
    // mapper 34 (BNROM).
    let snap = run_and_hash("nes-test-roms/240pee/240pee-bnrom.nes", 60);
    insta::assert_snapshot!("p240_bnrom_frame_60", snap);
}
