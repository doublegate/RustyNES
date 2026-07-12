//! Expansion-audio characterization corpus (`tests/roms/audio-tests/`).
//!
//! Wires the 19 bbbradsmith `nes-audio-tests` ROMs (the only in-tree
//! expansion-audio — VRC6 / VRC7 / Namco 163 / Sunsoft 5B / MMC5 —
//! characterization ROMs) into two complementary layers.
//!
//! **Layer 1 — a real decibel-level oracle** (`level_db_*`). The bbbradsmith
//! `db_*` "hotswap" ROMs each play a sustained full-volume 2A03 reference
//! square in one time segment and the expansion-chip square (or, for `db_apu`,
//! the APU triangle) in a later segment. On real hardware you compare the two
//! by ear / oscilloscope; the emulator equivalent is measuring the peak
//! amplitude of each segment in the rendered waveform and taking the ratio.
//! Each `level_db_*` test does exactly that via
//! [`common::capture_frame_peaks`] and asserts the measured expansion/reference
//! ratio against the documented Mesen2 / hardware target — a machine-verifiable
//! accuracy criterion, not just a hash. Targets (v2.1.6, all derived from
//! Mesen2 `NesSoundMixer::GetOutputVolume` — the project's stated accuracy bar —
//! cross-checked against nestopia / `puNES` / fceux / tetanes; see
//! `docs/apu-2a03.md` §Expansion-audio levels and the audio-tests README):
//!
//! | ROM        | comparison                        | target ratio |
//! |------------|-----------------------------------|--------------|
//! | `db_apu`   | APU triangle / APU square         | ~ 0.524      |
//! | `db_vrc6a` | VRC6 square / APU square           | ~ 1.506      |
//! | `db_vrc6b` | VRC6 square / APU square (Madara)  | ~ 1.506      |
//! | `db_mmc5`  | MMC5 square / APU square           | ~ 1.000      |
//! | `db_n163`  | N163 1-ch square / APU square      | ~ 6.02       |
//!
//! `db_5b` and `db_vrc7` do NOT have a `level_db_*` assertion — their absolute
//! levels are honest documented gaps (the i16 `mix_audio` contract can't
//! represent the 5B's ~3.6x full-volume level; the VRC7 OPLL level metric is
//! patch-/waveform-dependent and not cleanly oracle-pinned), so they stay
//! snapshot-guarded only. The VRC7 instrument patch ROM is instead verified
//! byte-for-byte against the canonical Nuke.YKT dump by a `rustynes_apu::opll`
//! unit test (the real `patch_vrc7` criterion); the 5B logarithmic volume DAC
//! step law by a `rustynes_mappers::sprint3` unit test. See
//! `docs/accuracy-ledger.md` §Expansion-audio levels.
//!
//! **Layer 2 — a byte-exact regression guard** (`snapshot_*`). Every ROM (all
//! 19) is also captured with [`common::run_and_capture_full`] (framebuffer
//! FNV-1a + CPU cycle count + audio sample count + audio-buffer FNV-1a) and
//! snapshotted via `insta`. The audio hash is the load-bearing sentinel: most
//! of these ROMs hold a near-static palette frame while emitting tones through
//! the APU + expansion mixer, so any regression in the VRC6/VRC7/N163/5B/MMC5
//! audio path surfaces as an audio-hash change. A snapshot must be re-blessed
//! only when the new output is provably MORE accurate (measured vs the ROM
//! target), documented in the commit.
//!
//! Determinism: the whole core is deterministic, so both the `(fb, cycles,
//! audio)` capture and the per-frame peak envelope are byte-stable run-to-run
//! (verified by a second pass after `INSTA_UPDATE=always` generation) — which
//! is why the level oracle can pin frame windows.
//!
//! Per `docs/testing-strategy.md` §Layer 4 and the audio-tests README.

#![cfg(feature = "test-roms")]
#![allow(clippy::doc_markdown)]

mod common;

use common::{capture_frame_peaks, run_and_capture_full, snapshot_line_full, window_peak};

const CORPUS: &str = "audio_expansion";

// -------------------------------------------------------------------------
// Layer 1 — decibel-level oracle (real accuracy assertions)
// -------------------------------------------------------------------------

/// Frame count for the level oracle. The expansion `db_*` ROMs copy
/// themselves to RAM, buzz, wait ~4 s for the (emulated no-op) hotswap, buzz
/// again, then play the reference square followed by the expansion square;
/// 660 frames (~11 s NES time) reaches the sustained expansion segment.
const DB_FRAMES: u64 = 660;

