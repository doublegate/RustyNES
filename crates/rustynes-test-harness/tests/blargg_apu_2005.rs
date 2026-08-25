//! blargg `blargg_apu_2005.07.30/*.nes` corpus (11 sub-ROMs) — NTSC region oracle.
//!
//! The full 2005-era APU regression suite: length-counter behaviour + table,
//! the frame-IRQ flag, clock jitter, length timing in both frame-counter
//! modes, IRQ-flag and IRQ timing, reset timing, and length halt/reload
//! timing. All NROM (mapper 0), driven through the full lockstep `Nes`.
//!
//! Per `docs/testing-strategy.md` §Layer 3.
//!
//! ## Why this suite reads the screen, not `$6000`
//!
//! These are the **2005-era** blargg APU ROMs, which predate the standardized
//! `$6000` WRAM status protocol. Each is plain NROM with **no PRG-RAM**, so
//! `$6000` is unmapped and reads back `0` forever — and `0` is blargg's
//! *success* code, so a `$6000` runner reports a **vacuous pass for every one
//! of them regardless of the real outcome**.
//!
//! Every revision of this file up to v2.6.2 did exactly that: it called
//! `run_nes_blargg` and asserted `status == 0`, so "all eleven PASS" was a
//! statement about an unmapped address rather than about the APU. The
//! identical defect was found and fixed for the PAL counterpart
//! (`pal_apu_tests.rs`, v2.1.5), whose header has called it "a **false
//! oracle** that validated nothing" ever since — but the NTSC half of the same
//! corpus was never migrated, so the fix covered one of the two files for five
//! minor releases.
//!
//! [`vacuity_of_the_6000_protocol_on_this_corpus`] pins the reason rather than
//! asserting it in prose: it fails if `$6000` ever stops reading back `0`, or
//! if blargg's `$DE $B0 $61` completion magic ever appears. If a future change
//! gives these ROMs real PRG-RAM, that test fails and this comment gets
//! revisited instead of quietly becoming wrong.
//!
//! ## Why it does not read `PASSED` / `FAILED` either
//!
//! The obvious repair — reuse `run_nes_screen`, the decoder built for the PAL
//! counterpart — is also wrong here, and wrong in a way that looks like a
//! failing emulator. These ROMs never print those words. The corpus's own
//! `tests.txt` states the convention: *"Each ROM runs several tests and reports
//! a result code on screen … A result code of 1 always indicates that all tests
//! were passed."* Run through `run_nes_screen` all eleven return
//! `ScreenVerdict::Unresolved` after burning the whole frame budget, while the
//! screen plainly reads `$01`.
//!
//! So the two halves of the same 2005-era corpus report **differently**: the
//! `pal_apu_tests` rebuild prints `PASSED` / `FAILED: #<n>`, and
//! `blargg_apu_2005.07.30` prints a numeric code. [`run_nes_result_code`]
//! decodes the latter, and `tests.txt` names what every non-1 code means.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::{CodeVerdict, run_nes_blargg, run_nes_result_code};

/// Frame budget. Matches `pal_apu_tests`; the slowest of these ROMs settles
/// well inside it, and an exhausted budget yields [`CodeVerdict::Unresolved`],
/// which every assertion treats as a hard failure rather than a pass.
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

const ROMS: [&str; 11] = [
    "01.len_ctr.nes",
    "02.len_table.nes",
    "03.irq_flag.nes",
    "04.clock_jitter.nes",
    "05.len_timing_mode0.nes",
    "06.len_timing_mode1.nes",
    "07.irq_flag_timing.nes",
    "08.irq_timing.nes",
    "09.reset_timing.nes",
    "10.len_halt_timing.nes",
    "11.len_reload_timing.nes",
];

