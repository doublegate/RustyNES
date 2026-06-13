//! MMC1 + PPU A12-transition regression test.
//!
//! Source ROM: `tests/roms/mmc1_a12/mmc1_a12.nes`, tepples,
//! public-domain via the `christopherpow/nes-test-roms` aggregator.
//!
//! The PPU's A12 line transitions on every BG / sprite CHR fetch.
//! For MMC3 those transitions are filtered through a small counter
//! that ultimately fires IRQ (see ADR-0002). For MMC1 — which has no
//! IRQ counter — the transitions must be **inert**. This ROM is the
//! control case: an MMC1 cart running in conditions that would over-
//! fire IRQs on a buggy emulator that routes A12 events to all
//! mappers indiscriminately.
//!
//! ## Why this matters
//!
//! `Mapper::notify_a12(level)` is a default-no-op in the trait; only
//! the MMC3 family overrides it. A regression in `rustynes-ppu` that
//! starts dispatching A12 to MMC1 (e.g. via a misplaced `if
//! mapper.has_irq() { ... }` short-circuit removal) would generate
//! spurious IRQs and corrupt MMC1's `$8000`-`$FFFF` shift register
//! state. The visible symptom would be CHR bank glitches mid-frame —
//! the kind of regression that a chip-level unit test wouldn't catch
//! but a real-ROM framebuffer baseline does.
//!
//! ## Frame count
//!
//! 240 frames (4 seconds) is past the ROM's static-pattern setup. No
//! controller input required.
//!
//! ## Diagnostic dump
//!
//! ```text
//! RUSTYNES_DUMP_FRAMES=1 cargo test -p rustynes-test-harness \
//!     --features test-roms --test mmc1_a12 -- --nocapture
//! ```

#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

mod common;

use common::{run_and_hash_with_dump, snapshot_line};

/// 240 frames = 4 seconds @ NTSC. The ROM's static display pattern
/// is stable from frame ~120 onwards; 240 gives margin for the
/// timing-window probes the test internally runs.
const STABILIZED_FRAME: u64 = 240;

#[test]
fn mmc1_a12_non_mmc3_a12_is_inert() {
    let rom = "mmc1_a12/mmc1_a12.nes";
    let hash = run_and_hash_with_dump("mmc1_a12", rom, STABILIZED_FRAME);
    let line = snapshot_line(rom, STABILIZED_FRAME, hash);
    insta::assert_snapshot!("mmc1_a12_non_mmc3_a12_is_inert", line);
}
