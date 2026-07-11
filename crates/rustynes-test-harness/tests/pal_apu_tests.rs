//! blargg `pal_apu_tests/*.nes` corpus (10 sub-ROMs) — PAL region oracle.
//!
//! The PAL counterpart of `blargg_apu_2005`: the same APU length-counter /
//! frame-IRQ / timing checks, rebuilt by blargg with the expected values
//! calibrated for **PAL** frame-counter timing. These ROMs ship as plain iNES
//! 1.0 with no NES-2.0 region byte, so [`run_nes_screen`] with `force_pal =
//! true` stamps a throwaway header copy (NES-2.0 marker + PAL region nibble)
//! so the core selects the PAL dividers (3.2:1 CPU:PPU, 50 Hz, PAL DMC/noise
//! rate tables).
//!
//! ## Why this suite reads the screen, not `$6000`
//!
//! These are the **2005-era** blargg APU ROMs, which predate the standardized
//! `$6000` WRAM status protocol. Each is plain NROM with **no PRG-RAM**, so
//! `$6000` is unmapped and reads back `0` forever — the `run_nes_blargg`
//! `$6000` runner reports a *vacuous* pass for every one of them regardless of
//! the real outcome (verified: `$6000-$7FFF` is all-zero for the whole run,
//! and the blargg `$DE $B0 $61` completion magic never appears). The prior
//! revision of this file asserted `status == 0` and so claimed "all ten PASS"
//! — a **false oracle** that validated nothing. The ROMs actually report their
//! verdict on-screen (`APU <title>` then `PASSED` / `FAILED: #<n>`), which
//! [`run_nes_screen`] decodes from the nametable. See
//! `docs/testing-strategy.md` §Layer 3 and `docs/accuracy-ledger.md`.
//!
//! ## Honest current state (v2.1.5 "Regression Net & Residual") — 8 / 10 PASS
//!
//! **Eight of the ten** checks PASS under forced PAL:
//!
//! - Region-independent (three): the length-counter operation, the length
//!   lookup table (a ROM constant), and the frame-IRQ flag set/clear
//!   *semantics*.
//! - PAL frame-counter-timing-sensitive (five, **newly passing in v2.1.5**):
//!   clock jitter, length timing in both frame-counter modes, and the two
//!   frame-IRQ timing checks. These flipped from FAIL to PASS when the APU
//!   frame counter (`crates/rustynes-apu/src/frame_counter.rs`) gained
//!   **region-gated PAL step positions** — the 2A07 sequencer clocks at
//!   8313 / 16627 / 24939 / 33252-33254 (4-step) and 8313 / 16627 / 24939 /
//!   41565-41566 (5-step), selected by `FrameCounter::pal` from the console
//!   region. Only true `Region::Pal` uses them; NTSC and Dendy keep the NTSC
//!   positions (7457 / 14913 / 22371 / 29828-29830; 37281-37282), so the
//!   NTSC/Dendy path is byte-identical — `AccuracyCoin` holds 141/141 and the
//!   NTSC `blargg_apu_2005` + `apu_test` suites are unchanged.
//!
//! **Two remain as documented residuals** — `10.len_halt_timing` and
//! `11.len_reload_timing`. With the PAL step positions in place they advanced
//! from `FAILED: #2` to `FAILED: #3` / `FAILED: #4` respectively, but do not
//! fully pass. The NTSC builds of these same two ROMs (`blargg_apu_2005` 10 &
//! 11) PASS, which localizes the residual to a **PAL-specific
//! length-counter halt/reload write-vs-half-frame-clock ordering** detail that
//! sits *adjacent* to — not inside — the frame-counter step-position model this
//! change delivered. It is recorded as a bounded PAL-accuracy residual in
//! `docs/accuracy-ledger.md`; closing it is a separate, deeper length-counter
//! timing investigation. (The suite has no `09.reset_timing` ROM — that variant
//! lives only in the NTSC `blargg_apu_2005` set.)
//!
//! The two residuals are pinned as fail-loud regression guards: each asserts
//! the ROM *currently* reports `FAILED` on-screen. If either later PASSES, the
//! pin trips and this file must be promoted — the honest, non-forcing
//! equivalent of the `mmc3_test_2/4` `_currently_fails` convention.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::{ScreenVerdict, run_nes_screen};