fn read_rom(name: &str) -> Vec<u8> {
    let path = rom_path(&format!("nes-test-roms/blargg_apu_2005.07.30/{name}"));
    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

fn run(name: &str) -> (CodeVerdict, String, u64) {
    let bytes = read_rom(name);
    let r = run_nes_result_code(&bytes, MAX_FRAMES, false).expect("rom must parse + run");
    eprintln!(
        "NTSC {name}: verdict={:?} frames={} screen={:?}",
        r.verdict, r.frames, r.text
    );
    (r.verdict, r.text, r.frames)
}

/// Asserts a `blargg_apu_2005.07.30` sub-ROM settles on on-screen result code
/// `$01` under NTSC timing. A non-1 code names a specific defect in that ROM's
/// `tests.txt` section; `Unresolved` means the ROM never settled and is treated
/// as a hard failure, never a pass.
macro_rules! blargg_apu_2005_pass {
    ($name:ident, $rom:literal) => {
        #[test]
        fn $name() {
            let (verdict, text, frames) = run($rom);
            assert_eq!(
                verdict,
                CodeVerdict::Passed,
                "blargg APU {}: expected on-screen result code $01 but ROM reported \
                 {verdict:?} after {frames} frames\n{text}",
                $rom
            );
        }
    };
}

blargg_apu_2005_pass!(blargg_apu_2005_01_len_ctr, "01.len_ctr.nes");
blargg_apu_2005_pass!(blargg_apu_2005_02_len_table, "02.len_table.nes");
blargg_apu_2005_pass!(blargg_apu_2005_03_irq_flag, "03.irq_flag.nes");
blargg_apu_2005_pass!(blargg_apu_2005_04_clock_jitter, "04.clock_jitter.nes");
blargg_apu_2005_pass!(
    blargg_apu_2005_05_len_timing_mode0,
    "05.len_timing_mode0.nes"
);
blargg_apu_2005_pass!(
    blargg_apu_2005_06_len_timing_mode1,
    "06.len_timing_mode1.nes"
);
blargg_apu_2005_pass!(blargg_apu_2005_07_irq_flag_timing, "07.irq_flag_timing.nes");
blargg_apu_2005_pass!(blargg_apu_2005_08_irq_timing, "08.irq_timing.nes");
blargg_apu_2005_pass!(blargg_apu_2005_09_reset_timing, "09.reset_timing.nes");
blargg_apu_2005_pass!(blargg_apu_2005_10_len_halt_timing, "10.len_halt_timing.nes");
blargg_apu_2005_pass!(
    blargg_apu_2005_11_len_reload_timing,
    "11.len_reload_timing.nes"
);

/// The `$6000` protocol reports a **vacuous** pass on every ROM in this corpus.
///
/// This is the reason the suite above reads the screen, pinned as an
/// executable fact rather than left as a comment. For all eleven ROMs the
/// `$6000` status reads back `0` — blargg's *success* code — while the
/// completion magic `$DE $B0 $61` never appears, which is the signature of an
/// **unmapped** address rather than a passing test.
///
/// If a future change maps PRG-RAM into these ROMs, this test fails and the
/// suite's runner choice gets revisited, instead of the module comment quietly
/// becoming wrong.
#[test]
fn vacuity_of_the_6000_protocol_on_this_corpus() {
    // A tenth of `MAX_FRAMES`. Every ROM in this corpus settles its result code
    // by frame 26, so 180 frames is nearly seven times the longest settling
    // time — if `$6000` were going to become meaningful it would have long
    // since. The full budget here costs ~100 s of CI to demonstrate the same
    // thing eleven times over.
    const VACUITY_FRAMES: u64 = MAX_FRAMES / 10;
    for name in ROMS {
        let bytes = read_rom(name);
        let r = run_nes_blargg(&bytes, VACUITY_FRAMES).expect("rom must parse + run");
        assert_eq!(
            r.status, 0,
            "{name}: $6000 returned {:#04x}; if PRG-RAM is now mapped here the \
             screen-vs-$6000 choice in this file needs revisiting",
            r.status
        );
        assert!(
            r.message.is_empty(),
            "{name}: $6000 message is {:?}, so the completion magic appeared and \
             the protocol is no longer vacuous on this corpus",
            r.message
        );
    }
}
