//! v2.1.7 "Hardware Revisions & DMA Frontier" — the 2A03 die-revision
//! ([`Cpu2A03Revision`]) DMA "unexpected read" axis, pinned honestly.
//!
//! The DMA frontier is the plan's officially-unsolved item, and this suite pins
//! exactly what is *verifiable* about the revision knob — no more (ADR 0033):
//!
//! 1. **Config surface + truth table.** The enum defaults to `Rp2A03G`, the
//!    setter/getter round-trips, and `has_unexpected_dma_extra_read()` is
//!    `true` only for `Rp2A03G`.
//! 2. **Default byte-identity.** An explicit `Rp2A03G` selection is
//!    bit-identical to the default (no-config) run — the revision gate is inert
//!    on the shipped path. (The `accuracycoin`, `nestest`, `dmc_dma`, and
//!    `dma_timing_pin` suites separately hold the wider 141/141 / 0-diff /
//!    all-`Passed` floor unchanged.)
//! 3. **Determinism per revision.** Same revision + ROM + input ⇒ bit-identical
//!    framebuffer *and* full snapshot, both arms.
//! 4. **The documented residual.** `Rp2A03H` is snapshot-identical to `Rp2A03G`
//!    across the whole committed DMA oracle corpus. This is NOT an accident to
//!    be "fixed": on this ported engine the halted-DMC-overlaps-OAM-read gate
//!    fires, but its parked address is always the post-`$4014` instruction fetch
//!    (PRG), never a `$2007`/`$4016`/`$4015`/`$4017` register, so the die-revision
//!    extra read is behaviorally unobservable (ADR 0033, `docs/scheduler.md`
//!    §"Unexpected DMA"). We PIN that equality so a future change that
//!    accidentally makes the non-default arm perturb the default corpus (or the
//!    default arm diverge) fails loudly. No hardware-correct value is asserted
//!    for `Rp2A03H` — no public reference or test ROM models the axis.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::{Cpu2A03Revision, Nes};

/// Every committed DMA-oracle ROM the revision axis could plausibly touch.
const DMA_CORPUS: &[&str] = &[
    "nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma.nes",
    "nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma_512.nes",
    "blargg/dmc_dma_during_read4/dma_2007_read.nes",
    "blargg/dmc_dma_during_read4/dma_2007_write.nes",
    "blargg/dmc_dma_during_read4/dma_4016_read.nes",
    "blargg/dmc_dma_during_read4/double_2007_read.nes",
    "blargg/dmc_dma_during_read4/read_write_2007.nes",
];

/// Boot `name`, force `revision`, run `frames` frames headless, and return
/// `(framebuffer_hash, snapshot_hash)`.
fn run(name: &str, revision: Cpu2A03Revision, frames: u64) -> (u64, u64) {
    let bytes = fs::read(rom_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    nes.set_cpu_2a03_revision(revision);
    assert_eq!(
        nes.cpu_2a03_revision(),
        revision,
        "revision setter must round-trip"
    );
    for _ in 0..frames {
        nes.run_frame();
    }
    (fnv1a64(nes.framebuffer()), fnv1a64(&nes.snapshot()))
}

/// Property 1: config surface + truth table.
#[test]
fn config_surface_and_truth_table() {
    let bytes = fs::read(rom_path(DMA_CORPUS[0])).expect("read sprdma");
    let mut nes = Nes::from_rom(&bytes).expect("parse sprdma");
    assert_eq!(nes.cpu_2a03_revision(), Cpu2A03Revision::Rp2A03G);
    assert_eq!(Cpu2A03Revision::default(), Cpu2A03Revision::Rp2A03G);

    nes.set_cpu_2a03_revision(Cpu2A03Revision::Rp2A03H);
    assert_eq!(nes.cpu_2a03_revision(), Cpu2A03Revision::Rp2A03H);
    nes.set_cpu_2a03_revision(Cpu2A03Revision::Rp2A03G);
    assert_eq!(nes.cpu_2a03_revision(), Cpu2A03Revision::Rp2A03G);

    assert!(Cpu2A03Revision::Rp2A03G.has_unexpected_dma_extra_read());
    assert!(!Cpu2A03Revision::Rp2A03H.has_unexpected_dma_extra_read());
}

/// Property 2: an explicit `Rp2A03G` selection equals the default (no-config)
/// run on the whole corpus — the gate never perturbs the shipped path.
#[test]
fn explicit_rp2a03g_equals_default() {
    for name in DMA_CORPUS {
        let bytes = fs::read(rom_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"));
        let mut default_nes = Nes::from_rom(&bytes).unwrap();
        for _ in 0..240 {
            default_nes.run_frame();
        }
        let default = (
            fnv1a64(default_nes.framebuffer()),
            fnv1a64(&default_nes.snapshot()),
        );
        let g = run(name, Cpu2A03Revision::Rp2A03G, 240);
        assert_eq!(
            default, g,
            "{name}: explicit Rp2A03G diverged from the default run — the revision \
             gate must be inert on the default path"
        );
    }
}

/// Property 3: determinism per revision (framebuffer + snapshot).
#[test]
fn each_revision_is_deterministic() {
    for name in DMA_CORPUS {
        for rev in [Cpu2A03Revision::Rp2A03G, Cpu2A03Revision::Rp2A03H] {
            let a = run(name, rev, 200);
            let b = run(name, rev, 200);
            assert_eq!(
                a, b,
                "{name} / {rev:?} is not deterministic across two runs"
            );
        }
    }
}

/// Property 4: the documented residual — `Rp2A03H` is snapshot-identical to
/// `Rp2A03G` across the whole DMA oracle corpus (ADR 0033). This is the honest
/// current state: the die-revision extra read is unobservable on this engine
/// (parked address during a DMC+OAM overlap is never a side-effect register).
/// Pinned as a regression guard; NOT a claim that `Rp2A03H` is correct.
#[test]
fn rp2a03h_matches_rp2a03g_documented_residual() {
    for name in DMA_CORPUS {
        let g = run(name, Cpu2A03Revision::Rp2A03G, 240);
        let h = run(name, Cpu2A03Revision::Rp2A03H, 240);
        assert_eq!(
            g, h,
            "{name}: Rp2A03H diverged from Rp2A03G. If this is an intentional, \
             oracle-grounded accuracy change, update ADR 0033 + the accuracy \
             ledger and re-bless; otherwise it is an unintended DMA-frontier \
             regression."
        );
    }
}
