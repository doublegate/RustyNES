//! v2.0.0 "Timebase" beta.1 — the R5 DMC-DMA span regression pin.
//!
//! `AccuracyCoin`'s DMA cluster gates on `CheckDMATiming; CPY #4`: a normal
//! (reload) DMC DMA must span **4** CPU cycles (halt + dummy + alignment +
//! get — the reload arm is invisible to the cycle in which it arms, per the
//! `TriCNES` `_EmulateAPU`-after-`_6502()` ordering, Mesen2's
//! `_transferStartDelay`, and the nesdev DMA page). The test ROM stores the
//! measured span in zero-page `$50` (`STY <$50`) and the DMC+OAM battery
//! result byte at `$0477`; the sweep of DMC-during-OAM landing offsets lives
//! in `$50-$5F` and must match the hardware KEY.
//!
//! History (docs/audit + the v2.0.0 plan): the engine-lineage experiment
//! configs measured a structural Y = 3 ("the drift") and 18 months of levers
//! could not move it without regressing an orthogonal surface — until the
//! promoted defaults (the unified interleaved-DMA engine, the
//! `pending_dmc_dma_next` reload-visibility latch, and the END-of-cycle DMC
//! byte-timer) closed it. The 2026-07-01 ground-truth measurement on the
//! v1.10.0 default build confirms **Y = 4 with the sweep exactly on KEY** —
//! i.e. residual R5 of the v2.0.0 plan is ALREADY CLOSED on the shipping
//! core.
//!
//! This test PINS that closure. The v2.0.0 one-clock / every-cycle rewrite
//! must hold it at every beta gate: if a timebase change regresses the
//! reload span to 3 (or knocks the DMC-during-OAM sweep off KEY), this fails
//! loud with the measured values. Do NOT weaken this pin — a "fix" that
//! moves Y is a regression by definition (see the DO-NOT-RETRY lever list in
//! the audit history).
//!
//! Per `docs/testing-strategy.md` §Layer 3 + the v2.0.0 plan §Verification.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};

/// Hardware key for the DMC-during-OAM landing-offset sweep at `$50-$5F`
/// (`TriCNES` golden / `AccuracyCoin` source): three 4/3 pairs while the DMC GET
/// lands in the OAM alignment regime, then 2/1 pairs after the regime
/// transition.
const DMC_OAM_SWEEP_KEY: [u8; 16] = [
    0x04, 0x03, 0x04, 0x03, 0x04, 0x03, 0x02, 0x01, 0x02, 0x01, 0x02, 0x01, 0x02, 0x01, 0x02, 0x01,
];

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

/// Boot `AccuracyCoin`, start the battery, and capture the `CheckDMATiming`
/// measurements at the frame the DMC+OAM result byte (`$0477`) is first set.
#[test]
fn dmc_reload_dma_span_is_4_and_oam_sweep_on_key() {
    let bytes = fs::read(rom_path("accuracycoin/AccuracyCoin.nes")).expect("read AccuracyCoin");
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin");

    // Boot + press START (the `run_battery_capturing_ram` protocol).
    for _ in 0..300 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::START);
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::empty());

    // Run until the DMC+OAM result lands (~frame 1640 on the current
    // default; cap generously). Capture Y + the sweep at that frame —
    // the NEXT battery test overwrites `$50-$5F` within about a frame.
    let mut measured: Option<(u8, [u8; 16], u8)> = None;
    for _ in 306..2600 {
        nes.run_frame();
        let ram = nes.bus().ram_bytes();
        if ram[0x0477] != 0 {
            let mut sweep = [0u8; 16];
            sweep.copy_from_slice(&ram[0x0050..0x0060]);
            measured = Some((ram[0x0050], sweep, ram[0x0477]));
            break;
        }
    }

    let (y, sweep, result) =
        measured.expect("AccuracyCoin DMC+OAM result ($0477) never set within the frame budget");

    assert_eq!(
        result, 0x01,
        "AccuracyCoin DMC+OAM battery result ($0477) = {result:#04x}, expected 0x01 (pass)"
    );
    assert_eq!(
        y, 4,
        "CheckDMATiming measured a {y}-cycle normal (reload) DMC-DMA span; hardware = 4 \
         (halt + dummy + alignment + get, reload arm invisible to its own cycle). \
         The R5 closure regressed — see the v2.0.0 context brief's DO-NOT-RETRY list \
         before touching any DMC arm/parity lever."
    );
    assert_eq!(
        sweep, DMC_OAM_SWEEP_KEY,
        "DMC-during-OAM landing-offset sweep ($50-$5F) off KEY:\n  measured {sweep:02x?}\n  \
         key      {DMC_OAM_SWEEP_KEY:02x?}"
    );
}
