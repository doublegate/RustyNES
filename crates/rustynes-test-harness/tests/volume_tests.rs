//! `volume_tests/volumes.nes` (v2.2.x coverage wiring).
//!
//! PROTOCOL FINDING (verified by probing the `$6000` window): this ROM has
//! NO `$6000` status protocol and renders a BLACK screen for the whole run —
//! it is a pure-audio test (it plays the four tone channels + DMC/PCM at
//! ramped volumes; the bbbradsmith corpus ships reference `.ogg` recordings
//! under `recordings/` for ear comparison, not an on-screen pass/fail).
//!
//! It is therefore wired as an audio-FNV-1a smoke (with the framebuffer hash
//! and audio-sample count alongside as secondary sentinels). The audio hash
//! reinterprets the drained `f32` samples as raw IEEE-754 LE bytes and
//! FNV-1a-hashes them — the same convention as `run_and_capture_full`. A hash
//! change flags an APU mixer / channel-volume / non-linear-DAC regression.
//!
//! Subsystem: APU channel mixing + per-channel DAC volume (the non-linear
//! analog mixer). Suspect on a drift: the lookup-table mixer or a per-channel
//! volume path.
//!
//! Per `docs/testing-strategy.md` §Layer 4 (audio regression).

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::Nes;

/// Run `rel` for `frames` frames (no input), draining audio each frame, and
/// return `(fb_hash, audio_sample_count, audio_hash)`.
fn run_audio(rel: &str, frames: u64) -> (u64, usize, u64) {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
    let mut samples: Vec<f32> = Vec::new();
    for _ in 0..frames {
        nes.run_frame();
        samples.extend(nes.drain_audio());
    }
    let fb_hash = fnv1a64(nes.framebuffer());
    let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        audio_bytes.extend_from_slice(&s.to_le_bytes());
    }
    (fb_hash, samples.len(), fnv1a64(&audio_bytes))
}

#[test]
fn volume_tests_audio_smoke() {
    const ROM: &str = "nes-test-roms/volume_tests/volumes.nes";
    const FRAMES: u64 = 600;
    let (fb, samples, audio) = run_audio(ROM, FRAMES);
    // Audio must actually have been produced — a zero-length buffer would mean
    // the APU mixer path went silent (a real regression this smoke must catch).
    assert!(
        samples > 0,
        "volumes.nes produced no audio samples — APU path went silent"
    );
    let snap = format!(
        "rom={ROM} frames={FRAMES} fb_bytes=245760 fb_fnv1a64={fb:016x} \
         audio_samples={samples} audio_fnv1a64={audio:016x}"
    );
    insta::assert_snapshot!("volume_tests_volumes_f600", snap);
}

#[test]
fn volume_tests_deterministic() {
    // Same input twice -> identical audio + framebuffer (determinism contract).
    const ROM: &str = "nes-test-roms/volume_tests/volumes.nes";
    const FRAMES: u64 = 600;
    let a = run_audio(ROM, FRAMES);
    let b = run_audio(ROM, FRAMES);
    assert_eq!(
        a, b,
        "volumes.nes run must be deterministic for identical input"
    );
}
