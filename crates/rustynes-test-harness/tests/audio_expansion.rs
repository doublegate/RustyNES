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
//! | `db_5b`    | 5B vol-12 square / APU square      | ~ 1.265      |
//!
//! `db_5b` gained its assertion in v2.2.3 (A1). It had been a documented gap
//! for one reason only: `Mapper::mix_audio` returned `i16`, and the corrected
//! level puts a full-scale 5B tone at `1882 * 18.471 = 34,761` — past
//! `i16::MAX` for a single channel. Widening the return type to `i32` is what
//! unblocked it; the level was then measured (`0.0685x`, ~23 dB too quiet) and
//! corrected to the Mesen2-derived `63 * 15 / 746.9 = 1.265`.
//!
//! `db_vrc7` still has NO `level_db_*` assertion — the OPLL level metric is
//! patch-/waveform-dependent and not cleanly oracle-pinned, so it stays
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
//! **That claim was FALSE until v2.2.3** — see [`SNAPSHOT_FRAMES`]. The capture
//! ran 120 frames while these ROMs do not switch the expansion chip in until
//! ~frame 560, so the hashes covered boot and the 2A03 reference tone and never
//! observed the chip under test. It was caught by accident: the A1 change moved
//! the Sunsoft 5B level by 18.5x and every 5B snapshot stayed byte-identical.
//! The window now spans the expansion segment, so the sentinel guards what its
//! name says. Verified by perturbation, not assumed: a **one-unit** change to
//! `SUNSOFT5B_MIX_SCALE_NUM` — a 0.04% level change — now fails **all six** 5B
//! snapshots (`db_5b`, `clip_5b`, `envelope_5b`, `phase_5b`, `noise_5b`,
//! `sweep_5b`), where an 18.5x change previously failed none.
//!
//! Getting the last two took per-ROM windows rather than a bigger shared one:
//! `noise_5b` and `sweep_5b` start much later than the rest (frames ~900 and
//! ~4740 — see `NOISE_5B_FRAMES` / `SWEEP_5B_FRAMES`), which is why a single
//! 660-frame window still missed them. Neither is broken and neither awaits
//! input; that was measured off the 5B register file, not guessed.
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
fn level_db_5b() {
    // Sunsoft 5B, the ROM's volume-12 square: ~1.265× the 2A03 pulse. Derived
    // from Mesen2 rather than from our own prior numbers — in
    // `NesSoundMixer::GetOutputVolume` a full-volume APU square is
    // `(95.88 * 5000) / (8128/15 + 100) = 746.9` units and the 5B is summed at
    // weight `* 15` over `_volumeLut = (uint8_t)1.1885^(2i)` (`LUT[12] = 63`),
    // giving `63 * 15 / 746.9 = 1.265`. Full scale (`LUT[15] = 177`) is
    // `3.554×`, which is what made this uncalibratable while `Mapper::mix_audio`
    // returned `i16`. See `SUNSOFT5B_MIX_SCALE_NUM` in `sprint3.rs`.
    //
    // This is the v2.2.3 A1 fix: the level was a documented gap (measured
    // `0.0685×`, ~23 dB too quiet) purely because the return type could not
    // hold the corrected value.
    assert_ratio("db_5b.nes", 1.265, 0.04);
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

/// Frames captured per snapshot.
///
/// **Was 120 through v2.2.2, which made this whole layer blind to expansion
/// audio.** These ROMs hold an APU reference tone first and only switch the
/// expansion chip in around frame 560 (see [`EXP_WINDOW`]) — more than 4x past
/// a 120-frame capture. The snapshots therefore hashed boot + the 2A03
/// reference section and never once observed the chip under test.
///
/// That was not a theoretical gap. The v2.2.3 A1 change altered the Sunsoft 5B
/// output level by **18.5x**, and all six 5B snapshots stayed byte-identical.
/// A sentinel that cannot see an 18x change in the thing it guards is not a
/// sentinel. [`DB_FRAMES`] covers the expansion window, so the capture uses it.
const SNAPSHOT_FRAMES: u64 = DB_FRAMES;

/// Generate one `insta` snapshot test per ROM. [`SNAPSHOT_FRAMES`] (~11 s NES
/// time) covers boot, the APU reference tone AND the expansion-chip section;
/// combined with the level oracle above, the snapshot is a byte-exact
/// regression sentinel for the whole APU + expansion-mixer path.
macro_rules! audio_expansion_test {
    // Default window ([`SNAPSHOT_FRAMES`]).
    ($name:ident, $rom:literal) => {
        audio_expansion_test!($name, $rom, SNAPSHOT_FRAMES);
    };
    // Explicit window, for ROMs whose chip section starts later (see
    // `LATE-STARTING ROMS` below).
    ($name:ident, $rom:literal, $frames:expr) => {
        #[test]
        fn $name() {
            let snap = capture($rom, $frames);
            insta::assert_snapshot!(concat!("audio_expansion_", stringify!($name)), snap);
        }
    };
}

// LATE-STARTING ROMS — measured, not guessed.
//
// Instrumenting the 5B register file (`Nes::mapper_info()`, `5b_*` rows added
// to the FME-7 debug window in v2.2.3) over a long run gives each ROM's first
// non-zero 5B output:
//
//   db_5b        frame ~540   (matches the pinned EXP_WINDOW)
//   envelope_5b  frame ~420
//   noise_5b     frame ~900   -> needs more than SNAPSHOT_FRAMES
//   sweep_5b     frame ~4740  -> needs far more
//
// Neither late ROM is broken and neither needs input: `noise_5b` enables noise
// on channel A (mixer `$37`, volume 12) about 15 s in, and `sweep_5b` runs a
// slow volume sweep from about 79 s in, holding mixer `$3F` — the wiki's
// "both bits 1 => constant output at volume" case — while modulating the
// volume registers directly. They simply take longer than the shared window,
// so they get their own.
const NOISE_5B_FRAMES: u64 = 1_200;
const SWEEP_5B_FRAMES: u64 = 5_400;

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
audio_expansion_test!(noise_5b, "noise_5b.nes", NOISE_5B_FRAMES);
audio_expansion_test!(sweep_5b, "sweep_5b.nes", SWEEP_5B_FRAMES);
audio_expansion_test!(envelope_5b, "envelope_5b.nes");
audio_expansion_test!(phase_5b, "phase_5b.nes");

// Base-APU quirks. `dac_square` (square DAC linearity) is pinned by the
// `rustynes_apu::mixer` `pulse_table_*` unit tests; `tri_silence`
// ($4008/$400B linear-counter interaction) is covered by the blargg APU +
// AccuracyCoin suites. Both kept here as byte-exact regression guards.
audio_expansion_test!(tri_silence, "tri_silence.nes");
audio_expansion_test!(dac_square, "dac_square.nes");