/// Peak amplitude of a full-volume 2A03 square through the non-linear mixer +
/// band-limited synthesis, as it appears in every `db_*` capture. The
/// reference-window alignment guard checks against this so a shifted ROM
/// timeline fails loudly instead of silently measuring noise.
const APU_SQUARE_PEAK: f32 = 0.14438;

/// Deterministic frame windows bounding the sustained **reference-square** and
/// **expansion-square** segments shared by all five expansion `db_*` ROMs
/// (`db_vrc6a/b`, `db_mmc5`, `db_n163`; `db_vrc7`/`db_5b` share the timeline
/// too, they just aren't level-asserted). Empirically located and stable
/// because the core is deterministic.
const REF_WINDOW: (usize, usize) = (400, 470);
const EXP_WINDOW: (usize, usize) = (560, 650);

/// Run an expansion `db_*` ROM and return `(ref_peak, exp_peak, ratio)` — the
/// reference-square peak, the expansion-square peak, and `exp / ref`. Panics
/// (via the alignment guard) if the reference window doesn't hold the APU
/// square, i.e. if the ROM timeline drifted out from under the pinned windows.
fn db_ratio(rom: &str) -> (f32, f32, f32) {
    let peaks = capture_frame_peaks(&format!("audio-tests/{rom}"), DB_FRAMES);
    let ref_peak = window_peak(&peaks, REF_WINDOW.0, REF_WINDOW.1);
    let exp_peak = window_peak(&peaks, EXP_WINDOW.0, EXP_WINDOW.1);
    assert!(
        (ref_peak - APU_SQUARE_PEAK).abs() < 0.01,
        "{rom}: reference window peak {ref_peak:.5} is not the full-volume APU \
         square (~{APU_SQUARE_PEAK}); the ROM timeline drifted — re-locate the \
         REF/EXP windows before trusting the ratio"
    );
    (ref_peak, exp_peak, exp_peak / ref_peak)
}

/// Assert an expansion-chip level ratio against `target` within `tol`
/// (absolute), printing the measured triple for diagnosis.
fn assert_ratio(rom: &str, target: f32, tol: f32) {
    let (ref_peak, exp_peak, ratio) = db_ratio(rom);
    assert!(
        (ratio - target).abs() <= tol,
        "{rom}: expansion/APU level ratio {ratio:.4} (exp={exp_peak:.5} ref={ref_peak:.5}) \
         is outside target {target} ± {tol}"
    );
}

#[test]
fn level_db_apu() {
    // Reference ROM: full-volume APU triangle vs full-volume APU square. The
    // ratio is a fixed 2A03 DAC characteristic (triangle `tnd_table` vs pulse
    // `pulse_table`), also pinned by the `rustynes_apu::mixer` LUT unit tests.
    // db_apu has NO hotswap wait, so it uses its own square/triangle windows.
    let peaks = capture_frame_peaks("audio-tests/db_apu.nes", DB_FRAMES);
    let square_peak = window_peak(&peaks, 120, 200);
    let triangle_peak = window_peak(&peaks, 300, 380);
    assert!(
        (square_peak - APU_SQUARE_PEAK).abs() < 0.01,
        "db_apu: square window peak {square_peak:.5} is not the APU square (~{APU_SQUARE_PEAK})"
    );
    let ratio = triangle_peak / square_peak;
    assert!(
        (ratio - 0.524).abs() <= 0.02,
        "db_apu: triangle/square DAC ratio {ratio:.4} (tri={triangle_peak:.5} \
         sq={square_peak:.5}) outside 0.524 ± 0.02"
    );
}

#[test]
fn level_db_vrc6a() {
    // VRC6a (Akumajou Densetsu pinout): a full-volume VRC6 square is ~1.5× the
    // 2A03 pulse (Mesen2 weights VRC6 `output*15` internally × `*5` mixer =
    // `15*15*5/746.9 ≈ 1.506`). See `VRC6_MIX_SCALE` in `sprint3.rs`.
    assert_ratio("db_vrc6a.nes", 1.506, 0.04);
}

#[test]
fn level_db_vrc6b() {
    // VRC6b (Madara pinout): identical audio path to VRC6a, same target.
    assert_ratio("db_vrc6b.nes", 1.506, 0.04);
}

#[test]
fn level_db_mmc5() {
    // MMC5 pulses reuse the 2A03 pulse DAC: "equivalent in volume to the
    // corresponding APU channels" (Mesen2 `Mmc5Audio.h`), i.e. ~1.0×. See the
    // 650/40 scale in `mmc5.rs::mix_audio`.
    assert_ratio("db_mmc5.nes", 1.000, 0.04);
}

