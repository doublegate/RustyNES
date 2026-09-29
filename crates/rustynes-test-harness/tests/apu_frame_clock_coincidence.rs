//! `tests/roms/extra/apu/apu_test_{1..10}.nes` — ten one-cycle probes of when
//! a `$4017` write's clock lands against the frame sequencer's own step.
//!
//! These ROMs sat in `tests/roms/extra/apu/` referenced by nothing until
//! v2.9.5. The v2.6.2 audit found that four of them report `TEST FAILED`
//! (`docs/accuracy-ledger.md`); this file is where that stops being a note.
//!
//! ## What each ROM tests (read from the ROMs, not assumed)
//!
//! Every ROM is the same ~200-byte program with a different delay. It loads
//! pulse 1's length counter with 10, spends some of it with a run of `$80`
//! writes to `$4017` (each one clocks the length counter immediately), then
//! writes the mode under test and waits almost exactly one sequencer period.
//! It then writes `$4017` a final time and reads `$4015` bit 0 to see whether
//! the length counter reached zero. The only thing that varies between ROMs
//! is the delay, and whether the pass branch is taken on zero or non-zero.
//!
//! Measured on this core, from the CPU cycle of the mode-setting `$4017` write
//! to the CPU cycle of the final write (`trace_frame_counter_irq`):
//!
//! | ROM | mode | final write | delta (cycles) | pass requires |
//! |---|---|---|---|---|
//! | 1 | 4-step | `$80` | 29830 | length != 0 |
//! | 2 | 4-step | `$80` | 29831 | length != 0 |
//! | 3 | 4-step | `$80` | 29832 | length == 0 |
//! | 4 | 4-step | `$80` | 29833 | length == 0 |
//! | 5 | 5-step | `$80` | 37282 | length != 0 |
//! | 6 | 5-step | `$80` | 37283 | length != 0 |
//! | 7 | 5-step | `$80` | 37284 | length == 0 |
//! | 8 | 5-step | `$80` | 37285 | length == 0 |
//! | 9 | 4-step | `$00` | 29830 | length == 0 |
//! | 10 | 4-step | `$00` | 29831 | length == 0 |
//!
//! Read together they state one rule. ROMs 9 and 10 show that the sequencer's
//! last half-frame step DOES happen before a reset landing at those deltas. So
//! in ROMs 1, 2, 5 and 6 both the step and the write's immediate clock
//! happen — and the length counter still ends non-zero, which leaves room for
//! only ONE decrement between them. One cycle later (ROMs 3, 4, 7, 8) there are
//! two. The write's clock is not a second pulse when it matures in the same
//! APU cycle as the step's: the quarter/half-frame triggers are emitted on APU
//! cycle boundaries (nesdev wiki, *APU Frame Counter* and its Talk page).
//!
//! The pass/fail branch of every ROM was decoded from its own code
//! (`lda $4015 / and #$01 / beq|bne`) before this file trusted the words on
//! screen, so a ROM whose expectation is inverted cannot pass here by accident.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_test_harness::{ScreenVerdict, run_nes_screen};

/// Every ROM in this set prints its verdict by frame 6. The budget is ten
/// times that, and an exhausted budget is `Unresolved`, which fails.
const MAX_FRAMES: u64 = 60;

fn run(n: u32) -> (ScreenVerdict, String) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(format!("tests/roms/extra/apu/apu_test_{n}.nes"));
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let r = run_nes_screen(&bytes, MAX_FRAMES, false).expect("rom must parse + run");
    (r.verdict, r.text)
}

macro_rules! apu_clock_probe {
    ($name:ident, $n:literal) => {
        #[test]
        fn $name() {
            let (verdict, text) = run($n);
            assert_eq!(
                verdict,
                ScreenVerdict::Passed,
                "apu_test_{}: expected TEST PASSED, screen reads\n{text}",
                $n
            );
        }
    };
}

apu_clock_probe!(apu_test_1_four_step_write_merges_at_29830, 1);
apu_clock_probe!(apu_test_2_four_step_write_merges_at_29831, 2);
apu_clock_probe!(apu_test_3_four_step_write_separate_at_29832, 3);
apu_clock_probe!(apu_test_4_four_step_write_separate_at_29833, 4);
apu_clock_probe!(apu_test_5_five_step_write_merges_at_37282, 5);
apu_clock_probe!(apu_test_6_five_step_write_merges_at_37283, 6);
apu_clock_probe!(apu_test_7_five_step_write_separate_at_37284, 7);
apu_clock_probe!(apu_test_8_five_step_write_separate_at_37285, 8);
apu_clock_probe!(apu_test_9_four_step_reset_after_step_at_29830, 9);
apu_clock_probe!(apu_test_10_four_step_reset_after_step_at_29831, 10);
