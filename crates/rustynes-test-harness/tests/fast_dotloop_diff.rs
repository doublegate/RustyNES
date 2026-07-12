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
//! spanning the accuracy-critical configurations (`nestest` a rendering-enabled
//! menu — where the fast path actually engages, `flowing_palette` a
//! rendering-DISABLED 64-colour backdrop-override demo — the guard-bail /
//! neutral case, `oam_stress` sprite-eval stress, `AccuracyCoin` the PPU-timing
//! gauntlet, and the Holy Mapperel MMC1/MMC3
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

use rustynes_core::{Buttons, Nes, PpuRevision};

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

/// FNV-1a 64-bit over a stream of bytes (identical algorithm/constants to
/// [`fnv1a64`], but folding an iterator so callers never materialize a `Vec`).
fn fnv1a64_stream(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Fold one frame's every observable output into a single 64-bit hash: the RGBA
/// framebuffer, the palette-index framebuffer, and the audio drained this frame.
/// The `u16` index buffer and `f32` samples are hashed by folding their
/// little-endian bytes directly (no per-frame `Vec` allocation — this runs on
/// every frame of every corpus ROM, twice per ROM).
fn frame_hash(nes: &Nes, audio: &[f32]) -> u64 {
    let mut h = fnv1a64(nes.framebuffer());
    h ^= fnv1a64_stream(nes.index_framebuffer().iter().flat_map(|v| v.to_le_bytes()))
        .rotate_left(17);
    h ^= fnv1a64_stream(audio.iter().flat_map(|s| s.to_le_bytes())).rotate_left(33);
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

/// Run `rom` for `frames` frames with the fast dot path `fast` (on/off) and the
/// given PPU die `revision`, feeding the scripted input, and capture every
/// observable stream.
fn capture(rom: &str, frames: u32, fast: bool, revision: PpuRevision) -> Capture {
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom}: {e:?}"));
    nes.set_ppu_revision(revision);
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

/// The core differential assertion for one ROM under one PPU revision: OFF
/// (exact path) and ON (fast path) must agree bit-for-bit on every stream,
/// every frame.
fn assert_byte_identical(rom: &str, frames: u32, revision: PpuRevision) {
    let exact = capture(rom, frames, false, revision);
    let fast = capture(rom, frames, true, revision);

    assert_eq!(
        exact.per_frame.len(),
        fast.per_frame.len(),
        "{rom} [{revision:?}]: frame count differs"
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
            "{rom} [{revision:?}]: fast dot path diverged at frame {i} \
             (framebuffer / index buffer / audio hash mismatch) — \
             the fast path is NOT byte-identical for this case"
        );
    }
    assert_eq!(
        exact.cpu_cycles, fast.cpu_cycles,
        "{rom} [{revision:?}]: cumulative CPU-cycle count differs (fast path changed timing)"
    );
    assert_eq!(
        fnv1a64(&exact.snapshot),
        fnv1a64(&fast.snapshot),
        "{rom} [{revision:?}]: final core snapshot differs (fast path changed internal state)"
    );
}

/// Corpus spanning the accuracy-critical configurations. `frames` is sized to
/// get each ROM well past its boot/blank period and into steady-state rendering
/// where the fast path is exercised, while keeping the test brisk.
const CORPUS: &[(&str, u32)] = &[
    // Rendering-ENABLED near-static menu (BG fetch + sprite eval active) — the
    // case where the fast path actually engages.
    ("nestest/nestest.nes", 180),
    // Rendering-DISABLED 64-colour backdrop-override demo: the fast path never
    // engages (the guard bails at `rendering_enabled()`), so this pins the
    // neutral / guard-bail case as byte-identical too.
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
        assert_byte_identical(rom, frames, PpuRevision::Rp2c02H);
    }
}

/// v2.1.7 P5 (#280) added the opt-in `Rp2c02G` die revision, whose only per-dot
/// effect is that an OAMADDR (`$2003`) write during rendering ARMS
/// `oam_corruption_pending`. That armed/pending state is one of the
/// disturbances the fast-path dispatch guard tests (`!oam_corruption_pending`),
/// so the fast path must drop to the exact path the instant a `$2003`-write
/// corruption is armed and let the exact path arm/commit it. This re-runs the
/// OAM-exercising corpus with the corruption-modelling revision enabled to
/// PROVE fast == exact even through #280's corruption paths.
#[test]
fn fast_dotloop_is_byte_identical_under_oamaddr_corruption_revision() {
    // The OAM / sprite-heavy members of the corpus — the ones most likely to
    // drive OAMADDR (`$2003`) writes during rendering and thus actually arm
    // #280's corruption on `Rp2c02G`.
    for &(rom, frames) in &[
        ("sprint-2/oam_stress.nes", 180u32),
        ("accuracycoin/AccuracyCoin.nes", 240),
        ("nestest/nestest.nes", 180),
        ("nes-test-roms/scanline/scanline.nes", 180),
    ] {
        assert_byte_identical(rom, frames, PpuRevision::Rp2c02G);
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
    let off = capture(rom, 120, false, PpuRevision::Rp2c02H);

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
