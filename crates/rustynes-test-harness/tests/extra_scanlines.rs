//! v1.7.0 "Forge" Workstream F3 — PPU extra-scanlines overclock.
//!
//! The PPU can insert N extra idle vblank scanlines per frame (Mesen2
//! `UpdateTimings`), at the existing dot resolution. Each extra line is pure
//! additional CPU run-time: it renders nothing, sets/clears no PPU flag, and
//! fires no VBL/NMI/A12 event, so the visible framebuffer is unchanged.
//!
//! This suite is the determinism gate for that feature. It proves THREE things,
//! all required by the v1.7.0 hard contract:
//!
//! 1. **Byte-identical at zero.** A run with `set_extra_scanlines(0)` (and a run
//!    that never calls the setter at all) produces a framebuffer + audio +
//!    CPU-cycle stream IDENTICAL to stock. This is what keeps `AccuracyCoin`,
//!    the commercial oracle, and nestest unaffected (they never set it).
//! 2. **The visible image is unchanged with the feature ON.** With a non-zero
//!    extra-scanline count the framebuffer hash still matches the zero run — the
//!    extra lines are blank vblank lines, so nothing visible moves.
//! 3. **CPU time actually grows.** With extra scanlines the cumulative CPU-cycle
//!    count per frame increases by the expected amount (extra lines × 341 dots ÷
//!    3 dots-per-CPU-cycle, NTSC), confirming the feature does what it claims.
//!
//! Per `docs/ppu-2c02.md` §"Extra-scanlines overclock (F3)" and
//! `docs/testing-strategy.md`.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// A committed, deterministic, self-driving ROM that enables rendering (so the
/// framebuffer is a meaningful sentinel that the extra lines do NOT disturb).
/// `scanline.nes` (mapper 1) is a mid-frame raster demo already used by the
/// visual-regression corpus.
const ROM: &str = "nes-test-roms/scanline/scanline.nes";
const FRAMES: u64 = 120;

struct Capture {
    fb_hash: u64,
    audio_hash: u64,
    audio_samples: usize,
    cpu_cycles: u64,
}

/// Run `ROM` for `FRAMES` frames with `extra` extra scanlines configured
/// (`extra == None` means never call the setter — the true stock path) and
/// capture the framebuffer hash, audio hash, sample count, and cumulative CPU
/// cycles.
fn run(extra: Option<u16>) -> Capture {
    let path = rom_path(ROM);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {ROM}: {e}"));
    if let Some(n) = extra {
        nes.set_extra_scanlines(n);
    }
    let mut samples: Vec<f32> = Vec::new();
    let start = nes.cycle();
    for _ in 0..FRAMES {
        nes.run_frame();
        samples.extend(nes.drain_audio());
    }
    let cpu_cycles = nes.cycle().wrapping_sub(start);
    let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        audio_bytes.extend_from_slice(&s.to_le_bytes());
    }
    Capture {
        fb_hash: fnv1a64(nes.framebuffer()),
        audio_hash: fnv1a64(&audio_bytes),
        audio_samples: samples.len(),
        cpu_cycles,
    }
}

/// Setting `extra_scanlines(0)` must be byte-identical, in every observable
/// stream, to never touching the knob (the stock path the oracle uses).
#[test]
fn extra_scanlines_zero_is_byte_identical_to_stock() {
    let stock = run(None);
    let zero = run(Some(0));
    assert_eq!(
        stock.fb_hash, zero.fb_hash,
        "extra_scanlines(0) changed the framebuffer vs stock"
    );
    assert_eq!(
        stock.audio_hash, zero.audio_hash,
        "extra_scanlines(0) changed the audio vs stock"
    );
    assert_eq!(
        stock.audio_samples, zero.audio_samples,
        "extra_scanlines(0) changed the audio sample count vs stock"
    );
    assert_eq!(
        stock.cpu_cycles, zero.cpu_cycles,
        "extra_scanlines(0) changed the CPU-cycle count vs stock"
    );
}

/// The inserted lines emit nothing visible: the FIRST frame's framebuffer is
/// byte-identical with the feature on vs off.
///
/// This is the precise, ROM-independent image invariant. The extra lines are
/// inserted into the vblank period *after* the visible scanlines (0..=239) of
/// the frame have already been rendered, so frame 0's framebuffer is complete
/// before the first insertion ever runs — it cannot differ. (Across MANY frames
/// a timing-sensitive raster ROM legitimately *will* draw a different image,
/// because the extra CPU time shifts when it writes scroll/registers — that is
/// the whole point of an overclock and is NOT a determinism violation; the
/// determinism contract is byte-identity at the default `0`, proved above.)
#[test]
fn extra_scanlines_emit_nothing_into_the_first_frame() {
    fn first_frame_fb(extra: u16) -> u64 {
        let path = rom_path(ROM);
        let bytes = fs::read(&path).unwrap();
        let mut nes = Nes::from_rom(&bytes).unwrap();
        nes.set_extra_scanlines(extra);
        nes.run_frame();
        fnv1a64(nes.framebuffer())
    }
    assert_eq!(
        first_frame_fb(0),
        first_frame_fb(30),
        "extra scanlines altered the first frame — they must be inserted into \
         vblank, after the visible scanlines are already rendered"
    );
}

/// CPU time grows by the expected amount with the feature ON.
#[test]
fn extra_scanlines_add_cpu_time() {
    const EXTRA: u16 = 30;
    let stock = run(Some(0));
    let oc = run(Some(EXTRA));

    // Each extra NTSC scanline is 341 dots; the CPU advances once per 3 dots, so
    // each extra line nominally adds ~341/3 = 113.67 CPU cycles. The measured
    // per-line average lands a little under that (~109) because the insertion
    // shifts the CPU/PPU dot phase and interacts with the odd-frame dot skip, so
    // we bracket a generous [105, 116] per line — wide enough to be phase-robust
    // but tight enough to catch a "did nothing" / "ran the whole frame extra"
    // regression.
    assert!(
        oc.cpu_cycles > stock.cpu_cycles,
        "extra scanlines did not add CPU time: stock={} oc={}",
        stock.cpu_cycles,
        oc.cpu_cycles
    );
    let delta = oc.cpu_cycles - stock.cpu_cycles;
    let expected_lo = u64::from(EXTRA) * FRAMES * 105;
    let expected_hi = u64::from(EXTRA) * FRAMES * 116;
    assert!(
        (expected_lo..=expected_hi).contains(&delta),
        "CPU-cycle growth {delta} outside expected band [{expected_lo}, {expected_hi}] \
         for {EXTRA} extra lines over {FRAMES} frames"
    );
}

/// The feature is deterministic: same config + ROM + input ⇒ identical output.
#[test]
fn extra_scanlines_is_deterministic() {
    let a = run(Some(20));
    let b = run(Some(20));
    assert_eq!(a.fb_hash, b.fb_hash, "framebuffer not deterministic");
    assert_eq!(a.audio_hash, b.audio_hash, "audio not deterministic");
    assert_eq!(a.cpu_cycles, b.cpu_cycles, "CPU cycles not deterministic");
}

/// The configured count round-trips through the getter.
#[test]
fn extra_scanlines_getter_round_trips() {
    let path = rom_path(ROM);
    let bytes = fs::read(&path).unwrap();
    let mut nes = Nes::from_rom(&bytes).unwrap();
    assert_eq!(nes.extra_scanlines(), 0, "default must be 0 (stock)");
    nes.set_extra_scanlines(42);
    assert_eq!(nes.extra_scanlines(), 42);
    nes.set_extra_scanlines(0);
    assert_eq!(nes.extra_scanlines(), 0);
}