/// Frame budget. The on-screen runner early-returns the instant the verdict
/// text appears (blargg writes the `PASSED`/`FAILED` line only once, at the
/// end), so this is only a safety ceiling; every ROM here settles well under
/// it. A ROM that never settles yields [`ScreenVerdict::Unresolved`], which
/// every assertion below treats as a hard failure — never a silent pass.
const MAX_FRAMES: u64 = 1800;

fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

fn run(name: &str) -> (ScreenVerdict, String) {
    let path = rom_path(&format!("nes-test-roms/pal_apu_tests/{name}"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let r = run_nes_screen(&bytes, MAX_FRAMES, true).expect("rom must parse + run");
    eprintln!(
        "PAL {name}: verdict={:?} frames={} screen={:?}",
        r.verdict, r.frames, r.text
    );
    (r.verdict, r.text)
}

/// A check that PASSES under forced PAL — either region-independent
/// (length-counter operation, length lookup table, frame-IRQ flag semantics)
/// or a PAL frame-counter-timing check now covered by the v2.1.5 PAL step
/// positions (clock jitter, mode-0/1 length timing, frame-IRQ flag/IRQ timing).
macro_rules! pal_apu_pass {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (verdict, text) = run($rom);
            assert_eq!(
                verdict,
                ScreenVerdict::Passed,
                "PAL {} must PASS on-screen (region-independent APU check) — got {verdict:?}\n{text}",
                $rom
            );
        }
    };
}

/// A PAL length-counter halt/reload-timing check that currently FAILS: the
/// v2.1.5 PAL frame-counter step positions advanced these ROMs but did not
/// fully close them (a deeper PAL write-vs-half-frame-clock ordering residual,
/// see the module docs + `docs/accuracy-ledger.md`). Pinned as a fail-loud
/// residual: the assertion trips (forcing this file to be updated) the moment
/// the ROM starts reporting `PASSED` — i.e. when the residual is closed — or if
/// it ever hangs (`Unresolved`).
macro_rules! pal_apu_residual {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (verdict, text) = run($rom);
            assert!(
                matches!(verdict, ScreenVerdict::Failed(_)),
                "PAL {} residual: expected an on-screen FAILED (PAL frame-counter \
                 step positions are unmodeled — see docs/accuracy-ledger.md). Got \
                 {verdict:?}. If this ROM now PASSES, PAL frame-counter timing has \
                 been implemented — promote it to `pal_apu_pass!` and update the \
                 ledger + docs/apu-2a03.md.\n{text}",
                $rom
            );
        }
    };
}

// Region-independent — PASS under forced PAL.
pal_apu_pass!(pal_apu_01_len_ctr, "01.len_ctr.nes");
pal_apu_pass!(pal_apu_02_len_table, "02.len_table.nes");
pal_apu_pass!(pal_apu_03_irq_flag, "03.irq_flag.nes");

// PAL frame-counter-timing-sensitive — PASS since v2.1.5 (PAL step positions).
pal_apu_pass!(pal_apu_04_clock_jitter, "04.clock_jitter.nes");
pal_apu_pass!(pal_apu_05_len_timing_mode0, "05.len_timing_mode0.nes");
pal_apu_pass!(pal_apu_06_len_timing_mode1, "06.len_timing_mode1.nes");
pal_apu_pass!(pal_apu_07_irq_flag_timing, "07.irq_flag_timing.nes");
pal_apu_pass!(pal_apu_08_irq_timing, "08.irq_timing.nes");

// PAL length halt/reload timing — documented residuals (currently FAIL at a
// later sub-test than before v2.1.5; see module docs + docs/accuracy-ledger.md).
pal_apu_residual!(pal_apu_10_len_halt_timing, "10.len_halt_timing.nes");
pal_apu_residual!(pal_apu_11_len_reload_timing, "11.len_reload_timing.nes");
