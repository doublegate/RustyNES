//! Fuzz target for the save-state (`.rns`) deserializer — untrusted **file
//! input**.
//!
//! A save-state slot is arbitrary on-disk bytes (a user can hand-edit or
//! corrupt one, or load one another program wrote). Three parse entry points
//! must reject malformed input with a typed `SnapshotError`, never panic / OOB /
//! hang:
//!
//! - [`parse_header`] — the fixed header (magic, format version, section table
//!   offset). The lightest structural check.
//! - `Nes::extract_thumbnail` — parses the header then walks the section list
//!   to find the embedded thumbnail, without touching emulator state (a pure
//!   parse over the whole container, incl. every section's length prefix).
//! - `Nes::restore_quiet` — the full restore into a live `Nes`, which decodes
//!   every section (CPU/PPU/APU/mapper/WRAM/…) and their bounded length fields.
//!
//! The base `Nes` is a synthesized minimal NROM so a *structurally valid* fuzz
//! case can actually reach the per-section decoders (not just bounce off the
//! magic check).
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run save_state
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_core::{Nes, parse_header};

/// A minimal iNES NROM (16 KiB PRG that spins in an infinite loop + 8 KiB CHR),
/// enough to construct a real `Nes` whose `restore` path can be exercised.
fn synth_nrom() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + 16 * 1024 + 8 * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(1); // 1 x 16 KiB PRG
    bytes.push(1); // 1 x 8 KiB CHR
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&[0u8; 8]);
    let mut prg = vec![0u8; 16 * 1024];
    // Reset vector -> $C000; a `JMP $C000` there.
    prg[0] = 0x4C;
    prg[1] = 0x00;
    prg[2] = 0xC0;
    let len = prg.len();
    prg[len - 4] = 0x00; // NMI  low
    prg[len - 3] = 0xC0; // NMI  high
    prg[len - 6] = 0x00; // reset low
    prg[len - 5] = 0xC0; // reset high
    prg[len - 2] = 0x00; // IRQ  low
    prg[len - 1] = 0xC0; // IRQ  high
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
    bytes
}

fuzz_target!(|data: &[u8]| {
    // 1. Pure header parse — must never panic on any byte slice.
    let _ = parse_header(data);

    // 2. Whole-container thumbnail walk (header + every section length prefix).
    let _ = Nes::extract_thumbnail(data);

    // 3. Full restore into a live NROM Nes. A malformed state must return an
    //    error and leave the Nes usable, not panic or corrupt memory.
    if let Ok(mut nes) = Nes::from_rom(&synth_nrom()) {
        let _ = nes.restore_quiet(data);
    }
});
