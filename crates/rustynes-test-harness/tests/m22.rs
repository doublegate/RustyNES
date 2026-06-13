//! `VRC2a` (mapper 22) CHR-banking regression test.
//!
//! Source ROM: `tests/roms/m22/0-127.nes`, `NewRisingSun`, public-domain
//! via the `christopherpow/nes-test-roms` aggregator.
//!
//! Boots `0-127.nes` for a fixed frame count, hashes the framebuffer
//! with FNV-1a 64-bit, and compares against an `insta` snapshot. The
//! ROM walks every 4 KiB CHR bank from 0 to 127, drawing each bank's
//! contents to the screen — `VRC2a`'s address-decoder quirk is the
//! "nybble-swap" pattern (low 4 bits at `$B000` / high 4 bits at
//! `$B001`), and a regression in the decoder silently drops banks.
//!
//! ## Why this matters
//!
//! The VRC2/4 register-address decoder is fragile: `VRC2a`, `VRC2b`,
//! `VRC4a`, `VRC4b` all share the same `$8000`-`$EFFF` decode space but
//! with **different bit positions for CHR-bank-LSB**. A regression in
//! `rustynes-mappers::vrc24` that mis-routes a bit produces a framebuffer
//! where some CHR banks render as zeros — exactly the kind of
//! "passes unit tests but breaks games" bug the
//! `feedback_emulator_fsm_mid_cycle_clobber` memory warns about.
//!
//! ## Frame count
//!
//! 240 frames (4 seconds) is well past the ROM's initial bank-walk
//! ramp-up. The ROM does not require any controller input.
//!
//! ## Diagnostic dump
//!
//! ```text
//! RUSTYNES_DUMP_FRAMES=1 cargo test -p rustynes-test-harness \
//!     --features test-roms --test m22 -- --nocapture
//! ```

#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

mod common;

use common::{run_and_hash_with_dump, snapshot_line};

/// Frame count chosen empirically: 240 frames lets the ROM walk
/// through all 128 CHR banks and settle on its final display pattern.
const STABILIZED_FRAME: u64 = 240;

#[test]
fn m22_vrc2a_chr_banking_0_127() {
    let rom = "m22/0-127.nes";
    let hash = run_and_hash_with_dump("m22", rom, STABILIZED_FRAME);
    let line = snapshot_line(rom, STABILIZED_FRAME, hash);
    insta::assert_snapshot!("m22_vrc2a_chr_banking_0_127", line);
}
