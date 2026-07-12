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
//! ## Current state (v2.1.5 "Regression Net & Residual") — 10 / 10 PASS
//!
//! **All ten** checks PASS under forced PAL:
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
//! - Length halt/reload write ordering (two, **newly passing in v2.1.5**):
//!   `10.len_halt_timing` and `11.len_reload_timing`. These closed once the
//!   length counter (`crates/rustynes-apu/src/length.rs`) gained the deferred
//!   **halt-after-clock** (`new_halt`) and **reload-ignored-during-clock**
//!   (`reload_val` / `previous_count`) mechanism — the owning APU promotes both
//!   once per CPU cycle in `Apu::tick_with_external`, *after* the half-frame
//!   length clock and *before* the mixer sample, mirroring `TetaNES`'s
//!   `LengthCounter::reload` and Mesen2's `_newHaltValue` + reload-request. The
//!   ordering change is invisible on the common non-coincident write cycle (the
//!   reload settles in-cycle), so it is byte-identical on NTSC — `AccuracyCoin`
//!   holds 141/141 and `blargg_apu_2005` stays 11/11. See `docs/apu-2a03.md`
//!   §Length halt/reload ordering + `docs/accuracy-ledger.md`. (The suite has
//!   no `09.reset_timing` ROM — that variant lives only in the NTSC
//!   `blargg_apu_2005` set.)

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

/// Asserts a `pal_apu_tests` sub-ROM reports on-screen `PASSED` under forced
/// PAL. Covers all three categories the suite exercises — region-independent
/// checks (length-counter operation, length lookup table, frame-IRQ flag
/// semantics), the PAL frame-counter step-timing checks (clock jitter, mode-0/1
/// length timing, frame-IRQ flag/IRQ timing), and the length halt/reload
/// write-ordering checks — so the failure message stays neutral and makes no
/// region-independence claim that would be false for the timing-sensitive ones.
macro_rules! pal_apu_pass {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (verdict, text) = run($rom);
            assert_eq!(
                verdict,
                ScreenVerdict::Passed,
                "PAL APU {}: expected on-screen PASSED but ROM reported {verdict:?}\n{text}",
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

// PAL length halt/reload timing — PASS since v2.1.5 (deferred halt/reload
// ordering, see the module docs + docs/apu-2a03.md + docs/accuracy-ledger.md).
pal_apu_pass!(pal_apu_10_len_halt_timing, "10.len_halt_timing.nes");
pal_apu_pass!(pal_apu_11_len_reload_timing, "11.len_reload_timing.nes");
