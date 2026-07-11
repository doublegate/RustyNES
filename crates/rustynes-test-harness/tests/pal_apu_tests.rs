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
//! ## Honest current state (v2.1.5 "Regression Net & Residual")
//!
//! Three of the ten checks are region-independent and **PASS** under forced
//! PAL: the length-counter operation, the length lookup table (a ROM
//! constant), and the frame-IRQ flag set/clear *semantics*.
//!
//! The other seven all hinge on the exact **PAL frame-counter step positions**
//! and currently **FAIL**: the `RustyNES` APU frame counter
//! (`crates/rustynes-apu/src/frame_counter.rs`) is region-agnostic — it clocks
//! the sequencer at the NTSC step positions (7457 / 14913 / 22371 /
//! 29828-29830, and 37281-37282) *unconditionally*, with no PAL variant
//! (8313 / 16627 / 24939 / 33252-33253, …). The PAL-calibrated ROMs bake in
//! the PAL step timing, so their jitter / length-timing / IRQ-timing sub-tests
//! never match — and, tellingly, they fail identically whether the ROM is run
//! under PAL **or** NTSC region, which pins the cause to the timing *model*,
//! not region selection. This is a genuine, documented PAL-accuracy residual
//! (`docs/accuracy-ledger.md`), **not** an NTSC defect: the NTSC APU frame
//! counter is oracle-verified exact by `AccuracyCoin` (APU Frame-Counter-IRQ
//! tests all pass, 141/141) and the newer `apu_test` `$6000` suite (8/8).
//!
//! The seven residuals are pinned as fail-loud regression guards: each asserts
//! the ROM *currently* reports `FAILED` on-screen. If PAL frame-counter step
//! positions are ever modeled, the corresponding ROM flips to `PASSED`, the
//! pin trips, and this file must be promoted — the honest, non-forcing
//! equivalent of the `mmc3_test_2/4` `_currently_fails` convention. (The suite
//! has no `09.reset_timing` ROM — that variant lives only in the NTSC
//! `blargg_apu_2005` set.)

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

/// A region-independent check that PASSES under forced PAL: length-counter
/// operation, the length lookup table, and the frame-IRQ flag semantics.
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

/// A PAL frame-counter-timing-sensitive check that currently FAILS because the
/// PAL step positions are not modeled. Pinned as a fail-loud residual: the
/// assertion trips (forcing this file to be updated) the moment the ROM starts
/// reporting `PASSED` — i.e. when PAL frame-counter timing is implemented — or
/// if it ever hangs (`Unresolved`).
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

// PAL frame-counter-timing-sensitive — documented residuals (currently FAIL).
pal_apu_residual!(pal_apu_04_clock_jitter, "04.clock_jitter.nes");
pal_apu_residual!(pal_apu_05_len_timing_mode0, "05.len_timing_mode0.nes");
pal_apu_residual!(pal_apu_06_len_timing_mode1, "06.len_timing_mode1.nes");
pal_apu_residual!(pal_apu_07_irq_flag_timing, "07.irq_flag_timing.nes");
pal_apu_residual!(pal_apu_08_irq_timing, "08.irq_timing.nes");
pal_apu_residual!(pal_apu_10_len_halt_timing, "10.len_halt_timing.nes");
pal_apu_residual!(pal_apu_11_len_reload_timing, "11.len_reload_timing.nes");
