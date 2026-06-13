//! blargg `dmc_dma_during_read4` corpus.  Validates the 2A03 DMC-DMA
//! cycle stealing + register-readout-during-DMA bug.
//!
//! Note: the upstream repo's `rom_singles/` directory does NOT include a
//! `dma_4015_read.nes` variant; the canonical `dmc_dma_during_read4`
//! release ships:
//!   - `dma_2007_read.nes` (DMC stalls during `$2007` read; PPU buffer
//!     advances multiple times — the documented bug).
//!   - `dma_2007_write.nes`
//!   - `dma_4016_read.nes` (controller shift register skips bits).
//!   - `double_2007_read.nes`
//!   - `read_write_2007.nes`
//!
//! v2.1.0 coverage wiring also folds in two adjacent DMA/DMC corpora:
//!
//! - `sprdma_and_dmc_dma/` (blargg) — OAM-DMA + DMC-DMA cycle-steal
//!   alignment. Both `.nes` and `_512.nes` variants PASS strictly on the
//!   R1 master-clock default build (the message ends in "Passed"). These
//!   ROMs use the `$6000` status protocol but write the result text/status
//!   directly WITHOUT the canonical `$DE $B0 $61` magic preamble, so the
//!   strict assertion additionally requires the "Passed" marker in the
//!   result text to guard against a false `$6000 == 0` fall-through.
//!
//! - `dmc_tests/` (4 ROMs: `buffer_retained`, `latency`, `status`,
//!   `status_irq`) — these exercise the DMC channel buffer / fetch latency /
//!   `$4015` DMC-status + DMC IRQ. They are NOT framebuffer-or-`$6000`
//!   reporters: each renders a single uniform (blank) frame for its entire
//!   run (all four share the byte-identical framebuffer hash) and reports
//!   results via APU audio tones. So they are wired as audio-FNV-1a smokes
//!   (the framebuffer is useless as a sentinel here — all four hash
//!   identically — but each ROM produces a DISTINCT, deterministic audio
//!   hash). A drift in the DMC buffer/latency/`$4015`/IRQ path surfaces as
//!   an audio-hash change. Suspect subsystem on drift: the rustynes-apu DMC
//!   channel (`apu.rs` DMC buffer + sample-fetch timing + `$4015` status).

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;
use rustynes_test_harness::run_nes_blargg;

fn run(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("blargg/dmc_dma_during_read4/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

#[test]
fn dmc_dma_2007_read() {
    let (s, m, f) = run("dma_2007_read.nes", 1500);
    eprintln!("dma_2007_read: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "dma_2007_read failed: {m}");
}

#[test]
fn dmc_dma_2007_write() {
    let (s, m, f) = run("dma_2007_write.nes", 1500);
    eprintln!("dma_2007_write: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "dma_2007_write failed: {m}");
}

#[test]
fn dmc_dma_4016_read() {
    let (s, m, f) = run("dma_4016_read.nes", 1500);
    eprintln!("dma_4016_read: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "dma_4016_read failed: {m}");
}

#[test]
fn dmc_double_2007_read() {
    let (s, m, f) = run("double_2007_read.nes", 1500);
    eprintln!("double_2007_read: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "double_2007_read failed: {m}");
}

#[test]
fn dmc_read_write_2007() {
    let (s, m, f) = run("read_write_2007.nes", 1500);
    eprintln!("read_write_2007: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "read_write_2007 failed: {m}");
}

// ============================================================================
// sprdma_and_dmc_dma — OAM-DMA + DMC-DMA cycle-steal alignment. Both PASS
// strictly on the R1 master-clock default build. These ROMs use the `$6000`
// status protocol but write the result WITHOUT the `$DE $B0 $61` magic
// preamble, so we additionally require "Passed" in the message to rule out a
// false `$6000 == 0` fall-through.
// ============================================================================

fn run_sprdma(name: &str, max_frames: u64) -> (u8, String, u64) {
    let path = rom_path(&format!("nes-test-roms/sprdma_and_dmc_dma/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_blargg(&bytes, max_frames).expect("rom must parse + run");
    (r.status, r.message, r.frames)
}

#[test]
fn sprdma_and_dmc_dma() {
    let (s, m, f) = run_sprdma("sprdma_and_dmc_dma.nes", 1500);
    eprintln!("sprdma_and_dmc_dma: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "sprdma_and_dmc_dma failed: {m}");
    assert!(
        m.contains("Passed"),
        "sprdma_and_dmc_dma did not report Passed (false $6000==0 fall-through?): {m}"
    );
}

#[test]
fn sprdma_and_dmc_dma_512() {
    let (s, m, f) = run_sprdma("sprdma_and_dmc_dma_512.nes", 1500);
    eprintln!("sprdma_and_dmc_dma_512: status={s:#x} frames={f} msg={m:?}");
    assert_eq!(s, 0, "sprdma_and_dmc_dma_512 failed: {m}");
    assert!(
        m.contains("Passed"),
        "sprdma_and_dmc_dma_512 did not report Passed (false $6000==0 fall-through?): {m}"
    );
}

// ============================================================================
// dmc_tests — DMC channel buffer / latency / `$4015` status / DMC IRQ. These
// report via audio (not `$6000` and not the framebuffer — all four render a
// byte-identical blank frame), so each is wired as an audio-FNV-1a smoke. The
// hashes are deterministic and DISTINCT per ROM. 240 frames (~4 s NES time)
// gives each ROM enough runtime to emit a representative DMC audio buffer.
// ============================================================================

/// Run a `dmc_tests` ROM for `frames` frames with no input and return the
/// FNV-1a hash of the drained audio samples (raw `f32` LE bytes).
fn dmc_audio_hash(name: &str, frames: u64) -> String {
    let path = rom_path(&format!("nes-test-roms/dmc_tests/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    let mut samples: Vec<f32> = Vec::new();
    for _ in 0..frames {
        nes.run_frame();
        samples.extend(nes.drain_audio());
    }
    let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        audio_bytes.extend_from_slice(&s.to_le_bytes());
    }
    format!(
        "rom=dmc_tests/{name} frames={frames} audio_samples={} audio_fnv1a64={:016x}",
        samples.len(),
        fnv1a64(&audio_bytes)
    )
}

#[test]
fn dmc_tests_buffer_retained() {
    let snap = dmc_audio_hash("buffer_retained.nes", 240);
    insta::assert_snapshot!("dmc_tests_buffer_retained_f240", snap);
}

#[test]
fn dmc_tests_latency() {
    let snap = dmc_audio_hash("latency.nes", 240);
    insta::assert_snapshot!("dmc_tests_latency_f240", snap);
}

#[test]
fn dmc_tests_status() {
    let snap = dmc_audio_hash("status.nes", 240);
    insta::assert_snapshot!("dmc_tests_status_f240", snap);
}

#[test]
fn dmc_tests_status_irq() {
    let snap = dmc_audio_hash("status_irq.nes", 240);
    insta::assert_snapshot!("dmc_tests_status_irq_f240", snap);
}
