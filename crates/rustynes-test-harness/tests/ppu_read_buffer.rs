//! `ppu_read_buffer/test_ppu_read_buffer.nes` (bisqwit's "PPU Read Buffer
//! Tests") — a mammoth corpus centred on the PPU `$2007` read buffer:
//! the one-byte non-palette read buffer, CIRAM sequential reads (1- and
//! 32-byte increments), palette-RAM reads (which bypass the buffer), open
//! bus, and `$2002`/`$2006`/`$2007` interactions.
//!
//! Protocol finding (v2.1.0 coverage wiring): this ROM does NOT use the
//! blargg `$6000` status protocol — the `$6000..$6003` window stays
//! `0x00 0x00 0x00 0x00` for the whole run (no `$DE $B0 $61` magic is ever
//! written). It reports pass/fail as on-screen text (with audio used for
//! progress while the screen is blanked, per its readme). A
//! `run_nes_blargg` "status == 0" check would therefore be a FALSE pass
//! (it would read the never-initialised `$6000 = 0x00`).
//!
//! So the ROM is wired as a framebuffer-FNV-1a visual smoke: the rendered
//! framebuffer (where the pass/fail and the numeric list of failed
//! sub-tests appear) is hashed via `insta` at a frame count where the
//! display has stabilised (verified stable at f1400/f1500, deterministic
//! across two runs). A hash change flags either a `$2007`-read-buffer
//! regression or a render regression.
//!
//! Suspect subsystem if this hash drifts: the rustynes-ppu `$2007` read path
//! (the 1-byte read buffer for non-palette VRAM, the palette-read
//! bypass, and the CIRAM/open-bus interactions in `ppu.rs`).
//!
//! Per `docs/testing-strategy.md` §Layer 4 (visual regression corpus).

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// Run `rel` for `frames` frames with no input and return a stable one-line
/// snapshot of the framebuffer FNV-1a hash. The ROM self-drives — no
/// controller input is needed to reach the on-screen result text.
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
fn ppu_read_buffer_test() {
    // 1500 frames (~25 s NES time): the readme notes the test takes ~20 s;
    // the framebuffer is stable from ~f1300 onward (verified f1400 == f1500).
    let snap = run_and_hash(
        "nes-test-roms/ppu_read_buffer/test_ppu_read_buffer.nes",
        1500,
    );
    insta::assert_snapshot!("ppu_read_buffer_test_f1500", snap);
}
