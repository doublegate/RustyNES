//! v2.1.8 "Performance" A1 — the differential byte-identity gate for the
//! specialized visible-scanline **fast dot path** (`Nes::set_fast_dotloop`).
//!
//! The PPU per-dot FSM (`Ppu::tick`) is the emulator's single hottest function
//! (~46% of a representative frame's self-time; `docs/performance.md`). The A1
//! optimization dispatches the common "clean" visible BG-render dots (a visible
//! scanline, dots `1..=256`, rendering stably enabled, no sub-dot disturbance)
//! to a straight-line handler (`Ppu::tick_visible_render_fast`) that runs the
//! *identical* helper sequence with the statically-dead event/bookkeeping
//! branches pruned. It is a pure internal speedup — the emulated output must not
//! move by a single bit.
//!
//! This suite is the hard contract for that claim. For each ROM in a corpus
//! spanning the accuracy-critical configurations (`nestest` CPU/idle-render,
//! `flowing_palette` full-BG-every-frame, `oam_stress` sprite-eval stress,
//! `AccuracyCoin` the PPU-timing gauntlet, and the Holy Mapperel MMC1/MMC3
//! banked boards), it runs the SAME scripted input twice — once with the fast
//! path OFF (the shipped exact path) and once ON — and asserts that EVERY
//! observable stream is bit-for-bit identical:
//!
//! * the RGBA framebuffer, every frame;
//! * the palette-index framebuffer (composite-filter input), every frame;
//! * the emitted audio samples, every frame;
//! * the cumulative CPU-cycle count; and
//! * the full serialized core snapshot (all internal PPU/CPU/APU/mapper state).
//!
//! A per-frame hash vector is compared so a divergence pinpoints the exact
//! frame. Any single-bit difference fails the gate — the fast path would then be
//! wrong for that case and must either widen its disturbance guard (fall back to
//! the exact path) or be dropped. This mirrors the byte-identity discipline the
//! `extra_scanlines` and OAM-decay knobs are held to.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::{Buttons, Nes};

/// Scripted, deterministic input for frame `f`: Start on a 4-of-7 cycle (drives
/// title screens forward — Mesen2's `PGOHelper` trick) plus a rotating
/// d-pad/A/B mix so scrolling + sprite + collision render paths actually run
/// (which is exactly what the visible-scanline fast path accelerates). Identical
/// across the OFF and ON runs, so it can never itself introduce a difference.
fn buttons_for(f: u32) -> Buttons {
    let mut b = Buttons::empty();
    if f % 7 <= 3 {
        b |= Buttons::START;
    }
    match (f / 30) % 4 {
        0 => b |= Buttons::RIGHT | Buttons::A,
        1 => b |= Buttons::LEFT,
        2 => b |= Buttons::A | Buttons::B,
        _ => b |= Buttons::DOWN,
    }
    b
}

/// Fold one frame's every observable output into a single 64-bit hash: the RGBA
/// framebuffer, the palette-index framebuffer (as little-endian bytes), and the
/// audio drained this frame.
fn frame_hash(nes: &Nes, audio: &[f32]) -> u64 {
    let mut h = fnv1a64(nes.framebuffer());
    // Mix the index framebuffer.
    let idx_bytes: Vec<u8> = nes
        .index_framebuffer()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    h ^= fnv1a64(&idx_bytes).rotate_left(17);
    // Mix the audio.
    let audio_bytes: Vec<u8> = audio.iter().flat_map(|s| s.to_le_bytes()).collect();
    h ^= fnv1a64(&audio_bytes).rotate_left(33);
    h
}

struct Capture {
    /// One combined hash per frame (framebuffer + index buffer + audio).
    per_frame: Vec<u64>,
    /// Cumulative CPU cycles across the whole run.
    cpu_cycles: u64,
    /// Full serialized core state at the end of the run.
    snapshot: Vec<u8>,
}