#[test]
fn level_db_n163() {
    // Namco 163, 1-channel mode: ~6.0× the 2A03 pulse — the Mesen2 `*20`
    // weight on the un-attenuated `(sample-8)*volume` channel (no reference
    // emulator attenuates 1-channel N163). See `NAMCO163_MIX_SCALE` (261) in
    // `sprint3.rs`. This is the v2.1.6 fix (was ~1.48×, ~12 dB too quiet).
    assert_ratio("db_n163.nes", 6.02, 0.20);
}

// -------------------------------------------------------------------------
// Layer 2 — byte-exact regression guards (insta snapshots)
// -------------------------------------------------------------------------

/// Run one audio-tests ROM for `frames` frames and return the stable
/// snapshot line (fb + cycles + audio samples + audio hash).
fn capture(rom: &str, frames: u64) -> String {
    let rel = format!("audio-tests/{rom}");
    let (fb, cycles, samples, audio) = run_and_capture_full(CORPUS, &rel, frames);
    snapshot_line_full(&rel, frames, fb, cycles, samples, audio)
}

/// Generate one `insta` snapshot test per ROM. 120 frames (~2 s NES time)
/// captures the boot + first tones and produces a representative audio buffer;
/// combined with the level oracle above, the snapshot is a byte-exact
/// regression sentinel for the whole APU + expansion-mixer path.
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
// The db_apu/db_vrc6a/db_vrc6b/db_mmc5/db_n163 LEVELS are additionally
// asserted by the `level_db_*` oracle above; these snapshots are the
// byte-exact regression layer.
audio_expansion_test!(db_apu, "db_apu.nes");
audio_expansion_test!(db_vrc6a, "db_vrc6a.nes");
audio_expansion_test!(db_vrc6b, "db_vrc6b.nes");
// db_vrc7: OPLL FM is fully implemented (emu2413 port) and the VRC7 patch ROM
// is verified canonical (opll unit test), but the absolute FM level vs the APU
// square is patch-/waveform-dependent and not cleanly oracle-pinned, so this
// stays a snapshot-only regression guard (see docs/accuracy-ledger.md).
audio_expansion_test!(db_vrc7, "db_vrc7.nes");
audio_expansion_test!(db_n163, "db_n163.nes");
// db_5b: the 5B's hardware-relative full-volume level (~3.6× the APU pulse)
// overflows the i16 `mix_audio` contract for the full 3-tone dynamic range;
// deferred (documented) — snapshot-only. The 5B log-volume DAC *step law* is
// verified by a sprint3.rs unit test. See docs/accuracy-ledger.md.
audio_expansion_test!(db_5b, "db_5b.nes");
audio_expansion_test!(db_mmc5, "db_mmc5.nes");

// VRC7 characterization. `patch_vrc7`'s real criterion (the built-in
// instrument set) is asserted by `rustynes_apu::opll` unit tests; `clip_vrc7`
// (amplifier clipping), `test_vrc7` (chip-reset `$0F` register) and
// `noise_vrc7` (internal-filter white noise) are listening/characterization
// ROMs kept as byte-exact regression guards only.
audio_expansion_test!(test_vrc7, "test_vrc7.nes");
audio_expansion_test!(patch_vrc7, "patch_vrc7.nes");
audio_expansion_test!(clip_vrc7, "clip_vrc7.nes");
audio_expansion_test!(noise_vrc7, "noise_vrc7.nes");

// Namco 163 characterization. `test_n163_longwave` exercises the long-period
// wavetable edge case; RustyNES's N163 uses the canonical `256-(reg&0xFC)`
// wave length + 64-bit phase accumulator wrapped at `length<<16` (verified by
// a sprint3.rs unit test), so this is a regression guard.
audio_expansion_test!(test_n163_longwave, "test_n163_longwave.nes");

// Sunsoft 5B / FME-7 characterization. `clip_5b` (amplifier nonlinearity) is a
// deep amplifier-model gap; `noise_5b`/`sweep_5b`/`envelope_5b`/`phase_5b` are
// pure listening/filter-characterization ROMs — all regression-guard only.
audio_expansion_test!(clip_5b, "clip_5b.nes");
audio_expansion_test!(noise_5b, "noise_5b.nes");
audio_expansion_test!(sweep_5b, "sweep_5b.nes");
audio_expansion_test!(envelope_5b, "envelope_5b.nes");
audio_expansion_test!(phase_5b, "phase_5b.nes");

// Base-APU quirks. `dac_square` (square DAC linearity) is pinned by the
// `rustynes_apu::mixer` `pulse_table_*` unit tests; `tri_silence`
// ($4008/$400B linear-counter interaction) is covered by the blargg APU +
// AccuracyCoin suites. Both kept here as byte-exact regression guards.
audio_expansion_test!(tri_silence, "tri_silence.nes");
audio_expansion_test!(dac_square, "dac_square.nes");
