//! MMC1 + PPU A12-transition regression test.
//!
//! Source ROM: `tests/roms/mmc1_a12/mmc1_a12.nes`, Bregalad (its title
//! screen: "MMC1 WRAM DISABLE SCANLINE COUNTER TEST", 2010),
//! public-domain via the `christopherpow/nes-test-roms` aggregator.
//!
//! ## What the frame shows (v2.7.2)
//!
//! A12 does not reach an IRQ on MMC1, but it does select which CHR
//! register drives SNROM's PRG-RAM enable in 4 KiB CHR mode:
//! `nesdev_wiki/MMC1.xhtml` says mismatched registers leave the RAM
//! "enabled as the PPU renders". This ROM relies on that, and draws a
//! grey raster bar where it sees the enable change. v2.7.2 models the
//! A12-selected register, so the bar appears and the snapshot was
//! re-blessed. Reverting just that selection (`outer_reg` in
//! `m001_mmc1.rs`) reproduces the pre-v2.7.2 hash exactly. See
//! `tests/roms/mmc1_a12/README.md`.
//!
//! **Re-blessed again at v2.9.7**, when the PPU began reporting the A12 level
//! of every read, including the sprite window's garbage nametable fetches
//! (`Ppu::read_vram`). A12 now drops eight times per rendered line, as on
//! hardware, so the A12-selected register flips back during the sprite
//! window. Nine pixels of frame 240 change: the partial grey span that marks
//! the enable change ends a few pixels later. That is the expected shape for a
//! board whose RAM enable follows CHR A12, and no oracle here can confirm the
//! exact pixel, so this stays a byte-identity pin, not an accuracy verdict.
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
//! `Mapper::notify_a12(level)` is a default-no-op in the trait. The MMC3
//! family counts IRQs from it; MMC1 (since v2.7.2) only tracks the level. A regression in `rustynes-ppu` that
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