/// Run `rom` for `frames` frames with the fast dot path `fast` (on/off), feeding
/// the scripted input, and capture every observable stream.
fn capture(rom: &str, frames: u32, fast: bool) -> Capture {
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom}: {e:?}"));
    nes.set_fast_dotloop(fast);
    assert_eq!(nes.fast_dotloop(), fast, "fast_dotloop knob did not stick");

    let start = nes.cycle();
    let mut per_frame = Vec::with_capacity(frames as usize);
    for f in 0..frames {
        nes.set_buttons(0, buttons_for(f));
        nes.run_frame();
        let audio = nes.drain_audio();
        per_frame.push(frame_hash(&nes, &audio));
    }
    Capture {
        per_frame,
        cpu_cycles: nes.cycle().wrapping_sub(start),
        snapshot: nes.snapshot(),
    }
}

/// The core differential assertion for one ROM: OFF (exact path) and ON (fast
/// path) must agree bit-for-bit on every stream, every frame.
fn assert_byte_identical(rom: &str, frames: u32) {
    let exact = capture(rom, frames, false);
    let fast = capture(rom, frames, true);

    assert_eq!(
        exact.per_frame.len(),
        fast.per_frame.len(),
        "{rom}: frame count differs"
    );
    // Pinpoint the FIRST diverging frame for a useful failure message.
    for (i, (a, b)) in exact
        .per_frame
        .iter()
        .zip(fast.per_frame.iter())
        .enumerate()
    {
        assert_eq!(
            a, b,
            "{rom}: fast dot path diverged at frame {i} \
             (framebuffer / index buffer / audio hash mismatch) — \
             the fast path is NOT byte-identical for this case"
        );
    }
    assert_eq!(
        exact.cpu_cycles, fast.cpu_cycles,
        "{rom}: cumulative CPU-cycle count differs (fast path changed timing)"
    );
    assert_eq!(
        fnv1a64(&exact.snapshot),
        fnv1a64(&fast.snapshot),
        "{rom}: final core snapshot differs (fast path changed internal state)"
    );
}

/// Corpus spanning the accuracy-critical configurations. `frames` is sized to
/// get each ROM well past its boot/blank period and into steady-state rendering
/// where the fast path is exercised, while keeping the test brisk.
const CORPUS: &[(&str, u32)] = &[
    // CPU-heavy, near-static menu (BG fetch + sprite eval active).
    ("nestest/nestest.nes", 180),
    // Full background rewritten every frame — the fast path's prime workload.
    ("sprint-2/flowing_palette.nes", 180),
    // Sprite-evaluation stress (OAM / secondary-OAM / overflow paths).
    ("sprint-2/oam_stress.nes", 180),
    // The PPU-timing gauntlet: sprite-0 hit, $2007 stress, ALE + Read, etc.
    ("accuracycoin/AccuracyCoin.nes", 240),
    // Banked MMC1 board (mapper 1) — A12/CHR-bank interaction with rendering.
    ("holy_mapperel/M1_P128K_CR8K.nes", 180),
    // Banked MMC3 board (mapper 4) — the dot-260 A12 IRQ path under rendering.
    ("holy_mapperel/M4_P128K_CR8K.nes", 180),
    // A mid-frame raster demo (mapper 1) exercising mid-scanline scroll writes,
    // which MUST force the exact path (disturbance guard).
    ("nes-test-roms/scanline/scanline.nes", 180),
];

#[test]
fn fast_dotloop_is_byte_identical_across_corpus() {
    for &(rom, frames) in CORPUS {
        assert_byte_identical(rom, frames);
    }
}

/// Sanity: setting the knob OFF must be byte-identical to never touching it at
/// all (the stock path the whole oracle uses). Guards against the field's mere
/// presence perturbing anything.
#[test]
fn fast_dotloop_off_equals_untouched() {
    let rom = "sprint-2/flowing_palette.nes";
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap();

    let untouched = {
        let mut nes = Nes::from_rom(&bytes).unwrap();
        let mut hashes = Vec::new();
        for f in 0..120 {
            nes.set_buttons(0, buttons_for(f));
            nes.run_frame();
            let audio = nes.drain_audio();
            hashes.push(frame_hash(&nes, &audio));
        }
        (hashes, nes.snapshot())
    };
    let off = capture(rom, 120, false);

    assert_eq!(
        untouched.0, off.per_frame,
        "{rom}: OFF != untouched (per-frame)"
    );
    assert_eq!(
        fnv1a64(&untouched.1),
        fnv1a64(&off.snapshot),
        "{rom}: OFF != untouched (snapshot)"
    );
}
