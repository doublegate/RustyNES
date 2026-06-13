//! Expansion-audio characterization corpus (`tests/roms/audio-tests/`).
//!
//! Re-wires the 19 bbbradsmith `nes-audio-tests` ROMs that were orphaned when
//! the legacy-scheduler-only `audio_tests.rs` was removed in v2.0.1 (commit
//! `49c4ccf`). These are the only in-tree expansion-audio (VRC6 / VRC7 /
//! Namco 163 / Sunsoft 5B / MMC5) characterization ROMs.
//!
//! None of these ROMs use the blargg `$6000` status protocol — they are
//! "hotswap" / listening / decibel-comparison ROMs whose output is the
//! rendered framebuffer plus the generated audio waveform. So each ROM is
//! captured with [`common::run_and_capture_full`] (framebuffer FNV-1a + CPU
//! cycle count + audio sample count + audio-buffer FNV-1a) and snapshotted
//! via `insta`. The audio hash is the load-bearing sentinel here: most of
//! these ROMs hold a near-static palette frame while emitting tones through
//! the APU + expansion-audio mixer, so a regression in the VRC6/VRC7/N163/5B/
//! MMC5 audio path surfaces as an audio-hash change.
//!
//! Determinism: the `(fb, cycles, audio)` capture is deterministic (the
//! whole core is), so the snapshots are stable run-to-run (verified by a
//! second pass after `INSTA_UPDATE=always` generation).
//!
//! Per `docs/testing-strategy.md` §Layer 4 and the audio-tests README.

#![cfg(feature = "test-roms")]

mod common;

use common::{run_and_capture_full, snapshot_line_full};

const CORPUS: &str = "audio_expansion";

/// Run one audio-tests ROM for `frames` frames and return the stable
/// snapshot line (fb + cycles + audio samples + audio hash).
fn capture(rom: &str, frames: u64) -> String {
    let rel = format!("audio-tests/{rom}");
    let (fb, cycles, samples, audio) = run_and_capture_full(CORPUS, &rel, frames);
    snapshot_line_full(&rel, frames, fb, cycles, samples, audio)
}

/// Generate one `insta` snapshot test per ROM. 120 frames (~2 s NES time)
/// is enough for each ROM to reach its steady tone/comparison state and emit
/// a representative audio buffer.
macro_rules! audio_expansion_test {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let snap = capture($rom, 120);
            insta::assert_snapshot!(concat!("audio_expansion_", stringify!($name)), snap);
        }
    };
}

// Decibel-comparison family (db_*): reference + per-chip square comparisons.
audio_expansion_test!(db_apu, "db_apu.nes");
audio_expansion_test!(db_vrc6a, "db_vrc6a.nes");
audio_expansion_test!(db_vrc6b, "db_vrc6b.nes");
audio_expansion_test!(db_vrc7, "db_vrc7.nes");
audio_expansion_test!(db_n163, "db_n163.nes");
audio_expansion_test!(db_5b, "db_5b.nes");
audio_expansion_test!(db_mmc5, "db_mmc5.nes");

// VRC7 characterization.
audio_expansion_test!(test_vrc7, "test_vrc7.nes");
audio_expansion_test!(patch_vrc7, "patch_vrc7.nes");
audio_expansion_test!(clip_vrc7, "clip_vrc7.nes");
audio_expansion_test!(noise_vrc7, "noise_vrc7.nes");

// Namco 163 characterization.
audio_expansion_test!(test_n163_longwave, "test_n163_longwave.nes");

// Sunsoft 5B / FME-7 characterization.
audio_expansion_test!(clip_5b, "clip_5b.nes");
audio_expansion_test!(noise_5b, "noise_5b.nes");
audio_expansion_test!(sweep_5b, "sweep_5b.nes");
audio_expansion_test!(envelope_5b, "envelope_5b.nes");
audio_expansion_test!(phase_5b, "phase_5b.nes");

// Base-APU quirks.
audio_expansion_test!(tri_silence, "tri_silence.nes");
audio_expansion_test!(dac_square, "dac_square.nes");
