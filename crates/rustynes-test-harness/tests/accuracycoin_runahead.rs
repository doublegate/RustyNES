//! `AccuracyCoin` under the frontend's **run-ahead** snapshot/restore cycle.
//!
//! The plain battery (`accuracycoin.rs`) drives the ROM with a straight
//! `run_frame` loop, which is *not* how the desktop frontend runs it: with
//! `[input] run_ahead = N` (default **1**, `rustynes_frontend::config`), every
//! visible frame is
//!
//! 1. one persistent `run_frame`,
//! 2. `Nes::snapshot_core_into`,
//! 3. `N` further `run_frame`s (hidden + visible),
//! 4. `Nes::restore_quiet` back to (2).
//!
//! That is a full PPU save-state round trip per frame, so any live PPU state
//! missing from the snapshot schema drifts the *persistent* timeline once per
//! frame. It has bitten twice: the Wizards & Warriors half-blank playfield
//! (closed by the `PPU_SNAPSHOT_VERSION` v6 tail) and, until the v8 tail, three
//! `AccuracyCoin` tests — `Sprite Evaluation :: Arbitrary Sprite zero` (error 2),
//! `Sprite Evaluation :: Misaligned OAM behavior` (error 1), and `PPU Behavior ::
//! Rendering Flag Behavior` (error 2) — which turned a headless 141/141 into a
//! desktop 138/141.
//!
//! `crates/rustynes-frontend/src/runahead.rs` carries a framebuffer-equality
//! regression for the same property, but only over two homebrew/commercial ROMs;
//! this pins it against the accuracy battery itself, which is the oracle that
//! actually exercises the sprite-evaluation FSM at dot resolution. The battery's
//! own pass count is the assertion, so a future unserialized-state regression
//! fails here with the offending test *named* rather than as an opaque pixel
//! diff.

#![cfg(feature = "test-roms")]

use rustynes_core::{Buttons, Nes};
use rustynes_test_harness::accuracy_coin;
use rustynes_test_harness::accuracy_coin_catalog as cat;

/// Frames of menu wait before pressing Start — matches `accuracy_coin`'s driver.
const MENU_FRAMES: u32 = 300;
/// Frames to hold Start (the ROM debounces internally).
const START_FRAMES: u32 = 6;
/// Battery budget. It completes in ~4200 frames; this is ~1.7x headroom and
/// bounds the test's wall time (run-ahead doubles the emulated frames).
const BATTERY_FRAMES: u32 = 7_000;

/// Drive the whole battery through a run-ahead cycle of depth `n` and return
/// the RAM-decoded per-test statuses.
///
/// Mirrors `rustynes_frontend::runahead::RunAhead::run_frame_ahead` +
/// `finish` exactly (persistent frame, snapshot, `n` more frames, restore,
/// rewind-capture suppression in between). Reimplemented rather than imported
/// because `rustynes-frontend` pulls in wgpu/winit/cpal, which this crate must
/// not depend on.
fn battery_with_run_ahead(n: u32) -> Vec<cat::TestStatus> {
    let bytes = std::fs::read(accuracy_coin::rom_path()).expect("read AccuracyCoin.nes");
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin.nes (NROM)");
    // The frontend arms rewind by default; run-ahead suppresses capture for the
    // off-timeline frames. Arm it here so that suppression path is exercised.
    nes.enable_rewind();

    let mut snap: Vec<u8> = Vec::new();
    // One NTSC frame at 192 kHz is ~3200 samples; 8192 is generous headroom.
    let mut audio_discard = vec![0.0f32; 8192];

    let mut step = |nes: &mut Nes| {
        if n == 0 {
            nes.run_frame();
            let _ = nes.drain_audio_into(&mut audio_discard);
            return;
        }
        // (1) The persistent frame — the real timeline.
        nes.run_frame();
        let _ = nes.drain_audio_into(&mut audio_discard);
        // (2) Checkpoint it.
        nes.snapshot_core_into(&mut snap);
        // (3) Hidden + visible frames are off-timeline.
        nes.set_rewind_capture(false);
        for _ in 0..n {
            nes.run_frame();
            let _ = nes.drain_audio_into(&mut audio_discard);
        }
        // (4) Roll back. Any state the hidden frames touched that the snapshot
        //     does not carry survives this — that is the bug class under test.
        nes.restore_quiet(&snap)
            .expect("run-ahead snapshot round-trips");
        nes.set_rewind_capture(true);
    };

    for _ in 0..MENU_FRAMES {
        step(&mut nes);
    }
    nes.set_buttons(0, Buttons::START);
    for _ in 0..START_FRAMES {
        step(&mut nes);
    }
    nes.set_buttons(0, Buttons::empty());
    for _ in 0..BATTERY_FRAMES {
        step(&mut nes);
    }

    cat::decode_results(nes.bus().ram_bytes()).expect("decode result bytes from CPU RAM")
}

/// The contract: run-ahead must not cost the battery a single test.
///
/// Asserted against the RAM-direct decoder (the authoritative measurement —
/// see `accuracycoin.rs`), at depth 1 (the shipped default) and depth 2.
#[test]
fn accuracycoin_is_unaffected_by_run_ahead() {
    let baseline = cat::summarise(&battery_with_run_ahead(0));
    assert_eq!(
        baseline.fail + baseline.unknown,
        0,
        "plain-run baseline regressed before run-ahead is even in play: {:?}",
        cat::failing_tests(&battery_with_run_ahead(0))
    );
    let expected = baseline.pass + baseline.pass_with_code;

    for depth in [1u32, 2] {
        let statuses = battery_with_run_ahead(depth);
        let s = cat::summarise(&statuses);
        let got = s.pass + s.pass_with_code;
        assert_eq!(
            got,
            expected,
            "run-ahead depth {depth} cost {} test(s) ({got}/{} vs {expected}/{} plain) — \
             live PPU/CPU state is missing from the save-state schema. Failing: {:?}",
            expected - got,
            s.assigned(),
            baseline.assigned(),
            cat::failing_tests(&statuses),
        );
    }
}
